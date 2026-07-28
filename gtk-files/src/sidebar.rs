//! Places sidebar (Home, XDG dirs, Computer, Trash, USB mounts, favorites, bookmarks, recent).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use crate::places;
use crate::util::{home_dir, xdg_user_dir};

#[derive(Clone)]
pub enum Place {
    Path(PathBuf),
    /// Remote / non-local bookmark (e.g. sftp://).
    Uri(String),
    Trash,
}

pub struct Sidebar {
    pub root: gtk::ScrolledWindow,
    list: gtk::ListBox,
    on_activate: RefCell<Option<Rc<dyn Fn(Place)>>>,
    on_open_tab: RefCell<Option<Rc<dyn Fn(Place)>>>,
    on_open_window: RefCell<Option<Rc<dyn Fn(Place)>>>,
    /// Keep VolumeMonitor alive for mount add/remove signals.
    _volume_monitor: gio::VolumeMonitor,
}

impl Sidebar {
    pub fn new() -> Rc<Self> {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Browse);
        list.set_activate_on_single_click(true);
        list.add_css_class("navigation-sidebar");

        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();

        let monitor = gio::VolumeMonitor::get();

        let sb = Rc::new(Self {
            root,
            list: list.clone(),
            on_activate: RefCell::new(None),
            on_open_tab: RefCell::new(None),
            on_open_window: RefCell::new(None),
            _volume_monitor: monitor.clone(),
        });

        sb.rebuild();

        {
            let sb2 = Rc::clone(&sb);
            list.connect_row_activated(move |_, row| {
                if let Some(place) = row_place(row) {
                    if let Some(cb) = sb2.on_activate.borrow().as_ref() {
                        cb(place);
                    }
                }
            });
        }
        // Also navigate on single left-click selection (more reliable than
        // activate-on-single-click alone for Recent / place rows).
        {
            let sb2 = Rc::clone(&sb);
            list.connect_row_selected(move |_, row| {
                let Some(row) = row else {
                    return;
                };
                if let Some(place) = row_place(row) {
                    if let Some(cb) = sb2.on_activate.borrow().as_ref() {
                        cb(place);
                    }
                }
            });
        }

        // Refresh Devices when USB drives appear / disappear.
        {
            let sb2 = Rc::clone(&sb);
            monitor.connect_mount_added(move |_, _| {
                let sb2 = Rc::clone(&sb2);
                glib::idle_add_local_once(move || sb2.rebuild());
            });
        }
        {
            let sb2 = Rc::clone(&sb);
            monitor.connect_mount_removed(move |_, _| {
                let sb2 = Rc::clone(&sb2);
                glib::idle_add_local_once(move || sb2.rebuild());
            });
        }
        {
            let sb2 = Rc::clone(&sb);
            monitor.connect_mount_changed(move |_, _| {
                let sb2 = Rc::clone(&sb2);
                glib::idle_add_local_once(move || sb2.rebuild());
            });
        }

