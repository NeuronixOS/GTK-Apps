//! Create new files from `~/Templates` (XDG templates directory).

use std::fs;
use std::path::{Path, PathBuf};

use crate::util::{home_dir, uniquify_path};

/// Resolve the templates directory (`$HOME/Templates`, following symlinks).
pub fn templates_dir() -> PathBuf {
    let dir = home_dir().join("Templates");
    fs::canonicalize(&dir).unwrap_or(dir)
}

/// Template files in `~/Templates` (sorted by display name).
pub fn list_templates() -> Vec<PathBuf> {
    let dir = templates_dir();
    let mut entries: Vec<PathBuf> = match fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect(),
        Err(_) => return Vec::new(),
    };
    entries.sort_by(|a, b| {
        let an = a.file_name().map(|n| n.to_ascii_lowercase());
        let bn = b.file_name().map(|n| n.to_ascii_lowercase());
        an.cmp(&bn)
    });
    entries
}

/// Copy `template` into `dest_dir`, uniquifying the name if needed.
pub fn create_from_template(template: &Path, dest_dir: &Path) -> Result<PathBuf, String> {
    if !template.is_file() {
        return Err(format!("Template not found: {}", template.display()));
    }
    if !dest_dir.is_dir() {
        return Err(format!("Not a folder: {}", dest_dir.display()));
    }
    let name = template
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| "Invalid template name".to_string())?;
    let dest = uniquify_path(dest_dir, &name);
    fs::copy(template, &dest).map_err(|e| e.to_string())?;
    // Preserve executable bit etc. from the template.
    if let Ok(meta) = fs::metadata(template) {
        let _ = fs::set_permissions(&dest, meta.permissions());
    }
    Ok(dest)
}
