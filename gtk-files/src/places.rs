//! Favorites, bookmarks, and recent folders — all in `places.toml`.

use std::fs;
use std::path::{Path, PathBuf};

use gtk4::gio;
use gtk4::prelude::*;
use serde::{Deserialize, Serialize};

use crate::util::home_dir;

const MAX_RECENT: usize = 20;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PlacesData {
    pub favorites: Vec<PathBuf>,
    pub recent_folders: Vec<PathBuf>,
    /// gtk-files-only bookmarks (not shared with Nautilus / gtk-3.0 bookmarks).
    pub bookmarks: Vec<Bookmark>,
    /// Remembered remote servers (SFTP / FTP / SMB / …) for Connect to Network.
    #[serde(default)]
    pub network_connections: Vec<NetworkConnection>,
    /// One-time import from `~/.config/gtk-3.0/bookmarks` completed.
    pub bookmarks_imported_from_gtk: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    /// Local filesystem path when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Original URI (kept for remote bookmarks like sftp://).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConnection {
    pub uri: String,
    #[serde(default)]
    pub label: String,
}

fn places_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| home_dir().join(".config"))
        .join("gtk-apps")
        .join("gtk-files")
        .join("places.toml")
}

fn legacy_gtk_bookmark_paths() -> [PathBuf; 2] {
    let home = home_dir();
    [
        home.join(".config/gtk-3.0/bookmarks"),
        home.join(".config/gtk-4.0/bookmarks"),
    ]
}

pub fn load() -> PlacesData {
    let path = places_path();
    let mut data: PlacesData = match fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_default(),
        Err(_) => PlacesData::default(),
    };
    data.favorites.retain(|p| p.exists());
    data.recent_folders.retain(|p| p.exists());
    data.bookmarks.retain(|b| match (&b.path, &b.uri) {
        (Some(p), _) if p.exists() => true,
        // Keep unmounted / remote entries that still have a URI.
        (_, Some(_)) => true,
        _ => false,
    });
    data.network_connections
        .retain(|c| !c.uri.trim().is_empty());
    // Seed remembered remotes from URI-only bookmarks once if empty.
    if data.network_connections.is_empty() {
        for bm in &data.bookmarks {
            if let Some(uri) = &bm.uri {
                if is_remote_uri(uri) {
                    data.network_connections.push(NetworkConnection {
                        uri: uri.clone(),
                        label: if bm.label.is_empty() {
                            uri.clone()
                        } else {
                            bm.label.clone()
                        },
                    });
                }
            }
        }
        if !data.network_connections.is_empty() {
            save(&data);
        }
    }

    if !data.bookmarks_imported_from_gtk {
        let migrated = import_legacy_gtk_bookmarks();
        if !migrated.is_empty() {
            for bm in migrated {
                if !data.bookmarks.iter().any(|e| bookmarks_equal(e, &bm)) {
                    data.bookmarks.push(bm);
                }
            }
        }
        data.bookmarks_imported_from_gtk = true;
        save(&data);
    }

    data
}

pub fn save(data: &PlacesData) {
    let path = places_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = toml::to_string_pretty(data) {
        let _ = fs::write(path, text);
    }
}

fn normalize(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn bookmarks_equal(a: &Bookmark, b: &Bookmark) -> bool {
    if let (Some(ap), Some(bp)) = (&a.path, &b.path) {
        if normalize(ap) == normalize(bp) {
            return true;
        }
    }
    match (&a.uri, &b.uri) {
        (Some(au), Some(bu)) => au == bu,
        _ => false,
    }
}

/// One-shot read of shared GTK bookmark files (does not modify them).
fn import_legacy_gtk_bookmarks() -> Vec<Bookmark> {
    let mut out = Vec::new();
    for path in legacy_gtk_bookmark_paths() {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut parts = line.splitn(2, char::is_whitespace);
            let uri = parts.next().unwrap_or("").trim();
            if uri.is_empty() {
                continue;
            }
            // Dedupe by URI so symlinked folders (e.g. Documents → SORT) stay distinct.
            if out
                .iter()
                .any(|e: &Bookmark| e.uri.as_deref() == Some(uri))
            {
                continue;
            }
            let display = parts.next().map(str::trim).filter(|s| !s.is_empty());
            let file = gio::File::for_uri(uri);
            // Prefer the path as written (not canonicalize) so XDG dirs keep identity.
            let local = file.path();
            let label = display
                .map(|s| s.to_string())
                .or_else(|| {
                    local
                        .as_ref()
                        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                })
                .unwrap_or_else(|| uri.to_string());
            out.push(Bookmark {
                path: local,
                uri: Some(uri.to_string()),
                label,
            });
        }
    }
    out
}

pub fn add_favorite(path: &Path) -> bool {
    let canon = normalize(path);
    if !canon.exists() {
        return false;
    }
    let mut data = load();
    if data.favorites.iter().any(|p| p == &canon) {
        return false;
    }
    data.favorites.push(canon);
    save(&data);
    true
}

pub fn remove_favorite(path: &Path) {
    let mut data = load();
    data.favorites
        .retain(|p| p != path && normalize(p) != normalize(path));
    save(&data);
}

#[allow(dead_code)]
pub fn is_favorite(path: &Path) -> bool {
    let canon = normalize(path);
    load().favorites.iter().any(|p| p == &canon || p == path)
}

