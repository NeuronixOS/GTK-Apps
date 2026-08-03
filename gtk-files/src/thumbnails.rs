//! Image and video thumbnails via XDG cache, gdk-pixbuf, and totem-video-thumbnailer.
//!
//! Work is capped to a small worker pool so opening a folder of thousands of
//! large JPEGs cannot spawn unbounded threads and freeze the UI.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::SystemTime;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gdk_pixbuf::Pixbuf;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

const MAX_WORKERS: usize = 3;
const MAX_QUEUED: usize = 64;

static THUMB_SEQ: AtomicU64 = AtomicU64::new(1);
/// Bumped on navigate/refresh so in-flight jobs can abort.
static GENERATION: AtomicU64 = AtomicU64::new(1);

struct ThumbJob {
    path: PathBuf,
    uri: String,
    content_type: String,
    gen_size: i32,
    display_size: i32,
    token: u64,
    generation: u64,
    /// When false, only apply an existing cache entry (no JPEG decode / generate).
    generate_if_missing: bool,
    weak: glib::SendWeakRef<gtk::Image>,
}

struct ThumbQueue {
    pending: VecDeque<ThumbJob>,
    active: usize,
}

fn queue() -> &'static Mutex<ThumbQueue> {
    static Q: OnceLock<Mutex<ThumbQueue>> = OnceLock::new();
    Q.get_or_init(|| {
        Mutex::new(ThumbQueue {
            pending: VecDeque::new(),
            active: 0,
        })
    })
}

/// Invalidate in-flight thumbnail work (call on navigate / refresh).
pub fn bump_generation() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut q) = queue().lock() {
        q.pending.clear();
    }
}

/// Whether this file type should get a content thumbnail.
pub fn is_thumbnailable(info: &gio::FileInfo) -> bool {
    let Some(ct) = info.content_type() else {
        return false;
    };
    let ct = ct.as_str();
    ct.starts_with("image/") || ct.starts_with("video/")
}

fn is_image(content_type: &str) -> bool {
    content_type.starts_with("image/")
}

fn is_video(content_type: &str) -> bool {
    content_type.starts_with("video/")
}

fn cache_dir(size: i32) -> PathBuf {
    let base = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from(".").join(".cache"))
        .join("thumbnails");
    if size > 128 {
        base.join("large")
    } else {
        base.join("normal")
    }
}

fn uri_md5(uri: &str) -> String {
    let mut c = glib::Checksum::new(glib::ChecksumType::Md5).expect("md5");
    c.update(uri.as_bytes());
    c.string().unwrap_or_default().to_string()
}

