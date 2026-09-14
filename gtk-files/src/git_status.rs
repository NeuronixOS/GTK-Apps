//! Recursive git-status coloring for the file list.
//!
//! Discovers `.git` repos under the folder being viewed (same prunes as the
//! `changes` shell helper), caches repo roots on disk, and keeps porcelain
//! snapshots in memory. Bind/restyle only read the cache — git never runs on
//! the UI thread.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use gtk4 as gtk;
use gtk::glib;

/// Name-label color class: blue / orange / red.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GitDecor {
    /// Untracked or newly added (`??`, `A`).
    Untracked,
    /// Tracked dirty (modified, deleted, renamed, …).
    Modified,
    /// Unmerged / both-added / both-deleted.
    Conflict,
}

impl GitDecor {
    pub fn css_class(self) -> &'static str {
        match self {
            Self::Untracked => "git-untracked",
            Self::Modified => "git-modified",
            Self::Conflict => "git-conflict",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Untracked => "New",
            Self::Modified => "Modified",
            Self::Conflict => "Conflict",
        }
    }

    fn severity(self) -> u8 {
        match self {
            Self::Untracked => 1,
            Self::Modified => 2,
            Self::Conflict => 3,
        }
    }

    fn merge(self, other: Self) -> Self {
        if other.severity() > self.severity() {
            other
        } else {
            self
        }
    }

    fn merge_opt(a: Option<Self>, b: Option<Self>) -> Option<Self> {
        match (a, b) {
            (None, x) | (x, None) => x,
            (Some(x), Some(y)) => Some(x.merge(y)),
        }
    }
}

const STATUS_TTL: Duration = Duration::from_millis(1500);
const FIND_TTL: Duration = Duration::from_secs(30);
const MAX_FIND_DEPTH: usize = 24;

struct RepoStatus {
    /// Relative paths with `/` separators, no trailing slash.
    files: BTreeMap<String, GitDecor>,
    aggregate: Option<GitDecor>,
    scanned_at: Instant,
}

struct GitCache {
    roots: BTreeSet<PathBuf>,
    statuses: HashMap<PathBuf, RepoStatus>,
    last_find: HashMap<PathBuf, Instant>,
    roots_loaded: bool,
}

fn cache() -> &'static Mutex<GitCache> {
    static CACHE: OnceLock<Mutex<GitCache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(GitCache {
            roots: BTreeSet::new(),
            statuses: HashMap::new(),
            last_find: HashMap::new(),
            roots_loaded: false,
        })
    })
}

fn cache_file() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".cache")
        })
        .join("gtk-files")
        .join("git-repos")
}

fn load_roots_from_disk(roots: &mut BTreeSet<PathBuf>) {
    let Ok(text) = fs::read_to_string(cache_file()) else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let path = PathBuf::from(line);
        if git_dir_exists(&path) {
            roots.insert(normalize_path(&path));
        }
    }
}

fn persist_roots(roots: &BTreeSet<PathBuf>) {
    let path = cache_file();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut body = String::new();
    for root in roots {
        body.push_str(&root.display().to_string());
        body.push('\n');
    }
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, body).is_ok() {
        let _ = fs::rename(&tmp, &path);
    }
}

fn git_dir_exists(repo: &Path) -> bool {
    let git = repo.join(".git");
    git.is_dir() || git.is_file()
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn is_virtual_fs(path: &Path) -> bool {
    path.starts_with("/proc") || path.starts_with("/sys") || path.starts_with("/dev")
}

fn skip_downward_find(cwd: &Path) -> bool {
    cwd == Path::new("/") || is_virtual_fs(cwd)
}

fn should_prune_name(name: &str) -> bool {
    matches!(
        name,
        "node_modules"
            | "target"
            | "venv"
            | ".venv"
            | "__pycache__"
            | ".cache"
            | ".cargo"
            | ".nvm"
            | ".npm"
            | ".cursor"
    )
}

fn should_prune_path(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if should_prune_name(name) {
            return true;
        }
    }
    if let Some(home) = dirs::home_dir() {
        if path == home.join(".local").join("share")
            || path == home.join(".cache")
            || path == home.join(".cargo")
            || path == home.join(".nvm")
            || path == home.join(".npm")
            || path == home.join(".cursor")
        {
            return true;
        }
    }
    false
}

