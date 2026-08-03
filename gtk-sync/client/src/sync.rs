use crate::discover;
use crate::status;
use base64::Engine;
use mimic_core::config::{ClientConfig, DiscoveredPeer};
use mimic_core::index::{FileIndex, Tombstone, TombstoneKind};
use mimic_core::protocol::{IndexResponse, PutBlobResponse, VersionsResponse};
use mimic_core::versioning::content_hash;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use walkdir::WalkDir;

/// While applying remote tombstones locally, ignore Remove events so we don't
/// re-POST thousands of tombstones and starve the reconcile loop.
static APPLYING_REMOTE_DELETES: AtomicBool = AtomicBool::new(false);

pub struct HttpPeer {
    pub base: String,
    pub auth: String,
    pub name: String,
}

pub fn http_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        // Large IDE / binary blobs need more than the old 60s default.
        .timeout(Duration::from_secs(600))
        .build()?)
}

pub fn basic_auth(user: &str, pass: &str) -> String {
    let token = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
    format!("Basic {token}")
}

pub fn encode_path(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub async fn resolve_peers(cfg: &ClientConfig) -> Vec<HttpPeer> {
    let auth = basic_auth(&cfg.username, &cfg.password);
    let mut peers: Vec<DiscoveredPeer> = cfg
        .peers
        .iter()
        .filter(|p| !p.excluded)
        .cloned()
        .collect();

    for s in &cfg.static_peers {
        if let Some((host, port)) = s.split_once(':') {
            if let Ok(port) = port.parse() {
                peers.push(DiscoveredPeer {
                    name: format!("static-{host}"),
                    host: host.to_string(),
                    port,
                    cert_fingerprint: None,
                    excluded: false,
                });
            }
        }
    }

    if cfg.auto_discover {
        if let Ok(found) = discover::browse_async(Duration::from_secs(2)).await {
            for f in found {
                if !peers.iter().any(|p| p.host == f.host && p.port == f.port) {
                    peers.push(f);
                }
            }
        }
    }

    peers
        .into_iter()
        .filter(|p| !p.excluded)
        .fold(Vec::new(), |mut acc, p| {
            if !acc.iter().any(|x: &DiscoveredPeer| x.host == p.host && x.port == p.port) {
                acc.push(p);
            }
            acc
        })
        .into_iter()
        .map(|p| HttpPeer {
            base: format!("https://{}:{}", p.host, p.port),
            auth: auth.clone(),
            name: p.name,
        })
        .collect()
}

pub async fn run(config_path: PathBuf) -> anyhow::Result<()> {
    let cfg = ClientConfig::load(&config_path)?;
    ensure_client_root(&cfg.root)?;
    status::init();
    tracing::info!("status file: {}", status::status_path().display());

    let (tx, rx) = mpsc::channel();
    let mut watcher = RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        },
        notify::Config::default(),
    )?;
    watcher.watch(&cfg.root, RecursiveMode::Recursive)?;

    // Initial full sync — scanning is not "busy" for the sidebar (large trees
    // can take minutes); only pull/push transfers flip the syncing icon.
    status::set_phase("scanning", false);
    if let Err(e) = full_sync(&cfg, &rx).await {
        tracing::warn!("initial sync: {e}");
    }
    status::set_phase("idle", false);

    let mut last_scan = std::time::Instant::now();
    loop {
        // Cap events per tick so a mass-delete cannot starve index reconcile /
        // status.json tombstone publish (needed for gtk-files Show deleted).
        let mut handled = 0u32;
        while handled < 24 {
            match rx.try_recv() {
                Ok(event) => {
                    if let Err(e) = handle_fs_event(&cfg, &event).await {
                        tracing::warn!("fs event: {e}");
                    }
                    handled += 1;
                }
                Err(_) => break,
            }
        }

        // Periodic reconcile
        if last_scan.elapsed() > Duration::from_secs(10) {
            status::set_phase("scanning", false);
            if let Err(e) = full_sync(&cfg, &rx).await {
                tracing::debug!("periodic sync: {e}");
            }
            status::set_phase("idle", false);
            last_scan = std::time::Instant::now();
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn ensure_client_root(root: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(root)?;
    Ok(())
}

fn is_ignored(rel: &str) -> bool {
    // Skip common VCS / editor noise; no GTK-Sync marker dirs anymore
    rel.starts_with(".git/")
        || rel == ".git"
        || rel.ends_with('~')
        || rel.is_empty()
}

fn local_file_map(root: &Path) -> anyhow::Result<HashMap<String, (u64, String)>> {
    let mut map = HashMap::new();
    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(rel) = mimic_core::index::relative_path(root, entry.path()) else {
            continue;
        };
        if is_ignored(&rel) {
            continue;
        }
        let data = std::fs::read(entry.path())?;
        let hash = content_hash(&data);
        let meta = entry.metadata()?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        map.insert(rel, (mtime, hash));
    }
    Ok(map)
}

async fn full_sync(cfg: &ClientConfig, rx: &mpsc::Receiver<notify::Event>) -> anyhow::Result<()> {
    let client = http_client()?;
    let peers = resolve_peers(cfg).await;
    if peers.is_empty() {
        tracing::debug!("no peers yet");
        status::publish_reconcile(std::collections::BTreeMap::new(), &[]);
        return Ok(());
    }

    // Merge remote indexes (LWW by ts)
    let mut merged = FileIndex::default();
    let mut peer_indexes: Vec<(HttpPeer, IndexResponse)> = Vec::new();

    for peer in &peers {
        let url = format!("{}/v1/index", peer.base);
        match client
            .get(&url)
            .header("Authorization", &peer.auth)
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => {
                let idx: IndexResponse = r.json().await?;
                for f in &idx.files {
                    let take = match merged.files.get(&f.path) {
                        Some(cur) => f.ts > cur.ts,
                        None => true,
                    };
                    if take {
                        merged.files.insert(f.path.clone(), f.clone());
                    }
                    merged
                        .history
                        .entry(f.path.clone())
                        .or_default()
                        .push(f.clone());
                }
                for t in &idx.tombstones {
                    if !merged
                        .tombstones
                        .iter()
                        .any(|x| x.path == t.path && x.ts == t.ts)
                    {
                        merged.tombstones.push(t.clone());
                    }
                }
                peer_indexes.push((
                    HttpPeer {
                        base: peer.base.clone(),
                        auth: peer.auth.clone(),
                        name: peer.name.clone(),
                    },
                    idx,
                ));
            }
            Ok(r) => tracing::debug!("{} index: {}", peer.name, r.status()),
            Err(e) => tracing::debug!("{} unreachable: {e}", peer.name),
        }
    }

    // Publish tombstones immediately — do not wait for pull/push/hashing.
    status::publish_tombstones(&merged.tombstones);

    let local = local_file_map(&cfg.root)?;

    // Apply remote deletes (tombstones) locally if local file older/missing awareness
    APPLYING_REMOTE_DELETES.store(true, Ordering::SeqCst);
    for t in &merged.tombstones {
        if matches!(t.kind, TombstoneKind::Delete) {
            // Partial folder restore: live index files under this path must not be wiped
            // by a leftover parent tombstone.
            let prefix_slash = format!("{}/", t.path);
            let has_live = merged.files.keys().any(|p| {
                *p == t.path || p.starts_with(&prefix_slash)
            });
            if has_live {
                continue;
            }
            let paths: Vec<String> = std::iter::once(t.path.clone())
                .chain(t.children.iter().cloned())
                .collect();
            for p in paths {
                let full = cfg.root.join(&p);
                if full.exists() {
                    // Only delete if we don't have a newer local edit than tombstone
                    if let Some((mtime, _)) = local.get(&p) {
                        if *mtime > t.ts {
                            continue;
                        }
                    }
                    if full.is_dir() {
                        let _ = std::fs::remove_dir_all(&full);
                    } else {
                        let _ = std::fs::remove_file(&full);
                    }
                    tracing::info!("applied delete for {p}");
                }
            }
        }
    }
    APPLYING_REMOTE_DELETES.store(false, Ordering::SeqCst);
    // Drop notify backlog from remove_dir_all so we don't re-tombstone it.
    while rx.try_recv().is_ok() {}

    // Pull newer remote files
    for (path, entry) in &merged.files {
        if status::is_unavailable(path, entry.ts) {
            continue;
        }
        let need_pull = match local.get(path) {
            Some((_, hash)) if hash == &entry.hash => false,
            Some((mtime, _)) if *mtime > entry.ts => false, // local newer — push instead
            _ => true,
        };
        if need_pull {
            let result = if let Some((peer, _)) = peer_indexes.iter().find(|(_, idx)| {
                idx.files.iter().any(|f| f.path == *path && f.ts == entry.ts)
            }) {
                pull_file(cfg, &client, peer, path, entry.ts).await
            } else if let Some((peer, _)) = peer_indexes.first() {
                pull_file(cfg, &client, peer, path, entry.ts).await
            } else {
                Ok(())
            };
            if let Err(e) = result {
                tracing::warn!("pull {path}: {e}");
            }
        }
    }

    // Push local files that are newer or missing remotely
    let local = local_file_map(&cfg.root)?;
    for (path, (mtime, hash)) in &local {
        let remote = merged.files.get(path);
        let need_push = match remote {
            Some(e) if &e.hash == hash => false,
            Some(_) => true, // content changed locally — push new version
            None => true,
        };
        let _ = mtime; // reserved for future conflict heuristics
        if need_push {
            if let Err(e) = push_file_to_all(cfg, &client, &peers, path).await {
                tracing::warn!("push {path}: {e}");
            }
        }
    }

    // Push local deletes: local missing but remote has file and no covering tombstone from us
    // (Handled by watcher primarily; here detect orphans)
    let local_paths: HashSet<_> = local.keys().cloned().collect();
    for (path, entry) in &merged.files {
        if !local_paths.contains(path) {
            // Check if a tombstone already exists newer than entry
            let tombstoned = merged.tombstones.iter().any(|t| {
                (t.path == *path || t.children.iter().any(|c| c == path)) && t.ts >= entry.ts
            });
            if !tombstoned {
                // Could be not yet pulled — if we intentionally deleted, watcher sent tombstone.
                // Skip aggressive delete propagation on scan to avoid wiping on first join.
            }
        }
    }

    // Publish reconciled per-file states for gtk-files.
    let local = local_file_map(&cfg.root)?;
    let mut file_states = std::collections::BTreeMap::new();
    for (path, entry) in &merged.files {
        if status::is_unavailable(path, entry.ts) {
            // Missing blob on server — don't leave the UI stuck on "1 pending".
            continue;
        }
        let state = match local.get(path) {
            Some((_, hash)) if hash == &entry.hash => "up_to_date",
            Some(_) => "pending",
            None => "pending",
        };
        file_states.insert(path.clone(), state.to_string());
    }
    for path in local.keys() {
        file_states
            .entry(path.clone())
            .or_insert_with(|| "up_to_date".into());
    }
    status::publish_reconcile(file_states, &merged.tombstones);

    Ok(())
}

