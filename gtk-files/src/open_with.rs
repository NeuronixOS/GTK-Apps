//! Open With dialog: pick an app for a MIME type and optionally set it as default.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gio;
use gtk::prelude::*;

use crate::util::{self, FILE_ATTRIBUTES};

pub fn show_open_with(parent: Option<&impl IsA<gtk::Window>>, paths: &[PathBuf]) {
    let Some(path) = paths.first() else {
        return;
    };
    if path.is_dir() {
        util::show_error(parent, "Open With", "Folders cannot be opened with an application.");
        return;
    }

    let file = gio::File::for_path(path);
    let info = match file.query_info(
        FILE_ATTRIBUTES,
        gio::FileQueryInfoFlags::NONE,
        None::<&gio::Cancellable>,
    ) {
        Ok(i) => i,
        Err(e) => {
            util::show_error(parent, "Open With", &e.to_string());
            return;
        }
    };

    let content_type = info
        .content_type()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "application/octet-stream".into());
    let type_desc = gio::content_type_get_description(&content_type);
    let display_name = info.display_name().to_string();

    let files: Vec<gio::File> = paths
        .iter()
        .filter(|p| p.is_file())
        .map(|p| gio::File::for_path(p))
        .collect();
    if files.is_empty() {
        return;
    }

    let win = gtk::Window::builder()
        .title("Open With")
        .default_width(440)
        .default_height(520)
        .modal(true)
        .build();
    gtk_theme::style_dialog(&win);
    if let Some(p) = parent {
        win.set_transient_for(Some(p.upcast_ref()));
    }

    let header = gtk::HeaderBar::new();
    win.set_titlebar(Some(&header));

    let page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    page.set_margin_start(16);
    page.set_margin_end(16);
    page.set_margin_top(16);
    page.set_margin_bottom(16);

    let intro = gtk::Label::new(Some(&format!("Open “{display_name}”")));
    intro.set_xalign(0.0);
    intro.add_css_class("title-3");
    intro.set_wrap(true);
    page.append(&intro);

    let mime_label = gtk::Label::new(Some(&format!("{type_desc}\nMIME type: {content_type}")));
    mime_label.set_xalign(0.0);
    mime_label.add_css_class("dim-label");
    page.append(&mime_label);

    let default_heading = gtk::Label::new(Some("Current default"));
    default_heading.set_xalign(0.0);
    default_heading.add_css_class("heading");
    page.append(&default_heading);

    let default_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    default_row.set_margin_top(2);
    default_row.set_margin_bottom(4);
    let default_icon = gtk::Image::new();
    default_icon.set_pixel_size(32);
    let default_name = gtk::Label::new(None);
    default_name.set_xalign(0.0);
    default_name.set_hexpand(true);
    default_name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    default_row.append(&default_icon);
    default_row.append(&default_name);
    page.append(&default_row);

    let apps_heading = gtk::Label::new(Some("Choose an application"));
    apps_heading.set_xalign(0.0);
    apps_heading.add_css_class("heading");
    page.append(&apps_heading);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    // Single-click selects; double-click (row-activated) opens.
    list.set_activate_on_single_click(false);
    list.add_css_class("gtk-content");
    list.set_vexpand(true);

    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .min_content_height(220)
        .build();
    scroll.add_css_class("gtk-content");
    page.append(&scroll);

    let apps = collect_apps(&content_type);
    let selected: Rc<RefCell<Option<gio::AppInfo>>> = Rc::new(RefCell::new(None));

    for app in &apps {
        let row = app_list_row(app);
        list.append(&row);
    }

    refresh_default_ui(&content_type, &default_icon, &default_name);

    {
        let apps = apps.clone();
        let selected = Rc::clone(&selected);
        list.connect_row_selected(move |_, row| {
            if let Some(row) = row {
                let idx = row.index() as usize;
                *selected.borrow_mut() = apps.get(idx).cloned();
            } else {
                *selected.borrow_mut() = None;
            }
        });
    }

    // Pre-select current default if present in the list.
    if let Some(def) = gio::AppInfo::default_for_type(&content_type, false) {
        if let Some((idx, _)) = apps.iter().enumerate().find(|(_, a)| a.equal(&def)) {
            if let Some(row) = list.row_at_index(idx as i32) {
                list.select_row(Some(&row));
            }
        }
    } else if let Some(row) = list.row_at_index(0) {
        list.select_row(Some(&row));
    }

    let btn_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk::Align::End);
    let set_default = gtk_theme::labeled_button(
        gtk_theme::icon_for_label("Set as Default"),
        "Set as Default",
    );
    let cancel =
        gtk_theme::labeled_button(gtk_theme::icon_for_label("Cancel"), "Cancel");
    let open = gtk_theme::labeled_button(gtk_theme::icon_for_label("Open"), "Open");
    open.add_css_class("suggested-action");
    btn_box.append(&set_default);
    btn_box.append(&cancel);
    btn_box.append(&open);
    page.append(&btn_box);

    {
        let win = win.clone();
        cancel.connect_clicked(move |_| win.close());
    }

    {
        let content_type = content_type.clone();
        let selected = Rc::clone(&selected);
        let default_icon = default_icon.clone();
        let default_name = default_name.clone();
        let parent_win = win.clone();
        set_default.connect_clicked(move |_| {
            let Some(app) = selected.borrow().clone() else {
                util::show_error(
                    Some(&parent_win),
                    "Open With",
                    "Select an application first.",
                );
                return;
            };
            if let Err(e) = app.set_as_default_for_type(&content_type) {
                util::show_error(Some(&parent_win), "Set as Default", &e.to_string());
                return;
            }
            refresh_default_ui(&content_type, &default_icon, &default_name);
        });
    }

    {
        let win = win.clone();
        let files = files.clone();
        let selected = Rc::clone(&selected);
        let do_open = Rc::new(move || {
            let Some(app) = selected.borrow().clone() else {
                util::show_error(Some(&win), "Open With", "Select an application first.");
                return;
            };
            if let Err(e) = app.launch(&files, None::<&gio::AppLaunchContext>) {
                util::show_error(Some(&win), "Open With", &e.to_string());
                return;
            }
            win.close();
        });
        let open_btn = Rc::clone(&do_open);
        open.connect_clicked(move |_| open_btn());
        let open_row = Rc::clone(&do_open);
        list.connect_row_activated(move |_, _| open_row());
    }

    win.set_child(Some(&page));
    win.present();
}

