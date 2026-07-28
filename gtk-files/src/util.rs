//! Formatting helpers, XDG user dirs, icons, and common file attributes.

use std::path::{Path, PathBuf};

use gtk4 as gtk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

/// Attributes requested from GtkDirectoryList / FileInfo queries.
pub const FILE_ATTRIBUTES: &str = concat!(
    "standard::name,standard::display-name,standard::icon,",
    "standard::symbolic-icon,standard::content-type,standard::type,",
    "standard::size,standard::is-hidden,standard::is-backup,",
    "standard::is-symlink,standard::symlink-target,time::modified,access::can-read,",
    "access::can-write,access::can-execute,trash::orig-path,trash::deletion-date"
);

pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

pub fn xdg_user_dir(key: &str, fallback: &str) -> PathBuf {
    let home = home_dir();
    let dirs_file = dirs::config_dir()
        .unwrap_or_else(|| home.join(".config"))
        .join("user-dirs.dirs");
    if let Ok(text) = std::fs::read_to_string(dirs_file) {
        let prefix = format!("XDG_{key}_DIR=");
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix(&prefix) {
                let val = rest.trim().trim_matches('"');
                let expanded = val.replace("$HOME", &home.to_string_lossy());
                let path = PathBuf::from(expanded);
                if path.exists() {
                    return path;
                }
            }
        }
    }
    home.join(fallback)
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

pub fn format_mtime(info: &gio::FileInfo) -> String {
    let Some(dt) = info.modification_date_time() else {
        return String::new();
    };
    dt.format("%Y-%m-%d %H:%M").unwrap_or_default().to_string()
}

pub fn display_name(info: &gio::FileInfo) -> String {
    info.display_name().to_string()
}

pub fn is_directory(info: &gio::FileInfo) -> bool {
    info.file_type() == gio::FileType::Directory
}

pub fn is_hidden(info: &gio::FileInfo) -> bool {
    info.is_hidden() || info.is_backup() || display_name(info).starts_with('.')
}

/// Whether the current user can write/modify this item (from `access::can-write`).
/// Missing attribute → treat as writable so we don't flash false lock badges.
pub fn can_write(info: &gio::FileInfo) -> bool {
    if !info.has_attribute("access::can-write") {
        return true;
    }
    info.boolean("access::can-write")
}

/// Whether the on-disk entry itself is a symbolic link (not its target).
pub fn is_symlink_path(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

/// Raw link target exactly as stored in the symlink (may be relative).
pub fn symlink_target(path: &Path) -> Option<PathBuf> {
    std::fs::read_link(path).ok()
}

/// Absolute link target: relative links are resolved against the link's parent.
pub fn resolved_symlink_target(path: &Path) -> Option<PathBuf> {
    let raw = symlink_target(path)?;
    if raw.is_absolute() {
        Some(raw)
    } else {
        let parent = path.parent().unwrap_or_else(|| Path::new("/"));
        Some(parent.join(raw))
    }
}

#[allow(dead_code)]
pub fn content_type_label(info: &gio::FileInfo) -> String {
    if is_directory(info) {
        return "Folder".into();
    }
    info.content_type()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "Unknown".into())
}

pub fn icon_for_info(info: &gio::FileInfo, symbolic: bool) -> gio::Icon {
    let base = if symbolic {
        info.symbolic_icon()
            .or_else(|| info.icon())
            .unwrap_or_else(|| fallback_icon(info))
    } else {
        info.icon().unwrap_or_else(|| fallback_icon(info))
    };

    // Prefer overlay badges in the list/grid views; still attach emblems here
    // for any caller that renders GIcons directly (e.g. fallbacks).
    let mut icon = base;
    if info.is_symlink() {
        let emblem_icon = gio::ThemedIcon::new("emblem-symbolic-link");
        let emblem = gio::Emblem::new(&emblem_icon);
        icon = gio::EmblemedIcon::new(&icon, Some(&emblem)).upcast();
    }
    if !can_write(info) {
        let emblem_icon = gio::ThemedIcon::new("changes-prevent-symbolic");
        let emblem = gio::Emblem::new(&emblem_icon);
        icon = gio::EmblemedIcon::new(&icon, Some(&emblem)).upcast();
    }
    icon
}

