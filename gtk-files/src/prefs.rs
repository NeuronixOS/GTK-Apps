//! Preferences dialog.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::prelude::*;

use crate::config::{self, Config};

pub fn show_preferences(
    parent: Option<&impl IsA<gtk::Window>>,
    config: Rc<RefCell<Config>>,
    on_apply: impl Fn(Option<&'static gtk_theme::Profile>) + 'static,
) {
    let win = gtk::Window::builder()
        .title("Preferences")
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

    let page = gtk::Box::new(gtk::Orientation::Vertical, 16);
    page.set_margin_start(16);
    page.set_margin_end(16);
    page.set_margin_top(16);
    page.set_margin_bottom(16);

    let cfg = config.borrow().clone();

    // View mode
    let view_label = gtk::Label::new(Some("Default View"));
    view_label.set_xalign(0.0);
    view_label.add_css_class("heading");
    page.append(&view_label);

    let view_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let list_radio = gtk::CheckButton::with_label("List");
    let grid_radio = gtk::CheckButton::with_label("Grid");
    grid_radio.set_group(Some(&list_radio));
    if cfg.is_grid() {
        grid_radio.set_active(true);
    } else {
        list_radio.set_active(true);
    }
    view_box.append(&list_radio);
    view_box.append(&grid_radio);
    page.append(&view_box);

    let thumb_label = gtk::Label::new(Some("Thumbnail Size (Grid)"));
    thumb_label.set_xalign(0.0);
    thumb_label.add_css_class("heading");
    page.append(&thumb_label);

    let thumb_model = gtk::StringList::new(&["Small", "Medium", "Large", "Larger", "Largest"]);
    let thumb_drop = gtk::DropDown::new(Some(thumb_model), None::<gtk::Expression>);
    let thumb_idx = match cfg.view.thumbnail_size.to_ascii_lowercase().as_str() {
        "small" => 0,
        "large" => 2,
        "larger" | "xlarge" | "x-large" => 3,
        "largest" | "xxlarge" | "xx-large" => 4,
        _ => 1, // medium / regular
    };
    thumb_drop.set_selected(thumb_idx);
    page.append(&thumb_drop);

    // Sort
    let sort_label = gtk::Label::new(Some("Sort By"));
    sort_label.set_xalign(0.0);
    sort_label.add_css_class("heading");
    page.append(&sort_label);

    let sort_model = gtk::StringList::new(&["Name", "Size", "Type", "Modified"]);
    let sort_drop = gtk::DropDown::new(Some(sort_model), None::<gtk::Expression>);
    let sort_idx = match cfg.view.sort_by.as_str() {
        "size" => 1,
        "type" => 2,
        "modified" => 3,
        _ => 0,
    };
    sort_drop.set_selected(sort_idx);
    page.append(&sort_drop);

    let folders_first = gtk::CheckButton::with_label("Sort folders before files");
    folders_first.set_active(cfg.view.sort_folders_first);
    page.append(&folders_first);

    let show_hidden = gtk::CheckButton::with_label("Show hidden files");
    show_hidden.set_active(cfg.view.show_hidden);
    page.append(&show_hidden);

    let single_click = gtk::CheckButton::with_label("Single-click to open");
    single_click.set_active(cfg.behavior.single_click);
    page.append(&single_click);

    let confirm_trash = gtk::CheckButton::with_label("Ask before moving files to Trash");
    confirm_trash.set_active(cfg.behavior.confirm_trash);
    page.append(&confirm_trash);

    let confirm_delete = gtk::CheckButton::with_label("Ask before permanently deleting");
    confirm_delete.set_active(cfg.behavior.confirm_delete);
    page.append(&confirm_delete);

    let theme_label = gtk::Label::new(Some("Profile"));
    theme_label.set_xalign(0.0);
    theme_label.add_css_class("heading");
    page.append(&theme_label);

    let (theme_drop, theme_ids) = gtk_theme::build_profile_dropdown(&gtk_theme::load_theme_id());
    page.append(&theme_drop);

    let theme_note = gtk::Label::new(Some(
        "Shared with other gtk apps (~/.config/gtk-apps/theme.toml).",
    ));
    theme_note.set_xalign(0.0);
    theme_note.set_wrap(true);
    theme_note.add_css_class("dim-label");
    page.append(&theme_note);

    let config_note = gtk::Label::new(Some(&format!(
        "Saved to:\n{}",
        config::config_path().display()
    )));
    config_note.set_xalign(0.0);
    config_note.set_wrap(true);
    config_note.add_css_class("dim-label");
    page.append(&config_note);

    let btn_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk::Align::End);
    let cancel =
        gtk_theme::labeled_button(gtk_theme::icon_for_label("Cancel"), "Cancel");
    let apply = gtk_theme::labeled_button(gtk_theme::icon_for_label("Apply"), "Apply");
    apply.add_css_class("suggested-action");
    btn_box.append(&cancel);
    btn_box.append(&apply);
    page.append(&btn_box);

    {
        let win = win.clone();
        cancel.connect_clicked(move |_| win.close());
    }

    {
        let win = win.clone();
        let config = Rc::clone(&config);
        apply.connect_clicked(move |_| {
            {
                let mut c = config.borrow_mut();
                c.view.mode = if grid_radio.is_active() {
                    "grid".into()
                } else {
                    "list".into()
                };
                c.view.sort_by = match sort_drop.selected() {
                    1 => "size".into(),
                    2 => "type".into(),
                    3 => "modified".into(),
                    _ => "name".into(),
                };
                c.view.sort_folders_first = folders_first.is_active();
                c.view.show_hidden = show_hidden.is_active();
                let thumb = match thumb_drop.selected() {
                    0 => "small",
                    2 => "large",
                    3 => "larger",
                    4 => "largest",
                    _ => "medium",
                };
                c.set_thumbnail_size(thumb);
                c.behavior.single_click = single_click.is_active();
                c.behavior.confirm_trash = confirm_trash.is_active();
                c.behavior.confirm_delete = confirm_delete.is_active();
                c.save();
            }
            let theme_profile = theme_ids
                .get(theme_drop.selected() as usize)
                .copied()
                .and_then(|id| {
                    gtk_theme::select_theme(id, |_| {});
                    gtk_theme::profile_by_id(id)
                });
            on_apply(theme_profile);
            win.close();
        });
    }

    let scroll = gtk::ScrolledWindow::builder()
        .child(&page)
        .propagate_natural_height(true)
        .build();
    win.set_child(Some(&scroll));
    win.present();
}
