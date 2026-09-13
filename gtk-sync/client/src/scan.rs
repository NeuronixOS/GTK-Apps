//! Local tree scan with a mtime/size hash cache so large sync roots are not
//! SHA-256'd from scratch every 10 seconds.

use mimic_core::{content_hash_path, is_ignored, is_ignored_dir_name};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct HashCache {
    files: HashMap<String, CacheEnt>,
}

#[derive(Clone, Serialize, Deserialize)]
struct CacheEnt {
    mtime: u64,
    size: u64,
    hash: String,
}

fn cache_path() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gtk-sync")
        .join("file-hashes.json")
}

impl HashCache {
    pub fn load() -> Self {
        std::fs::read_to_string(cache_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let p = cache_path();
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(self) {
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(tmp, p);
            }
        }
    }
}

/// Relative path → (mtime, sha256). Skips venvs, `.git`, symlinks, etc.
pub fn local_file_map(
    root: &Path,
    cache: &mut HashCache,
) -> anyhow::Result<HashMap<String, (u64, String)>> {
    let mut map = HashMap::new();
    let mut seen = HashSet::new();
    let walker = WalkDir::new(root).into_iter().filter_entry(|e| {
        if e.depth() == 0 {
            return true;
        }
        let name = e.file_name().to_str().unwrap_or("");
        !is_ignored_dir_name(name)
    });
    for entry in walker.filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() || entry.path_is_symlink() {
            continue;
        }
        let Some(rel) = mimic_core::index::relative_path(root, entry.path()) else {
            continue;
        };
        if is_ignored(&rel) {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let size = meta.len();
        let hash = match cache.files.get(&rel) {
            Some(ent) if ent.mtime == mtime && ent.size == size => ent.hash.clone(),
            _ => match content_hash_path(entry.path()) {
                Ok(h) => {
                    cache.files.insert(
                        rel.clone(),
                        CacheEnt {
                            mtime,
                            size,
                            hash: h.clone(),
                        },
                    );
                    h
                }
                Err(e) => {
                    tracing::warn!("hash {rel}: {e}");
                    continue;
                }
            },
        };
        seen.insert(rel.clone());
        map.insert(rel, (mtime, hash));
    }
    cache.files.retain(|k, _| seen.contains(k));
    Ok(map)
}
