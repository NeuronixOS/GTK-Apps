//! Preferences dialog mirroring gedit preference pages.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::glib;
use gtk::pango;
use gtk::prelude::*;

use crate::config::Config;
use crate::plugin::PluginEngine;

/// Available GtkSourceView style schemes: (id, display name).
fn list_style_schemes() -> Vec<(String, String)> {
    let sm = sourceview5::StyleSchemeManager::default();
    let mut schemes: Vec<(String, String)> = sm
        .scheme_ids()
        .into_iter()
        .map(|id| {
            let id = id.to_string();
            let name = sm
                .scheme(&id)
                .map(|s| {
                    let n = s.name().to_string();
                    if n.is_empty() {
                        id.clone()
                    } else {
                        n
                    }
                })
                .unwrap_or_else(|| id.clone());
            (id, name)
        })
        .collect();
    schemes.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    schemes
}

fn build_scheme_dropdown(current_id: &str) -> (gtk::DropDown, Rc<Vec<String>>) {
    let schemes = list_style_schemes();
    let ids: Rc<Vec<String>> = Rc::new(schemes.iter().map(|(id, _)| id.clone()).collect());
    let labels: Vec<&str> = schemes.iter().map(|(_, name)| name.as_str()).collect();
    let drop = gtk::DropDown::from_strings(&labels);
    drop.set_hexpand(true);
    if let Some(idx) = ids.iter().position(|id| id == current_id) {
        drop.set_selected(idx as u32);
    } else if !ids.is_empty() {
        // Prefer a sensible default if config id is missing
        let fallback = ids
            .iter()
            .position(|id| id == "Adwaita" || id == "classic" || id == "tango")
            .unwrap_or(0);
        drop.set_selected(fallback as u32);
    }
    (drop, ids)
}

fn build_font_button(current: &str, use_default: bool) -> gtk::FontDialogButton {
    let dialog = gtk::FontDialog::builder()
        .title("Editor Font")
        .modal(true)
        .build();

    let btn = gtk::FontDialogButton::new(Some(dialog));
    let desc = pango::FontDescription::from_string(current);
    btn.set_font_desc(&desc);
    btn.set_use_font(true);
    btn.set_use_size(true);
    btn.set_hexpand(true);
    btn.set_sensitive(!use_default);
    btn
}

struct PrefWidgets {
    line_numbers: gtk::CheckButton,
    highlight_line: gtk::CheckButton,
    bracket: gtk::CheckButton,
    right_margin: gtk::CheckButton,
    margin_pos: gtk::SpinButton,
    wrap: gtk::DropDown,
    tab_width: gtk::SpinButton,
    insert_spaces: gtk::CheckButton,
    auto_indent: gtk::CheckButton,
    syntax: gtk::CheckButton,
    trailing: gtk::CheckButton,
    backup: gtk::CheckButton,
    autosave: gtk::CheckButton,
    autosave_int: gtk::SpinButton,
    font_default: gtk::CheckButton,
    font_btn: gtk::FontDialogButton,
    scheme_drop: gtk::DropDown,
    scheme_ids: Rc<Vec<String>>,
    theme_drop: gtk::DropDown,
    theme_ids: Vec<&'static str>,
    statusbar: gtk::CheckButton,
    side: gtk::CheckButton,
    bottom: gtk::CheckButton,
}

