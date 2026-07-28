//! File operations: copy, move, trash, delete, rename, mkdir, link.

use std::path::{Path, PathBuf};

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
    if new_name.is_empty() || new_name.contains('/') {
        return Err("Invalid name".into());
    }
    let parent = path.parent().ok_or_else(|| "No parent".to_string())?;
    let dest = parent.join(new_name);
    if dest.exists() {
        return Err(format!("“{new_name}” already exists"));
    }
    std::fs::rename(path, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
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
    if src.is_dir() {
        std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
        for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            copy_recursive(&entry.path(), &dest.join(name))?;
        }
        Ok(())
    } else {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::copy(src, dest).map_err(|e| e.to_string())?;
        Ok(())
    }
}

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
    drop_into(parent, dest_dir, &paths, op == ClipOp::Cut, || {});
    if op == ClipOp::Cut {
        clear_after_cut_paste(clip);
    }
    on_done();
}

/// Copy or move `paths` into `dest_dir` (used by paste and drag-and-drop).
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

    for src in paths {
        // Cut into the same folder is a no-op. Copy into the same folder still
        // proceeds — uniquify_path makes "name (1).ext" so Ctrl+V duplicates.
        if move_files && src.parent().is_some_and(|p| p == dest_dir) {
            continue;
        }
        let name = src
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "item".into());
        let dest = uniquify_path(&dest_dir, &name);
        let result = if move_files {
            if std::fs::rename(src, &dest).is_ok() {
                Ok(())
            } else {
                copy_recursive(src, &dest).and_then(|_| {
                    if src.is_dir() {
                        std::fs::remove_dir_all(src).map_err(|e| e.to_string())
                    } else {
                        std::fs::remove_file(src).map_err(|e| e.to_string())
                    }
                })
            }
        } else {
            copy_recursive(src, &dest)
        };
        if let Err(e) = result {
            show_error(
                parent_win.as_ref(),
                if move_files { "Move failed" } else { "Copy failed" },
                &format!("{} → {}: {e}", src.display(), dest.display()),
            );
        }
    }
    on_done();
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
