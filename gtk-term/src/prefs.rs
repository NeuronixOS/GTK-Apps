//! Preferences dialog (Text + Scrolling), modeled on gnome-terminal.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::glib;
use gtk::prelude::*;

use crate::config::{self, Config, CursorBlinkSetting};

type ApplyFn = Rc<dyn Fn(&Config)>;

/// Show Preferences. `on_apply` runs after each change / on close with the
/// latest config so the caller can push settings to open terminals.
pub fn show_preferences(
    parent: &impl IsA<gtk::Window>,
    config: Rc<RefCell<Config>>,
    on_apply: impl Fn(&Config) + 'static,
) {
    let win = gtk::Window::builder()
        .title("Preferences")
        .default_width(560)
        .default_height(480)
        .modal(true)
        .build();
    gtk_theme::style_dialog(&win);
    win.set_transient_for(Some(parent.upcast_ref()));

    let header = gtk::HeaderBar::new();
    win.set_titlebar(Some(&header));

    let notebook = gtk::Notebook::new();
    notebook.set_hexpand(true);
    notebook.set_vexpand(true);
    notebook.set_margin_start(8);
    notebook.set_margin_end(8);
    notebook.set_margin_top(8);
    notebook.set_margin_bottom(8);

    let apply: ApplyFn = Rc::new(on_apply);
    let text_page = build_text_page(Rc::clone(&config), Rc::clone(&apply));
    let scroll_page = build_scrolling_page(Rc::clone(&config), Rc::clone(&apply));

    notebook.append_page(&text_page, Some(&gtk::Label::new(Some("Text"))));
    notebook.append_page(&scroll_page, Some(&gtk::Label::new(Some("Scrolling"))));

    win.set_child(Some(&notebook));

    {
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        win.connect_close_request(move |_| {
            config::save(&config.borrow());
            apply(&config.borrow());
            glib::Propagation::Proceed
        });
    }

    win.present();
}

fn persist_and_apply(config: &Rc<RefCell<Config>>, apply: &ApplyFn) {
    config::save(&config.borrow());
    apply(&config.borrow());
}

fn section_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.add_css_class("heading");
    label.set_margin_top(8);
    label.set_margin_bottom(4);
    label
}