        sb
    }

    pub fn set_on_activate<F: Fn(Place) + 'static>(&self, f: F) {
        *self.on_activate.borrow_mut() = Some(Rc::new(f));
    }

    pub fn set_on_open_tab<F: Fn(Place) + 'static>(&self, f: F) {
        *self.on_open_tab.borrow_mut() = Some(Rc::new(f));
    }

    pub fn set_on_open_window<F: Fn(Place) + 'static>(&self, f: F) {
        *self.on_open_window.borrow_mut() = Some(Rc::new(f));
    }

    pub fn rebuild(self: &Rc<Self>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        let places_fixed: Vec<(&str, &str, Place)> = vec![
            ("user-home-symbolic", "Home", Place::Path(home_dir())),
            (
                "user-desktop-symbolic",
                "Desktop",
                Place::Path(xdg_user_dir("DESKTOP", "Desktop")),
            ),
            (
                "folder-documents-symbolic",
                "Documents",
                Place::Path(xdg_user_dir("DOCUMENTS", "Documents")),
            ),
            (
                "folder-download-symbolic",
                "Downloads",
                Place::Path(xdg_user_dir("DOWNLOAD", "Downloads")),
            ),
            (
                "folder-music-symbolic",
                "Music",
                Place::Path(xdg_user_dir("MUSIC", "Music")),
            ),
            (
                "folder-pictures-symbolic",
                "Pictures",
                Place::Path(xdg_user_dir("PICTURES", "Pictures")),
            ),
            (
                "folder-videos-symbolic",
                "Videos",
                Place::Path(xdg_user_dir("VIDEOS", "Videos")),
            ),
            (
                "drive-harddisk-symbolic",
                "Computer",
                Place::Path(PathBuf::from("/")),
            ),
            ("user-trash-symbolic", "Trash", Place::Trash),
        ];

        for (icon, label, place) in places_fixed {
            if let Place::Path(ref p) = place {
                if *p != home_dir() && p != Path::new("/") && !p.exists() {
                    continue;
                }
            }
            let row = make_row(icon, label, place.clone());
            install_place_context_menu(&row, place, Rc::clone(self), None);
            self.list.append(&row);
        }

        // Removable USB mounts only (skip internal NVMe / fixed disks).
        let usb_mounts = removable_usb_mounts();
        if !usb_mounts.is_empty() {
            self.list.append(&make_header("Devices"));
            for mount in usb_mounts {
                let row = make_mount_row(&mount, Rc::clone(self));
                self.list.append(&row);
            }
        }

        let data = places::load();

        // Favorites
        if !data.favorites.is_empty() {
            self.list.append(&make_header("Favorites"));
            for fav in &data.favorites {
                let name = fav
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| fav.display().to_string());
                let place = Place::Path(fav.clone());
                let row = make_row("starred-symbolic", &name, place.clone());
                let path = fav.clone();
                let remove = PlaceRemove::Favorite(path);
                install_place_context_menu(&row, place, Rc::clone(self), Some(remove));
                self.list.append(&row);
            }
        }

        // Bookmarks (gtk-files places.toml only)
        let bookmarks = places::load_bookmarks();
        if !bookmarks.is_empty() {
            self.list.append(&make_header("Bookmarks"));
            for bm in bookmarks {
                let label = if bm.label.is_empty() {
                    bm.path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .or_else(|| bm.uri.clone())
                        .unwrap_or_else(|| "Bookmark".into())
                } else {
                    bm.label.clone()
                };
                let place = if let Some(ref p) = bm.path {
                    Place::Path(p.clone())
                } else if let Some(ref uri) = bm.uri {
                    Place::Uri(uri.clone())
                } else {
                    Place::Path(PathBuf::from("."))
                };
                let row = make_row("user-bookmarks-symbolic", &label, place.clone());
                let remove = PlaceRemove::Bookmark {
                    path: bm.path.clone(),
                    uri: bm.uri.clone(),
                };
                install_place_context_menu(&row, place, Rc::clone(self), Some(remove));
                self.list.append(&row);
            }
        }

        // Recent folders
        if !data.recent_folders.is_empty() {
            self.list.append(&make_header("Recent"));
            for recent in &data.recent_folders {
                let name = recent
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| recent.display().to_string());
                let place = Place::Path(recent.clone());
                let row = make_row("document-open-recent-symbolic", &name, place.clone());
                install_place_context_menu(&row, place, Rc::clone(self), None);
                self.list.append(&row);
            }
        }
    }

    pub fn select_path(&self, path: &Path) {
        let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let mut i = 0;
        while let Some(row) = self.list.row_at_index(i) {
            if let Some(Place::Path(p)) = row_place(&row) {
                let pc = p.canonicalize().unwrap_or_else(|_| p.clone());
                if pc == canon || p == path {
                    self.list.select_row(Some(&row));
                    return;
                }
            }
            i += 1;
        }
        self.list.unselect_all();
    }

    pub fn select_trash(&self) {
        let mut i = 0;
        while let Some(row) = self.list.row_at_index(i) {
            if matches!(row_place(&row), Some(Place::Trash)) {
                self.list.select_row(Some(&row));
                return;
            }
            i += 1;
        }
    }
}

