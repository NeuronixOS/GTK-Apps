//! Directory image collection — the rusty equivalent of eog's EogListStore.
//!
//! Opening a single file loads every supported image in its parent directory
//! and selects that file. Opening a folder loads its images. Navigation is
//! just moving the current index.

use std::path::{Path, PathBuf};

use gtk4 as gtk;
use gtk::gdk_pixbuf::{Colorspace, InterpType, Pixbuf};

/// Common image extensions accepted by the viewer.
const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "png", "gif", "webp", "bmp", "tif", "tiff", "svg", "svgz", "ico",
    "xpm", "pnm", "pbm", "pgm", "ppm", "tga", "heic", "heif", "avif", "jxl",
];

#[derive(Debug, Clone, Default)]
pub struct ImageList {
    paths: Vec<PathBuf>,
    index: Option<usize>,
}

impl ImageList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn index(&self) -> Option<usize> {
        self.index
    }

    pub fn current(&self) -> Option<&Path> {
        self.index.and_then(|i| self.paths.get(i).map(|p| p.as_path()))
    }

    pub fn position_label(&self) -> String {
        match self.index {
            Some(i) if !self.paths.is_empty() => format!("{} / {}", i + 1, self.paths.len()),
            _ => String::new(),
        }
    }

    /// Replace the collection with every image in `dir`, selecting `select` if present.
    pub fn load_directory(&mut self, dir: &Path, select: Option<&Path>) {
        let mut paths = collect_images(dir);

        // Always include an explicitly chosen file (filter/MIME may allow types
        // our extension list misses, or the file may be a portal path).
        if let Some(sel) = select {
            if sel.is_file() {
                let sel_canon = canonicalize_loose(sel);
                if !paths
                    .iter()
                    .any(|p| canonicalize_loose(p) == sel_canon)
                {
                    paths.push(sel.to_path_buf());
                }
            }
        }

        paths.sort_by(|a, b| {
            a.file_name()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .cmp(&b.file_name().unwrap_or_default().to_ascii_lowercase())
        });

        let index = select
            .and_then(|sel| {
                let sel = canonicalize_loose(sel);
                paths.iter().position(|p| canonicalize_loose(p) == sel)
            })
            .or_else(|| if paths.is_empty() { None } else { Some(0) });

        self.paths = paths;
        self.index = index;
    }

    /// Open a file: scan its parent directory and select it.
    pub fn open_file(&mut self, path: &Path) {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        self.load_directory(parent, Some(path));
        // Last resort: show at least this file even if the parent isn't readable.
        if self.index.is_none() && path.is_file() {
            self.paths = vec![path.to_path_buf()];
            self.index = Some(0);
        }
    }

    /// Open a directory: load its images, select the first.
    pub fn open_directory(&mut self, path: &Path) {
        self.load_directory(path, None);
    }

    pub fn go_next(&mut self) -> bool {
        let Some(i) = self.index else {
            return false;
        };
        if i + 1 < self.paths.len() {
            self.index = Some(i + 1);
            true
        } else {
            false
        }
    }

    pub fn go_previous(&mut self) -> bool {
        let Some(i) = self.index else {
            return false;
        };
        if i > 0 {
            self.index = Some(i - 1);
            true
        } else {
            false
        }
    }

    pub fn go_first(&mut self) -> bool {
        if self.paths.is_empty() {
            return false;
        }
        if self.index == Some(0) {
            return false;
        }
        self.index = Some(0);
        true
    }

    pub fn go_last(&mut self) -> bool {
        if self.paths.is_empty() {
            return false;
        }
        let last = self.paths.len() - 1;
        if self.index == Some(last) {
            return false;
        }
        self.index = Some(last);
        true
    }

    /// Remove the current path (e.g. after trash) and select a neighbour.
    pub fn remove_current(&mut self) {
        let Some(i) = self.index else {
            return;
        };
        if i >= self.paths.len() {
            self.index = None;
            return;
        }
        self.paths.remove(i);
        if self.paths.is_empty() {
            self.index = None;
        } else if i >= self.paths.len() {
            self.index = Some(self.paths.len() - 1);
        } else {
            self.index = Some(i);
        }
    }
}

pub fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.iter().any(|ext| e.eq_ignore_ascii_case(ext)))
        .unwrap_or(false)
}

fn collect_images(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && is_supported_image(&path) {
            out.push(path);
        }
    }
    out
}

fn canonicalize_loose(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Load a display-safe pixbuf: EXIF orientation applied, forced to 8-bit RGBA.
///
/// `gdk::Texture::for_pixbuf` scrambles some formats (odd rowstride / RGB24);
/// normalizing to contiguous RGBA fixes that.
pub fn load_pixbuf(path: &Path) -> Result<Pixbuf, String> {
    let pb = Pixbuf::from_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(normalize_pixbuf(&pb))
}

pub fn normalize_pixbuf(pb: &Pixbuf) -> Pixbuf {
    let oriented = pb
        .apply_embedded_orientation()
        .unwrap_or_else(|| pb.clone());

    let rgba = if oriented.has_alpha()
        && oriented.colorspace() == Colorspace::Rgb
        && oriented.bits_per_sample() == 8
        && oriented.rowstride() == oriented.width() * 4
    {
        oriented
    } else {
        // add_alpha copies into a tightly packed RGBA buffer.
        oriented
            .add_alpha(false, 0, 0, 0)
            .unwrap_or_else(|_| oriented.clone())
    };

    // Final defensive copy through scale_simple(identity) when rowstride is still odd.
    let expect = rgba.width() * if rgba.has_alpha() { 4 } else { 3 };
    if rgba.bits_per_sample() == 8 && rgba.rowstride() != expect {
        rgba.scale_simple(rgba.width(), rgba.height(), InterpType::Nearest)
            .unwrap_or(rgba)
    } else {
        rgba
    }
}
