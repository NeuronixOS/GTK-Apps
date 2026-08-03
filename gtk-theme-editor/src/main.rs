//! GTK Theme Editor — a front-end for the shared `gtk-theme` suite profiles.
//!
//! Load any built-in or custom profile, tweak the foreground / background and
//! the 16-color ANSI palette with a live preview (the whole window recolors as
//! you edit), then save it under a custom name. Saved profiles land in
//! `~/.config/gtk-apps/custom-profiles.json` and show up in every suite app's
//! Profile menu. "Apply to Suite" switches all running apps to it at once.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::cairo;
use gtk::gdk;
use gtk::gio;
use gtk::prelude::*;

use gtk_theme::ProfileData;

const APP_ID: &str = "org.neuronix.GtkThemeEditor";
const SUITE_WEBSITE: &str = "https://github.com/NeuronixOS/GTK-Apps";
const SUITE_WEBSITE_LABEL: &str = "github.com/NeuronixOS/GTK-Apps";
const SUITE_AUTHOR: &str = "Created by Kevin Hinds";

/// ANSI slot roles for the 16-color palette (matches terminal color numbering).
const PALETTE_LABELS: [&str; 16] = [
    "0  Black",
    "1  Red",
    "2  Green",
    "3  Yellow",
    "4  Accent / Blue",
    "5  Magenta",
    "6  Cyan",
    "7  White",
    "8  Br Black",
    "9  Br Red",
    "10 Br Green",
    "11 Br Yellow",
    "12 Br Blue",
    "13 Br Magenta",
    "14 Br Cyan",
    "15 Br White",
];

/// All the widgets + edit state the signal handlers need to reach.
struct Ui {
    working: RefCell<ProfileData>,
    /// Guards against re-entrant updates while pushing values into widgets.
    updating: Cell<bool>,

    window: gtk::ApplicationWindow,
    profile_dropdown: gtk::DropDown,
    dropdown_ids: RefCell<Vec<String>>,

    name_entry: gtk::Entry,
    fg_btn: gtk::ColorDialogButton,
    fg_hex: gtk::Entry,
    bg_btn: gtk::ColorDialogButton,
    bg_hex: gtk::Entry,
    pal_btns: Vec<gtk::ColorDialogButton>,
    pal_hex: Vec<gtk::Entry>,

    palette_area: gtk::DrawingArea,
    fg_swatch: gtk::DrawingArea,
    bg_swatch: gtk::DrawingArea,

    delete_btn: gtk::Button,
    status: gtk::Label,
}

fn main() -> gtk::glib::ExitCode {
    let app = gtk::Application::builder().application_id(APP_ID).build();
    app.connect_startup(install_about_action);
    app.connect_activate(build_ui);
    app.run()
}

fn install_about_action(app: &gtk::Application) {
    let about = gio::SimpleAction::new("about", None);
    {
        let app = app.clone();
        about.connect_activate(move |_, _| show_about(&app));
    }
    app.add_action(&about);
}

fn show_about(app: &gtk::Application) {
    let about = gtk::AboutDialog::builder()
        .program_name("GTK Theme Editor")
        .version(env!("CARGO_PKG_VERSION"))
        .comments(
            "Edit suite color profiles (foreground, background, and 16-color palette) for Neuronix GTK-Apps.",
        )
        .authors([SUITE_AUTHOR])
        .website(SUITE_WEBSITE)
        .website_label(SUITE_WEBSITE_LABEL)
        .license_type(gtk::License::Gpl30)
        .build();
    if let Some(win) = app.active_window() {
        about.set_transient_for(Some(&win));
    }
    about.set_modal(true);
    about.present();
}

