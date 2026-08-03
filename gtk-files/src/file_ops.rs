//! File operations: copy, move, trash, delete, rename, mkdir, link.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use gtk4 as gtk;
use gtk::gio;
use gtk::prelude::*;

use crate::clipboard::{clear_after_cut_paste, ClipOp, SharedClipboard};
use crate::util::{self, show_error, uniquify_path};

pub fn create_folder(parent_dir: &Path, name: &str) -> Result<PathBuf, String> {
    let dest = parent_dir.join(name);
    if dest.exists() {
        return Err(format!("“{name}” already exists"));
    }
    std::fs::create_dir(&dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

pub fn create_empty_file(parent_dir: &Path, name: &str) -> Result<PathBuf, String> {
    let dest = parent_dir.join(name);
    if dest.exists() {
        return Err(format!("“{name}” already exists"));
    }
    std::fs::File::create(&dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

pub fn rename(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let new_name = new_name.trim();
    if new_name.is_empty() || new_name.contains('/') || new_name.contains('\0') {
        return Err("Invalid name".into());
    }
    if new_name == "." || new_name == ".." {
        return Err("Invalid name".into());
    }
    let parent = path.parent().ok_or_else(|| "No parent".to_string())?;
    let dest = parent.join(new_name);

    // Same path / same file → no-op (also covers case-only renames on
    // case-insensitive volumes after canonicalize).
    if paths_same_file(path, &dest) {
        return Ok(path.to_path_buf());
    }

    if dest.exists() {
        return Err(format!("“{new_name}” already exists"));
    }

    // Atomic no-clobber on Linux so a race can't overwrite another file
    // (std::fs::rename replaces the destination, which then confuses the
    // directory model and can crash the UI).
    rename_no_replace(path, &dest).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            format!("“{new_name}” already exists")
        } else {
            e.to_string()
        }
    })?;
    Ok(dest)
}

fn paths_same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) if ca == cb => return true,
        _ => {}
    }
    // Hard-link / same inode fallback.
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(ma), Ok(mb)) => {
            use std::os::unix::fs::MetadataExt;
            ma.dev() == mb.dev() && ma.ino() == mb.ino()
        }
        _ => false,
    }
}

/// Rename that refuses to replace an existing destination.
fn rename_no_replace(src: &Path, dest: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        const AT_FDCWD: libc::c_int = -100;
        const RENAME_NOREPLACE: libc::c_uint = 1;

        let src_c = CString::new(src.as_os_str().as_bytes())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid path"))?;
        let dest_c = CString::new(dest.as_os_str().as_bytes())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid path"))?;

        let rc = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                AT_FDCWD,
                src_c.as_ptr(),
                AT_FDCWD,
                dest_c.as_ptr(),
                RENAME_NOREPLACE,
            )
        };
        if rc == 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        // Fall back only when renameat2 is unavailable; still never call
        // plain rename if the destination exists.
        if err.raw_os_error() == Some(libc::ENOSYS) {
            if dest.exists() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "destination exists",
                ));
            }
            return std::fs::rename(src, dest);
        }
        return Err(err);
    }
    #[cfg(not(target_os = "linux"))]
    {
        if dest.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "destination exists",
            ));
        }
        std::fs::rename(src, dest)
    }
}

pub fn trash_paths(
    parent: Option<&impl IsA<gtk::Window>>,
    paths: &[PathBuf],
    confirm: bool,
    on_done: impl FnOnce() + 'static,
) {
    if paths.is_empty() {
        on_done();
        return;
    }
    let do_trash = {
        let paths = paths.to_vec();
        let parent_win = parent.map(|w| w.clone().upcast::<gtk::Window>());
        move || {
            for path in &paths {
                let file = gio::File::for_path(path);
                if let Err(e) = file.trash(None::<&gio::Cancellable>) {
                    show_error(
                        parent_win.as_ref(),
                        "Could not move to Trash",
                        &format!("{}: {e}", path.display()),
                    );
                }
            }
            on_done();
        }
    };

    if confirm {
        let n = paths.len();
        let detail = if n == 1 {
            format!("Move “{}” to the Trash?", paths[0].file_name().unwrap_or_default().to_string_lossy())
        } else {
            format!("Move {n} items to the Trash?")
        };
        util::confirm_dialog(parent, "Move to Trash", &detail, "Move to Trash", move |ok| {
            if ok {
                do_trash();
            }
        });
    } else {
        do_trash();
    }
}