async fn pull_file(
    cfg: &ClientConfig,
    client: &reqwest::Client,
    peer: &HttpPeer,
    path: &str,
    ts: u64,
) -> anyhow::Result<()> {
    status::begin_transfer(path, "down");
    let url = format!(
        "{}/v1/blob?path={}&ts={ts}",
        peer.base,
        encode_path(path)
    );
    let result = async {
        let resp = client
            .get(&url)
            .header("Authorization", &peer.auth)
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            status::mark_unavailable(path, ts);
            anyhow::bail!("pull {path} from {}: 404 Not Found (blob missing)", peer.name);
        }
        if !resp.status().is_success() {
            anyhow::bail!("pull {path} from {}: {}", peer.name, resp.status());
        }
        let data = resp.bytes().await?;
        let dest = cfg.root.join(path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, &data)?;
        status::clear_unavailable(path);
        tracing::info!("pulled {path}@{ts} from {}", peer.name);
        Ok(())
    }
    .await;
    if status::is_unavailable(path, ts) {
        // Already cleared pending via mark_unavailable.
        return result;
    }
    status::end_transfer(
        path,
        if result.is_ok() {
            "up_to_date"
        } else {
            "pending"
        },
    );
    result
}

async fn push_file_to_all(
    cfg: &ClientConfig,
    client: &reqwest::Client,
    peers: &[HttpPeer],
    path: &str,
) -> anyhow::Result<()> {
    status::begin_transfer(path, "up");
    let result = async {
        let data = std::fs::read(cfg.root.join(path))?;
        let mut any_ok = false;
        let mut last_err: Option<String> = None;
        for peer in peers {
            let url = format!("{}/v1/blob?path={}", peer.base, encode_path(path));
            match client
                .put(&url)
                .header("Authorization", &peer.auth)
                .body(data.clone())
                .send()
                .await
            {
                Ok(r) if r.status().is_success() => {
                    let _: PutBlobResponse = r.json().await.unwrap_or(PutBlobResponse {
                        entry: mimic_core::index::FileEntry {
                            path: path.to_string(),
                            ts: 0,
                            hash: String::new(),
                            size: 0,
                            stored_name: String::new(),
                        },
                    });
                    tracing::info!("pushed {path} to {}", peer.name);
                    any_ok = true;
                }
                Ok(r) => {
                    let msg = format!("HTTP {}", r.status());
                    tracing::warn!("push {path} to {}: {msg}", peer.name);
                    last_err = Some(msg);
                }
                Err(e) => {
                    tracing::warn!("push {path} to {}: {e}", peer.name);
                    last_err = Some(e.to_string());
                }
            }
        }
        if any_ok {
            Ok(())
        } else {
            anyhow::bail!(last_err.unwrap_or_else(|| "push failed on all peers".into()))
        }
    }
    .await;
    status::end_transfer(
        path,
        if result.is_ok() {
            "up_to_date"
        } else {
            "pending"
        },
    );
    result
}