fn walk_up_git(start: &Path) -> Option<PathBuf> {
    let mut dir = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        if is_virtual_fs(&dir) {
            return None;
        }
        if git_dir_exists(&dir) {
            return Some(normalize_path(&dir));
        }
        if dir == Path::new("/") || !dir.pop() {
            return None;
        }
    }
}

fn walk_down_git(dir: &Path, depth: usize, out: &mut BTreeSet<PathBuf>) {
    if depth > MAX_FIND_DEPTH || is_virtual_fs(dir) {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let path = entry.path();
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_symlink() {
            continue;
        }
        if name_str == ".git" {
            if let Some(parent) = path.parent() {
                out.insert(normalize_path(parent));
            }
            continue;
        }
        if !ft.is_dir() {
            continue;
        }
        if should_prune_path(&path) {
            continue;
        }
        walk_down_git(&path, depth + 1, out);
    }
}

fn path_is_under(child: &Path, parent: &Path) -> bool {
    child == parent || child.starts_with(parent)
}

fn containing_root<'a>(path: &'a Path, roots: &'a BTreeSet<PathBuf>) -> Option<&'a PathBuf> {
    roots
        .iter()
        .filter(|root| path_is_under(path, root))
        .max_by_key(|root| root.as_os_str().len())
}

fn roots_under<'a>(
    dir: &'a Path,
    roots: &'a BTreeSet<PathBuf>,
) -> impl Iterator<Item = &'a PathBuf> {
    roots.range(dir.to_path_buf()..).take_while(move |p| path_is_under(p, dir))
}

fn rel_key(path: &Path, repo: &Path) -> Option<String> {
    let rel = path.strip_prefix(repo).ok()?;
    if rel.as_os_str().is_empty() {
        return Some(String::new());
    }
    Some(rel.to_string_lossy().replace('\\', "/"))
}

fn untracked_ancestor(files: &BTreeMap<String, GitDecor>, rel: &str) -> Option<GitDecor> {
    if rel.is_empty() {
        return None;
    }
    let mut cur = rel;
    while let Some((parent, _)) = cur.rsplit_once('/') {
        if files.get(parent) == Some(&GitDecor::Untracked) {
            return Some(GitDecor::Untracked);
        }
        cur = parent;
    }
    None
}

fn file_state(status: &RepoStatus, rel: &str) -> Option<GitDecor> {
    GitDecor::merge_opt(status.files.get(rel).copied(), untracked_ancestor(&status.files, rel))
}

fn dir_state(status: &RepoStatus, rel: &str) -> Option<GitDecor> {
    if rel.is_empty() {
        return status.aggregate;
    }
    let mut worst = status.files.get(rel).copied();
    worst = GitDecor::merge_opt(worst, untracked_ancestor(&status.files, rel));
    let prefix = format!("{rel}/");
    for (key, decor) in status.files.range(prefix.clone()..) {
        if !key.starts_with(&prefix) {
            break;
        }
        worst = GitDecor::merge_opt(worst, Some(*decor));
        if worst == Some(GitDecor::Conflict) {
            break;
        }
    }
    worst
}

/// Color for a local path. `is_dir` includes folders that contain nested repos.
pub fn state_for_path(path: &Path, is_dir: bool) -> Option<GitDecor> {
    let path = normalize_path(path);
    if is_virtual_fs(&path) {
        return None;
    }
    let Ok(cache) = cache().lock() else {
        return None;
    };
    let mut worst = None;
    if let Some(repo) = containing_root(&path, &cache.roots) {
        if let Some(status) = cache.statuses.get(repo) {
            if let Some(rel) = rel_key(&path, repo) {
                let here = if is_dir || rel.is_empty() {
                    dir_state(status, &rel)
                } else {
                    file_state(status, &rel)
                };
                worst = GitDecor::merge_opt(worst, here);
            }
        }
    }
    if is_dir {
        for repo in roots_under(&path, &cache.roots) {
            if let Some(status) = cache.statuses.get(repo) {
                worst = GitDecor::merge_opt(worst, status.aggregate);
            }
        }
    }
    worst
}