fn build_ui(app: &gtk::Application) {
    // Start themed by the current suite profile.
    gtk_theme::apply_chrome(gtk_theme::load_profile());

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("GTK Theme Editor")
        .default_width(960)
        .default_height(660)
        .build();

    // ---- header bar ----------------------------------------------------
    let header = gtk::HeaderBar::new();
    gtk_theme::prepare_headerbar(&header);

    let base_label = gtk::Label::new(Some("Base:"));
    base_label.add_css_class("dim-label");
    let profile_dropdown = gtk::DropDown::new(None::<gtk::StringList>, None::<gtk::Expression>);
    profile_dropdown.set_tooltip_text(Some("Load a profile to edit"));
    header.pack_start(&base_label);
    header.pack_start(&profile_dropdown);

    let save_btn = gtk::Button::with_label("Save");
    save_btn.add_css_class("suggested-action");
    save_btn.set_tooltip_text(Some("Save as a custom profile"));
    let apply_btn = gtk::Button::with_label("Apply to Suite");
    apply_btn.set_tooltip_text(Some("Save and switch all suite apps to this profile"));
    let delete_btn = gtk::Button::from_icon_name("user-trash-symbolic");
    delete_btn.set_tooltip_text(Some("Delete this custom profile"));
    delete_btn.add_css_class("destructive-action");
    header.pack_end(&save_btn);
    header.pack_end(&apply_btn);
    header.pack_end(&delete_btn);

    let about_btn = gtk::MenuButton::new();
    about_btn.set_icon_name("open-menu-symbolic");
    about_btn.set_tooltip_text(Some("Menu"));
    let about_menu = gio::Menu::new();
    about_menu.append(Some("About"), Some("app.about"));
    about_btn.set_menu_model(Some(&about_menu));
    header.pack_end(&about_btn);

    window.set_titlebar(Some(&header));

    // ---- editor column -------------------------------------------------
    let editor = gtk::Box::new(gtk::Orientation::Vertical, 12);
    editor.set_margin_top(16);
    editor.set_margin_bottom(16);
    editor.set_margin_start(16);
    editor.set_margin_end(16);

    let name_entry = gtk::Entry::new();
    name_entry.set_placeholder_text(Some("My Profile"));
    name_entry.set_hexpand(true);
    editor.append(&field_row("Profile name", &name_entry));

    let (fg_btn, fg_hex, fg_row) = color_field("Foreground");
    let (bg_btn, bg_hex, bg_row) = color_field("Background");
    editor.append(&fg_row);
    editor.append(&bg_row);

    editor.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let pal_heading = gtk::Label::new(Some("Palette (ANSI 0–15)"));
    pal_heading.set_xalign(0.0);
    pal_heading.add_css_class("heading");
    editor.append(&pal_heading);
    let accent_hint = gtk::Label::new(Some(
        "Slot 4 (Accent / Blue) is --accent-blue: file selection, text highlight, tabs, suggested buttons.",
    ));
    accent_hint.set_xalign(0.0);
    accent_hint.set_wrap(true);
    accent_hint.add_css_class("dim-label");
    editor.append(&accent_hint);

    let pal_grid = gtk::Grid::new();
    pal_grid.set_row_spacing(6);
    pal_grid.set_column_spacing(10);
    let mut pal_btns = Vec::with_capacity(16);
    let mut pal_hex = Vec::with_capacity(16);
    for (i, label) in PALETTE_LABELS.iter().enumerate() {
        let row = i as i32;
        let name = gtk::Label::new(Some(label));
        name.set_xalign(0.0);
        name.set_width_chars(16);
        name.add_css_class("dim-label");
        let btn = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
        btn.set_valign(gtk::Align::Center);
        let hex = gtk::Entry::new();
        hex.set_max_width_chars(9);
        hex.set_width_chars(9);
        hex.set_hexpand(true);
        pal_grid.attach(&name, 0, row, 1, 1);
        pal_grid.attach(&btn, 1, row, 1, 1);
        pal_grid.attach(&hex, 2, row, 1, 1);
        pal_btns.push(btn);
        pal_hex.push(hex);
    }
    editor.append(&pal_grid);

    let editor_scroll = gtk::ScrolledWindow::new();
    editor_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    editor_scroll.set_child(Some(&editor));
    editor_scroll.set_hexpand(true);
    editor_scroll.set_vexpand(true);

    // ---- preview column ------------------------------------------------
    let (preview, palette_area, fg_swatch, bg_swatch) = build_preview();

    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    paned.set_start_child(Some(&editor_scroll));
    paned.set_end_child(Some(&preview));
    paned.set_position(380);
    paned.set_wide_handle(true);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&paned);

    let status = gtk::Label::new(Some("Ready"));
    status.set_xalign(0.0);
    status.add_css_class("dim-label");
    status.set_margin_top(6);
    status.set_margin_bottom(6);
    status.set_margin_start(12);
    status.set_margin_end(12);
    let statusbar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    statusbar.add_css_class("statusbar");
    statusbar.append(&status);
    root.append(&statusbar);

    window.set_child(Some(&root));

    // ---- assemble state + wiring --------------------------------------
    let initial = gtk_theme::profile_data_by_id(&gtk_theme::load_theme_id())
        .unwrap_or_else(|| ProfileData::from_profile(gtk_theme::default_profile()));

    let ui = Rc::new(Ui {
        working: RefCell::new(initial.clone()),
        updating: Cell::new(false),
        window,
        profile_dropdown,
        dropdown_ids: RefCell::new(Vec::new()),
        name_entry,
        fg_btn,
        fg_hex,
        bg_btn,
        bg_hex,
        pal_btns,
        pal_hex,
        palette_area,
        fg_swatch,
        bg_swatch,
        delete_btn,
        status,
    });

    wire_preview(&ui);
    wire_color_field(&ui, Field::Foreground);
    wire_color_field(&ui, Field::Background);
    for i in 0..16 {
        wire_color_field(&ui, Field::Palette(i));
    }
    wire_name(&ui);
    wire_dropdown(&ui);
    wire_buttons(&ui, &save_btn, &apply_btn);

    refresh_dropdown(&ui, Some(&initial.id));
    load_into_fields(&ui, initial);

    ui.window.present();
}