pub fn delete_permanent(
    parent: Option<&impl IsA<gtk::Window>>,
    paths: &[PathBuf],
    confirm: bool,
    on_done: impl FnOnce() + 'static,
) {
    if paths.is_empty() {
        on_done();
        return;
    }
    let do_delete = {
        let paths = paths.to_vec();
        let parent_win = parent.map(|w| w.clone().upcast::<gtk::Window>());
        move || {
            for path in &paths {
                let res = if path.is_dir() {
                    std::fs::remove_dir_all(path)
                } else {
                    std::fs::remove_file(path)
                };
                if let Err(e) = res {
                    show_error(
                        parent_win.as_ref(),
                        "Could not delete",
                        &format!("{}: {e}", path.display()),
                    );
                }
            }
            on_done();
        }
    };

    if confirm {
        let n = paths.len();
        let detail = if n == 1 {
            format!(
                "Permanently delete “{}”? This cannot be undone.",
                paths[0].file_name().unwrap_or_default().to_string_lossy()
            )
        } else {
            format!("Permanently delete {n} items? This cannot be undone.")
        };
        util::confirm_dialog(parent, "Delete Permanently", &detail, "Delete", move |ok| {
            if ok {
                do_delete();
            }
        });
    } else {
        do_delete();
    }
}

pub fn empty_trash(parent: Option<&impl IsA<gtk::Window>>, on_done: impl FnOnce() + 'static) {
    let parent_win = parent.map(|w| w.clone().upcast::<gtk::Window>());
    let parent_for_cb = parent_win.clone();
    util::confirm_dialog(
        parent_win.as_ref(),
        "Empty Trash",
        "All items in the Trash will be permanently deleted.",
        "Empty Trash",
        move |ok| {
            if !ok {
                return;
            }
            let trash = util::trash_file();
            match trash.enumerate_children(
                "standard::name",
                gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                None::<&gio::Cancellable>,
            ) {
                Ok(enumerator) => {
                    while let Ok(Some(info)) = enumerator.next_file(None::<&gio::Cancellable>) {
                        let child = trash.child(info.name());
                        let _ = child.delete(None::<&gio::Cancellable>);
                    }
                }
                Err(e) => {
                    show_error(
                        parent_for_cb.as_ref(),
                        "Could not empty Trash",
                        &e.to_string(),
                    );
                }
            }
            on_done();
        },
    );
}

fn copy_recursive(src: &Path, dest: &Path) -> Result<(), String> {
    copy_recursive_progressive(src, dest, &mut |_| Ok(()))
}

fn copy_recursive_progressive(
    src: &Path,
    dest: &Path,
    on_bytes: &mut dyn FnMut(u64) -> Result<(), String>,
) -> Result<(), String> {
    if src.is_dir() {
        std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
        for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            copy_recursive_progressive(&entry.path(), &dest.join(name), on_bytes)?;
        }
        Ok(())
    } else {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        copy_file_chunked(src, dest, on_bytes)
    }
}

fn copy_file_chunked(
    src: &Path,
    dest: &Path,
    on_bytes: &mut dyn FnMut(u64) -> Result<(), String>,
) -> Result<(), String> {
    use std::io::{Read, Write};

    let mut reader = std::fs::File::open(src).map_err(|e| e.to_string())?;
    let mut writer = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        on_bytes(n as u64)?;
    }
    writer.flush().map_err(|e| e.to_string())?;
    if let Ok(meta) = std::fs::metadata(src) {
        let _ = std::fs::set_permissions(dest, meta.permissions());
    }
    Ok(())
}