/// Mounted removable USB volumes suitable for the Devices sidebar section.
fn removable_usb_mounts() -> Vec<gio::Mount> {
    let monitor = gio::VolumeMonitor::get();
    let mut mounts: Vec<gio::Mount> = monitor
        .mounts()
        .into_iter()
        .filter(is_removable_usb_mount)
        .collect();
    mounts.sort_by(|a, b| {
        a.name()
            .to_ascii_lowercase()
            .cmp(&b.name().to_ascii_lowercase())
    });
    mounts
}

fn is_removable_usb_mount(mount: &gio::Mount) -> bool {
    // Never list shadow / system mounts.
    let root = mount.root();
    if let Some(path) = root.path() {
        let s = path.to_string_lossy();
        if s == "/" || s.starts_with("/boot") || s.starts_with("/snap") {
            return false;
        }
        // Internal NVMe partitions often appear under /media — exclude by device.
        if unix_device_looks_internal(&path) {
            return false;
        }
    }

    if let Some(drive) = mount.drive() {
        if drive_looks_internal(&drive) {
            return false;
        }
        // USB / SD / optical / other removable media.
        return drive.is_removable() || drive.can_eject() || mount.can_eject();
    }

    // No Drive object: only accept classic removable automount locations,
    // and only when the volume can be ejected/unmounted.
    let Some(path) = root.path() else {
        return false;
    };
    let s = path.to_string_lossy();
    let under_media = s.starts_with("/media/") || s.starts_with("/run/media/");
    under_media && (mount.can_eject() || mount.can_unmount()) && !unix_device_looks_internal(&path)
}

fn drive_looks_internal(drive: &gio::Drive) -> bool {
    let id = drive
        .identifier("unix-device")
        .unwrap_or_default()
        .to_ascii_lowercase();
    id.contains("nvme")
        || (id.contains("mmcblk") && !drive.is_removable())
        || (!drive.is_removable() && !drive.can_eject() && !id.is_empty() && !id.contains("usb"))
}

fn unix_device_looks_internal(mount_path: &Path) -> bool {
    // Resolve SOURCE from /proc/mounts for this mount point.
    let Ok(text) = std::fs::read_to_string("/proc/mounts") else {
        return false;
    };
    let target = mount_path.to_string_lossy();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(source) = parts.next() else {
            continue;
        };
        let Some(dest) = parts.next() else {
            continue;
        };
        if dest == target.as_ref() {
            let src = source.to_ascii_lowercase();
            return src.contains("nvme");
        }
    }
    false
}

fn make_header(text: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_selectable(false);
    row.set_activatable(false);
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.add_css_class("dim-label");
    label.add_css_class("caption-heading");
    label.set_margin_start(10);
    label.set_margin_end(8);
    label.set_margin_top(10);
    label.set_margin_bottom(2);
    row.set_child(Some(&label));
    row.set_widget_name("header");
    row
}

fn make_row(icon: &str, label: &str, place: Place) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let box_ = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    box_.set_margin_start(8);
    box_.set_margin_end(8);
    box_.set_margin_top(4);
    box_.set_margin_bottom(4);
    let image = gtk::Image::from_icon_name(icon);
    let lbl = gtk::Label::new(Some(label));
    lbl.set_xalign(0.0);
    lbl.set_hexpand(true);
    lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
    box_.append(&image);
    box_.append(&lbl);
    row.set_child(Some(&box_));

    // Keep a CSS-safe name for headers/trash; store the real Place in widget data
    // so paths with `/` are not lost via widget-name / CSS identifier limits.
    row.set_widget_name(match &place {
        Place::Path(_) | Place::Uri(_) => "place",
        Place::Trash => "trash",
    });
    set_row_place(&row, place);
    row
}