fn cached_thumb_path(uri: &str, size: i32) -> PathBuf {
    cache_dir(size).join(format!("{}.png", uri_md5(uri)))
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Cached thumb is usable only when it is at least as new as the source file.
fn thumbnail_is_fresh(thumb: &Path, source_mtime: SystemTime) -> bool {
    match file_mtime(thumb) {
        Some(thumb_mtime) => thumb_mtime >= source_mtime,
        None => false,
    }
}

fn find_cached_thumbnail(uri: &str, size: i32, source_mtime: SystemTime) -> Option<PathBuf> {
    let candidates = [
        cached_thumb_path(uri, size),
        cached_thumb_path(uri, if size > 128 { 128 } else { 256 }),
    ];
    for path in candidates {
        if !path.is_file() {
            continue;
        }
        if thumbnail_is_fresh(&path, source_mtime) {
            return Some(path);
        }
        // Stale (source was modified after the thumb was written) — drop it so
        // the next generate writes a fresh primary-size cache entry.
        let _ = std::fs::remove_file(&path);
    }
    None
}

/// Ensure a thumbnail file exists on disk; returns its path. Runs off the UI thread.
fn ensure_thumbnail_file(
    path: &Path,
    uri: &str,
    content_type: &str,
    size: i32,
    generate_if_missing: bool,
) -> Option<PathBuf> {
    let source_mtime = file_mtime(path).unwrap_or(SystemTime::UNIX_EPOCH);
    if let Some(cached) = find_cached_thumbnail(uri, size, source_mtime) {
        return Some(cached);
    }
    if !generate_if_missing {
        return None;
    }

    let cache_path = cached_thumb_path(uri, size);
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Remove a stale primary entry before regenerating (alt size already cleared).
    if cache_path.is_file() && !thumbnail_is_fresh(&cache_path, source_mtime) {
        let _ = std::fs::remove_file(&cache_path);
    }

    if is_image(content_type) {
        let pixbuf = Pixbuf::from_file_at_scale(path, size, size, true).ok()?;
        pixbuf.savev(&cache_path, "png", &[]).ok()?;
        return Some(cache_path);
    }

    if is_video(content_type) {
        let status = Command::new("totem-video-thumbnailer")
            .args(["-s", &size.to_string()])
            .arg(path)
            .arg(&cache_path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if matches!(status, Ok(s) if s.success()) && cache_path.is_file() {
            return Some(cache_path);
        }

        let tmp = cache_path.with_extension("tmp.jpg");
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-ss",
                "1",
                "-i",
                &path.to_string_lossy(),
                "-frames:v",
                "1",
                "-vf",
                &format!("scale={size}:{size}:force_original_aspect_ratio=decrease"),
                &tmp.to_string_lossy(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if matches!(status, Ok(s) if s.success()) && tmp.is_file() {
            if let Ok(pixbuf) = Pixbuf::from_file_at_scale(&tmp, size, size, true) {
                let _ = pixbuf.savev(&cache_path, "png", &[]);
            }
            let _ = std::fs::remove_file(&tmp);
            if cache_path.is_file() {
                return Some(cache_path);
            }
        }
        let _ = std::fs::remove_file(&tmp);
    }

    None
}

fn pump_queue() {
    let job = {
        let Ok(mut q) = queue().lock() else {
            return;
        };
        if q.active >= MAX_WORKERS {
            return;
        }
        let Some(job) = q.pending.pop_front() else {
            return;
        };
        q.active += 1;
        job
    };

    thread::spawn(move || {
        run_job(job);
        if let Ok(mut q) = queue().lock() {
            q.active = q.active.saturating_sub(1);
        }
        pump_queue();
    });
}

fn run_job(job: ThumbJob) {
    if job.generation != GENERATION.load(Ordering::Relaxed) {
        return;
    }
    let thumb_path = ensure_thumbnail_file(
        &job.path,
        &job.uri,
        &job.content_type,
        job.gen_size,
        job.generate_if_missing,
    );
    if job.generation != GENERATION.load(Ordering::Relaxed) {
        return;
    }
    let Some(thumb_path) = thumb_path else {
        return;
    };
    let ThumbJob {
        uri,
        display_size,
        token,
        generation,
        weak,
        ..
    } = job;
    // Pixbuf is not Send — load the (already small) cache file on the UI thread.
    glib::MainContext::default().invoke(move || {
        if generation != GENERATION.load(Ordering::Relaxed) {
            return;
        }
        let Some(image) = weak.upgrade() else {
            return;
        };
        let still_valid = unsafe {
            image
                .data::<u64>("thumb-token")
                .map(|p| *p.as_ref() == token)
                .unwrap_or(false)
                && image
                    .data::<String>("thumb-uri")
                    .map(|p| p.as_ref() == &uri)
                    .unwrap_or(false)
        };
        if !still_valid {
            return;
        }
        if let Ok(pixbuf) =
            Pixbuf::from_file_at_scale(&thumb_path, display_size, display_size, true)
        {
            let texture = gdk::Texture::for_pixbuf(&pixbuf);
            image.set_paintable(Some(&texture));
            image.set_pixel_size(display_size);
        }
    });
}

fn enqueue(job: ThumbJob) {
    if let Ok(mut q) = queue().lock() {
        // Prefer newest visible binds: drop oldest when saturated.
        if q.pending.len() >= MAX_QUEUED {
            q.pending.pop_front();
        }
        q.pending.push_back(job);
    }
    pump_queue();
}

/// Show MIME icon immediately, then replace with a content thumbnail when ready.
///
/// `generate_if_missing`: list views should pass `false` so opening thousands of
/// large photos only uses already-cached thumbs (MIME icon otherwise). Grid
/// views pass `true` to fill the XDG cache via the bounded worker pool.
pub fn apply_thumbnail(
    image: &gtk::Image,
    file: &gio::File,
    info: &gio::FileInfo,
    size: i32,
    generate_if_missing: bool,
) {
    image.set_pixel_size(size);
    image.set_from_gicon(&crate::util::icon_for_info(info, false));

    if !is_thumbnailable(info) {
        return;
    }

    let Some(path) = file.path() else {
        return;
    };
    let uri = file.uri().to_string();
    let content_type = info
        .content_type()
        .map(|c| c.to_string())
        .unwrap_or_default();

    let token = THUMB_SEQ.fetch_add(1, Ordering::Relaxed);
    unsafe {
        image.set_data("thumb-token", token);
        image.set_data("thumb-uri", uri.clone());
    }

    let weak = glib::SendWeakRef::from(image.downgrade());
    // Request a cache-appropriate size (128 or 256), display scaled via pixel_size.
    let gen_size = if size > 96 { 256 } else { 128 };
    enqueue(ThumbJob {
        path,
        uri,
        content_type,
        gen_size,
        display_size: size,
        token,
        generation: GENERATION.load(Ordering::Relaxed),
        generate_if_missing,
        weak,
    });
}