fn build_text_page(config: Rc<RefCell<Config>>, apply: ApplyFn) -> gtk::ScrolledWindow {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 10);
    page.set_margin_start(16);
    page.set_margin_end(16);
    page.set_margin_top(12);
    page.set_margin_bottom(16);

    let cfg = config.borrow().clone();

    // ---- Text Appearance ----
    page.append(&section_label("Text Appearance"));

    // Initial terminal size
    let size_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let size_label = gtk::Label::new(Some("Initial terminal size:"));
    size_label.set_xalign(0.0);
    size_label.set_hexpand(true);

    let cols_spin = gtk::SpinButton::with_range(1.0, 512.0, 1.0);
    cols_spin.set_value(cfg.columns as f64);
    cols_spin.set_tooltip_text(Some("Columns"));
    let cols_cap = gtk::Label::new(Some("columns"));

    let rows_spin = gtk::SpinButton::with_range(1.0, 256.0, 1.0);
    rows_spin.set_value(cfg.rows as f64);
    rows_spin.set_tooltip_text(Some("Rows"));
    let rows_cap = gtk::Label::new(Some("rows"));

    let size_reset = gtk::Button::with_label("Reset");
    size_reset.set_tooltip_text(Some("Reset to 80×24"));

    size_row.append(&size_label);
    size_row.append(&cols_spin);
    size_row.append(&cols_cap);
    size_row.append(&rows_spin);
    size_row.append(&rows_cap);
    size_row.append(&size_reset);
    page.append(&size_row);

    {
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        cols_spin.connect_value_changed(move |s| {
            config.borrow_mut().columns = s.value() as i64;
            persist_and_apply(&config, &apply);
        });
    }
    {
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        rows_spin.connect_value_changed(move |s| {
            config.borrow_mut().rows = s.value() as i64;
            persist_and_apply(&config, &apply);
        });
    }
    {
        let cols_spin = cols_spin.clone();
        let rows_spin = rows_spin.clone();
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        size_reset.connect_clicked(move |_| {
            cols_spin.set_value(80.0);
            rows_spin.set_value(24.0);
            {
                let mut c = config.borrow_mut();
                c.columns = 80;
                c.rows = 24;
            }
            persist_and_apply(&config, &apply);
        });
    }

    // Custom font
    let font_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let font_check = gtk::CheckButton::with_label("Custom font");
    font_check.set_active(cfg.use_custom_font);
    font_check.set_hexpand(true);

    let font_dialog = gtk::FontDialog::builder().modal(true).build();
    let font_btn = gtk::FontDialogButton::new(Some(font_dialog));
    let desc = gtk::pango::FontDescription::from_string(&cfg.font);
    font_btn.set_font_desc(&desc);
    font_btn.set_sensitive(cfg.use_custom_font);
    font_btn.set_use_font(true);
    font_btn.set_level(gtk::FontLevel::Font);

    font_row.append(&font_check);
    font_row.append(&font_btn);
    page.append(&font_row);

    {
        let font_btn = font_btn.clone();
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        font_check.connect_toggled(move |c| {
            let on = c.is_active();
            font_btn.set_sensitive(on);
            config.borrow_mut().use_custom_font = on;
            persist_and_apply(&config, &apply);
        });
    }
    {
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        font_btn.connect_notify_local(Some("font-desc"), move |btn, _| {
            if let Some(desc) = btn.font_desc() {
                config.borrow_mut().font = desc.to_str().to_string();
                persist_and_apply(&config, &apply);
            }
        });
    }

    // Cell spacing
    let cell_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let cell_label = gtk::Label::new(Some("Cell spacing:"));
    cell_label.set_xalign(0.0);
    cell_label.set_hexpand(true);

    let width_spin = gtk::SpinButton::with_range(0.5, 4.0, 0.05);
    width_spin.set_digits(2);
    width_spin.set_value(cfg.cell_width_scale);
    width_spin.set_tooltip_text(Some("Width"));
    let width_cap = gtk::Label::new(Some("width"));

    let height_spin = gtk::SpinButton::with_range(0.5, 4.0, 0.05);
    height_spin.set_digits(2);
    height_spin.set_value(cfg.cell_height_scale);
    height_spin.set_tooltip_text(Some("Height"));
    let height_cap = gtk::Label::new(Some("height"));

    let cell_reset = gtk::Button::with_label("Reset");

    cell_row.append(&cell_label);
    cell_row.append(&width_spin);
    cell_row.append(&width_cap);
    cell_row.append(&height_spin);
    cell_row.append(&height_cap);
    cell_row.append(&cell_reset);
    page.append(&cell_row);

    {
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        width_spin.connect_value_changed(move |s| {
            config.borrow_mut().cell_width_scale = s.value();
            persist_and_apply(&config, &apply);
        });
    }
    {
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        height_spin.connect_value_changed(move |s| {
            config.borrow_mut().cell_height_scale = s.value();
            persist_and_apply(&config, &apply);
        });
    }
    {
        let width_spin = width_spin.clone();
        let height_spin = height_spin.clone();
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        cell_reset.connect_clicked(move |_| {
            width_spin.set_value(1.0);
            height_spin.set_value(1.0);
            {
                let mut c = config.borrow_mut();
                c.cell_width_scale = 1.0;
                c.cell_height_scale = 1.0;
            }
            persist_and_apply(&config, &apply);
        });
    }

    // Allow blinking text
    let blink_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let blink_label = gtk::Label::new(Some("Allow blinking text:"));
    blink_label.set_xalign(0.0);
    blink_label.set_hexpand(true);
    let blink_model = gtk::StringList::new(&["Never", "When focused", "When unfocused", "Always"]);
    let blink_drop = gtk::DropDown::new(Some(blink_model), None::<gtk::Expression>);
    blink_drop.set_selected(cfg.text_blink_index());
    blink_row.append(&blink_label);
    blink_row.append(&blink_drop);
    page.append(&blink_row);

    {
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        blink_drop.connect_selected_notify(move |d| {
            config.borrow_mut().text_blink = Config::text_blink_from_index(d.selected());
            persist_and_apply(&config, &apply);
        });
    }

    // ---- Cursor ----
    page.append(&section_label("Cursor"));

    let shape_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let shape_label = gtk::Label::new(Some("Cursor shape:"));
    shape_label.set_xalign(0.0);
    shape_label.set_hexpand(true);
    let shape_model = gtk::StringList::new(&["Block", "I-Beam", "Underline"]);
    let shape_drop = gtk::DropDown::new(Some(shape_model), None::<gtk::Expression>);
    shape_drop.set_selected(cfg.cursor_shape_index());
    shape_row.append(&shape_label);
    shape_row.append(&shape_drop);
    page.append(&shape_row);

    {
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        shape_drop.connect_selected_notify(move |d| {
            config.borrow_mut().cursor_shape = Config::cursor_shape_from_index(d.selected());
            persist_and_apply(&config, &apply);
        });
    }

    let cblink_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let cblink_label = gtk::Label::new(Some("Cursor blinking:"));
    cblink_label.set_xalign(0.0);
    cblink_label.set_hexpand(true);
    let cblink_model = gtk::StringList::new(&["Default", "On", "Off"]);
    let cblink_drop = gtk::DropDown::new(Some(cblink_model), None::<gtk::Expression>);
    cblink_drop.set_selected(cfg.cursor_blink_mode.as_index());
    cblink_row.append(&cblink_label);
    cblink_row.append(&cblink_drop);
    page.append(&cblink_row);

    {
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        cblink_drop.connect_selected_notify(move |d| {
            config.borrow_mut().cursor_blink_mode = CursorBlinkSetting::from_index(d.selected());
            persist_and_apply(&config, &apply);
        });
    }

    // ---- Sound ----
    page.append(&section_label("Sound"));

    let bell = gtk::CheckButton::with_label("Terminal bell");
    bell.set_active(cfg.audible_bell);
    page.append(&bell);
    {
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        bell.connect_toggled(move |c| {
            config.borrow_mut().audible_bell = c.is_active();
            persist_and_apply(&config, &apply);
        });
    }

    let scroller = gtk::ScrolledWindow::builder()
        .child(&page)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .build();
    scroller
}

