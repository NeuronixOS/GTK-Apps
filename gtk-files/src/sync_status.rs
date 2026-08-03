//! Read gtk-sync-client status.json and resolve paths under the sync root.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use gtk4 as gtk;
use gtk::glib;
use gtk::prelude::*;
use serde::Deserialize;

use crate::sync_setup;
use crate::util::{format_size, show_error};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncFileState {
    UpToDate,
    Syncing,
    Pending,
    Deleted,
}

impl SyncFileState {
    pub fn label(self) -> &'static str {
        match self {
            Self::UpToDate => "Up to date",
            Self::Syncing => "Syncing",
            Self::Pending => "Pending",
            Self::Deleted => "Deleted",
        }
    }

    pub fn icon_name(self) -> &'static str {
        // Use Adwaita names that exist on Debian/Ubuntu (emblem-ok / emblem-synchronizing often missing).
        match self {
            Self::UpToDate => "object-select-symbolic",
            Self::Syncing => "view-refresh-symbolic",
            Self::Pending => "content-loading-symbolic",
            Self::Deleted => "user-trash-symbolic",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "up_to_date" => Some(Self::UpToDate),
            "syncing" => Some(Self::Syncing),
            "pending" => Some(Self::Pending),
            "deleted" => Some(Self::Deleted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ActiveTransfer {
    pub path: String,
    #[allow(dead_code)]
    pub direction: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TombstoneStatus {
    pub path: String,
    pub ts: u64,
    #[serde(default)]
    #[allow(dead_code)]
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClientStatus {
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub busy: bool,
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub active: Vec<ActiveTransfer>,
    #[serde(default)]
    pub files: HashMap<String, String>,
    #[serde(default)]
    pub tombstones: Vec<TombstoneStatus>,
}

impl ClientStatus {
    pub fn fingerprint(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            self.updated_at,
            self.busy,
            self.phase,
            self.files.len(),
            self.tombstones.len()
        )
    }

    /// True while a file is mid-transfer, or a push/pull queue still has work.
    /// Background index scans alone must not keep the sidebar on "Syncing".
    pub fn is_transferring(&self) -> bool {
        if !self.active.is_empty() {
            return true;
        }
        // Queued copies (pending) during an upload burst — not stuck historical pending
        // while phase is idle after a finished scan.
        matches!(self.phase.as_str(), "pulling" | "pushing")
            && self
                .files
                .values()
                .any(|v| *v == "pending" || *v == "syncing")
    }

    /// Resolve sync state for a file or directory under the client root.
    /// Unknown entries are never assumed up-to-date while a transfer is running.
    ///
    /// `on_disk`: true for real DirectoryList rows. Leftover folder tombstones
    /// after a partial restore must not keep showing those rows as Deleted.
    pub fn state_for_entry(&self, rel: &str, is_dir: bool, on_disk: bool) -> Option<SyncFileState> {
        let prefix = if rel.is_empty() {
            String::new()
        } else {
            format!("{rel}/")
        };

        // A folder that has live (non-deleted) children must not stay "Deleted"
        // just because a leftover parent tombstone remains after a partial restore.
        let dir_has_live_children = is_dir
            && self.files.iter().any(|(k, v)| {
                v != "deleted"
                    && (*k == rel || (!prefix.is_empty() && k.starts_with(&prefix)))
            });
        let suppress_deleted = on_disk || dir_has_live_children;

        if let Some(s) = self.files.get(rel).and_then(|s| SyncFileState::from_str(s)) {
            if !(s == SyncFileState::Deleted && suppress_deleted) {
                return Some(s);
            }
        }
        if self.tombstones.iter().any(|t| t.path == rel) && !suppress_deleted {
            return Some(SyncFileState::Deleted);
        }
        if self.active.iter().any(|a| a.path == rel) {
            return Some(SyncFileState::Syncing);
        }

        if is_dir {
            if self
                .active
                .iter()
                .any(|a| a.path == rel || (!prefix.is_empty() && a.path.starts_with(&prefix)))
            {
                return Some(SyncFileState::Syncing);
            }
            let mut saw_child = false;
            let mut saw_incomplete = false;
            for (k, v) in &self.files {
                let under = k == rel || (!prefix.is_empty() && k.starts_with(&prefix));
                if !under || v == "deleted" {
                    continue;
                }
                saw_child = true;
                if v == "syncing" {
                    return Some(SyncFileState::Syncing);
                }
                if v == "pending" {
                    saw_incomplete = true;
                }
            }
            if saw_incomplete {
                return Some(SyncFileState::Pending);
            }
            if saw_child || on_disk {
                return Some(SyncFileState::UpToDate);
            }
            if self.tombstones.iter().any(|t| t.path == rel) {
                return Some(SyncFileState::Deleted);
            }
            if self.is_transferring() {
                return Some(SyncFileState::Pending);
            }
            return None;
        }

        // File not listed yet: pending while work is happening, otherwise no emblem.
        if self.is_transferring() || matches!(self.phase.as_str(), "scanning" | "pushing" | "pulling")
        {
            return Some(SyncFileState::Pending);
        }
        None
    }

    /// Short label for the header bar (fixed width; left-aligned in a right-side chip).
    pub fn header_message(&self) -> Option<String> {
        // Idle with leftover pending marks (e.g. vanished blobs) must not keep
        // a permanent "1 pending" chip in the header.
        if !self.is_transferring() && self.phase != "scanning" {
            return None;
        }

        let raw = if let Some(a) = self.active.first() {
            let name = a
                .path
                .rsplit('/')
                .next()
                .unwrap_or(a.path.as_str());
            let dir = if a.direction == "down" { "↓" } else { "↑" };
            let extra = self.active.len().saturating_sub(1);
            let pending = self
                .files
                .values()
                .filter(|v| *v == "pending")
                .count();
            if extra > 0 || pending > 0 {
                format!("Syncing {dir} {name} (+{} more)", extra + pending)
            } else {
                format!("Syncing {dir} {name}")
            }
        } else {
            let pending = self
                .files
                .values()
                .filter(|v| *v == "pending" || *v == "syncing")
                .count();
            if pending > 0 {
                format!("Syncing ({pending} files)")
            } else if self.phase == "scanning" {
                "Scanning sync folder…".into()
            } else {
                "Syncing…".into()
            }
        };
        Some(fit_header_chars(&raw, 24))
    }

    /// Multi-line tooltip listing files currently transferring / queued.
    pub fn header_tooltip(&self) -> String {
        let mut lines = Vec::new();
        if !self.active.is_empty() {
            lines.push("Currently transferring:".into());
            for a in self.active.iter().take(12) {
                let arrow = if a.direction == "down" { "↓" } else { "↑" };
                lines.push(format!("  {arrow} {}", a.path));
            }
            if self.active.len() > 12 {
                lines.push(format!("  …and {} more", self.active.len() - 12));
            }
        }
        let pending: Vec<&String> = self
            .files
            .iter()
            .filter(|(_, v)| *v == "pending")
            .map(|(k, _)| k)
            .take(12)
            .collect();
        if !pending.is_empty() {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push("Queued:".into());
            for p in &pending {
                lines.push(format!("  • {p}"));
            }
            let total_pending = self.files.values().filter(|v| *v == "pending").count();
            if total_pending > pending.len() {
                lines.push(format!("  …and {} more", total_pending - pending.len()));
            }
        }
        if lines.is_empty() {
            "gtk-sync".into()
        } else {
            lines.join("\n")
        }
    }
}

/// Pad or truncate to exactly `width` display characters for a stable header chip.
fn fit_header_chars(s: &str, width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() == width {
        return s.to_string();
    }
    if chars.len() < width {
        let mut out = s.to_string();
        out.extend(std::iter::repeat(' ').take(width - chars.len()));
        return out;
    }
    if width <= 1 {
        return chars.into_iter().take(width).collect();
    }
    let mut out: String = chars.into_iter().take(width - 1).collect();
    out.push('…');
    out
}

pub fn status_file_path() -> PathBuf {
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        let p = PathBuf::from(runtime).join("gtk-sync").join("status.json");
        if p.is_file() {
            return p;
        }
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gtk-sync")
        .join("status.json")
}

pub fn load_client_status() -> Option<ClientStatus> {
    refresh_sync_cache_if_stale();
    sync_cache()
        .lock()
        .ok()
        .and_then(|c| c.status.clone())
}

/// Client sync root when the user service is active (same as sidebar).
pub fn client_sync_root() -> Option<PathBuf> {
    refresh_sync_cache_if_stale();
    sync_cache().lock().ok().and_then(|c| c.root.clone())
}

struct SyncCache {
    root: Option<PathBuf>,
    status: Option<ClientStatus>,
    at: std::time::Instant,
}

static SYNC_CACHE: OnceLock<Mutex<SyncCache>> = OnceLock::new();

fn sync_cache() -> &'static Mutex<SyncCache> {
    SYNC_CACHE.get_or_init(|| {
        Mutex::new(SyncCache {
            root: None,
            status: None,
            at: std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(60))
                .unwrap_or_else(std::time::Instant::now),
        })
    })
}

fn refresh_sync_cache_if_stale() {
    let Ok(mut cache) = sync_cache().lock() else {
        return;
    };
    if cache.at.elapsed() < std::time::Duration::from_millis(800) {
        return;
    }
    let root = sync_setup::probe_sync_status().client_root;
    // Only trust status.json while a client is actually configured/running.
    // A killed client can leave a busy status file in $XDG_RUNTIME_DIR.
    let status = if root.is_some() {
        let path = status_file_path();
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
    } else {
        None
    };
    cache.root = root;
    cache.status = status;
    cache.at = std::time::Instant::now();
}

/// Force the next bind/status read to re-probe (after setup / disconnect).
pub fn invalidate_sync_cache() {
    if let Ok(mut cache) = sync_cache().lock() {
        cache.at = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(60))
            .unwrap_or_else(std::time::Instant::now);
        cache.root = None;
        cache.status = None;
    }
}

