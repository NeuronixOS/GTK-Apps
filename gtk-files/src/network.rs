//! Network / remote mounts (SFTP, FTP, SMB, WebDAV) via GVFS.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use crate::places;
use crate::util::{home_dir, show_error};

const NETWORK_SCHEMES: &[&str] = &["sftp", "ssh", "ftp", "ftps", "smb", "dav", "davs", "nfs"];

/// `~/Network` — local shortcuts (symlinks) to mounted remotes.
pub fn network_home_dir() -> PathBuf {
    home_dir().join("Network")
}

/// Active GVFS network mounts (SFTP / FTP / SMB / …), not USB/local disks.
pub fn network_mounts() -> Vec<gio::Mount> {
    let monitor = gio::VolumeMonitor::get();
    let mut mounts: Vec<gio::Mount> = monitor
        .mounts()
        .into_iter()
        .filter(is_network_mount)
        .collect();
    mounts.sort_by(|a, b| {
        a.name()
            .to_ascii_lowercase()
            .cmp(&b.name().to_ascii_lowercase())
    });
    mounts
}

pub fn is_network_mount(mount: &gio::Mount) -> bool {
    let root = mount.root();
    let uri = root.uri().to_string();
    let scheme = gio::File::for_uri(&uri)
        .uri_scheme()
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    if NETWORK_SCHEMES.iter().any(|s| *s == scheme) {
        return true;
    }
    // FUSE GVFS path even when scheme probing fails.
    if let Some(path) = root.path() {
        let s = path.to_string_lossy();
        if s.contains("/gvfs/")
            && NETWORK_SCHEMES
                .iter()
                .any(|sch| s.contains(&format!("{sch}:")))
        {
            return true;
        }
    }
    false
}

/// Ensure `~/Network/<label>` → mount path, and keep `~/Network` present.
/// Returns the symlink path when created or already correct.
pub fn ensure_home_shortcut(mount: &gio::Mount) -> Option<PathBuf> {
    let root = mount.root();
    let target = root.path()?;
    if !target.exists() {
        return None;
    }

    let dir = network_home_dir();
    let _ = std::fs::create_dir_all(&dir);

    let label = sanitize_link_name(&mount.name());
    if label.is_empty() {
        return None;
    }
    let link = dir.join(&label);

    if link.exists() || link.symlink_metadata().is_ok() {
        // Refresh symlink if it points elsewhere.
        if let Ok(existing) = std::fs::read_link(&link) {
            if existing == target {
                return Some(link);
            }
        }
        let _ = std::fs::remove_file(&link);
    }

    match std::os::unix::fs::symlink(&target, &link) {
        Ok(()) => Some(link),
        Err(e) => {
            eprintln!(
                "gtk-files: could not link {} → {}: {e}",
                link.display(),
                target.display()
            );
            None
        }
    }
}

/// Refresh ~/Network shortcuts for every current network mount.
pub fn sync_home_shortcuts() {
    for mount in network_mounts() {
        let _ = ensure_home_shortcut(&mount);
    }
}

fn sanitize_link_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
            out.push(c);
        } else if c == ' ' || c == '@' || c == ':' || c == '/' {
            if !out.ends_with('-') {
                out.push('-');
            }
        }
    }
    out.trim_matches('-').to_string()
}

/// Build a remote URI from Connect-to-Server fields.
pub fn build_remote_uri(
    scheme: &str,
    host: &str,
    port: &str,
    folder: &str,
    user: &str,
) -> Result<String, String> {
    let host = host.trim();
    if host.is_empty() {
        return Err("Server address is required".into());
    }
    let scheme = scheme.trim().to_ascii_lowercase();
    if !NETWORK_SCHEMES.iter().any(|s| *s == scheme) {
        return Err(format!("Unsupported protocol: {scheme}"));
    }

    let mut auth = String::new();
    let user = user.trim();
    if !user.is_empty() {
        auth.push_str(&urlencoding_user(user));
        auth.push('@');
    }

    let mut authority = format!("{auth}{host}");
    let port = port.trim();
    if !port.is_empty() {
        if port.parse::<u16>().is_err() {
            return Err("Port must be a number".into());
        }
        authority.push(':');
        authority.push_str(port);
    }

    let mut path = folder.trim().replace('\\', "/");
    if path.is_empty() {
        path = "/".into();
    }
    if !path.starts_with('/') {
        path.insert(0, '/');
    }

    Ok(format!("{scheme}://{authority}{path}"))
}

