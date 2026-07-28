use std::path::PathBuf;

use gtk4 as gtk;
use gtk::gio;
use gtk::prelude::*;

use crate::plugin::activatable::{Plugin, PluginInfo, WindowActivatable, WindowContext};
use crate::plugins::{info, make_factory};

struct QuickOpenPlugin {
    info: PluginInfo,
    action: Option<gio::SimpleAction>,
}

impl Plugin for QuickOpenPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn as_window(&mut self) -> Option<&mut dyn WindowActivatable> {
        Some(self)
    }
}

impl WindowActivatable for QuickOpenPlugin {
    fn activate(&mut self, ctx: &WindowContext) {
        let win = ctx.window.clone();
        let action = gio::SimpleAction::new("quick-open", None);
        action.connect_activate(move |_, _| show_quick_open(&win));
        ctx.window.add_action(&action);
        ctx.menu_icons.borrow_mut().append(
            &ctx.tools_menu,
            "Quick Open…",
            "win.quick-open",
            "document-open-symbolic",
        );
        self.action = Some(action);
    }

    fn deactivate(&mut self) {
        self.action = None;
    }
}

fn show_quick_open(win: &gtk::ApplicationWindow) {
    let dialog = gtk::Window::builder()
        .title("Quick Open")
        .transient_for(win)
        .modal(true)
        .default_width(480)
        .default_height(360)
        .build();
    gtk_theme::style_dialog(&dialog);

    let entry = gtk::Entry::builder()
        .placeholder_text("Filter files…")
        .hexpand(true)
        .build();
    let list = gtk::ListBox::new();
    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .build();

    let root = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let files = collect_files(&root, 400);

    let populate = {
        let list = list.clone();
        let files = files.clone();
        move |filter: &str| {
            while let Some(c) = list.first_child() {
                list.remove(&c);
            }
            let filter = filter.to_lowercase();
            for path in files.iter().filter(|p| {
                filter.is_empty()
                    || p.to_string_lossy().to_lowercase().contains(&filter)
            }) {
                let row = gtk::ListBoxRow::new();
                let label = gtk::Label::new(Some(&path.display().to_string()));
                label.set_halign(gtk::Align::Start);
                label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                label.set_margin_start(8);
                label.set_margin_top(3);
                label.set_margin_bottom(3);
                row.set_child(Some(&label));
                unsafe {
                    row.set_data("path", path.clone());
                }
                list.append(&row);
            }
        }
    };
    populate("");

    {
        let populate = populate.clone();
        entry.connect_changed(move |e| {
            populate(&e.text());
        });
    }

    {
        let win2 = win.clone();
        let d = dialog.clone();
        list.connect_row_activated(move |_, row| {
            let path = unsafe {
                row.data::<PathBuf>("path")
                    .map(|p| p.as_ref().clone())
            };
            if let Some(path) = path {
                crate::window::open_path_in_window(&win2, &path);
            }
            d.close();
        });
    }

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 6);
    outer.set_margin_top(8);
    outer.set_margin_bottom(8);
    outer.set_margin_start(8);
    outer.set_margin_end(8);
    outer.append(&entry);
    outer.append(&scroll);
    dialog.set_child(Some(&outer));
    dialog.present();
    entry.grab_focus();
}

fn collect_files(root: &std::path::Path, limit: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if out.len() >= limit {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
                if out.len() >= limit {
                    break;
                }
            }
        }
    }
    out.sort();
    out
}

pub fn factory() -> (PluginInfo, crate::plugin::activatable::PluginFactory) {
    let i = info(
        "quickopen",
        "Quick Open",
        "Quickly open files from your home directory.",
    );
    make_factory(i.clone(), move || {
        Box::new(QuickOpenPlugin {
            info: i.clone(),
            action: None,
        })
    })
}