fn build_scrolling_page(config: Rc<RefCell<Config>>, apply: ApplyFn) -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 10);
    page.set_margin_start(16);
    page.set_margin_end(16);
    page.set_margin_top(12);
    page.set_margin_bottom(16);

    let cfg = config.borrow().clone();

    let show_sb = gtk::CheckButton::with_label("Show scrollbar");
    show_sb.set_active(cfg.show_scrollbar);
    page.append(&show_sb);

    let scroll_out = gtk::CheckButton::with_label("Scroll on output");
    scroll_out.set_active(cfg.scroll_on_output);
    page.append(&scroll_out);

    let scroll_key = gtk::CheckButton::with_label("Scroll on keystroke");
    scroll_key.set_active(cfg.scroll_on_keystroke);
    page.append(&scroll_key);

    let scroll_paste = gtk::CheckButton::with_label("Scroll on paste");
    scroll_paste.set_active(cfg.scroll_on_paste);
    page.append(&scroll_paste);

    let limit_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let limit_check = gtk::CheckButton::with_label("Limit scrollback to:");
    limit_check.set_active(cfg.limit_scrollback);
    let lines_spin = gtk::SpinButton::with_range(0.0, 1_000_000.0, 100.0);
    lines_spin.set_value(cfg.scrollback_lines as f64);
    lines_spin.set_sensitive(cfg.limit_scrollback);
    let lines_cap = gtk::Label::new(Some("lines"));
    limit_row.append(&limit_check);
    limit_row.append(&lines_spin);
    limit_row.append(&lines_cap);
    page.append(&limit_row);

    let warn = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    warn.add_css_class("warning");
    warn.set_margin_top(12);
    let warn_icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
    let warn_label = gtk::Label::new(Some(
        "Warning! Large scrollback buffers may lead to exhaustion of system resources.",
    ));
    warn_label.set_wrap(true);
    warn_label.set_xalign(0.0);
    warn_label.set_hexpand(true);
    warn.append(&warn_icon);
    warn.append(&warn_label);
    page.append(&warn);

    {
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        show_sb.connect_toggled(move |c| {
            config.borrow_mut().show_scrollbar = c.is_active();
            persist_and_apply(&config, &apply);
        });
    }
    {
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        scroll_out.connect_toggled(move |c| {
            config.borrow_mut().scroll_on_output = c.is_active();
            persist_and_apply(&config, &apply);
        });
    }
    {
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        scroll_key.connect_toggled(move |c| {
            config.borrow_mut().scroll_on_keystroke = c.is_active();
            persist_and_apply(&config, &apply);
        });
    }
    {
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        scroll_paste.connect_toggled(move |c| {
            config.borrow_mut().scroll_on_paste = c.is_active();
            persist_and_apply(&config, &apply);
        });
    }
    {
        let lines_spin = lines_spin.clone();
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        limit_check.connect_toggled(move |c| {
            let on = c.is_active();
            lines_spin.set_sensitive(on);
            config.borrow_mut().limit_scrollback = on;
            persist_and_apply(&config, &apply);
        });
    }
    {
        let config = Rc::clone(&config);
        let apply = Rc::clone(&apply);
        lines_spin.connect_value_changed(move |s| {
            config.borrow_mut().scrollback_lines = s.value() as i64;
            persist_and_apply(&config, &apply);
        });
    }

    page
}