fn urlencoding_user(user: &str) -> String {
    // Encode reserved URI userinfo characters lightly.
    let mut out = String::new();
    for b in user.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Connect-to-Server dialog: mount via GVFS, link under ~/Network, optional bookmark.
pub fn show_connect_dialog(
    parent: &impl IsA<gtk::Window>,
    on_mounted: Rc<dyn Fn(gio::File)>,
) {
    show_connect_dialog_prefill(parent, on_mounted, None);
}

fn wire_dismissible_dialog(dialog: &gtk::Window, parent: &impl IsA<gtk::Window>) {
    let header = gtk::HeaderBar::new();
    dialog.set_titlebar(Some(&header));
    dialog.set_destroy_with_parent(true);
    dialog.set_deletable(true);
    dialog.set_modal(false);
    let parent_win = parent.clone().upcast::<gtk::Window>();
    if let Some(app) = parent_win.application() {
        dialog.set_application(Some(&app));
    }

    {
        let d = dialog.clone();
        dialog.connect_close_request(move |_| {
            // Defer destroy — tearing down from inside close-request can wedge GTK.
            let d = d.clone();
            glib::idle_add_local_once(move || {
                d.destroy();
            });
            glib::Propagation::Stop
        });
    }

    let key = gtk::EventControllerKey::new();
    let d = dialog.clone();
    key.connect_key_pressed(move |_, keyval, _, _| {
        if keyval == gdk::Key::Escape {
            dismiss_dialog(&d);
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    dialog.add_controller(key);
}

/// Hide immediately, destroy on the next idle so we never tear down a window
/// from inside a widget signal (ListBox row-activated / button-clicked).
fn dismiss_dialog(dialog: &gtk::Window) {
    dialog.set_visible(false);
    let d = dialog.clone();
    glib::idle_add_local_once(move || {
        d.destroy();
    });
}

/// Prefill fields from an existing URI when editing / duplicating a connection.
pub fn show_connect_dialog_prefill(
    parent: &impl IsA<gtk::Window>,
    on_mounted: Rc<dyn Fn(gio::File)>,
    prefill_uri: Option<&str>,
) {
    let dialog = gtk::Window::builder()
        .title("Connect to Server")
        .transient_for(parent)
        .default_width(440)
        .build();
    gtk_theme::style_dialog(&dialog);
    wire_dismissible_dialog(&dialog, parent);

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 10);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);

    let type_drop = gtk::DropDown::from_strings(&["SFTP", "FTP", "SMB", "WebDAV"]);
    type_drop.set_selected(0);

    let server = gtk::Entry::new();
    server.set_placeholder_text(Some("hostname or IP"));
    server.set_hexpand(true);

    let port = gtk::Entry::new();
    port.set_placeholder_text(Some("optional"));
    port.set_width_chars(8);

    let folder = gtk::Entry::new();
    folder.set_text("/");
    folder.set_hexpand(true);

    let user = gtk::Entry::new();
    user.set_placeholder_text(Some("username"));
    user.set_hexpand(true);

    if let Some(uri) = prefill_uri {
        apply_uri_prefill(uri, &type_drop, &server, &port, &folder, &user);
    }

    let bookmark = gtk::CheckButton::with_label("Add bookmark after connecting");
    bookmark.set_active(true);

    let status = gtk::Label::new(None);
    status.add_css_class("dim-label");
    status.set_wrap(true);
    status.set_xalign(0.0);

    vbox.append(&labeled_row("Type", &type_drop));
    vbox.append(&labeled_row("Server", &server));
    vbox.append(&labeled_row("Port", &port));
    vbox.append(&labeled_row("Folder", &folder));
    vbox.append(&labeled_row("Username", &user));
    vbox.append(&bookmark);
    vbox.append(&status);

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    let cancel = gtk_theme::labeled_button(gtk_theme::icon_for_label("Cancel"), "Cancel");
    let connect = gtk_theme::labeled_button(gtk_theme::icon_for_label("Connect"), "Connect");
    connect.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&connect);
    vbox.append(&buttons);
    dialog.set_child(Some(&vbox));

    {
        let d = dialog.clone();
        cancel.connect_clicked(move |_| dismiss_dialog(&d));
    }

    let parent_win = parent.clone().upcast::<gtk::Window>();
    let run = {
        let dialog = dialog.clone();
        let status = status.clone();
        let connect_btn = connect.clone();
        let server = server.clone();
        let type_drop = type_drop.clone();
        let port = port.clone();
        let folder = folder.clone();
        let user = user.clone();
        let bookmark = bookmark.clone();
        let on_mounted = Rc::clone(&on_mounted);
        Rc::new(move || {
            let scheme = match type_drop.selected() {
                0 => "sftp",
                1 => "ftp",
                2 => "smb",
                3 => "davs",
                _ => "sftp",
            };
            let uri = match build_remote_uri(
                scheme,
                &server.text(),
                &port.text(),
                &folder.text(),
                &user.text(),
            ) {
                Ok(u) => u,
                Err(e) => {
                    status.set_text(&e);
                    return;
                }
            };

            status.set_text("Connecting…");
            connect_btn.set_sensitive(false);

            let add_bm = bookmark.is_active();
            let label = {
                let host = server.text().to_string();
                let u = user.text().to_string();
                if u.is_empty() {
                    host
                } else {
                    format!("{u}@{host}")
                }
            };
            let on_mounted = Rc::clone(&on_mounted);
            let parent_win = parent_win.clone();
            dismiss_dialog(&dialog);
            glib::idle_add_local_once(move || {
                mount_uri_with_parent(&parent_win, &uri, &label, add_bm, on_mounted);
            });
        })
    };

    {
        let run = run.clone();
        connect.connect_clicked(move |_| run());
    }
    {
        let run = run.clone();
        let server = server.clone();
        server.connect_activate(move |_| run());
    }

    dialog.present();
    server.grab_focus();
}

/// Sidebar / Go menu: list remembered connections, then connect or add new.
pub fn show_network_picker(
    parent: &impl IsA<gtk::Window>,
    on_mounted: Rc<dyn Fn(gio::File)>,
) {
    let connections = places::load_network_connections();
    // No saved remotes yet → go straight to the new-connection form.
    if connections.is_empty() {
        show_connect_dialog(parent, on_mounted);
        return;
    }

    let dialog = gtk::Window::builder()
        .title("Connect to Network")
        .transient_for(parent)
        .default_width(420)
        .default_height(400)
        .build();
    gtk_theme::style_dialog(&dialog);
    wire_dismissible_dialog(&dialog, parent);

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 10);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);

    let intro = gtk::Label::new(Some("Remembered connections"));
    intro.set_xalign(0.0);
    intro.add_css_class("heading");
    vbox.append(&intro);

    let status = gtk::Label::new(Some("Double-click a server, or select one and press Connect."));
    status.add_css_class("dim-label");
    status.set_wrap(true);
    status.set_xalign(0.0);
    vbox.append(&status);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.set_activate_on_single_click(false);
    list.add_css_class("boxed-list");
    list.set_vexpand(true);

    let connections = Rc::new(std::cell::RefCell::new(connections));
    for conn in connections.borrow().iter().cloned().collect::<Vec<_>>() {
        let row = gtk::ListBoxRow::new();
        let box_ = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        box_.set_margin_start(10);
        box_.set_margin_end(6);
        box_.set_margin_top(8);
        box_.set_margin_bottom(8);

        let icon = gtk::Image::from_icon_name("folder-remote-symbolic");
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
        texts.set_hexpand(true);
        let title = gtk::Label::new(Some(&conn.label));
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let subtitle = gtk::Label::new(Some(&conn.uri));
        subtitle.set_xalign(0.0);
        subtitle.add_css_class("dim-label");
        subtitle.add_css_class("caption");
        subtitle.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        texts.append(&title);
        texts.append(&subtitle);

        let forget = gtk::Button::from_icon_name("user-trash-symbolic");
        forget.add_css_class("flat");
        forget.set_tooltip_text(Some("Forget this connection"));
        forget.set_valign(gtk::Align::Center);
        forget.set_focus_on_click(false);

        box_.append(&icon);
        box_.append(&texts);
        box_.append(&forget);
        row.set_child(Some(&box_));

        {
            let uri = conn.uri.clone();
            let list = list.clone();
            let row = row.clone();
            let connections = Rc::clone(&connections);
            forget.connect_clicked(move |_| {
                places::forget_network_connection(&uri);
                connections.borrow_mut().retain(|c| c.uri != uri);
                list.remove(&row);
            });
        }

        list.append(&row);
    }

    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .hexpand(true)
        .min_content_height(180)
        .build();
    vbox.append(&scroll);

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    let close = gtk::Button::with_label("Close");
    let new_btn = gtk::Button::with_label("New Connection…");
    let connect_btn = gtk::Button::with_label("Connect");
    connect_btn.add_css_class("suggested-action");
    buttons.append(&close);
    buttons.append(&new_btn);
    buttons.append(&connect_btn);
    vbox.append(&buttons);
    dialog.set_child(Some(&vbox));

    let start_connect = {
        let dialog = dialog.clone();
        let parent_win = parent.clone().upcast::<gtk::Window>();
        let on_mounted = Rc::clone(&on_mounted);
        let connections = Rc::clone(&connections);
        Rc::new(move |uri: String| {
            let label = connections
                .borrow()
                .iter()
                .find(|c| c.uri == uri)
                .map(|c| c.label.clone())
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| uri_display_label(&uri));

            let parent_win = parent_win.clone();
            let on_mounted = Rc::clone(&on_mounted);
            // Never destroy from inside row-activated — defer dismiss + mount.
            dismiss_dialog(&dialog);
            glib::idle_add_local_once(move || {
                mount_uri_with_parent(&parent_win, &uri, &label, false, on_mounted);
            });
        })
    };

    {
        let d = dialog.clone();
        close.connect_clicked(move |_| dismiss_dialog(&d));
    }
    {
        let d = dialog.clone();
        let parent = parent.clone().upcast::<gtk::Window>();
        let on_mounted = Rc::clone(&on_mounted);
        new_btn.connect_clicked(move |_| {
            dismiss_dialog(&d);
            let parent = parent.clone();
            let on_mounted = Rc::clone(&on_mounted);
            glib::idle_add_local_once(move || {
                show_connect_dialog(&parent, on_mounted);
            });
        });
    }

    let row_uri = {
        let connections = Rc::clone(&connections);
        move |row: &gtk::ListBoxRow| -> Option<String> {
            let idx = row.index();
            if idx < 0 {
                return None;
            }
            connections
                .borrow()
                .get(idx as usize)
                .map(|c| c.uri.clone())
        }
    };

    {
        let start_connect = Rc::clone(&start_connect);
        let row_uri = row_uri.clone();
        list.connect_row_activated(move |_, row| {
            if let Some(uri) = row_uri(row) {
                start_connect(uri);
            }
        });
    }
    {
        let start_connect = Rc::clone(&start_connect);
        let list = list.clone();
        let status = status.clone();
        connect_btn.connect_clicked(move |_| {
            let uri = list.selected_row().and_then(|row| row_uri(&row));
            if let Some(uri) = uri {
                start_connect(uri);
            } else {
                status.set_text("Select a connection first.");
            }
        });
    }

    dialog.present();
}