// ---------------------------------------------------------------------------
// layout helpers
// ---------------------------------------------------------------------------

fn field_row(label: &str, child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let lbl = gtk::Label::new(Some(label));
    lbl.set_xalign(0.0);
    lbl.set_width_chars(12);
    lbl.add_css_class("dim-label");
    row.append(&lbl);
    row.append(child);
    row
}

fn color_field(label: &str) -> (gtk::ColorDialogButton, gtk::Entry, gtk::Box) {
    let btn = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
    btn.set_valign(gtk::Align::Center);
    let hex = gtk::Entry::new();
    hex.set_max_width_chars(9);
    hex.set_width_chars(9);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let lbl = gtk::Label::new(Some(label));
    lbl.set_xalign(0.0);
    lbl.set_width_chars(12);
    lbl.add_css_class("dim-label");
    row.append(&lbl);
    row.append(&btn);
    row.append(&hex);
    (btn, hex, row)
}

fn build_preview() -> (gtk::Box, gtk::DrawingArea, gtk::DrawingArea, gtk::DrawingArea) {
    let preview = gtk::Box::new(gtk::Orientation::Vertical, 12);
    preview.set_margin_top(16);
    preview.set_margin_bottom(16);
    preview.set_margin_start(16);
    preview.set_margin_end(16);
    preview.add_css_class("gtk-content");

    let heading = gtk::Label::new(Some("Live preview"));
    heading.set_xalign(0.0);
    heading.add_css_class("heading");
    preview.append(&heading);

    // Fake header bar
    let fake_header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    fake_header.add_css_class("headerbar");
    let ht = gtk::Label::new(Some("Header Bar"));
    ht.set_hexpand(true);
    ht.set_xalign(0.0);
    fake_header.append(&ht);
    let hb = gtk::Button::from_icon_name("open-menu-symbolic");
    hb.add_css_class("flat");
    fake_header.append(&hb);
    preview.append(&fake_header);

    // Buttons row
    let btns = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let b1 = gtk::Button::with_label("Normal");
    let b2 = gtk::Button::with_label("Suggested");
    b2.add_css_class("suggested-action");
    let b3 = gtk::Button::with_label("Destructive");
    b3.add_css_class("destructive-action");
    btns.append(&b1);
    btns.append(&b2);
    btns.append(&b3);
    preview.append(&btns);

    let entry = gtk::Entry::new();
    entry.set_text("Sample text field");
    preview.append(&entry);

    // Selected folder (gtk-files style) — accent at 35% opacity
    let folder_label = gtk::Label::new(Some("Selected folder (files view)"));
    folder_label.set_xalign(0.0);
    folder_label.add_css_class("dim-label");
    preview.append(&folder_label);

    let list = gtk::ListBox::new();
    list.add_css_class("side-panel");
    list.add_css_class("files-view");
    list.set_selection_mode(gtk::SelectionMode::Single);
    for (i, (icon, text)) in [
        ("folder-documents-symbolic", "Documents"),
        ("folder-download-symbolic", "Downloads"),
        ("folder-pictures-symbolic", "Pictures"),
    ]
    .iter()
    .enumerate()
    {
        let r = gtk::ListBoxRow::new();
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.set_margin_top(6);
        row.set_margin_bottom(6);
        row.set_margin_start(10);
        row.set_margin_end(10);
        row.append(&gtk::Image::from_icon_name(icon));
        let l = gtk::Label::new(Some(text));
        l.set_xalign(0.0);
        l.set_hexpand(true);
        row.append(&l);
        r.set_child(Some(&row));
        list.append(&r);
        if i == 0 {
            list.select_row(Some(&r));
        }
    }
    let list_frame = gtk::Frame::new(None);
    list_frame.set_child(Some(&list));
    preview.append(&list_frame);

    // Highlighted text (editor selection) — accent at 45% opacity
    let text_label = gtk::Label::new(Some("Highlighted text (editor selection)"));
    text_label.set_xalign(0.0);
    text_label.add_css_class("dim-label");
    preview.append(&text_label);

    let text_view = gtk::TextView::new();
    text_view.set_editable(false);
    text_view.set_cursor_visible(false);
    text_view.set_wrap_mode(gtk::WrapMode::Word);
    text_view.set_left_margin(10);
    text_view.set_right_margin(10);
    text_view.set_top_margin(8);
    text_view.set_bottom_margin(8);
    text_view.add_css_class("editor-view");
    text_view.add_css_class("gtk-edit-view");
    text_view.add_css_class("gtk-content");
    let sample = "The quick brown fox jumps over the lazy dog.\nChange Accent / Blue (slot 4) to recolor this highlight.";
    text_view.buffer().set_text(sample);
    // Select the first sentence so the accent highlight is visible.
    {
        let buf = text_view.buffer();
        let start = buf.iter_at_offset(0);
        let end = buf.iter_at_offset(44);
        buf.select_range(&start, &end);
    }
    let text_scroll = gtk::ScrolledWindow::builder()
        .child(&text_view)
        .min_content_height(72)
        .vexpand(false)
        .hexpand(true)
        .build();
    text_scroll.add_css_class("editor-view");
    let text_frame = gtk::Frame::new(None);
    text_frame.set_child(Some(&text_scroll));
    preview.append(&text_frame);

    // Foreground / background / accent swatches
    let swatch_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let (fg_box, fg_swatch) = labeled_swatch("Foreground");
    let (bg_box, bg_swatch) = labeled_swatch("Background");
    swatch_row.append(&fg_box);
    swatch_row.append(&bg_box);
    preview.append(&swatch_row);

    // Palette strip
    let pal_label = gtk::Label::new(Some("ANSI palette (slot 4 = accent)"));
    pal_label.set_xalign(0.0);
    pal_label.add_css_class("dim-label");
    preview.append(&pal_label);
    let palette_area = gtk::DrawingArea::new();
    palette_area.set_content_height(44);
    palette_area.set_hexpand(true);
    preview.append(&palette_area);

    (preview, palette_area, fg_swatch, bg_swatch)
}

