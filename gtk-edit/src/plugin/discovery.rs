//! Discover `.plugin` desktop-style metadata and optional `cdylib` paths.

use std::fs;
use std::path::{Path, PathBuf};

use super::activatable::PluginInfo;

#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    pub info: PluginInfo,
    pub library_path: Option<PathBuf>,
    pub meta_path: PathBuf,
}

/// Parse a simple key=value .plugin / .desktop style file.
pub fn parse_plugin_file(path: &Path) -> Option<DiscoveredPlugin> {
    let text = fs::read_to_string(path).ok()?;
    let mut module = String::new();
    let mut name = String::new();
    let mut description = String::new();
    let mut authors = String::new();
    let mut copyright = String::new();
    let mut website = String::new();
    let mut library = None::<String>;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim().to_string();
        match k.trim() {
            "Module" => module = v,
            "Name" => name = v,
            "Description" => description = v,
            "Authors" => authors = v,
            "Copyright" => copyright = v,
            "Website" => website = v,
            "Library" => library = Some(v),
            _ => {}
        }
    }

    if module.is_empty() {
        module = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
    }
    if name.is_empty() {
        name = module.clone();
    }

    let library_path = library.map(|lib| {
        let p = PathBuf::from(&lib);
        if p.is_absolute() {
            p
        } else {
            path.parent().unwrap_or(Path::new(".")).join(p)
        }
    });

    Some(DiscoveredPlugin {
        info: PluginInfo {
            module,
            name,
            description,
            authors,
            copyright,
            website,
            builtin: false,
        },
        library_path,
        meta_path: path.to_path_buf(),
    })
}

pub fn scan_plugin_dir(dir: &Path) -> Vec<DiscoveredPlugin> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Look for *.plugin inside subdirectory
            if let Ok(inner) = fs::read_dir(&path) {
                for e in inner.flatten() {
                    let p = e.path();
                    if p.extension().and_then(|x| x.to_str()) == Some("plugin") {
                        if let Some(d) = parse_plugin_file(&p) {
                            out.push(d);
                        }
                    }
                }
            }
        } else if path.extension().and_then(|x| x.to_str()) == Some("plugin") {
            if let Some(d) = parse_plugin_file(&path) {
                out.push(d);
            }
        }
    }
    out
}