fn collect_apps(content_type: &str) -> Vec<gio::AppInfo> {
    let mut apps = gio::AppInfo::recommended_for_type(content_type);
    let all = gio::AppInfo::all_for_type(content_type);
    for app in all {
        if !apps.iter().any(|a| a.equal(&app)) {
            apps.push(app);
        }
    }
    // Prefer apps that can open files (skip those that only handle URIs oddly).
    apps.retain(|a| a.supports_files() || a.supports_uris());
    apps.sort_by(|a, b| {
        a.name()
            .to_ascii_lowercase()
            .cmp(&b.name().to_ascii_lowercase())
    });
    apps
}

fn app_list_row(app: &gio::AppInfo) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let box_ = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    box_.set_margin_start(10);
    box_.set_margin_end(10);
    box_.set_margin_top(8);
    box_.set_margin_bottom(8);

    let image = gtk::Image::new();
    image.set_pixel_size(28);
    if let Some(icon) = app.icon() {
        image.set_from_gicon(&icon);
    } else {
        image.set_icon_name(Some("application-x-executable"));
    }

    let label = gtk::Label::new(Some(&app.name()));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);

    box_.append(&image);
    box_.append(&label);
    row.set_child(Some(&box_));
    row
}

fn refresh_default_ui(content_type: &str, icon: &gtk::Image, name: &gtk::Label) {
    match gio::AppInfo::default_for_type(content_type, false) {
        Some(app) => {
            name.set_text(&app.name());
            if let Some(gicon) = app.icon() {
                icon.set_from_gicon(&gicon);
            } else {
                icon.set_icon_name(Some("application-x-executable"));
            }
        }
        None => {
            name.set_text("No default application");
            icon.set_icon_name(Some("dialog-question-symbolic"));
        }
    }
}

#[allow(dead_code)]
pub fn content_type_for_path(path: &Path) -> Option<String> {
    let file = gio::File::for_path(path);
    let info = file
        .query_info(
            "standard::content-type",
            gio::FileQueryInfoFlags::NONE,
            None::<&gio::Cancellable>,
        )
        .ok()?;
    info.content_type().map(|c| c.to_string())
}