fn labeled_swatch(label: &str) -> (gtk::Box, gtk::DrawingArea) {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let l = gtk::Label::new(Some(label));
    l.set_xalign(0.0);
    l.add_css_class("dim-label");
    let area = gtk::DrawingArea::new();
    area.set_content_width(120);
    area.set_content_height(34);
    b.append(&l);
    b.append(&area);
    (b, area)
}

// ---------------------------------------------------------------------------
// wiring
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Field {
    Foreground,
    Background,
    Palette(usize),
}

impl Field {
    fn get<'a>(&self, ui: &'a Ui) -> (&'a gtk::ColorDialogButton, &'a gtk::Entry) {
        match self {
            Field::Foreground => (&ui.fg_btn, &ui.fg_hex),
            Field::Background => (&ui.bg_btn, &ui.bg_hex),
            Field::Palette(i) => (&ui.pal_btns[*i], &ui.pal_hex[*i]),
        }
    }

    fn set_value(&self, ui: &Ui, hex: String) {
        let mut w = ui.working.borrow_mut();
        match self {
            Field::Foreground => w.foreground = hex,
            Field::Background => w.background = hex,
            Field::Palette(i) => {
                if w.palette.len() < 16 {
                    w.palette.resize(16, "#000000".to_string());
                }
                w.palette[*i] = hex;
            }
        }
    }
}

fn wire_color_field(ui: &Rc<Ui>, field: Field) {
    let (btn, hex) = field.get(ui);

    {
        let ui = ui.clone();
        btn.connect_rgba_notify(move |btn| {
            if ui.updating.get() {
                return;
            }
            let value = rgba_to_hex(&btn.rgba());
            field.set_value(&ui, value.clone());
            ui.updating.set(true);
            field.get(&ui).1.set_text(&value);
            ui.updating.set(false);
            apply_live(&ui);
        });
    }
    {
        let ui = ui.clone();
        hex.connect_changed(move |entry| {
            if ui.updating.get() {
                return;
            }
            let text = entry.text().to_string();
            let Ok(rgba) = text.trim().parse::<gdk::RGBA>() else {
                entry.add_css_class("error");
                return;
            };
            entry.remove_css_class("error");
            let value = rgba_to_hex(&rgba);
            field.set_value(&ui, value);
            ui.updating.set(true);
            field.get(&ui).0.set_rgba(&rgba);
            ui.updating.set(false);
            apply_live(&ui);
        });
    }
}

