//! Archive extract / compress helpers (zip, tar, gzip, … via CLI tools).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use gtk4 as gtk;
use gtk::prelude::*;

use crate::util::{show_error, uniquify_path};

const ARCHIVE_EXTS: &[&str] = &[
    "zip", "tar", "gz", "tgz", "bz2", "tbz", "tbz2", "xz", "txz", "zst", "tzst", "7z", "rar",
    "tar.gz", "tar.bz2", "tar.xz", "tar.zst",
];

/// True when the path looks like a common archive by extension.
pub fn is_archive(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    for ext in ARCHIVE_EXTS {
        if name.ends_with(&format!(".{ext}")) {
            return true;
        }
    }
    false
}

fn which(bin: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {bin} >/dev/null 2>&1")])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn archive_kind(path: &Path) -> Option<&'static str> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Some("tar")
    } else if name.ends_with(".tar.bz2") || name.ends_with(".tbz") || name.ends_with(".tbz2") {
        Some("tar")
    } else if name.ends_with(".tar.xz") || name.ends_with(".txz") {
        Some("tar")
    } else if name.ends_with(".tar.zst") || name.ends_with(".tzst") {
        Some("tar")
    } else if name.ends_with(".tar") {
        Some("tar")
    } else if name.ends_with(".zip") {
        Some("zip")
    } else if name.ends_with(".7z") {
        Some("7z")
    } else if name.ends_with(".rar") {
        Some("rar")
    } else if name.ends_with(".gz") || name.ends_with(".bz2") || name.ends_with(".xz") || name.ends_with(".zst")
    {
        // Single-file compressions — treat via tar -xaf when possible, else gunzip/etc.
        Some("tar")
    } else {
        None
    }
}