fn path_byte_size(path: &Path) -> u64 {
    if path.is_file() {
        return std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }
    if !path.is_dir() {
        return 0;
    }
    let mut total = 0u64;
    let walker = walkdir_or_manual(path);
    for p in walker {
        if let Ok(meta) = std::fs::metadata(&p) {
            if meta.is_file() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

fn walkdir_or_manual(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}

#[derive(Clone)]
struct ProgressUpdate {
    fraction: f64,
    title: String,
    detail: String,
}

const PROGRESS_THRESHOLD_BYTES: u64 = 512 * 1024;

pub fn paste_into(
    parent: Option<&impl IsA<gtk::Window>>,
    dest_dir: &Path,
    clip: &SharedClipboard,
    on_done: impl FnOnce() + 'static,
) {
    let Some((paths, op)) = crate::clipboard::take_for_paste(clip) else {
        on_done();
        return;
    };
    let clip = Rc::clone(clip);
    drop_into(parent, dest_dir, &paths, op == ClipOp::Cut, move || {
        if op == ClipOp::Cut {
            clear_after_cut_paste(&clip);
        }
        on_done();
    });
}

/// Copy or move `paths` into `dest_dir` (used by paste and drag-and-drop).
/// Large jobs run off the UI thread with a non-blocking sidebar progress panel.
pub fn drop_into(
    parent: Option<&impl IsA<gtk::Window>>,
    dest_dir: &Path,
    paths: &[PathBuf],
    move_files: bool,
    on_done: impl FnOnce() + 'static,
) {
    if paths.is_empty() || !dest_dir.is_dir() {
        on_done();
        return;
    }
    let parent_win = parent.map(|w| w.clone().upcast::<gtk::Window>());
    let dest_dir = dest_dir.to_path_buf();
    let paths: Vec<PathBuf> = paths.to_vec();

    let jobs: Vec<(PathBuf, PathBuf)> = paths
        .iter()
        .filter_map(|src| {
            if move_files && src.parent().is_some_and(|p| p == dest_dir) {
                return None;
            }
            let name = src
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "item".into());
            let dest = uniquify_path(&dest_dir, &name);
            Some((src.clone(), dest))
        })
        .collect();

    if jobs.is_empty() {
        on_done();
        return;
    }

    let total_bytes: u64 = jobs.iter().map(|(src, _)| path_byte_size(src)).sum();
    let show_progress = total_bytes >= PROGRESS_THRESHOLD_BYTES || jobs.len() > 3;

    if !show_progress {
        // Tiny jobs: keep the old synchronous path (feels instant).
        for (src, dest) in &jobs {
            let result = transfer_one(src, dest, move_files, &mut |_| Ok(()));
            if let Err(e) = result {
                show_error(
                    parent_win.as_ref(),
                    if move_files { "Move failed" } else { "Copy failed" },
                    &format!("{} → {}: {e}", src.display(), dest.display()),
                );
            }
        }
        on_done();
        return;
    }

    run_transfer_with_progress(parent_win.as_ref(), jobs, move_files, total_bytes, on_done);
}

fn transfer_one(
    src: &Path,
    dest: &Path,
    move_files: bool,
    on_bytes: &mut dyn FnMut(u64) -> Result<(), String>,
) -> Result<(), String> {
    if move_files {
        if std::fs::rename(src, dest).is_ok() {
            // Same-volume rename: count full size as done.
            let _ = on_bytes(path_byte_size(src));
            return Ok(());
        }
        copy_recursive_progressive(src, dest, on_bytes).and_then(|_| {
            if src.is_dir() {
                std::fs::remove_dir_all(src).map_err(|e| e.to_string())
            } else {
                std::fs::remove_file(src).map_err(|e| e.to_string())
            }
        })
    } else {
        copy_recursive_progressive(src, dest, on_bytes)
    }
}

fn format_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let n = n as f64;
    if n >= GB {
        format!("{:.1} GB", n / GB)
    } else if n >= MB {
        format!("{:.1} MB", n / MB)
    } else if n >= KB {
        format!("{:.0} KB", n / KB)
    } else {
        format!("{n:.0} B")
    }
}

fn format_duration(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h {m:02}m {s:02}s")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

fn run_transfer_with_progress(
    parent: Option<&gtk::Window>,
    jobs: Vec<(PathBuf, PathBuf)>,
    move_files: bool,
    total_bytes: u64,
    on_done: impl FnOnce() + 'static,
) {
    use gtk::glib;

    let panel = crate::transfer_panel::active();
    let cancelled = if let Some(ref panel) = panel {
        match panel.begin(move_files) {
            Some(flag) => flag,
            None => {
                show_error(
                    parent,
                    if move_files { "Move" } else { "Copy" },
                    "Another file transfer is already in progress.",
                );
                on_done();
                return;
            }
        }
    } else {
        Arc::new(AtomicBool::new(false))
    };

    let verb = if move_files { "Moving" } else { "Copying" };
    let (tx, rx) = std::sync::mpsc::channel::<Result<ProgressUpdate, String>>();
    let cancelled_worker = Arc::clone(&cancelled);
    let total = total_bytes.max(1);

    thread::spawn(move || {
        let started = Instant::now();
        let mut done_bytes = 0u64;
        let job_count = jobs.len();

        for (i, (src, dest)) in jobs.into_iter().enumerate() {
            if cancelled_worker.load(Ordering::SeqCst) {
                let _ = tx.send(Err("Cancelled".into()));
                return;
            }
            let name = src
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| src.display().to_string());
            let _ = tx.send(Ok(ProgressUpdate {
                fraction: done_bytes as f64 / total as f64,
                title: format!("{verb} “{name}” ({}/{job_count})", i + 1),
                detail: format!(
                    "{} / {}  ·  elapsed {}",
                    format_bytes(done_bytes),
                    format_bytes(total),
                    format_duration(started.elapsed().as_secs())
                ),
            }));

            let tx_prog = tx.clone();
            let cancelled = Arc::clone(&cancelled_worker);
            let verb = verb.to_string();
            let name_c = name.clone();
            let result = transfer_one(&src, &dest, move_files, &mut |n| {
                if cancelled.load(Ordering::SeqCst) {
                    return Err("Cancelled".into());
                }
                done_bytes = done_bytes.saturating_add(n);
                let elapsed = started.elapsed().as_secs_f64().max(0.001);
                let rate = done_bytes as f64 / elapsed;
                let remain = total.saturating_sub(done_bytes);
                let eta = if rate > 1.0 {
                    format_duration((remain as f64 / rate) as u64)
                } else {
                    "…".into()
                };
                let _ = tx_prog.send(Ok(ProgressUpdate {
                    fraction: (done_bytes as f64 / total as f64).clamp(0.0, 1.0),
                    title: format!("{verb} “{name_c}” ({}/{job_count})", i + 1),
                    detail: format!(
                        "{} / {}  ·  {}/s  ·  ~{} left",
                        format_bytes(done_bytes),
                        format_bytes(total),
                        format_bytes(rate as u64),
                        eta
                    ),
                }));
                Ok(())
            });

            if let Err(e) = result {
                if e == "Cancelled" {
                    let _ = std::fs::remove_file(&dest);
                    let _ = std::fs::remove_dir_all(&dest);
                    let _ = tx.send(Err("Cancelled".into()));
                    return;
                }
                let _ = tx.send(Err(format!("{} → {}: {e}", src.display(), dest.display())));
                return;
            }
        }

        let _ = tx.send(Ok(ProgressUpdate {
            fraction: 1.0,
            title: if move_files {
                "Move complete".into()
            } else {
                "Copy complete".into()
            },
            detail: format!(
                "{} in {}",
                format_bytes(done_bytes.max(total_bytes)),
                format_duration(started.elapsed().as_secs())
            ),
        }));
        let _ = tx.send(Err(String::new())); // sentinel: success
    });

    let parent_c = parent.cloned();
    let mut on_done = Some(on_done);
    let move_files = move_files;

    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        let mut finished = false;
        let mut fatal: Option<String> = None;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                Ok(upd) => {
                    if let Some(ref panel) = panel {
                        panel.update(&upd.title, &upd.detail, upd.fraction);
                    }
                }
                Err(e) if e.is_empty() => {
                    finished = true;
                }
                Err(e) => {
                    fatal = Some(e);
                    finished = true;
                }
            }
        }
        if finished {
            if let Some(ref panel) = panel {
                panel.finish();
            }
            if let Some(err) = fatal {
                if err != "Cancelled" {
                    show_error(
                        parent_c.as_ref(),
                        if move_files { "Move failed" } else { "Copy failed" },
                        &err,
                    );
                }
            }
            if let Some(cb) = on_done.take() {
                cb();
            }
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

pub fn create_link(src: &Path, dest_dir: &Path) -> Result<PathBuf, String> {
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "link".into());
    let link_name = format!("Link to {name}");
    let dest = uniquify_path(dest_dir, &link_name);
    std::os::unix::fs::symlink(src, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

pub fn duplicate(path: &Path) -> Result<PathBuf, String> {
    let parent = path.parent().ok_or_else(|| "No parent".to_string())?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "copy".into());
    let dest = uniquify_path(parent, &name);
    copy_recursive(path, &dest)?;
    Ok(dest)
}
