//! File / folder properties dialog.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use gtk4 as gtk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use crate::util::{
    content_type_description, format_mtime, format_size, icon_for_info, FILE_ATTRIBUTES,
};

#[derive(Debug, Clone, Default)]
struct SelectionTotals {
    /// Folders found under the selection (recursive; does not count selected roots).
    folders: u64,
    /// Files found under the selection (recursive), plus selected files themselves.
    files: u64,
    /// Total byte size of all files counted.
    size: u64,
    /// Walk errors (permission denied, etc.) — informational only.
    errors: u64,
}

/// Show properties for one or more selected paths (recursive size / counts).
pub fn show_properties(parent: Option<&impl IsA<gtk::Window>>, paths: &[PathBuf]) {
    if paths.is_empty() {
        return;
    }

    let win = gtk::Window::builder()
        .title(if paths.len() == 1 {
            "Properties"
        } else {
            "Properties — Multiple Items"
        })
        .default_width(440)
        .default_height(400)
        .modal(true)
        .build();
    gtk_theme::style_dialog(&win);
    if let Some(p) = parent {
        win.set_transient_for(Some(p.upcast_ref()));
    }

    let header = gtk::HeaderBar::new();
    win.set_titlebar(Some(&header));

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 12);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let image = gtk::Image::new();
    image.set_pixel_size(64);
    let title = gtk::Label::new(None);
    title.add_css_class("title-2");
    title.set_xalign(0.0);
    title.set_wrap(true);
    top.append(&image);
    top.append(&title);
    vbox.append(&top);

    let grid = gtk::Grid::builder()
        .row_spacing(8)
        .column_spacing(12)
        .build();

    let mut row = 0;

    if paths.len() == 1 {
        let path = &paths[0];
        let file = gio::File::for_path(path);
        let info = match file.query_info(
            FILE_ATTRIBUTES,
            gio::FileQueryInfoFlags::NONE,
            None::<&gio::Cancellable>,
        ) {
            Ok(i) => i,
            Err(e) => {
                crate::util::show_error(parent, "Properties", &e.to_string());
                return;
            }
        };
        image.set_from_gicon(&icon_for_info(&info, false));
        title.set_text(&info.display_name());
        add_row(&grid, &mut row, "Type", &content_type_description(&info));
        add_row(
            &grid,
            &mut row,
            "Location",
            &path.parent().unwrap_or(path).display().to_string(),
        );
        add_row(&grid, &mut row, "Modified", &format_mtime(&info));
        if let Ok(meta) = std::fs::metadata(path) {
            use std::os::unix::fs::PermissionsExt;
            let mode = meta.permissions().mode() & 0o777;
            add_row(&grid, &mut row, "Permissions", &format!("{mode:o}"));
        }
    } else {
        image.set_icon_name(Some("document-multiple-symbolic"));
        title.set_text(&format!("{} items selected", paths.len()));
        add_row(&grid, &mut row, "Selection", &format!("{} items", paths.len()));
        let parents: Vec<_> = paths
            .iter()
            .filter_map(|p| p.parent().map(|p| p.display().to_string()))
            .collect();
        let location = if parents.windows(2).all(|w| w[0] == w[1]) {
            parents.first().cloned().unwrap_or_default()
        } else {
            "Multiple locations".into()
        };
        add_row(&grid, &mut row, "Location", &location);
    }

    let size_value = add_row_label(&grid, &mut row, "Size", "Calculating…");
    let contents_value = add_row_label(&grid, &mut row, "Contents", "Calculating…");

    vbox.append(&grid);

    let close = gtk_theme::labeled_button(gtk_theme::icon_for_label("Close"), "Close");
    close.add_css_class("suggested-action");
    close.set_halign(gtk::Align::End);
    {
        let win = win.clone();
        close.connect_clicked(move |_| win.close());
    }
    vbox.append(&close);

    win.set_child(Some(&vbox));
    win.present();

    // Recursive totals on a background thread so large trees don't freeze the UI.
    let paths = paths.to_vec();
    let (tx, rx) = mpsc::channel::<SelectionTotals>();
    thread::spawn(move || {
        let totals = compute_selection_totals(&paths);
        let _ = tx.send(totals);
    });

    let size_value = size_value.clone();
    let contents_value = contents_value.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        match rx.try_recv() {
            Ok(totals) => {
                size_value.set_text(&format_size(totals.size));
                let mut contents = format!(
                    "{} folders, {} files",
                    totals.folders, totals.files
                );
                if totals.errors > 0 {
                    contents.push_str(&format!(" ({} inaccessible)", totals.errors));
                }
                contents_value.set_text(&contents);
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                size_value.set_text("—");
                contents_value.set_text("Could not calculate");
                glib::ControlFlow::Break
            }
        }
    });
}

fn add_row(grid: &gtk::Grid, row: &mut i32, key: &str, value: &str) {
    let _ = add_row_label(grid, row, key, value);
}

fn add_row_label(grid: &gtk::Grid, row: &mut i32, key: &str, value: &str) -> gtk::Label {
    let k = gtk::Label::new(Some(key));
    k.add_css_class("dim-label");
    k.set_xalign(1.0);
    let v = gtk::Label::new(Some(value));
    v.set_xalign(0.0);
    v.set_wrap(true);
    v.set_hexpand(true);
    v.set_selectable(true);
    grid.attach(&k, 0, *row, 1, 1);
    grid.attach(&v, 1, *row, 1, 1);
    *row += 1;
    v
}

/// Recursively total size / file / folder counts for everything under `paths`.
fn compute_selection_totals(paths: &[PathBuf]) -> SelectionTotals {
    let mut totals = SelectionTotals::default();
    for path in paths {
        accumulate_selected_root(path, &mut totals);
    }
    totals
}

/// Count a top-level selected item: files contribute themselves; folders contribute
/// everything *under* them (recursive).
fn accumulate_selected_root(path: &Path, totals: &mut SelectionTotals) {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => {
            totals.errors += 1;
            return;
        }
    };
    let ft = meta.file_type();
    if ft.is_dir() && !ft.is_symlink() {
        walk_dir_contents(path, totals);
    } else {
        // Regular file, symlink, or other — count as one file.
        totals.files += 1;
        totals.size += meta.len();
    }
}

fn walk_dir_contents(path: &Path, totals: &mut SelectionTotals) {
    let rd = match std::fs::read_dir(path) {
        Ok(rd) => rd,
        Err(_) => {
            totals.errors += 1;
            return;
        }
    };
    for entry in rd {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                totals.errors += 1;
                continue;
            }
        };
        let child = entry.path();
        let meta = match std::fs::symlink_metadata(&child) {
            Ok(m) => m,
            Err(_) => {
                totals.errors += 1;
                continue;
            }
        };
        let ft = meta.file_type();
        if ft.is_dir() && !ft.is_symlink() {
            totals.folders += 1;
            walk_dir_contents(&child, totals);
        } else {
            totals.files += 1;
            totals.size += meta.len();
        }
    }
}