impl PrefWidgets {
    /// Persist View / Editor / UI pages. Profile is applied after the config
    /// borrow is released so theme watchers cannot re-enter the RefCell.
    fn save(&self, config: &Rc<RefCell<Config>>) {
        let profile_id = self
            .theme_ids
            .get(self.theme_drop.selected() as usize)
            .copied();

        {
            let mut c = config.borrow_mut();
            c.editor.display_line_numbers = self.line_numbers.is_active();
            c.editor.highlight_current_line = self.highlight_line.is_active();
            c.editor.bracket_matching = self.bracket.is_active();
            c.editor.display_right_margin = self.right_margin.is_active();
            c.editor.right_margin_position = self.margin_pos.value() as u32;
            let modes = ["none", "char", "word", "word-char"];
            c.editor.wrap_mode = modes
                .get(self.wrap.selected() as usize)
                .unwrap_or(&"word")
                .to_string();
            c.editor.tabs_size = self.tab_width.value() as u32;
            c.editor.insert_spaces = self.insert_spaces.is_active();
            c.editor.auto_indent = self.auto_indent.is_active();
            c.editor.syntax_highlighting = self.syntax.is_active();
            c.editor.ensure_trailing_newline = self.trailing.is_active();
            c.editor.create_backup_copy = self.backup.is_active();
            c.editor.auto_save = self.autosave.is_active();
            c.editor.auto_save_interval = self.autosave_int.value() as u32;
            c.editor.use_default_font = self.font_default.is_active();
            if let Some(desc) = self.font_btn.font_desc() {
                c.editor.editor_font = desc.to_str().to_string();
            }
            let idx = self.scheme_drop.selected() as usize;
            if let Some(id) = self.scheme_ids.get(idx) {
                c.editor.scheme = id.clone();
            }
            c.ui.toolbar_visible = false;
            c.ui.statusbar_visible = self.statusbar.is_active();
            c.ui.side_panel_visible = self.side.is_active();
            c.ui.bottom_panel_visible = self.bottom.is_active();
            if let Err(e) = c.save() {
                eprintln!("gtk-edit: failed to save preferences: {e}");
            }
        }

        if let Some(profile_id) = profile_id {
            gtk_theme::select_theme(profile_id, |_| {});
            let mgr = sourceview5::StyleSchemeManager::default();
            let scheme =
                gtk_theme::resolve_sourceview_scheme(profile_id, |id| mgr.scheme(id).is_some());
            let mut c = config.borrow_mut();
            c.editor.scheme = scheme.to_string();
            if let Err(e) = c.save() {
                eprintln!("gtk-edit: failed to save preferences: {e}");
            }
        }
    }
}