/// Drop porcelain so the next scan re-runs `git status` (find index is kept).
pub fn invalidate_under(path: &Path) {
    let path = normalize_path(path);
    let Ok(mut cache) = cache().lock() else {
        return;
    };
    cache.statuses.retain(|repo, _| {
        !(path_is_under(&path, repo) || path_is_under(repo, &path))
    });
}

fn ensure_roots_loaded(cache: &mut GitCache) {
    if cache.roots_loaded {
        return;
    }
    load_roots_from_disk(&mut cache.roots);
    cache.roots_loaded = true;
}

fn relevant_repos(cwd: &Path, roots: &BTreeSet<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(c) = containing_root(cwd, roots) {
        out.push(c.clone());
    }
    for r in roots_under(cwd, roots) {
        if !out.iter().any(|x| x == r) {
            out.push(r.clone());
        }
    }
    out
}

fn status_fresh(status: &RepoStatus) -> bool {
    status.scanned_at.elapsed() < STATUS_TTL
}

fn run_git_status(repo: &Path) -> Option<RepoStatus> {
    let output = Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "--no-pager", "status", "--porcelain=v1"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "1")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut files: BTreeMap<String, GitDecor> = BTreeMap::new();
    let mut aggregate = None;
    for line in stdout.lines() {
        let Some((decor, rel)) = parse_porcelain_line(line) else {
            continue;
        };
        if rel.is_empty() {
            continue;
        }
        let merged = files
            .get(&rel)
            .copied()
            .map(|old| old.merge(decor))
            .unwrap_or(decor);
        files.insert(rel, merged);
        aggregate = GitDecor::merge_opt(aggregate, Some(decor));
    }
    Some(RepoStatus {
        files,
        aggregate,
        scanned_at: Instant::now(),
    })
}

fn parse_porcelain_line(line: &str) -> Option<(GitDecor, String)> {
    if line.len() < 3 {
        return None;
    }
    let xy = &line[..2];
    let rest = line.get(3..).unwrap_or("").trim_start();
    let decor = classify_xy(xy)?;
    let path_part = if xy.contains('R') || xy.contains('C') {
        rest.rsplit_once(" -> ").map(|(_, p)| p).unwrap_or(rest)
    } else {
        rest
    };
    let rel = unquote_git_path(path_part);
    if rel.is_empty() {
        return None;
    }
    Some((decor, rel))
}

fn classify_xy(xy: &str) -> Option<GitDecor> {
    let bytes = xy.as_bytes();
    if bytes.len() != 2 {
        return None;
    }
    let a = bytes[0] as char;
    let b = bytes[1] as char;
    if a == 'U' || b == 'U' || xy == "AA" || xy == "DD" {
        return Some(GitDecor::Conflict);
    }
    if xy == "??" || xy == "!!" {
        return if xy == "??" {
            Some(GitDecor::Untracked)
        } else {
            None
        };
    }
    let added = a == 'A' || b == 'A';
    let changed = matches!(a, 'M' | 'D' | 'R' | 'T' | 'C') || matches!(b, 'M' | 'D' | 'R' | 'T' | 'C');
    if added && changed {
        return Some(GitDecor::Modified);
    }
    if added {
        return Some(GitDecor::Untracked);
    }
    if changed || a != ' ' || b != ' ' {
        return Some(GitDecor::Modified);
    }
    None
}

fn unquote_git_path(s: &str) -> String {
    let s = s.trim();
    let raw = if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        unescape_git(&s[1..s.len() - 1])
    } else {
        s.to_string()
    };
    raw.trim_end_matches('/').to_string()
}