fn make_mount_row(mount: &gio::Mount, sidebar: Rc<Sidebar>) -> gtk::ListBoxRow {
    let name = mount.name().to_string();
    let icon_name = mount_icon_name(mount);
    let root = mount.root();
    let place = root
        .path()
        .map(Place::Path)
        .unwrap_or_else(|| Place::Uri(root.uri().to_string()));

    let row = gtk::ListBoxRow::new();
    let box_ = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    box_.set_margin_start(8);
    box_.set_margin_end(4);
    box_.set_margin_top(2);
    box_.set_margin_bottom(2);

    let image = gtk::Image::from_icon_name(&icon_name);
    let lbl = gtk::Label::new(Some(&name));
    lbl.set_xalign(0.0);
    lbl.set_hexpand(true);
    lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
    box_.append(&image);
    box_.append(&lbl);

    let can_eject = mount.can_eject() || mount.can_unmount();
    if can_eject {
        let eject = gtk::Button::from_icon_name("media-eject-symbolic");
        eject.add_css_class("flat");
        eject.add_css_class("circular");
        eject.set_tooltip_text(Some("Eject"));
        eject.set_valign(gtk::Align::Center);
        eject.set_focus_on_click(false);
        let mount = mount.clone();
        eject.connect_clicked(move |btn| {
            // Stop the ListBox from activating the row when eject is clicked.
            if let Some(row) = btn
                .ancestor(gtk::ListBoxRow::static_type())
                .and_downcast::<gtk::ListBoxRow>()
            {
                row.set_activatable(false);
                glib::idle_add_local_once({
                    let row = row.clone();
                    move || row.set_activatable(true)
                });
            }
            eject_or_unmount(&mount);
        });
        box_.append(&eject);
    }

    row.set_child(Some(&box_));
    row.set_widget_name("place");
    set_row_place(&row, place.clone());
    install_place_context_menu(&row, place, sidebar, None);
    row
}

fn mount_icon_name(mount: &gio::Mount) -> String {
    if let Some(drive) = mount.drive() {
        let id = drive
            .identifier("unix-device")
            .unwrap_or_default()
            .to_ascii_lowercase();
        if id.contains("sr") || id.contains("cdrom") {
            return "media-optical-symbolic".into();
        }
    }
    "drive-removable-media-symbolic".into()
}

fn eject_or_unmount(mount: &gio::Mount) {
    let mount = mount.clone();
    if mount.can_eject() {
        mount.eject_with_operation(
            gio::MountUnmountFlags::NONE,
            None::<&gio::MountOperation>,
            None::<&gio::Cancellable>,
            move |res| {
                if let Err(e) = res {
                    eprintln!("gtk-files: eject failed: {e}");
                }
            },
        );
    } else if mount.can_unmount() {
        mount.unmount_with_operation(
            gio::MountUnmountFlags::NONE,
            None::<&gio::MountOperation>,
            None::<&gio::Cancellable>,
            move |res| {
                if let Err(e) = res {
                    eprintln!("gtk-files: unmount failed: {e}");
                }
            },
        );
    }
}

enum PlaceRemove {
    Favorite(PathBuf),
    Bookmark {
        path: Option<PathBuf>,
        uri: Option<String>,
    },
}

impl PlaceRemove {
    fn label(&self) -> &'static str {
        match self {
            PlaceRemove::Favorite(_) => "Remove from Favorites",
            PlaceRemove::Bookmark { .. } => "Remove Bookmark",
        }
    }
}