fn wire_name(ui: &Rc<Ui>) {
    let ui2 = ui.clone();
    ui.name_entry.connect_changed(move |e| {
        if ui2.updating.get() {
            return;
        }
        ui2.working.borrow_mut().name = e.text().to_string();
    });
}

fn wire_dropdown(ui: &Rc<Ui>) {
    let ui2 = ui.clone();
    ui.profile_dropdown.connect_selected_notify(move |dd| {
        if ui2.updating.get() {
            return;
        }
        let idx = dd.selected() as usize;
        let id = ui2.dropdown_ids.borrow().get(idx).cloned();
        if let Some(id) = id {
            if let Some(data) = gtk_theme::profile_data_by_id(&id) {
                load_into_fields(&ui2, data);
                set_status(&ui2, &format!("Loaded “{}”", ui2.working.borrow().name));
            }
        }
    });
}

fn wire_buttons(ui: &Rc<Ui>, save_btn: &gtk::Button, apply_btn: &gtk::Button) {
    {
        let ui = ui.clone();
        save_btn.connect_clicked(move |_| {
            save_current(&ui);
        });
    }
    {
        let ui = ui.clone();
        apply_btn.connect_clicked(move |_| {
            if save_current(&ui) {
                let id = ui.working.borrow().id.clone();
                gtk_theme::select_theme(&id, |_| {});
                set_status(&ui, "Applied to all suite apps");
            }
        });
    }
    {
        let del = ui.delete_btn.clone();
        let ui = ui.clone();
        del.connect_clicked(move |_| {
            let id = ui.working.borrow().id.clone();
            if !gtk_theme::is_custom_profile(&id) {
                set_status(&ui, "Built-in profiles can't be deleted");
                return;
            }
            let name = ui.working.borrow().name.clone();
            gtk_theme::delete_custom_profile(&id);
            let fallback = ProfileData::from_profile(gtk_theme::default_profile());
            refresh_dropdown(&ui, Some(&fallback.id));
            load_into_fields(&ui, fallback);
            set_status(&ui, &format!("Deleted “{name}”"));
        });
    }
}

fn wire_preview(ui: &Rc<Ui>) {
    {
        let area = ui.palette_area.clone();
        let u = ui.clone();
        area.set_draw_func(move |_, cr, w, h| {
            draw_palette(cr, w, h, &u.working.borrow());
        });
    }
    {
        let area = ui.fg_swatch.clone();
        let u = ui.clone();
        area.set_draw_func(move |_, cr, w, h| {
            draw_single(cr, w, h, &u.working.borrow().foreground);
        });
    }
    {
        let area = ui.bg_swatch.clone();
        let u = ui.clone();
        area.set_draw_func(move |_, cr, w, h| {
            draw_single(cr, w, h, &u.working.borrow().background);
        });
    }
}

// ---------------------------------------------------------------------------
// behaviour
// ---------------------------------------------------------------------------

/// Push a full profile into every widget without triggering edit handlers.
fn load_into_fields(ui: &Rc<Ui>, data: ProfileData) {
    ui.updating.set(true);
    *ui.working.borrow_mut() = data.clone();
    ui.name_entry.set_text(&data.name);
    set_swatch(&ui.fg_btn, &ui.fg_hex, &data.foreground);
    set_swatch(&ui.bg_btn, &ui.bg_hex, &data.background);
    let pal = data.normalized_palette();
    for i in 0..16 {
        set_swatch(&ui.pal_btns[i], &ui.pal_hex[i], &pal[i]);
    }
    ui.updating.set(false);
    apply_live(ui);
    update_delete_sensitivity(ui);
}