/// Mount `uri` (prompting for credentials if needed) then invoke `on_mounted`.
pub fn mount_and_open(
    parent: &impl IsA<gtk::Window>,
    uri: &str,
    on_mounted: Rc<dyn Fn(gio::File)>,
) {
    let label = places::load_network_connections()
        .into_iter()
        .find(|c| c.uri == uri)
        .map(|c| c.label)
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| uri_display_label(uri));
    mount_uri_with_parent(parent, uri, &label, false, on_mounted);
}

fn mount_uri_with_parent(
    parent: &impl IsA<gtk::Window>,
    uri: &str,
    label: &str,
    add_bm: bool,
    on_mounted: Rc<dyn Fn(gio::File)>,
) {
    // Already connected — open immediately (no MountOperation / no dialog).
    if remote_already_reachable(uri) {
        finish_connect_inner(add_bm, uri, label, &on_mounted);
        return;
    }

    let file = gio::File::for_uri(uri);
    let parent_win = parent.clone().upcast::<gtk::Window>();
    let op = gtk::MountOperation::new(Some(&parent_win));
    let uri_owned = uri.to_string();
    let label = label.to_string();

    file.mount_enclosing_volume(
        gio::MountMountFlags::NONE,
        Some(&op),
        None::<&gio::Cancellable>,
        move |result| {
            match result {
                Ok(()) => {
                    finish_connect_inner(add_bm, &uri_owned, &label, &on_mounted);
                }
                Err(e) if e.matches(gio::IOErrorEnum::AlreadyMounted) => {
                    finish_connect_inner(add_bm, &uri_owned, &label, &on_mounted);
                }
                Err(e) if e.matches(gio::IOErrorEnum::Cancelled) => {}
                Err(e) => {
                    show_error(
                        Some(&parent_win),
                        "Connect to Network",
                        &e.message().to_string(),
                    );
                }
            }
        },
    );
}