fn unescape_git(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(o) => out.push(o),
                None => break,
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn refresh_statuses(cwd: &Path) {
    let repos = {
        let Ok(mut cache) = cache().lock() else {
            return;
        };
        ensure_roots_loaded(&mut cache);
        relevant_repos(cwd, &cache.roots)
    };
    for repo in repos {
        let skip = {
            let Ok(cache) = cache().lock() else {
                continue;
            };
            cache.statuses.get(&repo).is_some_and(status_fresh)
        };
        if skip {
            continue;
        }
        if let Some(status) = run_git_status(&repo) {
            if let Ok(mut cache) = cache().lock() {
                cache.statuses.insert(repo, status);
            }
        }
    }
}

fn discover_under(cwd: &Path, force: bool) {
    if skip_downward_find(cwd) {
        return;
    }
    let due = {
        let Ok(mut cache) = cache().lock() else {
            return;
        };
        ensure_roots_loaded(&mut cache);
        force
            || cache
                .last_find
                .get(cwd)
                .map(|t| t.elapsed() >= FIND_TTL)
                .unwrap_or(true)
    };
    if !due {
        return;
    }
    let mut found = BTreeSet::new();
    walk_down_git(cwd, 0, &mut found);
    let Ok(mut cache) = cache().lock() else {
        return;
    };
    cache.roots.extend(found);
    cache.last_find.insert(cwd.to_path_buf(), Instant::now());
    persist_roots(&cache.roots);
}

enum ScanMsg {
    Tick,
    Done,
}

/// Background discover + `git status` for `cwd`. `on_progress` runs on the GTK
/// thread whenever the cache has new data (and when the scan finishes).
pub fn request_scan(
    cwd: PathBuf,
    generation: u64,
    latest: std::sync::Arc<AtomicU64>,
    discover: bool,
    force: bool,
    on_progress: impl Fn() + 'static,
) {
    if is_virtual_fs(&cwd) {
        return;
    }
    let (tx, rx) = mpsc::channel::<ScanMsg>();
    thread::spawn(move || {
        let cwd = normalize_path(&cwd);
        if let Some(repo) = walk_up_git(&cwd) {
            if let Ok(mut cache) = cache().lock() {
                ensure_roots_loaded(&mut cache);
                cache.roots.insert(repo);
            }
        } else if let Ok(mut cache) = cache().lock() {
            ensure_roots_loaded(&mut cache);
        }
        refresh_statuses(&cwd);
        let _ = tx.send(ScanMsg::Tick);
        if discover {
            discover_under(&cwd, force);
            refresh_statuses(&cwd);
            let _ = tx.send(ScanMsg::Tick);
        }
        let _ = tx.send(ScanMsg::Done);
    });

    let on_progress = std::rc::Rc::new(on_progress);
    glib::timeout_add_local(Duration::from_millis(40), move || {
        let mut got = false;
        loop {
            match rx.try_recv() {
                Ok(ScanMsg::Tick) => got = true,
                Ok(ScanMsg::Done) => {
                    if latest.load(AtomicOrdering::Relaxed) == generation {
                        on_progress();
                    }
                    return glib::ControlFlow::Break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if latest.load(AtomicOrdering::Relaxed) == generation {
                        on_progress();
                    }
                    return glib::ControlFlow::Break;
                }
            }
        }
        if got && latest.load(AtomicOrdering::Relaxed) == generation {
            on_progress();
        }
        glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_conflict_untracked_modified() {
        assert_eq!(classify_xy("UU"), Some(GitDecor::Conflict));
        assert_eq!(classify_xy("AA"), Some(GitDecor::Conflict));
        assert_eq!(classify_xy("DU"), Some(GitDecor::Conflict));
        assert_eq!(classify_xy("??"), Some(GitDecor::Untracked));
        assert_eq!(classify_xy("A "), Some(GitDecor::Untracked));
        assert_eq!(classify_xy("AM"), Some(GitDecor::Modified));
        assert_eq!(classify_xy(" M"), Some(GitDecor::Modified));
        assert_eq!(classify_xy("M "), Some(GitDecor::Modified));
        assert_eq!(classify_xy("D "), Some(GitDecor::Modified));
        assert_eq!(classify_xy("!!"), None);
    }

    #[test]
    fn parse_rename_and_untracked_dir() {
        let (d, p) = parse_porcelain_line("?? newdir/").unwrap();
        assert_eq!(d, GitDecor::Untracked);
        assert_eq!(p, "newdir");
        let (d, p) = parse_porcelain_line("R  old.rs -> src/new.rs").unwrap();
        assert_eq!(d, GitDecor::Modified);
        assert_eq!(p, "src/new.rs");
    }

    #[test]
    fn worst_wins() {
        assert_eq!(
            GitDecor::Untracked.merge(GitDecor::Modified),
            GitDecor::Modified
        );
        assert_eq!(
            GitDecor::Modified.merge(GitDecor::Conflict),
            GitDecor::Conflict
        );
    }
}