fn run_checked(mut cmd: Command, label: &str) -> Result<(), String> {
    let output = cmd
        .output()
        .map_err(|e| format!("{label} failed to start: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let msg = stderr.trim();
    if !msg.is_empty() {
        return Err(format!("{label}: {msg}"));
    }
    let msg = stdout.trim();
    if !msg.is_empty() {
        return Err(format!("{label}: {msg}"));
    }
    Err(format!("{label} failed"))
}

/// `folder` already present → `folder(1)`, then `folder(2)`, …
fn uniquify_extract_name(dir: &Path, name: &str) -> PathBuf {
    let direct = dir.join(name);
    if !direct.exists() {
        return direct;
    }
    for i in 1..10_000 {
        let candidate = dir.join(format!("{name}({i})"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{name}-copy"))
}

fn copy_recursive(from: &Path, to: &Path) -> Result<(), String> {
    if to.exists() {
        return Err(format!("refusing to overwrite {}", to.display()));
    }
    if from.is_dir() {
        std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
        for entry in std::fs::read_dir(from).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::copy(from, to)
            .map_err(|e| format!("copy {} → {}: {e}", from.display(), to.display()))?;
        Ok(())
    }
}

fn move_entry(from: &Path, to: &Path) -> Result<(), String> {
    let to = if to.exists() {
        match (to.parent(), to.file_name()) {
            (Some(parent), Some(name)) => uniquify_extract_name(parent, &name.to_string_lossy()),
            _ => to.to_path_buf(),
        }
    } else {
        to.to_path_buf()
    };
    if to.exists() {
        return Err(format!("refusing to overwrite {}", to.display()));
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    match std::fs::rename(from, &to) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_recursive(from, &to)?;
            if from.is_dir() {
                std::fs::remove_dir_all(from).map_err(|e| e.to_string())?;
            } else {
                std::fs::remove_file(from).map_err(|e| e.to_string())?;
            }
            Ok(())
        }
    }
}

fn skip_extract_name(name: &str) -> bool {
    name == "__MACOSX"
        || name == ".DS_Store"
        || name.starts_with(".gtk-files-x-")
}

fn relocate_extracted(tmp: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let mut moved = 0usize;
    for entry in std::fs::read_dir(tmp).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if skip_extract_name(&name_str) {
            continue;
        }
        let to = uniquify_extract_name(dest, &name_str);
        move_entry(&entry.path(), &to)?;
        moved += 1;
    }
    if moved == 0 {
        return Err("archive contained no files to extract".into());
    }
    Ok(())
}

/// Unpack `src` into `dest`, renaming top-level items that already exist
/// (`folder` → `folder(1)` → `folder(2)`).
fn extract_into_dest_uniquified(src: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let dest = dest
        .canonicalize()
        .map_err(|e| format!("{}: {e}", dest.display()))?;
    let src = src
        .canonicalize()
        .map_err(|e| format!("{}: {e}", src.display()))?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Same filesystem as `dest` so rename() works ( /tmp is often tmpfs → EXDEV ).
    let tmp = dest.join(format!(".gtk-files-x-{}-{nanos}", std::process::id()));
    if tmp.exists() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    let result = (|| {
        extract_one(&src, &tmp)?;
        relocate_extracted(&tmp, &dest)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

fn extract_one(src: &Path, dest_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;
    let dest_dir = dest_dir
        .canonicalize()
        .map_err(|e| format!("{}: {e}", dest_dir.display()))?;
    // Always run from `dest_dir` so a tool that ignores -d/-C cannot spill into
    // the file manager's cwd (and overwrite `folder` on Extract Here).
    match archive_kind(src).unwrap_or("tar") {
        "zip" => {
            if !which("unzip") {
                return Err("unzip is not installed".into());
            }
            let mut cmd = Command::new("unzip");
            cmd.current_dir(&dest_dir);
            cmd.args(["-o", "-q", "-d", "."]).arg(src);
            run_checked(cmd, "unzip")
        }
        "7z" => {
            if !which("7z") && !which("7za") {
                return Err("7z is not installed".into());
            }
            let bin = if which("7z") { "7z" } else { "7za" };
            let mut cmd = Command::new(bin);
            cmd.current_dir(&dest_dir);
            cmd.args(["x", "-y", "-o."]).arg(src);
            run_checked(cmd, bin)
        }
        "rar" => {
            if !which("unrar") {
                return Err("unrar is not installed".into());
            }
            let mut cmd = Command::new("unrar");
            cmd.current_dir(&dest_dir);
            cmd.args(["x", "-o+"]).arg(src).arg("./");
            run_checked(cmd, "unrar")
        }
        _ => {
            if !which("tar") {
                return Err("tar is not installed".into());
            }
            let mut cmd = Command::new("tar");
            cmd.current_dir(&dest_dir);
            // -a: auto-compress from suffix; -x extract; -f file; -C dest
            cmd.args(["-xaf"]).arg(src).args(["-C", "."]);
            run_checked(cmd, "tar")
        }
    }
}

/// Extract each archive directly into its parent directory (Extract Here).
pub fn extract_here(parent: Option<&impl IsA<gtk::Window>>, paths: &[PathBuf]) -> usize {
    let archives: Vec<&Path> = paths
        .iter()
        .map(PathBuf::as_path)
        .filter(|p| is_archive(p))
        .collect();
    if archives.is_empty() {
        show_error(parent, "Extract Here", "Select one or more archive files");
        return 0;
    }

    let mut ok = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for src in archives {
        let dest = src.parent().unwrap_or_else(|| Path::new("."));
        match extract_into_dest_uniquified(src, dest) {
            Ok(()) => ok += 1,
            Err(e) => errors.push(format!("{}: {e}", src.display())),
        }
    }

    if !errors.is_empty() {
        let detail = if errors.len() == 1 {
            errors[0].clone()
        } else {
            format!(
                "{} archive(s) extracted, {} failed:\n{}",
                ok,
                errors.len(),
                errors.join("\n")
            )
        };
        show_error(parent, "Extract Here", &detail);
    }
    ok
}

#[derive(Debug, Clone, Copy)]
pub enum CompressFormat {
    Zip,
    TarGz,
}

impl CompressFormat {
    pub fn action_name(self) -> &'static str {
        match self {
            Self::Zip => "compress-zip",
            Self::TarGz => "compress-tar-gz",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Zip => "Compress as ZIP…",
            Self::TarGz => "Compress as tar.gz…",
        }
    }

    fn default_name(self, paths: &[PathBuf]) -> String {
        let base = if paths.len() == 1 {
            paths[0]
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "archive".into())
        } else {
            "archive".into()
        };
        match self {
            Self::Zip => format!("{base}.zip"),
            Self::TarGz => format!("{base}.tar.gz"),
        }
    }
}

fn compress_one(paths: &[PathBuf], dest: &Path, format: CompressFormat) -> Result<(), String> {
    if paths.is_empty() {
        return Err("Nothing to compress".into());
    }
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    // Relative names keep the archive layout tidy.
    let names: Vec<String> = paths
        .iter()
        .map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string())
        })
        .collect();

    match format {
        CompressFormat::Zip => {
            if !which("zip") {
                return Err("zip is not installed".into());
            }
            let mut cmd = Command::new("zip");
            cmd.current_dir(parent);
            cmd.args(["-r", "-q"]);
            cmd.arg(dest.file_name().unwrap_or_default());
            for n in &names {
                cmd.arg(n);
            }
            run_checked(cmd, "zip")
        }
        CompressFormat::TarGz => {
            if !which("tar") {
                return Err("tar is not installed".into());
            }
            let mut cmd = Command::new("tar");
            cmd.current_dir(parent);
            cmd.args(["-czf"]);
            cmd.arg(dest.file_name().unwrap_or_default());
            for n in &names {
                cmd.arg(n);
            }
            run_checked(cmd, "tar")
        }
    }
}

/// Compress selected files/folders into a new archive next to them.
pub fn compress_selection(
    parent: Option<&impl IsA<gtk::Window>>,
    paths: &[PathBuf],
    format: CompressFormat,
) -> usize {
    if paths.is_empty() {
        show_error(parent, format.label(), "Select files or folders to compress");
        return 0;
    }

    // All items must share a parent so relative paths work.
    let Some(first_parent) = paths[0].parent().map(Path::to_path_buf) else {
        show_error(parent, format.label(), "Invalid selection");
        return 0;
    };
    if paths
        .iter()
        .any(|p| p.parent().map(Path::to_path_buf).as_ref() != Some(&first_parent))
    {
        show_error(
            parent,
            format.label(),
            "Select items in the same folder to compress together",
        );
        return 0;
    }

    let name = format.default_name(paths);
    let dest = uniquify_path(&first_parent, &name);
    match compress_one(paths, &dest, format) {
        Ok(()) => 1,
        Err(e) => {
            show_error(parent, format.label(), &e);
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_zip_with_folder(root: &Path) -> PathBuf {
        let src = root.join("src");
        std::fs::create_dir_all(src.join("folder")).unwrap();
        std::fs::write(src.join("folder/f.txt"), "hi").unwrap();
        let zip = root.join("test.zip");
        let status = Command::new("zip")
            .current_dir(&src)
            .args(["-r", "-q"])
            .arg(&zip)
            .arg("folder")
            .status()
            .unwrap();
        assert!(status.success());
        zip
    }

    #[test]
    fn extract_here_uniquifies_existing_top_level_folder() {
        let root = std::env::temp_dir().join(format!(
            "gtk-files-extract-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let zip = make_zip_with_folder(&root);

        extract_into_dest_uniquified(&zip, &root).unwrap();
        assert!(root.join("folder/f.txt").is_file());

        extract_into_dest_uniquified(&zip, &root).unwrap();
        assert!(root.join("folder(1)/f.txt").is_file());
        assert_eq!(std::fs::read_to_string(root.join("folder/f.txt")).unwrap(), "hi");

        extract_into_dest_uniquified(&zip, &root).unwrap();
        assert!(root.join("folder(2)/f.txt").is_file());

        let _ = std::fs::remove_dir_all(&root);
    }
}
