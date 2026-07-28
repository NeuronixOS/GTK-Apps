//! Context-menu Scripts (Nautilus-style convert helpers via ImageMagick).

use std::path::{Path, PathBuf};
use std::process::Command;

use gtk4 as gtk;
use gtk::prelude::*;

use crate::util::{show_error, uniquify_path};

#[derive(Debug, Clone, Copy)]
pub enum ConvertFormat {
    Jpeg,
    Png,
    Pdf,
    Webp,
}

impl ConvertFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Jpeg => "Convert to JPEG",
            Self::Png => "Convert to PNG",
            Self::Pdf => "Convert to PDF",
            Self::Webp => "Convert to WebP",
        }
    }

    pub fn action_name(self) -> &'static str {
        match self {
            Self::Jpeg => "convert-to-jpeg",
            Self::Png => "convert-to-png",
            Self::Pdf => "convert-to-pdf",
            Self::Webp => "convert-to-webp",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Pdf => "pdf",
            Self::Webp => "webp",
        }
    }

    fn magick_format(self) -> &'static str {
        match self {
            Self::Jpeg => "jpeg",
            Self::Png => "png",
            Self::Pdf => "pdf",
            Self::Webp => "webp",
        }
    }
}

/// Replace (or append) the file extension, matching the old Nautilus scripts.
fn with_extension(path: &Path, ext: &str) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "converted".into());
    let name = format!("{stem}.{ext}");
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
    {
        // Same format → allow in-place re-encode (e.g. JPEG quality).
        parent.join(name)
    } else {
        uniquify_path(parent, &name)
    }
}

fn convert_one(src: &Path, format: ConvertFormat) -> Result<PathBuf, String> {
    let dest = with_extension(src, format.extension());
    let dest_arg = format!("{}:{}", format.magick_format(), dest.display());

    let mut cmd = Command::new("convert");
    if matches!(format, ConvertFormat::Jpeg) {
        cmd.args(["-quality", "75"]);
    }
    let output = cmd
        .arg(src)
        .arg(&dest_arg)
        .output()
        .map_err(|e| format!("ImageMagick `convert` failed to start: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let msg = stderr.trim();
        if msg.is_empty() {
            return Err(format!("convert failed for {}", src.display()));
        }
        return Err(format!("{}: {msg}", src.display()));
    }
    Ok(dest)
}

/// Convert selected files with ImageMagick (folders are skipped).
pub fn convert_files(
    parent: Option<&impl IsA<gtk::Window>>,
    paths: &[PathBuf],
    format: ConvertFormat,
) -> usize {
    let files: Vec<&Path> = paths
        .iter()
        .map(PathBuf::as_path)
        .filter(|p| p.is_file())
        .collect();
    if files.is_empty() {
        show_error(
            parent,
            format.label(),
            "Select one or more files to convert",
        );
        return 0;
    }

    let mut ok = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for src in files {
        match convert_one(src, format) {
            Ok(_) => ok += 1,
            Err(e) => errors.push(e),
        }
    }

    if !errors.is_empty() {
        let detail = if errors.len() == 1 {
            errors[0].clone()
        } else {
            format!(
                "{} file(s) converted, {} failed:\n{}",
                ok,
                errors.len(),
                errors.join("\n")
            )
        };
        show_error(parent, format.label(), &detail);
    }
    ok
}