/// True when GVFS already has a mount covering this URI (local D-Bus check only).
fn remote_already_reachable(uri: &str) -> bool {
    for mount in network_mounts() {
        let root_uri = mount.root().uri().to_string();
        if uri == root_uri || uri.starts_with(&root_uri) || root_uri.starts_with(uri) {
            return true;
        }
    }
    false
}

fn labeled_row(title: &str, child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.set_width_chars(10);
    row.append(&label);
    row.append(child);
    row
}

fn finish_connect_inner(
    add_bm: bool,
    uri: &str,
    label: &str,
    on_mounted: &Rc<dyn Fn(gio::File)>,
) {
    places::remember_network_connection(uri, label);
    if add_bm {
        places::add_bookmark_uri(uri, label);
    }
    // Never call find_enclosing_mount() here — it is synchronous and can hang
    // indefinitely on GVFS/SFTP, freezing the UI.
    best_effort_home_link(uri, label);
    sync_home_shortcuts();
    on_mounted(gio::File::for_uri(uri));
}

fn best_effort_home_link(uri: &str, _label: &str) {
    // Only touch mounts we already know about — never path.exists() on GVFS
    // (that is synchronous network I/O and freezes the UI).
    for mount in network_mounts() {
        let root = mount.root();
        let root_uri = root.uri().to_string();
        if uri == root_uri || uri.starts_with(&root_uri) || root_uri.starts_with(uri) {
            let _ = ensure_home_shortcut(&mount);
            return;
        }
    }
}