/// Record a folder that was visited or contained an opened file.
///
/// Returns `true` when the Recent list changed (caller should rebuild the
/// sidebar). Returns `false` when the folder was already the most-recent entry
/// so we can avoid a rebuild → re-select → navigate loop.
pub fn record_recent_folder(path: &Path) -> bool {
    let canon = normalize(path);
    if !canon.is_dir() {
        return false;
    }
    let mut data = load();
    if data.recent_folders.first() == Some(&canon) {
        return false;
    }
    data.recent_folders.retain(|p| p != &canon);
    data.recent_folders.insert(0, canon);
    data.recent_folders.truncate(MAX_RECENT);
    save(&data);
    true
}

/// When a file is opened, remember its parent folder.
/// Returns whether the Recent list changed (same as [`record_recent_folder`]).
pub fn record_recent_for_file(path: &Path) -> bool {
    path.parent()
        .map(|parent| record_recent_folder(parent))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Bookmarks (gtk-files places.toml only)
// ---------------------------------------------------------------------------

pub fn load_bookmarks() -> Vec<Bookmark> {
    load().bookmarks
}

pub fn add_bookmark(path: &Path) -> bool {
    let canon = normalize(path);
    if !canon.exists() {
        return false;
    }
    let mut data = load();
    let label = canon
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| canon.display().to_string());
    let uri = gio::File::for_path(&canon).uri().to_string();
    let bm = Bookmark {
        path: Some(canon),
        uri: Some(uri),
        label,
    };
    if data.bookmarks.iter().any(|e| bookmarks_equal(e, &bm)) {
        return false;
    }
    data.bookmarks.push(bm);
    save(&data);
    true
}

/// Bookmark a remote URI (SFTP / FTP / SMB / …) for the sidebar.
pub fn add_bookmark_uri(uri: &str, label: &str) -> bool {
    let uri = uri.trim();
    if uri.is_empty() {
        return false;
    }
    let label = {
        let t = label.trim();
        if t.is_empty() {
            uri.to_string()
        } else {
            t.to_string()
        }
    };
    let bm = Bookmark {
        path: None,
        uri: Some(uri.to_string()),
        label,
    };
    let mut data = load();
    if data.bookmarks.iter().any(|e| bookmarks_equal(e, &bm)) {
        return false;
    }
    data.bookmarks.push(bm);
    save(&data);
    true
}

fn is_remote_uri(uri: &str) -> bool {
    let lower = uri.to_ascii_lowercase();
    lower.starts_with("sftp://")
        || lower.starts_with("ssh://")
        || lower.starts_with("ftp://")
        || lower.starts_with("ftps://")
        || lower.starts_with("smb://")
        || lower.starts_with("dav://")
        || lower.starts_with("davs://")
        || lower.starts_with("nfs://")
}

pub fn load_network_connections() -> Vec<NetworkConnection> {
    load().network_connections
}

/// Remember a remote connection (most-recent first). Always updates label if URI exists.
pub fn remember_network_connection(uri: &str, label: &str) -> bool {
    let uri = uri.trim();
    if uri.is_empty() || !is_remote_uri(uri) {
        return false;
    }
    let label = {
        let t = label.trim();
        if t.is_empty() {
            uri.to_string()
        } else {
            t.to_string()
        }
    };
    let mut data = load();
    data.network_connections.retain(|c| c.uri != uri);
    data.network_connections.insert(
        0,
        NetworkConnection {
            uri: uri.to_string(),
            label,
        },
    );
    // Cap list length.
    data.network_connections.truncate(30);
    save(&data);
    true
}

pub fn forget_network_connection(uri: &str) -> bool {
    let mut data = load();
    let before = data.network_connections.len();
    data.network_connections.retain(|c| c.uri != uri);
    let removed = data.network_connections.len() != before;
    if removed {
        save(&data);
    }
    removed
}

#[allow(dead_code)]
pub fn is_bookmark(path: &Path) -> bool {
    let canon = normalize(path);
    load_bookmarks().iter().any(|b| {
        b.path
            .as_ref()
            .map(|p| p == &canon || p == path || normalize(p) == canon)
            .unwrap_or(false)
    })
}

pub fn remove_bookmark(path: &Path) -> bool {
    let canon = normalize(path);
    let mut data = load();
    let before = data.bookmarks.len();
    data.bookmarks.retain(|b| {
        let path_match = b
            .path
            .as_ref()
            .map(|p| p == &canon || p == path || normalize(p) == canon)
            .unwrap_or(false);
        !path_match
    });
    let removed = data.bookmarks.len() != before;
    if removed {
        save(&data);
    }
    removed
}

pub fn remove_bookmark_uri(uri: &str) -> bool {
    let mut data = load();
    let before = data.bookmarks.len();
    data.bookmarks
        .retain(|b| b.uri.as_deref() != Some(uri));
    let removed = data.bookmarks.len() != before;
    if removed {
        save(&data);
    }
    removed
}

/// Format paths/names as a space-separated quoted list:
/// `"name one" "name two"`
pub fn quoted_list(items: impl IntoIterator<Item = String>) -> String {
    items
        .into_iter()
        .map(|s| format!("\"{}\"", s.replace('\"', "\\\"")))
        .collect::<Vec<_>>()
        .join(" ")
}