pub fn show_preferences(
    parent: &impl IsA<gtk::Window>,
    config: Rc<RefCell<Config>>,
    engine: Rc<PluginEngine>,
    on_apply: Rc<dyn Fn()>,
) {
    let dialog = gtk::Window::builder()
        .title("Preferences")
        .transient_for(parent)
        .modal(true)
        .default_width(560)
        .default_height(480)
        .build();
    gtk_theme::style_dialog(&dialog);

    let notebook = gtk::Notebook::new();

    // --- View page ---
    let view_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    view_box.set_margin_top(12);
    view_box.set_margin_bottom(12);
    view_box.set_margin_start(12);
    view_box.set_margin_end(12);

    let cfg = config.borrow().clone();
    let line_numbers = gtk::CheckButton::with_label("Display line numbers");
    line_numbers.set_active(cfg.editor.display_line_numbers);
    let highlight_line = gtk::CheckButton::with_label("Highlight current line");
    highlight_line.set_active(cfg.editor.highlight_current_line);
    let bracket = gtk::CheckButton::with_label("Highlight matching brackets");
    bracket.set_active(cfg.editor.bracket_matching);
    let right_margin = gtk::CheckButton::with_label("Display right margin");
    right_margin.set_active(cfg.editor.display_right_margin);
    let margin_pos = gtk::SpinButton::with_range(1.0, 200.0, 1.0);
    margin_pos.set_value(cfg.editor.right_margin_position as f64);
    let wrap = gtk::DropDown::from_strings(&["none", "char", "word", "word-char"]);
    let wrap_idx = ["none", "char", "word", "word-char"]
        .iter()
        .position(|s| *s == cfg.editor.wrap_mode.as_str())
        .unwrap_or(2);
    wrap.set_selected(wrap_idx as u32);

    view_box.append(&line_numbers);
    view_box.append(&highlight_line);
    view_box.append(&bracket);
    view_box.append(&right_margin);
    let margin_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    margin_row.append(&gtk::Label::new(Some("Right margin at column:")));
    margin_row.append(&margin_pos);
    view_box.append(&margin_row);
    let wrap_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    wrap_row.append(&gtk::Label::new(Some("Text wrapping:")));
    wrap_row.append(&wrap);
    view_box.append(&wrap_row);
    notebook.append_page(&view_box, Some(&gtk::Label::new(Some("View"))));

    // --- Editor page ---
    let editor_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    editor_box.set_margin_top(12);
    editor_box.set_margin_bottom(12);
    editor_box.set_margin_start(12);
    editor_box.set_margin_end(12);
    let tab_width = gtk::SpinButton::with_range(1.0, 32.0, 1.0);
    tab_width.set_value(cfg.editor.tabs_size as f64);
    let insert_spaces = gtk::CheckButton::with_label("Insert spaces instead of tabs");
    insert_spaces.set_active(cfg.editor.insert_spaces);
    let auto_indent = gtk::CheckButton::with_label("Enable automatic indentation");
    auto_indent.set_active(cfg.editor.auto_indent);
    let syntax = gtk::CheckButton::with_label("Enable syntax highlighting");
    syntax.set_active(cfg.editor.syntax_highlighting);
    let trailing = gtk::CheckButton::with_label("Ensure trailing newline on save");
    trailing.set_active(cfg.editor.ensure_trailing_newline);
    let backup = gtk::CheckButton::with_label(
        "Create a backup copy of files before saving (filename~)",
    );
    backup.set_active(cfg.editor.create_backup_copy);
    let autosave = gtk::CheckButton::with_label("Autosave files every few minutes");
    autosave.set_active(cfg.editor.auto_save);
    let autosave_int = gtk::SpinButton::with_range(1.0, 120.0, 1.0);
    autosave_int.set_value(cfg.editor.auto_save_interval as f64);

    let font_default = gtk::CheckButton::with_label("Use default theme font");
    font_default.set_active(cfg.editor.use_default_font);
    let font_btn = build_font_button(&cfg.editor.editor_font, cfg.editor.use_default_font);
    {
        let font_btn2 = font_btn.clone();
        font_default.connect_toggled(move |c| {
            font_btn2.set_sensitive(!c.is_active());
        });
    }

    let (scheme_drop, scheme_ids) = build_scheme_dropdown(&cfg.editor.scheme);
    let (theme_drop, theme_ids) = gtk_theme::build_profile_dropdown(&gtk_theme::load_theme_id());

    let tw_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    tw_row.append(&gtk::Label::new(Some("Tab width:")));
    tw_row.append(&tab_width);
    editor_box.append(&tw_row);
    editor_box.append(&insert_spaces);
    editor_box.append(&auto_indent);
    editor_box.append(&syntax);
    editor_box.append(&trailing);
    editor_box.append(&backup);
    editor_box.append(&autosave);
    let as_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    as_row.append(&gtk::Label::new(Some("Autosave interval (minutes):")));
    as_row.append(&autosave_int);
    editor_box.append(&as_row);

    editor_box.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    editor_box.append(&font_default);
    let font_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let font_label = gtk::Label::new(Some("Editor font:"));
    font_label.set_halign(gtk::Align::Start);
    font_row.append(&font_label);
    font_row.append(&font_btn);
    editor_box.append(&font_row);

    let theme_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let theme_label = gtk::Label::new(Some("Profile:"));
    theme_label.set_halign(gtk::Align::Start);
    theme_row.append(&theme_label);
    theme_row.append(&theme_drop);
    editor_box.append(&theme_row);

    let theme_hint = gtk::Label::new(Some(
        "Shared suite theme (~/.config/gtk-apps/theme.toml). Updates chrome + color scheme.",
    ));
    theme_hint.add_css_class("dim-label");
    theme_hint.set_halign(gtk::Align::Start);
    theme_hint.set_wrap(true);
    editor_box.append(&theme_hint);

    let scheme_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let scheme_label = gtk::Label::new(Some("Color scheme:"));
    scheme_label.set_halign(gtk::Align::Start);
    scheme_row.append(&scheme_label);
    scheme_row.append(&scheme_drop);
    editor_box.append(&scheme_row);

    let scheme_hint = gtk::Label::new(Some(
        "GtkSourceView schemes (Adwaita, classic, solarized, …). Overridden when Profile changes.",
    ));
    scheme_hint.add_css_class("dim-label");
    scheme_hint.set_halign(gtk::Align::Start);
    scheme_hint.set_wrap(true);
    editor_box.append(&scheme_hint);

    notebook.append_page(&editor_box, Some(&gtk::Label::new(Some("Editor"))));

    // --- UI page ---
    let ui_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    ui_box.set_margin_top(12);
    ui_box.set_margin_bottom(12);
    ui_box.set_margin_start(12);
    ui_box.set_margin_end(12);
    let statusbar = gtk::CheckButton::with_label("Show statusbar");
    statusbar.set_active(cfg.ui.statusbar_visible);
    let side = gtk::CheckButton::with_label("Show side panel");
    side.set_active(cfg.ui.side_panel_visible);
    let bottom = gtk::CheckButton::with_label("Show bottom tools panel");
    bottom.set_active(cfg.ui.bottom_panel_visible);
    ui_box.append(&statusbar);
    ui_box.append(&side);
    ui_box.append(&bottom);
    notebook.append_page(&ui_box, Some(&gtk::Label::new(Some("UI"))));

    // --- Plugins page ---
    let plugins_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    plugins_box.set_margin_top(12);
    plugins_box.set_margin_bottom(12);
    plugins_box.set_margin_start(12);
    plugins_box.set_margin_end(12);
    let plugins_list = gtk::ListBox::new();
    plugins_list.set_selection_mode(gtk::SelectionMode::None);
    plugins_list.add_css_class("gtk-content");
    for info in engine.list_plugins() {
        let row = gtk::ListBoxRow::new();
        let h = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        h.set_margin_top(4);
        h.set_margin_bottom(4);
        h.set_margin_start(4);
        h.set_margin_end(4);
        let check = gtk::CheckButton::new();
        check.set_active(engine.is_active(&info.module));
        let module = info.module.clone();
        let engine2 = Rc::clone(&engine);
        let config2 = Rc::clone(&config);
        let on_apply2 = Rc::clone(&on_apply);
        check.connect_toggled(move |c| {
            engine2.set_active(&module, c.is_active(), &mut config2.borrow_mut());
            on_apply2();
        });
        let v = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let name = gtk::Label::new(Some(&info.name));
        name.set_halign(gtk::Align::Start);
        let desc = gtk::Label::new(Some(&info.description));
        desc.set_halign(gtk::Align::Start);
        desc.add_css_class("dim-label");
        desc.set_wrap(true);
        v.append(&name);
        v.append(&desc);
        h.append(&check);
        h.append(&v);
        row.set_child(Some(&h));
        plugins_list.append(&row);
    }
    let scroll = gtk::ScrolledWindow::builder()
        .child(&plugins_list)
        .vexpand(true)
        .min_content_height(220)
        .build();
    scroll.add_css_class("gtk-content");
    plugins_box.append(&scroll);
    notebook.append_page(&plugins_box, Some(&gtk::Label::new(Some("Plugins"))));

    let close = gtk_theme::labeled_button(gtk_theme::icon_for_label("Close"), "Close");
    let btn_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    btn_box.set_halign(gtk::Align::End);
    btn_box.set_margin_top(8);
    btn_box.set_margin_end(12);
    btn_box.set_margin_bottom(12);
    btn_box.append(&close);

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    outer.append(&notebook);
    outer.append(&btn_box);
    dialog.set_child(Some(&outer));

    let widgets = Rc::new(PrefWidgets {
        line_numbers,
        highlight_line,
        bracket,
        right_margin,
        margin_pos,
        wrap,
        tab_width,
        insert_spaces,
        auto_indent,
        syntax,
        trailing,
        backup,
        autosave,
        autosave_int,
        font_default,
        font_btn,
        scheme_drop,
        scheme_ids,
        theme_drop,
        theme_ids,
        statusbar,
        side,
        bottom,
    });

    let saved = Rc::new(RefCell::new(false));
    {
        let config = Rc::clone(&config);
        let on_apply = Rc::clone(&on_apply);
        let widgets = Rc::clone(&widgets);
        let saved = Rc::clone(&saved);
        dialog.connect_close_request(move |_| {
            if !*saved.borrow() {
                widgets.save(&config);
                on_apply();
                *saved.borrow_mut() = true;
            }
            glib::Propagation::Proceed
        });
    }
    {
        let d = dialog.clone();
        close.connect_clicked(move |_| {
            d.close();
        });
    }

    dialog.present();
}