fn apply_uri_prefill(
    uri: &str,
    type_drop: &gtk::DropDown,
    server: &gtk::Entry,
    port: &gtk::Entry,
    folder: &gtk::Entry,
    user: &gtk::Entry,
) {
    let file = gio::File::for_uri(uri);
    let scheme = file
        .uri_scheme()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let idx = match scheme.as_str() {
        "sftp" | "ssh" => 0,
        "ftp" | "ftps" => 1,
        "smb" => 2,
        "dav" | "davs" => 3,
        _ => 0,
    };
    type_drop.set_selected(idx);

    if let Some(rest) = uri.split("://").nth(1) {
        let (auth, path) = match rest.split_once('/') {
            Some((a, p)) => (a, format!("/{p}")),
            None => (rest, "/".to_string()),
        };
        folder.set_text(&path);
        let (user_part, host_part) = match auth.rsplit_once('@') {
            Some((u, h)) => (u, h),
            None => ("", auth),
        };
        if !user_part.is_empty() {
            user.set_text(user_part);
        }
        if let Some((host, p)) = host_part.rsplit_once(':') {
            if p.chars().all(|c| c.is_ascii_digit()) {
                server.set_text(host);
                port.set_text(p);
            } else {
                server.set_text(host_part);
            }
        } else {
            server.set_text(host_part);
        }
    }
}

fn uri_display_label(uri: &str) -> String {
    if let Some(rest) = uri.split("://").nth(1) {
        let auth = rest.split('/').next().unwrap_or(rest);
        if !auth.is_empty() {
            return auth.to_string();
        }
    }
    uri.to_string()
}

/// Icon name for a network mount row.
pub fn network_mount_icon(_mount: &gio::Mount) -> &'static str {
    "folder-remote-symbolic"
}

/// Whether `path` is under `~/Network` or a GVFS network fuse path.
#[allow(dead_code)]
pub fn is_network_local_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.contains("/gvfs/") || path.starts_with(network_home_dir())
}