fn fallback_icon(info: &gio::FileInfo) -> gio::Icon {
    let name = if is_directory(info) {
        "folder"
    } else {
        "text-x-generic"
    };
    gio::ThemedIcon::new(name).upcast()
}

pub fn file_from_dir_and_info(dir: &gio::File, info: &gio::FileInfo) -> gio::File {
    dir.child(info.name())
}

#[allow(dead_code)]
pub fn path_display(path: &Path) -> String {
    let home = home_dir();
    if let Ok(rel) = path.strip_prefix(&home) {
        if rel.as_os_str().is_empty() {
            return "Home".into();
        }
        return format!("~/{}", rel.display());
    }
    if path == Path::new("/") {
        return "Computer".into();
    }
    path.display().to_string()
}

pub fn title_for_location(file: &gio::File) -> String {
    if let Some(path) = file.path() {
        if path == home_dir() {
            return "Home".into();
        }
        if file.uri() == "trash:///" || path.ends_with(".local/share/Trash/files") {
            return "Trash".into();
        }
        return path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
    }
    file.basename()
        .map(|b| b.to_string_lossy().to_string())
        .unwrap_or_else(|| file.uri().to_string())
}

pub fn trash_file() -> gio::File {
    gio::File::for_uri("trash:///")
}

pub fn is_trash_location(file: &gio::File) -> bool {
    file.uri().starts_with("trash:")
}

pub fn open_file_default(parent: Option<&impl IsA<gtk::Window>>, file: &gio::File) {
    match file.query_default_handler(None::<&gio::Cancellable>) {
        Ok(appinfo) => {
            if let Err(e) = appinfo.launch(&[file.clone()], None::<&gio::AppLaunchContext>) {
                eprintln!("launch failed: {e}");
                fallback_xdg_open(file);
            }
        }
        Err(e) => {
            eprintln!("no default handler: {e}");
            let launcher = gtk::FileLauncher::new(Some(file));
            let parent = parent.map(|w| w.clone().upcast::<gtk::Window>());
            launcher.launch(
                parent.as_ref(),
                None::<&gio::Cancellable>,
                move |res| {
                    if let Err(err) = res {
                        eprintln!("FileLauncher failed: {err}");
                    }
                },
            );
            // Also try xdg-open immediately as a reliable fallback for local files.
            fallback_xdg_open(file);
        }
    }
}

fn fallback_xdg_open(file: &gio::File) {
    if let Some(path) = file.path() {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

#[allow(dead_code)]
pub fn open_containing_folder(parent: Option<&impl IsA<gtk::Window>>, file: &gio::File) {
    if let Some(parent_file) = file.parent() {
        open_file_default(parent, &parent_file);
    }
}

pub fn show_error(parent: Option<&impl IsA<gtk::Window>>, title: &str, message: &str) {
    let dialog = gtk::AlertDialog::builder()
        .modal(true)
        .message(title)
        .detail(message)
        .buttons(["OK"])
        .build();
    dialog.show(parent.map(|w| w.upcast_ref()));
}

pub fn confirm_dialog(
    parent: Option<&impl IsA<gtk::Window>>,
    title: &str,
    detail: &str,
    confirm_label: &str,
    cb: impl FnOnce(bool) + 'static,
) {
    let dialog = gtk::AlertDialog::builder()
        .modal(true)
        .message(title)
        .detail(detail)
        .buttons(["Cancel", confirm_label])
        .default_button(1)
        .cancel_button(0)
        .build();
    dialog.choose(
        parent.map(|w| w.upcast_ref()),
        None::<&gio::Cancellable>,
        move |res| {
            let ok = matches!(res, Ok(1));
            cb(ok);
        },
    );
}

/// Unique destination path when copying/moving would overwrite.
pub fn uniquify_path(dest_dir: &Path, name: &str) -> PathBuf {
    let candidate = dest_dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for i in 1..10_000 {
        let candidate = dest_dir.join(format!("{stem} ({i}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dest_dir.join(format!("{stem}-copy{ext}"))
}

pub fn content_type_description(info: &gio::FileInfo) -> String {
    if is_directory(info) {
        return "Folder".into();
    }
    let Some(ct) = info.content_type() else {
        return "Unknown".into();
    };
    gio::content_type_get_description(&ct).to_string()
}

#[allow(dead_code)]
pub fn idle_add_local_once(f: impl FnOnce() + 'static) {
    glib::idle_add_local_once(f);
}