/// Remove leftover runtime status so gtk-files does not keep showing Syncing.
pub fn clear_runtime_status_file() {
    let path = status_file_path();
    let _ = std::fs::remove_file(&path);
    if let Some(dir) = path.parent() {
        let _ = std::fs::remove_dir(dir);
    }
    invalidate_sync_cache();
}

fn rel_posix(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// If `path` is under the active client sync root, return the relative POSIX path.
pub fn path_under_sync_root(path: &Path) -> Option<(PathBuf, String)> {
    let root = client_sync_root()?;
    let root_c = root.canonicalize().unwrap_or(root.clone());

    // Missing files (tombstones) cannot canonicalize — strip against root as given.
    if let Ok(rel) = path.strip_prefix(&root_c).or_else(|_| path.strip_prefix(&root)) {
        return Some((root_c, rel_posix(rel)));
    }
    let path_c = path.canonicalize().ok()?;
    let rel = path_c.strip_prefix(&root_c).ok()?;
    Some((root_c, rel_posix(rel)))
}

pub fn is_under_sync_root(path: &Path) -> bool {
    path_under_sync_root(path).is_some()
}

/// Tombstones whose parent directory equals `dir_rel` ("" = sync root).
/// Skips entries that are "live again" (folder/file exists under the sync root
/// with non-deleted children) so Show deleted does not re-list restored trees.
pub fn tombstones_in_dir(status: &ClientStatus, dir_rel: &str) -> Vec<TombstoneStatus> {
    status
        .tombstones
        .iter()
        .filter(|t| {
            let parent = match t.path.rfind('/') {
                Some(i) => &t.path[..i],
                None => "",
            };
            if parent != dir_rel {
                return false;
            }
            !tombstone_superseded_by_live(status, &t.path)
        })
        .cloned()
        .collect()
}

/// True when live (non-deleted) files exist at or under `rel`, so a leftover
/// tombstone should not present the path as deleted.
pub fn tombstone_superseded_by_live(status: &ClientStatus, rel: &str) -> bool {
    let prefix = format!("{rel}/");
    status.files.iter().any(|(k, v)| {
        v != "deleted" && (*k == rel || k.starts_with(&prefix))
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionEntry {
    pub ts: u64,
    pub size: u64,
    #[serde(default)]
    #[allow(dead_code)]
    pub hash: String,
}

fn gtk_sync_client_bin() -> PathBuf {
    if let Ok(p) = std::env::var("GTK_SYNC_CLIENT") {
        return PathBuf::from(p);
    }
    let local = dirs::home_dir()
        .unwrap_or_default()
        .join(".local/bin/gtk-sync-client");
    if local.is_file() {
        return local;
    }
    PathBuf::from("gtk-sync-client")
}

/// `gtk-sync-client versions --json <rel>`
pub fn fetch_versions_json(rel_path: &str) -> Result<Vec<VersionEntry>, String> {
    let bin = gtk_sync_client_bin();
    let out = Command::new(&bin)
        .args(["versions", "--json", rel_path])
        .output()
        .map_err(|e| format!("Could not run {}: {e}", bin.display()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(if err.trim().is_empty() {
            format!("versions failed ({})", out.status)
        } else {
            err.trim().to_string()
        });
    }
    let text = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(text.trim()).map_err(|e| format!("Bad versions JSON: {e}"))
}

/// `gtk-sync-client restore <rel> <ts>`
pub fn restore_version(rel_path: &str, ts: u64) -> Result<(), String> {
    let bin = gtk_sync_client_bin();
    let out = Command::new(&bin)
        .args(["restore", rel_path, &ts.to_string()])
        .output()
        .map_err(|e| format!("Could not run {}: {e}", bin.display()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(if err.trim().is_empty() {
            format!("restore failed ({})", out.status)
        } else {
            err.trim().to_string()
        });
    }
    Ok(())
}

/// `gtk-sync-client restore-tree <rel>` — undelete folder + all file contents.
pub fn restore_tree(rel_path: &str) -> Result<String, String> {
    let bin = gtk_sync_client_bin();
    let out = Command::new(&bin)
        .args(["restore-tree", rel_path])
        .output()
        .map_err(|e| format!("Could not run {}: {e}", bin.display()))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(if err.trim().is_empty() {
            if stdout.is_empty() {
                format!("restore-tree failed ({})", out.status)
            } else {
                stdout
            }
        } else {
            err.trim().to_string()
        });
    }
    Ok(stdout)
}

/// Deleted sync folder (has tombstoned children) — restore brings the whole tree back.
pub fn path_is_deleted_folder(path: &Path) -> bool {
    let Some((_root, rel)) = path_under_sync_root(path) else {
        return false;
    };
    let Some(status) = load_client_status() else {
        return false;
    };
    if path.exists() && tombstone_superseded_by_live(&status, &rel) {
        return false;
    }
    if !path_is_sync_deleted(path) && path.exists() {
        return false;
    }
    tombstone_looks_like_dir(&status, &rel)
}

/// Approximate count of tombstone paths under a folder (files + dir markers).
pub fn count_tombstones_under(rel: &str) -> usize {
    let Some(status) = load_client_status() else {
        return 0;
    };
    let prefix_slash = format!("{rel}/");
    status
        .tombstones
        .iter()
        .filter(|t| {
            (t.path == rel || t.path.starts_with(&prefix_slash))
                && !tombstone_superseded_by_live(&status, &t.path)
        })
        .count()
}

pub fn format_version_ts(ts: u64) -> String {
    let Ok(dt) = glib_datetime_from_unix(ts) else {
        return format!("ts={ts}");
    };
    dt
}

fn glib_datetime_from_unix(ts: u64) -> Result<String, ()> {
    use gtk4::glib;
    let dt = glib::DateTime::from_unix_local(ts as i64).map_err(|_| ())?;
    Ok(dt.format("%Y-%m-%d %H:%M:%S").map_err(|_| ())?.to_string())
}

#[allow(dead_code)]
pub fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// True when any tombstone lives under `rel/` (so the ghost should open as a folder).
pub fn tombstone_looks_like_dir(status: &ClientStatus, rel: &str) -> bool {
    let prefix = format!("{rel}/");
    status.tombstones.iter().any(|t| t.path.starts_with(&prefix))
}

/// Build a FileInfo ghost for a deleted sync path (file may not exist on disk).
pub fn make_deleted_file_info(
    sync_root: &Path,
    rel: &str,
    ts: u64,
    is_dir: bool,
) -> gtk::gio::FileInfo {
    use gtk::gio;
    use gtk::glib;

    let name = Path::new(rel)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| rel.to_string());
    let full = sync_root.join(rel);
    let file = gio::File::for_path(&full);
    let info = gio::FileInfo::new();
    info.set_name(&name);
    info.set_display_name(&name);
    if is_dir {
        info.set_file_type(gio::FileType::Directory);
        info.set_content_type("inode/directory");
        info.set_icon(&gio::ThemedIcon::new("folder"));
        info.set_symbolic_icon(&gio::ThemedIcon::new("folder-symbolic"));
    } else {
        info.set_file_type(gio::FileType::Regular);
        let (ctype, _uncertain) = gio::content_type_guess(Some(&name), &[]);
        info.set_content_type(ctype.as_str());
        info.set_icon(&gio::content_type_get_icon(ctype.as_str()));
        info.set_symbolic_icon(&gio::content_type_get_symbolic_icon(ctype.as_str()));
    }
    info.set_size(0);
    info.set_attribute_object("standard::file", &file);
    info.set_attribute_string("xattr::gtk-files-sync", "deleted");
    info.set_attribute_string("xattr::gtk-files-sync-rel", rel);
    if let Ok(dt) = glib::DateTime::from_unix_local(ts as i64) {
        info.set_modification_date_time(&dt);
    }
    info
}

pub fn is_deleted_file_info(info: &gtk::gio::FileInfo) -> bool {
    info.attribute_string("xattr::gtk-files-sync")
        .as_deref()
        == Some("deleted")
}

/// True when path is a tombstoned sync file (may not exist on disk).
pub fn path_is_sync_deleted(path: &Path) -> bool {
    // Anything still present on disk is live for the UI — leftover parent
    // tombstones after a partial restore must not keep the folder "deleted".
    if path.exists() {
        return false;
    }
    let Some((_root, rel)) = path_under_sync_root(path) else {
        return false;
    };
    let Some(status) = load_client_status() else {
        return false;
    };
    if tombstone_superseded_by_live(&status, &rel) {
        return false;
    }
    status.tombstones.iter().any(|t| t.path == rel)
}

/// Confirm and restore an entire deleted folder tree.
pub fn show_restore_folder_dialog(
    parent: &impl IsA<gtk::Window>,
    abs_path: &Path,
    on_done: impl Fn() + 'static,
) {
    let Some((_root, rel)) = path_under_sync_root(abs_path) else {
        show_error(
            Some(parent),
            "Restore folder",
            "This path is not inside the active gtk-sync client folder.",
        );
        return;
    };

    let n = count_tombstones_under(&rel);
    let detail = if n > 0 {
        format!(
            "Restore deleted folder “{rel}” and all of its contents?\n\n\
             About {n} deleted paths will be cleared; every file that still has \
             history on the server will be downloaded again."
        )
    } else {
        format!("Restore deleted folder “{rel}” and all of its contents from the server?")
    };

    let parent_win = parent.clone().upcast::<gtk::Window>();
    let rel_for_confirm = rel.clone();
    crate::util::confirm_dialog(
        Some(&parent_win.clone()),
        "Restore deleted folder",
        &detail,
        "Restore",
        move |ok| {
            if !ok {
                return;
            }
            let parent_win = parent_win.clone();
            let rel_label = rel_for_confirm.clone();
            let rel_worker = rel_for_confirm.clone();
            // Large trees can take a while — keep the UI responsive.
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(restore_tree(&rel_worker));
            });

            let progress = gtk::Window::builder()
                .title("Restoring folder…")
                .transient_for(&parent_win)
                .modal(true)
                .default_width(360)
                .default_height(120)
                .build();
            gtk_theme::style_dialog(&progress);
            let box_ = gtk::Box::new(gtk::Orientation::Vertical, 12);
            box_.set_margin_start(20);
            box_.set_margin_end(20);
            box_.set_margin_top(20);
            box_.set_margin_bottom(20);
            let label = gtk::Label::new(Some(&format!("Restoring {rel_label}…")));
            label.set_wrap(true);
            let spinner = gtk::Spinner::new();
            spinner.start();
            box_.append(&spinner);
            box_.append(&label);
            progress.set_child(Some(&box_));
            progress.present();

            let progress_c = progress.clone();
            let parent_c = parent_win.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
                match rx.try_recv() {
                    Ok(Ok(msg)) => {
                        progress_c.close();
                        if !msg.is_empty() {
                            let d = gtk::AlertDialog::builder()
                                .modal(true)
                                .message("Folder restored")
                                .detail(&msg)
                                .buttons(["OK"])
                                .build();
                            d.show(Some(&parent_c));
                        }
                        on_done();
                        glib::ControlFlow::Break
                    }
                    Ok(Err(e)) => {
                        progress_c.close();
                        show_error(Some(&parent_c), "Restore folder failed", &e);
                        glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        progress_c.close();
                        show_error(
                            Some(&parent_c),
                            "Restore folder failed",
                            "Restore process ended unexpectedly.",
                        );
                        glib::ControlFlow::Break
                    }
                }
            });
        },
    );
}

