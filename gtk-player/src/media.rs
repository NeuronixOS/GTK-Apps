//! File gathering. Playback itself is GTK `MediaFile` (GStreamer), which
//! decodes whatever the installed plugins support — including H.264, H.265,
//! and audio-only files — rather than an extension allow-list.

use std::path::{Path, PathBuf};

const AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "wav", "ogg", "oga", "opus", "aac", "m4a", "wma", "aiff", "aif", "alac",
    "ape", "wv", "mp2", "ac3", "dts", "mka", "spx", "caf",
];

pub const MEDIA_PATTERNS: &[&str] = &[
    "*.mp4", "*.m4v", "*.mkv", "*.webm", "*.avi", "*.mov", "*.mpg", "*.mpeg", "*.ts", "*.m2ts",
    "*.wmv", "*.flv", "*.ogv", "*.3gp", "*.mxf", "*.vob", "*.asf", "*.divx", "*.hevc", "*.h264",
    "*.h265", "*.av1", "*.mp3", "*.flac", "*.wav", "*.ogg", "*.opus", "*.aac", "*.m4a", "*.wma",
    "*.aiff", "*.aif", "*.alac", "*.mka",
];

pub fn looks_like_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTS.iter().any(|a| e.eq_ignore_ascii_case(a)))
        .unwrap_or(false)
}

/// Files stay as-is. A directory contributes the files in that directory
/// (not nested folders), sorted by name.
pub fn collect(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in paths {
        if path.is_file() {
            out.push(path);
        } else if path.is_dir() {
            let mut kids = Vec::new();
            if let Ok(rd) = std::fs::read_dir(&path) {
                for entry in rd.flatten() {
                    let p = entry.path();
                    if p.is_file() {
                        kids.push(p);
                    }
                }
            }
            kids.sort();
            out.extend(kids);
        }
    }
    out
}