fn install_place_context_menu(
    row: &gtk::ListBoxRow,
    place: Place,
    sidebar: Rc<Sidebar>,
    remove: Option<PlaceRemove>,
) {
    let group = gio::SimpleActionGroup::new();
    {
        let sidebar = Rc::clone(&sidebar);
        let place = place.clone();
        let act = gio::SimpleAction::new("open-tab", None);
        act.connect_activate(move |_, _| {
            if let Some(cb) = sidebar.on_open_tab.borrow().as_ref() {
                cb(place.clone());
            }
        });
        group.add_action(&act);
    }
    {
        let sidebar = Rc::clone(&sidebar);
        let place = place.clone();
        let act = gio::SimpleAction::new("open-window", None);
        act.connect_activate(move |_, _| {
            if let Some(cb) = sidebar.on_open_window.borrow().as_ref() {
                cb(place.clone());
            }
        });
        group.add_action(&act);
    }
    let remove_label = remove.as_ref().map(|r| r.label());
    if let Some(remove) = remove {
        let sidebar = Rc::clone(&sidebar);
        let act = gio::SimpleAction::new("remove", None);
        act.connect_activate(move |_, _| {
            let removed = match &remove {
                PlaceRemove::Favorite(p) => {
                    places::remove_favorite(p);
                    true
                }
                PlaceRemove::Bookmark { path, uri } => {
                    if let Some(p) = path {
                        places::remove_bookmark(p)
                    } else if let Some(u) = uri {
                        places::remove_bookmark_uri(u)
                    } else {
                        false
                    }
                }
            };
            if removed {
                sidebar.rebuild();
            }
        });
        group.add_action(&act);
    }
    row.insert_action_group("place", Some(&group));

    let menu = gio::Menu::new();
    let mut icons = gtk_theme::IconMenu::new();
    let open = gio::Menu::new();
    icons.append(
        &open,
        "Open in New Tab",
        "place.open-tab",
        "tab-new-symbolic",
    );
    icons.append(
        &open,
        "Open in New Window",
        "place.open-window",
        "window-new-symbolic",
    );
    menu.append_section(None, &open);

    if let Some(label) = remove_label {
        let danger = gio::Menu::new();
        icons.append(&danger, label, "place.remove", "list-remove-symbolic");
        menu.append_section(None, &danger);
    }

    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    icons.bind_popover(&popover);
    popover.set_parent(row);
    popover.set_has_arrow(false);
    {
        let popover_weak = popover.downgrade();
        row.connect_destroy(move |_| {
            if let Some(p) = popover_weak.upgrade() {
                p.unparent();
            }
        });
    }

    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);
    {
        let popover = popover.clone();
        gesture.connect_pressed(move |g, _, x, y| {
            g.set_state(gtk::EventSequenceState::Claimed);
            popover.set_pointing_to(Some(&gdk::Rectangle::new(
                x.round() as i32,
                y.round() as i32,
                1,
                1,
            )));
            popover.popup();
        });
    }
    row.add_controller(gesture);

    // Middle-click → open in a new tab (Home, bookmarks, Recent, Devices, Trash…).
    let middle = gtk::GestureClick::new();
    middle.set_button(gdk::BUTTON_MIDDLE);
    {
        let sidebar = Rc::clone(&sidebar);
        let place = place.clone();
        middle.connect_pressed(move |g, _, _, _| {
            g.set_state(gtk::EventSequenceState::Claimed);
            if let Some(cb) = sidebar.on_open_tab.borrow().as_ref() {
                cb(place.clone());
            }
        });
    }
    row.add_controller(middle);
}

fn set_row_place(row: &gtk::ListBoxRow, place: Place) {
    // Store as a plain String — more reliable than boxing the Place enum via set_data.
    let encoded = match place {
        Place::Trash => "trash".to_string(),
        Place::Path(p) => format!("path:{}", p.to_string_lossy()),
        Place::Uri(u) => format!("uri:{u}"),
    };
    unsafe {
        row.set_data("gtk-files-place", encoded);
    }
}

fn row_place(row: &gtk::ListBoxRow) -> Option<Place> {
    let name = row.widget_name();
    if name == "header" {
        return None;
    }
    unsafe {
        if let Some(ptr) = row.data::<String>("gtk-files-place") {
            let encoded = ptr.as_ref();
            if encoded == "trash" {
                return Some(Place::Trash);
            }
            if let Some(rest) = encoded.strip_prefix("path:") {
                return Some(Place::Path(PathBuf::from(rest)));
            }
            if let Some(rest) = encoded.strip_prefix("uri:") {
                return Some(Place::Uri(rest.to_string()));
            }
        }
    }
    if name == "trash" {
        return Some(Place::Trash);
    }
    None
}