async fn handle_fs_event(cfg: &ClientConfig, event: &notify::Event) -> anyhow::Result<()> {
    if APPLYING_REMOTE_DELETES.load(Ordering::Relaxed) {
        return Ok(());
    }

    let client = http_client()?;
    let peers = resolve_peers(cfg).await;
    if peers.is_empty() {
        return Ok(());
    }

    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) => {
            let mut to_push = Vec::new();
            for path in &event.paths {
                if let Some(rel) = mimic_core::index::relative_path(&cfg.root, path) {
                    if is_ignored(&rel) || !path.is_file() {
                        continue;
                    }
                    to_push.push(rel);
                }
            }
            // Mark the whole burst pending first so gtk-files doesn't show checkmarks
            // on files that have not been uploaded yet.
            status::mark_pending(&to_push);
            for rel in &to_push {
                push_file_to_all(cfg, &client, &peers, rel).await?;
            }
        }
        EventKind::Remove(_) => {
            for path in &event.paths {
                if let Some(rel) = mimic_core::index::relative_path(&cfg.root, path) {
                    if is_ignored(&rel) {
                        continue;
                    }
                    // Already published from the server index — don't re-POST.
                    if status::is_known_tombstone(&rel) {
                        continue;
                    }
                    let children = collect_former_children(cfg, &rel);
                    let tomb = Tombstone {
                        path: rel.clone(),
                        kind: TombstoneKind::Delete,
                        ts: FileIndex::now_ts(),
                        children,
                    };
                    for peer in &peers {
                        let url = format!("{}/v1/tombstone", peer.base);
                        let _ = client
                            .post(&url)
                            .header("Authorization", &peer.auth)
                            .json(&mimic_core::protocol::TombstoneRequest {
                                tombstone: tomb.clone(),
                            })
                            .send()
                            .await;
                    }
                    status::note_tombstone(&tomb.path, tomb.ts);
                    tracing::info!("tombstoned {}", tomb.path);
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn collect_former_children(_cfg: &ClientConfig, _rel: &str) -> Vec<String> {
    // Without a local snapshot of the tree we cannot enumerate deleted dir children;
    // server retains versioned blobs; tombstone path covers the root delete.
    Vec::new()
}

pub async fn fetch_versions(
    cfg: &ClientConfig,
    path: &str,
) -> anyhow::Result<Vec<mimic_core::index::FileEntry>> {
    let client = http_client()?;
    let peers = resolve_peers(cfg).await;
    let mut all = Vec::new();
    for peer in peers {
        let url = format!("{}/v1/versions?path={}", peer.base, encode_path(path));
        if let Ok(r) = client
            .get(&url)
            .header("Authorization", &peer.auth)
            .send()
            .await
        {
            if r.status().is_success() {
                let resp: VersionsResponse = r.json().await?;
                for v in resp.versions {
                    if !all.iter().any(|e: &mimic_core::index::FileEntry| e.ts == v.ts && e.hash == v.hash)
                    {
                        all.push(v);
                    }
                }
            }
        }
    }
    all.sort_by_key(|e| e.ts);
    Ok(all)
}