/// Dialog: pick a historical version and restore via gtk-sync-client.
pub fn show_restore_dialog(
    parent: &impl IsA<gtk::Window>,
    abs_path: &Path,
    on_done: impl Fn() + 'static,
) {
    let Some((_root, rel)) = path_under_sync_root(abs_path) else {
        show_error(
            Some(parent),
            "Restore",
            "This path is not inside the active gtk-sync client folder.",
        );
        return;
    };

    if path_is_deleted_folder(abs_path) {
        show_restore_folder_dialog(parent, abs_path, on_done);
        return;
    }

    let versions = match fetch_versions_json(&rel) {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => {
            show_error(
                Some(parent),
                "Restore",
                &format!("No versions found for {rel}"),
            );
            return;
        }
        Err(e) => {
            show_error(Some(parent), "Restore", &e);
            return;
        }
    };

    let dialog = gtk::Window::builder()
        .title("Restore previous version")
        .transient_for(parent)
        .modal(true)
        .default_width(420)
        .default_height(360)
        .build();
    gtk_theme::style_dialog(&dialog);

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 10);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);

    let intro = gtk::Label::new(Some(&format!("Versions of {rel}")));
    intro.set_xalign(0.0);
    intro.add_css_class("heading");
    intro.set_wrap(true);
    vbox.append(&intro);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.add_css_class("boxed-list");
    let versions = Rc::new(versions);
    for (i, v) in versions.iter().enumerate().rev() {
        let row = gtk::ListBoxRow::new();
        let box_ = gtk::Box::new(gtk::Orientation::Vertical, 2);
        box_.set_margin_start(10);
        box_.set_margin_end(10);
        box_.set_margin_top(8);
        box_.set_margin_bottom(8);
        let title = gtk::Label::new(Some(&format_version_ts(v.ts)));
        title.set_xalign(0.0);
        let sub = gtk::Label::new(Some(&format!(
            "{} · ts={}",
            format_size(v.size),
            v.ts
        )));
        sub.set_xalign(0.0);
        sub.add_css_class("dim-label");
        sub.add_css_class("caption");
        box_.append(&title);
        box_.append(&sub);
        row.set_child(Some(&box_));
        unsafe {
            row.set_data("version-index", i);
        }
        list.append(&row);
    }
    if let Some(first) = list.row_at_index(0) {
        list.select_row(Some(&first));
    }

    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .min_content_height(180)
        .build();
    vbox.append(&scroll);

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Cancel");
    let restore = gtk::Button::with_label("Restore");
    restore.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&restore);
    vbox.append(&buttons);
    dialog.set_child(Some(&vbox));

    {
        let d = dialog.clone();
        cancel.connect_clicked(move |_| d.close());
    }
    {
        let d = dialog.clone();
        let list = list.clone();
        let versions = Rc::clone(&versions);
        let rel = rel.clone();
        let parent = parent.clone().upcast::<gtk::Window>();
        restore.connect_clicked(move |_| {
            let Some(row) = list.selected_row() else {
                return;
            };
            let idx = unsafe {
                row.data::<usize>("version-index")
                    .map(|p| *p.as_ref())
                    .unwrap_or(0)
            };
            let Some(v) = versions.get(idx) else {
                return;
            };
            match restore_version(&rel, v.ts) {
                Ok(()) => {
                    d.close();
                    on_done();
                }
                Err(e) => show_error(Some(&parent), "Restore failed", &e),
            }
        });
    }

    dialog.present();
}