/// Save the working profile as a custom profile. Returns false on empty name.
fn save_current(ui: &Rc<Ui>) -> bool {
    let name = ui.name_entry.text().trim().to_string();
    if name.is_empty() {
        set_status(ui, "Enter a profile name before saving");
        ui.name_entry.grab_focus();
        return false;
    }

    let mut data = ui.working.borrow().clone();
    data.name = name.clone();
    if data.palette.len() < 16 {
        data.palette = data.normalized_palette().to_vec();
    }

    // Editing an existing custom profile keeps its id (rename in place);
    // anything derived from a built-in gets a fresh custom id.
    let id = if gtk_theme::is_custom_profile(&data.id) {
        data.id.clone()
    } else {
        gtk_theme::custom_id_for_name(&name, None)
    };
    data.id = id.clone();

    gtk_theme::save_custom_profile(&data);
    *ui.working.borrow_mut() = data;
    refresh_dropdown(ui, Some(&id));
    update_delete_sensitivity(ui);
    set_status(ui, &format!("Saved “{name}”"));
    true
}

/// Rebuild the base-profile dropdown from all_profiles(), selecting `select_id`.
fn refresh_dropdown(ui: &Rc<Ui>, select_id: Option<&str>) {
    ui.updating.set(true);
    let profiles = gtk_theme::all_profiles();
    let ids: Vec<String> = profiles.iter().map(|p| p.id.to_string()).collect();
    let labels: Vec<&str> = profiles.iter().map(|p| p.name).collect();
    let model = gtk::StringList::new(&labels);
    ui.profile_dropdown.set_model(Some(&model));
    let sel = select_id
        .and_then(|id| ids.iter().position(|x| x == id))
        .unwrap_or(0);
    ui.profile_dropdown.set_selected(sel as u32);
    *ui.dropdown_ids.borrow_mut() = ids;
    ui.updating.set(false);
}

fn update_delete_sensitivity(ui: &Rc<Ui>) {
    let id = ui.working.borrow().id.clone();
    ui.delete_btn.set_sensitive(gtk_theme::is_custom_profile(&id));
}

fn apply_live(ui: &Rc<Ui>) {
    {
        let data = ui.working.borrow();
        gtk_theme::apply_chrome_data(&data);
    }
    ui.palette_area.queue_draw();
    ui.fg_swatch.queue_draw();
    ui.bg_swatch.queue_draw();
}

fn set_status(ui: &Rc<Ui>, text: &str) {
    ui.status.set_text(text);
}

// ---------------------------------------------------------------------------
// small utilities
// ---------------------------------------------------------------------------

fn set_swatch(btn: &gtk::ColorDialogButton, hex_entry: &gtk::Entry, hex: &str) {
    if let Ok(rgba) = hex.parse::<gdk::RGBA>() {
        btn.set_rgba(&rgba);
    }
    hex_entry.set_text(hex);
    hex_entry.remove_css_class("error");
}

fn rgba_to_hex(c: &gdk::RGBA) -> String {
    let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", to(c.red()), to(c.green()), to(c.blue()))
}

fn draw_single(cr: &cairo::Context, w: i32, h: i32, hex: &str) {
    let rgba = hex.parse::<gdk::RGBA>().unwrap_or(gdk::RGBA::BLACK);
    cr.set_source_rgb(rgba.red() as f64, rgba.green() as f64, rgba.blue() as f64);
    let _ = cr.paint();
    cr.set_source_rgba(0.5, 0.5, 0.5, 0.4);
    cr.set_line_width(1.0);
    cr.rectangle(0.5, 0.5, (w - 1) as f64, (h - 1) as f64);
    let _ = cr.stroke();
}

fn draw_palette(cr: &cairo::Context, w: i32, h: i32, data: &ProfileData) {
    let pal = data.normalized_palette();
    let n = pal.len();
    let cell = w as f64 / n as f64;
    for (i, hex) in pal.iter().enumerate() {
        let rgba = hex.parse::<gdk::RGBA>().unwrap_or(gdk::RGBA::BLACK);
        cr.set_source_rgb(rgba.red() as f64, rgba.green() as f64, rgba.blue() as f64);
        cr.rectangle(i as f64 * cell, 0.0, cell.ceil(), h as f64);
        let _ = cr.fill();

        let lum = 0.2126 * rgba.red() + 0.7152 * rgba.green() + 0.0722 * rgba.blue();
        if lum < 0.5 {
            cr.set_source_rgb(1.0, 1.0, 1.0);
        } else {
            cr.set_source_rgb(0.0, 0.0, 0.0);
        }
        cr.select_font_face("monospace", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        cr.set_font_size(9.0);
        cr.move_to(i as f64 * cell + 3.0, h as f64 - 5.0);
        let _ = cr.show_text(&format!("{i}"));
    }
}
