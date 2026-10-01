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
use gtk::glib;
use gtk::pango;
use gtk::prelude::*;

use gtk_theme::{
    BUTTON_KITS, ChromeBevel, ChromeGradient,
    ProfileData, WindowChrome,
};

const APP_ID: &str = "org.neuronix.GtkThemeEditor";
const EDITOR_WIDTH: i32 = 960;
const EDITOR_HEIGHT: i32 = 720;
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
    active_border_btns: Vec<gtk::ColorDialogButton>,
    active_border_hex: Vec<gtk::Entry>,
    inactive_border_btns: Vec<gtk::ColorDialogButton>,
    inactive_border_hex: Vec<gtk::Entry>,
    bar_btns: Vec<gtk::ColorDialogButton>,
    bar_hex: Vec<gtk::Entry>,
    bar_inactive_btns: Vec<gtk::ColorDialogButton>,
    bar_inactive_hex: Vec<gtk::Entry>,
    pal_btns: Vec<gtk::ColorDialogButton>,
    pal_hex: Vec<gtk::Entry>,

    palette_area: gtk::DrawingArea,
    fg_swatch: gtk::DrawingArea,
    bg_swatch: gtk::DrawingArea,
    active_border_swatch: gtk::DrawingArea,
    inactive_border_swatch: gtk::DrawingArea,
    chrome_preview: gtk::DrawingArea,

    thickness: gtk::SpinButton,
    rounding: gtk::SpinButton,
    gradient_dropdown: gtk::DropDown,
    bevel_dropdown: gtk::DropDown,
    kit_dropdown: gtk::DropDown,
    min_entry: gtk::Entry,
    max_entry: gtk::Entry,
    close_entry: gtk::Entry,

    delete_btn: gtk::Button,
    status: gtk::Label,
}

fn main() -> gtk::glib::ExitCode {
    let app = gtk::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|app| {
        install_about_action(app);
        install_glyph_css();
    });
    app.connect_activate(build_ui);
    app.run()
}

fn install_glyph_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        ".chrome-glyph { font-family: \"DejaVu Sans\", \"Symbols Nerd Font\", sans-serif; font-size: 14pt; }",
    );
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
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
            "Edit suite color profiles, window chrome (thickness, rounding, − □ ×), and 16-color palette for Neuronix GTK-Apps.",
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
        .default_width(EDITOR_WIDTH)
        .default_height(EDITOR_HEIGHT)
        .decorated(false)
        .build();

    // Toolbar lives in the content box (not a CSD titlebar). Hyprbars owns
    // the real title bar; a GTK titlebar advertises CSD extents that make
    // this window jump and snap size when you click-drag it.
    let header = gtk::HeaderBar::new();
    gtk_theme::prepare_headerbar(&header);
    header.set_show_title_buttons(false);

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
    let border_heading = gtk::Label::new(Some("Window borders"));
    border_heading.set_xalign(0.0);
    border_heading.add_css_class("heading");
    editor.append(&border_heading);
    let border_hint = wrap_hint(
        "Hyprland draws a two-color left→right or right→left gradient around each window. Active is the focused ring; Inactive is every other window.",
    );
    editor.append(&border_hint);

    let (active_border_btns, active_border_hex, active_section) =
        gradient_pair_fields("Active window", "Color 1", "Color 2");
    let (inactive_border_btns, inactive_border_hex, inactive_section) =
        gradient_pair_fields("Inactive window", "Color 1", "Color 2");
    editor.append(&active_section);
    editor.append(&inactive_section);

    let gradient_dropdown =
        gtk::DropDown::from_strings(&["Left → Right", "Right → Left"]);
    gradient_dropdown.set_tooltip_text(Some(
        "Direction for window borders and the hyprbars title bar",
    ));
    editor.append(&field_row("Gradient", &gradient_dropdown));

    editor.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let (
        thickness,
        rounding,
        bevel_dropdown,
        kit_dropdown,
        min_entry,
        max_entry,
        close_entry,
        bar_btns,
        bar_hex,
        bar_inactive_btns,
        bar_inactive_hex,
        chrome_section,
    ) = build_chrome_fields();
    editor.append(&chrome_section);

    editor.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let pal_heading = gtk::Label::new(Some("Palette (ANSI 0–15)"));
    pal_heading.set_xalign(0.0);
    pal_heading.add_css_class("heading");
    editor.append(&pal_heading);
    let accent_hint = wrap_hint(
        "Slot 4 (Accent / Blue) is --accent-blue: file selection, text highlight, tabs, suggested buttons.",
    );
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
    editor_scroll.set_propagate_natural_width(false);
    editor_scroll.set_propagate_natural_height(false);
    editor_scroll.set_child(Some(&editor));
    editor_scroll.set_hexpand(true);
    editor_scroll.set_vexpand(true);

    // ---- preview column ------------------------------------------------
    let (
        preview,
        palette_area,
        fg_swatch,
        bg_swatch,
        active_border_swatch,
        inactive_border_swatch,
        chrome_preview,
    ) = build_preview();

    let preview_scroll = gtk::ScrolledWindow::new();
    preview_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    preview_scroll.set_propagate_natural_width(false);
    preview_scroll.set_propagate_natural_height(false);
    preview_scroll.set_child(Some(&preview));
    preview_scroll.set_hexpand(true);
    preview_scroll.set_vexpand(true);

    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    paned.set_start_child(Some(&editor_scroll));
    paned.set_end_child(Some(&preview_scroll));
    paned.set_position(380);
    paned.set_wide_handle(true);
    paned.set_hexpand(true);
    paned.set_vexpand(true);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&header);
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
        active_border_btns,
        active_border_hex,
        inactive_border_btns,
        inactive_border_hex,
        bar_btns,
        bar_hex,
        bar_inactive_btns,
        bar_inactive_hex,
        pal_btns,
        pal_hex,
        palette_area,
        fg_swatch,
        bg_swatch,
        active_border_swatch,
        inactive_border_swatch,
        chrome_preview,
        thickness,
        rounding,
        gradient_dropdown,
        bevel_dropdown,
        kit_dropdown,
        min_entry,
        max_entry,
        close_entry,
        delete_btn,
        status,
    });

    wire_preview(&ui);
    wire_chrome(&ui);
    wire_color_field(&ui, Field::Foreground);
    wire_color_field(&ui, Field::Background);
    for i in 0..2 {
        wire_color_field(&ui, Field::ActiveBorder(i));
        wire_color_field(&ui, Field::InactiveBorder(i));
        wire_color_field(&ui, Field::Bar(i));
        wire_color_field(&ui, Field::BarInactive(i));
    }
    for i in 0..16 {
        wire_color_field(&ui, Field::Palette(i));
    }
    wire_name(&ui);
    wire_dropdown(&ui);
    wire_buttons(&ui, &save_btn, &apply_btn);

    refresh_dropdown(&ui, Some(&initial.id));
    load_into_fields(&ui, initial);

    if let Some((_, _, w, h)) = screen_two_thirds() {
        ui.window.set_default_size(w, h);
    } else {
        ui.window.set_default_size(EDITOR_WIDTH, EDITOR_HEIGHT);
    }
    ui.window.present();
    snap_editor_window_size();
}

// ---------------------------------------------------------------------------
// layout helpers
// ---------------------------------------------------------------------------

/// Hyprland restores the last floating size for this class. Float it, then
/// keep it at two-thirds of the screen and centered until that restore settles.
fn snap_editor_window_size() {
    let attempts = Rc::new(Cell::new(0u32));
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        let n = attempts.get();
        attempts.set(n + 1);
        if let Some((tx, ty, tw, th)) = screen_two_thirds() {
            if let Some((floating, x, y, w, h)) = theme_editor_geom() {
                let target = "class:^(org.neuronix.GtkThemeEditor)$";
                if !floating {
                    hyprctl_dispatch("setfloating", target);
                }
                if (w - tw).abs() > 4 || (h - th).abs() > 4 {
                    hyprctl_dispatch("resizewindowpixel", &format!("exact {tw} {th},{target}"));
                }
                if (x - tx).abs() > 8 || (y - ty).abs() > 8 {
                    hyprctl_dispatch("movewindowpixel", &format!("exact {tx} {ty},{target}"));
                }
            }
        }
        if n >= 24 {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

/// Focused monitor, two-thirds wide and tall, centered. `(x, y, w, h)`.
fn screen_two_thirds() -> Option<(i32, i32, i32, i32)> {
    let (mx, my, mw, mh) = focused_monitor()?;
    let w = mw * 2 / 3;
    let h = mh * 2 / 3;
    if w < 200 || h < 200 {
        return None;
    }
    Some((mx + (mw - w) / 2, my + (mh - h) / 2, w, h))
}

fn focused_monitor() -> Option<(i32, i32, i32, i32)> {
    let text = hyprctl_stdout(&["monitors", "-j"])?;
    let at = text.rfind("\"focused\": true")?;
    let start = text[..at].rfind("\"id\":").unwrap_or(0);
    let slice = &text[start..at];
    Some((
        json_i32(slice, "x")?,
        json_i32(slice, "y")?,
        json_i32(slice, "width")?,
        json_i32(slice, "height")?,
    ))
}

fn json_i32(slice: &str, key: &str) -> Option<i32> {
    let pat = format!("\"{key}\":");
    let at = slice.rfind(&pat)?;
    let rest = slice[at + pat.len()..].trim_start();
    let mut end = 0;
    let bytes = rest.as_bytes();
    if bytes.first() == Some(&b'-') {
        end = 1;
    }
    let digits = end;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == digits {
        return None;
    }
    rest[..end].parse().ok()
}

fn hyprctl_dispatch(name: &str, args: &str) {
    let _ = std::process::Command::new("hyprctl")
        .args(["dispatch", name, args])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

fn hyprctl_stdout(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("hyprctl")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    String::from_utf8(out.stdout).ok()
}

fn theme_editor_geom() -> Option<(bool, i32, i32, i32, i32)> {
    let text = hyprctl_stdout(&["clients", "-j"])?;
    let idx = text.find("\"class\": \"org.neuronix.GtkThemeEditor\"")?;
    let slice = &text[idx.saturating_sub(1600)..idx];
    let floating_at = slice.rfind("\"floating\":")?;
    let floating = slice[floating_at..].starts_with("\"floating\": true");
    let (x, y) = json_pair(slice, "at")?;
    let (w, h) = json_pair(slice, "size")?;
    Some((floating, x, y, w, h))
}

fn json_pair(slice: &str, key: &str) -> Option<(i32, i32)> {
    let pat = format!("\"{key}\":");
    let at = slice.rfind(&pat)?;
    let rest = &slice[at..];
    let a = rest.find('[')?;
    let b = rest.find(']')?;
    let mut parts = rest[a + 1..b].split(',');
    let x = parts.next()?.trim().parse().ok()?;
    let y = parts.next()?.trim().parse().ok()?;
    Some((x, y))
}

fn wrap_hint(text: &str) -> gtk::Label {
    let hint = gtk::Label::new(Some(text));
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    hint.set_max_width_chars(42);
    hint.set_hexpand(true);
    hint.add_css_class("dim-label");
    hint
}

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

fn gradient_pair_fields(
    heading: &str,
    label_a: &str,
    label_b: &str,
) -> (Vec<gtk::ColorDialogButton>, Vec<gtk::Entry>, gtk::Box) {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let h = gtk::Label::new(Some(heading));
    h.set_xalign(0.0);
    h.add_css_class("dim-label");
    section.append(&h);
    let mut btns = Vec::with_capacity(2);
    let mut hexes = Vec::with_capacity(2);
    for label in [label_a, label_b] {
        let (btn, hex, row) = color_field(label);
        section.append(&row);
        btns.push(btn);
        hexes.push(hex);
    }
    (btns, hexes, section)
}

fn spin_row(label: &str, min: f64, max: f64, value: f64) -> (gtk::SpinButton, gtk::Box) {
    let adj = gtk::Adjustment::new(value, min, max, 1.0, 5.0, 0.0);
    let spin = gtk::SpinButton::new(Some(&adj), 1.0, 0);
    spin.set_numeric(true);
    spin.set_snap_to_ticks(true);
    spin.set_width_chars(4);
    let row = field_row(label, &spin);
    (spin, row)
}

fn build_chrome_fields() -> (
    gtk::SpinButton,
    gtk::SpinButton,
    gtk::DropDown,
    gtk::DropDown,
    gtk::Entry,
    gtk::Entry,
    gtk::Entry,
    Vec<gtk::ColorDialogButton>,
    Vec<gtk::Entry>,
    Vec<gtk::ColorDialogButton>,
    Vec<gtk::Entry>,
    gtk::Box,
) {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let heading = gtk::Label::new(Some("Window"));
    heading.set_xalign(0.0);
    heading.add_css_class("heading");
    section.append(&heading);
    let hint = wrap_hint(
        "Border size and rounding apply to every Hyprland window. − □ × are hyprbars font glyphs.",
    );
    section.append(&hint);

    let (thickness, thick_row) = spin_row("Border size", 0.0, 20.0, 3.0);
    let (rounding, round_row) = spin_row("Rounding", 0.0, 20.0, 8.0);
    section.append(&thick_row);
    section.append(&round_row);

    let bevel_dropdown =
        gtk::DropDown::from_strings(&["Flat", "Inner highlight", "Double ring"]);
    bevel_dropdown.set_tooltip_text(Some("Preview bevel; Hyprland still uses the outline colors"));
    section.append(&field_row("Bevel", &bevel_dropdown));

    let (bar_btns, bar_hex, bar_section) =
        gradient_pair_fields("Active title bar", "Color 1", "Color 2");
    section.append(&bar_section);
    let (bar_inactive_btns, bar_inactive_hex, ibar_section) =
        gradient_pair_fields("Inactive title bar", "Color 1", "Color 2");
    section.append(&ibar_section);

    let kit_labels: Vec<&str> = BUTTON_KITS.iter().map(|k| k.1).collect();
    let kit_dropdown = gtk::DropDown::from_strings(kit_labels.as_slice());
    kit_dropdown.set_tooltip_text(Some("Glyph kit for minimize / maximize / close"));
    section.append(&field_row("Buttons", &kit_dropdown));

    let glyphs = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let min_entry = gtk::Entry::new();
    min_entry.set_max_width_chars(4);
    min_entry.set_width_chars(3);
    min_entry.set_tooltip_text(Some("Minimize"));
    style_glyph_entry(&min_entry);
    let max_entry = gtk::Entry::new();
    max_entry.set_max_width_chars(4);
    max_entry.set_width_chars(3);
    max_entry.set_tooltip_text(Some("Maximize"));
    style_glyph_entry(&max_entry);
    let close_entry = gtk::Entry::new();
    close_entry.set_max_width_chars(4);
    close_entry.set_width_chars(3);
    close_entry.set_tooltip_text(Some("Close"));
    style_glyph_entry(&close_entry);
    glyphs.append(&min_entry);
    glyphs.append(&max_entry);
    glyphs.append(&close_entry);
    section.append(&field_row("Glyphs", &glyphs));

    (
        thickness,
        rounding,
        bevel_dropdown,
        kit_dropdown,
        min_entry,
        max_entry,
        close_entry,
        bar_btns,
        bar_hex,
        bar_inactive_btns,
        bar_inactive_hex,
        section,
    )
}

fn build_preview() -> (
    gtk::Box,
    gtk::DrawingArea,
    gtk::DrawingArea,
    gtk::DrawingArea,
    gtk::DrawingArea,
    gtk::DrawingArea,
    gtk::DrawingArea,
) {
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

    let chrome_label = gtk::Label::new(Some("Window chrome"));
    chrome_label.set_xalign(0.0);
    chrome_label.add_css_class("dim-label");
    preview.append(&chrome_label);
    let chrome_preview = gtk::DrawingArea::new();
    chrome_preview.set_content_height(220);
    chrome_preview.set_hexpand(true);
    preview.append(&chrome_preview);

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

    let border_swatch_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let (active_box, active_border_swatch) = labeled_swatch("Active border");
    let (inactive_box, inactive_border_swatch) = labeled_swatch("Inactive border");
    border_swatch_row.append(&active_box);
    border_swatch_row.append(&inactive_box);
    preview.append(&border_swatch_row);

    // Palette strip
    let pal_label = gtk::Label::new(Some("ANSI palette (slot 4 = accent)"));
    pal_label.set_xalign(0.0);
    pal_label.add_css_class("dim-label");
    preview.append(&pal_label);
    let palette_area = gtk::DrawingArea::new();
    palette_area.set_content_height(44);
    palette_area.set_hexpand(true);
    preview.append(&palette_area);

    (preview, palette_area, fg_swatch, bg_swatch, active_border_swatch, inactive_border_swatch, chrome_preview)
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
    ActiveBorder(usize),
    InactiveBorder(usize),
    Bar(usize),
    BarInactive(usize),
    Palette(usize),
}

impl Field {
    fn get<'a>(&self, ui: &'a Ui) -> (&'a gtk::ColorDialogButton, &'a gtk::Entry) {
        match self {
            Field::Foreground => (&ui.fg_btn, &ui.fg_hex),
            Field::Background => (&ui.bg_btn, &ui.bg_hex),
            Field::ActiveBorder(i) => (&ui.active_border_btns[*i], &ui.active_border_hex[*i]),
            Field::InactiveBorder(i) => (&ui.inactive_border_btns[*i], &ui.inactive_border_hex[*i]),
            Field::Bar(i) => (&ui.bar_btns[*i], &ui.bar_hex[*i]),
            Field::BarInactive(i) => (&ui.bar_inactive_btns[*i], &ui.bar_inactive_hex[*i]),
            Field::Palette(i) => (&ui.pal_btns[*i], &ui.pal_hex[*i]),
        }
    }

    fn set_value(&self, ui: &Ui, hex: String) {
        let mut w = ui.working.borrow_mut();
        match self {
            Field::Foreground => w.foreground = hex,
            Field::Background => w.background = hex,
            Field::ActiveBorder(i) => w.set_active_border_stop(*i, hex),
            Field::InactiveBorder(i) => w.set_inactive_border_stop(*i, hex),
            Field::Bar(i) => w.set_bar_stop(*i, hex),
            Field::BarInactive(i) => w.set_bar_inactive_stop(*i, hex),
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
            preview_border_if_needed(&ui, field);
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
            preview_border_if_needed(&ui, field);
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
        let area = ui.chrome_preview.clone();
        let u = ui.clone();
        area.set_draw_func(move |_, cr, w, h| {
            draw_window_chrome(cr, w, h, &u.working.borrow());
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
    {
        let area = ui.active_border_swatch.clone();
        let u = ui.clone();
        area.set_draw_func(move |_, cr, w, h| {
            let data = u.working.borrow();
            let [a, b] = data.active_border_stops().unwrap_or_else(|| {
                let h = data.border_hex();
                [h.clone(), h]
            });
            draw_gradient(cr, w, h, &a, &b, data.chrome_or_default().gradient.is_rtl());
        });
    }
    {
        let area = ui.inactive_border_swatch.clone();
        let u = ui.clone();
        area.set_draw_func(move |_, cr, w, h| {
            let data = u.working.borrow();
            let [c0, c1] = data.inactive_border_stops().unwrap_or_else(|| {
                let x = data.background.clone();
                [x.clone(), x]
            });
            draw_gradient(cr, w, h, &c0, &c1, data.chrome_or_default().gradient.is_rtl());
        });
    }
}

// ---------------------------------------------------------------------------
// behaviour
// ---------------------------------------------------------------------------

/// Push a full profile into every widget without triggering edit handlers.
fn load_into_fields(ui: &Rc<Ui>, data: ProfileData) {
    ui.updating.set(true);
    let mut seeded = data.clone();
    seeded.seed_window_borders();
    ui.name_entry.set_text(&seeded.name);
    set_swatch(&ui.fg_btn, &ui.fg_hex, &seeded.foreground);
    set_swatch(&ui.bg_btn, &ui.bg_hex, &seeded.background);
    let active = seeded
        .active_border_stops()
        .unwrap_or_else(|| [seeded.border_hex(), seeded.border_hex()]);
    let inactive = seeded.inactive_border_stops().unwrap_or_else(|| {
        let a = seeded.background.clone();
        [a.clone(), a]
    });
    for i in 0..2 {
        set_swatch(&ui.active_border_btns[i], &ui.active_border_hex[i], &active[i]);
        set_swatch(
            &ui.inactive_border_btns[i],
            &ui.inactive_border_hex[i],
            &inactive[i],
        );
    }
    let pal = seeded.normalized_palette();
    for i in 0..16 {
        set_swatch(&ui.pal_btns[i], &ui.pal_hex[i], &pal[i]);
    }
    let chrome = seeded.chrome_or_default();
    ui.thickness.set_value(chrome.border_size as f64);
    ui.rounding.set_value(chrome.rounding as f64);
    ui.bevel_dropdown.set_selected(match chrome.bevel {
        ChromeBevel::Flat => 0,
        ChromeBevel::Inner => 1,
        ChromeBevel::Double => 2,
    });
    ui.gradient_dropdown.set_selected(match chrome.gradient {
        ChromeGradient::Ltr => 0,
        ChromeGradient::Rtl => 1,
    });
    let bar = chrome.bar_stops(&seeded.background, &seeded.foreground);
    let ibar = chrome.bar_inactive_stops(&seeded.background, &seeded.background);
    for i in 0..2 {
        set_swatch(&ui.bar_btns[i], &ui.bar_hex[i], &bar[i]);
        set_swatch(&ui.bar_inactive_btns[i], &ui.bar_inactive_hex[i], &ibar[i]);
    }
    let kit_idx = BUTTON_KITS
        .iter()
        .position(|(id, _, _, _, _)| *id == chrome.buttons.kit)
        .unwrap_or(0);
    ui.kit_dropdown.set_selected(kit_idx as u32);
    ui.min_entry.set_text(&chrome.buttons.minimize);
    ui.max_entry.set_text(&chrome.buttons.maximize);
    ui.close_entry.set_text(&chrome.buttons.close);
    *ui.working.borrow_mut() = seeded;
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
    ui.active_border_swatch.queue_draw();
    ui.inactive_border_swatch.queue_draw();
    ui.chrome_preview.queue_draw();
}

fn preview_border_if_needed(ui: &Rc<Ui>, field: Field) {
    if matches!(
        field,
        Field::ActiveBorder(_) | Field::InactiveBorder(_) | Field::Bar(_) | Field::BarInactive(_)
    ) {
        gtk_theme::preview_window_border(&ui.working.borrow());
    }
}

fn wire_chrome(ui: &Rc<Ui>) {
    {
        let scale = ui.thickness.clone();
        let ui = ui.clone();
        scale.connect_value_changed(move |scale| {
            if ui.updating.get() {
                return;
            }
            ui.working.borrow_mut().chrome_mut().border_size = scale.value().round() as u32;
            apply_live(&ui);
            gtk_theme::preview_window_border(&ui.working.borrow());
        });
    }
    {
        let scale = ui.rounding.clone();
        let ui = ui.clone();
        scale.connect_value_changed(move |scale| {
            if ui.updating.get() {
                return;
            }
            ui.working.borrow_mut().chrome_mut().rounding = scale.value().round() as u32;
            apply_live(&ui);
            gtk_theme::preview_window_border(&ui.working.borrow());
        });
    }
    {
        let dd = ui.gradient_dropdown.clone();
        let ui = ui.clone();
        dd.connect_selected_notify(move |dd| {
            if ui.updating.get() {
                return;
            }
            let gradient = match dd.selected() {
                1 => ChromeGradient::Rtl,
                _ => ChromeGradient::Ltr,
            };
            ui.working.borrow_mut().chrome_mut().gradient = gradient;
            apply_live(&ui);
            gtk_theme::preview_window_border(&ui.working.borrow());
        });
    }
    {
        let dd = ui.bevel_dropdown.clone();
        let ui = ui.clone();
        dd.connect_selected_notify(move |dd| {
            if ui.updating.get() {
                return;
            }
            let bevel = match dd.selected() {
                1 => ChromeBevel::Inner,
                2 => ChromeBevel::Double,
                _ => ChromeBevel::Flat,
            };
            ui.working.borrow_mut().chrome_mut().bevel = bevel;
            apply_live(&ui);
        });
    }
    {
        let dd = ui.kit_dropdown.clone();
        let ui = ui.clone();
        dd.connect_selected_notify(move |dd| {
            if ui.updating.get() {
                return;
            }
            let idx = dd.selected() as usize;
            let kit = BUTTON_KITS.get(idx).map(|k| k.0).unwrap_or("gnome");
            {
                let mut data = ui.working.borrow_mut();
                data.chrome_mut().buttons.apply_kit(kit);
            }
            let chrome = ui.working.borrow().chrome_or_default();
            ui.updating.set(true);
            ui.min_entry.set_text(&chrome.buttons.minimize);
            ui.max_entry.set_text(&chrome.buttons.maximize);
            ui.close_entry.set_text(&chrome.buttons.close);
            ui.updating.set(false);
            apply_live(&ui);
        });
    }
    wire_glyph_entry(ui, &ui.min_entry, |c, v| c.buttons.minimize = v);
    wire_glyph_entry(ui, &ui.max_entry, |c, v| c.buttons.maximize = v);
    wire_glyph_entry(ui, &ui.close_entry, |c, v| c.buttons.close = v);
}

fn wire_glyph_entry(ui: &Rc<Ui>, entry: &gtk::Entry, set: fn(&mut WindowChrome, String)) {
    let ui = ui.clone();
    entry.connect_changed(move |e| {
        if ui.updating.get() {
            return;
        }
        set(ui.working.borrow_mut().chrome_mut(), e.text().to_string());
        apply_live(&ui);
    });
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

fn draw_gradient(cr: &cairo::Context, w: i32, h: i32, a: &str, b: &str, rtl: bool) {
    let ra = a.parse::<gdk::RGBA>().unwrap_or(gdk::RGBA::BLACK);
    let rb = b.parse::<gdk::RGBA>().unwrap_or(gdk::RGBA::BLACK);
    let (x0, x1) = if rtl {
        (w as f64, 0.0)
    } else {
        (0.0, w as f64)
    };
    let pat = cairo::LinearGradient::new(x0, 0.0, x1, 0.0);
    pat.add_color_stop_rgb(0.0, ra.red() as f64, ra.green() as f64, ra.blue() as f64);
    pat.add_color_stop_rgb(1.0, rb.red() as f64, rb.green() as f64, rb.blue() as f64);
    let _ = cr.set_source(&pat);
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

fn rgb_of(hex: &str) -> (f64, f64, f64) {
    let rgba = hex.parse::<gdk::RGBA>().unwrap_or(gdk::RGBA::BLACK);
    (rgba.red() as f64, rgba.green() as f64, rgba.blue() as f64)
}

fn mix_rgb(a: (f64, f64, f64), b: (f64, f64, f64), t: f64) -> (f64, f64, f64) {
    (
        a.0 + (b.0 - a.0) * t,
        a.1 + (b.1 - a.1) * t,
        a.2 + (b.2 - a.2) * t,
    )
}

fn bar_rgb_pair(
    chrome: &WindowChrome,
    fill: (f64, f64, f64),
    fg: (f64, f64, f64),
    focused: bool,
) -> ((f64, f64, f64), (f64, f64, f64)) {
    let src = if focused {
        chrome.bar.as_ref()
    } else {
        chrome.bar_inactive.as_ref().or(chrome.bar.as_ref())
    };
    if let Some(v) = src {
        let a = v
            .first()
            .map(|s| rgb_of(s))
            .unwrap_or_else(|| mix_rgb(fill, fg, if focused { 0.10 } else { 0.06 }));
        let b = v
            .get(1)
            .map(|s| rgb_of(s))
            .unwrap_or_else(|| mix_rgb(a, fg, if focused { 0.22 } else { 0.10 }));
        return (a, b);
    }
    if focused {
        (mix_rgb(fill, fg, 0.10), mix_rgb(fill, fg, 0.22))
    } else {
        (mix_rgb(fill, fg, 0.06), mix_rgb(fill, (0.0, 0.0, 0.0), 0.12))
    }
}

/// Hyprbars draws − □ × / Nerd PUA via Pango/fontconfig. Cairo's toy API
/// (`select_font_face`) does not, which is why the preview showed empty boxes.
const CHROME_FONT: &str = "DejaVu Sans, Symbols Nerd Font, Symbols Nerd Font Mono, Ubuntu Nerd Font, sans-serif";

fn style_glyph_entry(entry: &gtk::Entry) {
    entry.add_css_class("chrome-glyph");
}

fn chrome_font(px: f64, bold: bool) -> pango::FontDescription {
    let mut desc = pango::FontDescription::from_string(CHROME_FONT);
    if bold {
        desc.set_weight(pango::Weight::Bold);
    }
    desc.set_absolute_size(px * f64::from(pango::SCALE));
    desc
}

fn pango_layout(cr: &cairo::Context, text: &str, px: f64, bold: bool) -> pango::Layout {
    let layout = pangocairo::functions::create_layout(cr);
    layout.set_font_description(Some(&chrome_font(px, bold)));
    layout.set_text(text);
    layout
}

fn pango_measure(cr: &cairo::Context, text: &str, px: f64, bold: bool) -> (i32, i32) {
    pango_layout(cr, text, px, bold).pixel_size()
}

fn pango_show(cr: &cairo::Context, x: f64, y: f64, text: &str, px: f64, bold: bool) {
    let layout = pango_layout(cr, text, px, bold);
    cr.move_to(x, y);
    pangocairo::functions::show_layout(cr, &layout);
}

fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    if r < 0.5 {
        cr.rectangle(x, y, w, h);
        return;
    }
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
    cr.close_path();
}

fn draw_window_chrome(cr: &cairo::Context, w: i32, h: i32, data: &ProfileData) {
    let chrome = data.chrome_or_default();
    let bg = rgb_of(&data.background);
    let fg = rgb_of(&data.foreground);
    let [a0, a1] = data.active_border_stops().unwrap_or_else(|| {
        let hex = data.border_hex();
        [hex.clone(), hex]
    });
    let [i0, i1] = data.inactive_border_stops().unwrap_or_else(|| {
        let hex = data.background.clone();
        [hex.clone(), hex]
    });

    cr.set_source_rgb(bg.0 * 0.55, bg.1 * 0.55, bg.2 * 0.55);
    let _ = cr.paint();

    let pad = 8.0;
    let stack = 20.0;
    let fw = (w as f64 - pad * 2.0 - stack).max(48.0);
    let fh = (h as f64 - pad * 2.0 - stack).max(56.0);

    paint_fake_window(
        cr,
        pad + stack,
        pad,
        fw,
        fh,
        &chrome,
        bg,
        mix_rgb(fg, bg, 0.45),
        rgb_of(&i0),
        rgb_of(&i1),
        "Inactive",
        false,
    );
    paint_fake_window(
        cr,
        pad,
        pad + stack,
        fw,
        fh,
        &chrome,
        bg,
        fg,
        rgb_of(&a0),
        rgb_of(&a1),
        "Window",
        true,
    );
}

fn paint_fake_window(
    cr: &cairo::Context,
    x: f64,
    y: f64,
    fw: f64,
    fh: f64,
    chrome: &WindowChrome,
    bg: (f64, f64, f64),
    fg: (f64, f64, f64),
    outer: (f64, f64, f64),
    inner_col: (f64, f64, f64),
    title: &str,
    focused: bool,
) {
    let thickness = chrome.border_size as f64;
    let radius = chrome.rounding as f64;
    let highlight = mix_rgb(outer, (1.0, 1.0, 1.0), 0.45);
    let shadow = mix_rgb(outer, (0.0, 0.0, 0.0), 0.45);
    let fill = if focused {
        bg
    } else {
        mix_rgb(bg, (0.0, 0.0, 0.0), 0.08)
    };

    {
        rounded_rect(cr, x, y, fw, fh, radius);
        cr.set_source_rgb(fill.0, fill.1, fill.2);
        let _ = cr.fill();

        match chrome.bevel {
            ChromeBevel::Flat => {
                let rtl = chrome.gradient.is_rtl();
                let (x0, x1) = if rtl { (x + fw, x) } else { (x, x + fw) };
                let pat = cairo::LinearGradient::new(x0, y, x1, y);
                pat.add_color_stop_rgb(0.0, outer.0, outer.1, outer.2);
                pat.add_color_stop_rgb(1.0, inner_col.0, inner_col.1, inner_col.2);
                let _ = cr.set_source(&pat);
                cr.set_line_width(thickness.max(1.0));
                rounded_rect(
                    cr,
                    x + thickness / 2.0,
                    y + thickness / 2.0,
                    (fw - thickness).max(1.0),
                    (fh - thickness).max(1.0),
                    radius.max(0.0),
                );
                let _ = cr.stroke();
            }
            ChromeBevel::Inner => {
                cr.set_source_rgb(shadow.0, shadow.1, shadow.2);
                cr.set_line_width(thickness.max(2.0));
                rounded_rect(
                    cr,
                    x + thickness / 2.0,
                    y + thickness / 2.0,
                    (fw - thickness).max(1.0),
                    (fh - thickness).max(1.0),
                    radius,
                );
                let _ = cr.stroke();
                cr.set_source_rgb(highlight.0, highlight.1, highlight.2);
                cr.set_line_width((thickness * 0.35).max(1.0));
                let inset = thickness * 0.55;
                rounded_rect(
                    cr,
                    x + inset,
                    y + inset,
                    (fw - inset * 2.0).max(1.0),
                    (fh - inset * 2.0).max(1.0),
                    (radius - inset * 0.3).max(0.0),
                );
                let _ = cr.stroke();
            }
            ChromeBevel::Double => {
                cr.set_source_rgb(outer.0, outer.1, outer.2);
                cr.set_line_width((thickness * 0.4).max(1.0));
                rounded_rect(
                    cr,
                    x + thickness * 0.25,
                    y + thickness * 0.25,
                    (fw - thickness * 0.5).max(1.0),
                    (fh - thickness * 0.5).max(1.0),
                    radius,
                );
                let _ = cr.stroke();
                cr.set_source_rgb(inner_col.0, inner_col.1, inner_col.2);
                let inset = thickness * 0.75;
                rounded_rect(
                    cr,
                    x + inset,
                    y + inset,
                    (fw - inset * 2.0).max(1.0),
                    (fh - inset * 2.0).max(1.0),
                    (radius - inset * 0.4).max(0.0),
                );
                let _ = cr.stroke();
            }
        }

        if focused {
            draw_nine_cell_guides(cr, x, y, fw, fh, thickness, radius, fg);
        }
    }

    let bar_h = 22.0;
    let bar_inset = thickness.max(2.0);
    let inner_x = x + bar_inset;
    let inner_y = y + bar_inset;
    let inner_w = (fw - bar_inset * 2.0).max(8.0);
    let (mut b0, mut b1) = bar_rgb_pair(chrome, fill, fg, focused);
    if !focused {
        b0 = mix_rgb(b0, (0.0, 0.0, 0.0), 0.12);
        b1 = mix_rgb(b1, (0.0, 0.0, 0.0), 0.12);
    }
    let rtl = chrome.gradient.is_rtl();
    let (x0, x1) = if rtl {
        (inner_x + inner_w, inner_x)
    } else {
        (inner_x, inner_x + inner_w)
    };
    let pat = cairo::LinearGradient::new(x0, inner_y, x1, inner_y);
    pat.add_color_stop_rgb(0.0, b0.0, b0.1, b0.2);
    pat.add_color_stop_rgb(1.0, b1.0, b1.1, b1.2);
    let _ = cr.set_source(&pat);
    cr.rectangle(inner_x, inner_y, inner_w, bar_h);
    let _ = cr.fill();

    cr.set_source_rgb(fg.0, fg.1, fg.2);
    pango_show(cr, inner_x + 8.0, inner_y + 4.0, title, 11.0, true);

    let glyphs = [
        chrome.buttons.minimize.as_str(),
        chrome.buttons.maximize.as_str(),
        chrome.buttons.close.as_str(),
    ];
    let glyph_px = chrome.buttons.size.min(22) as f64;
    let mut gx = inner_x + inner_w - 8.0;
    for g in glyphs.iter().rev() {
        let (tw, _) = pango_measure(cr, g, glyph_px, false);
        gx -= tw as f64 + 10.0;
        pango_show(cr, gx, inner_y + 2.0, g, glyph_px, false);
    }
}

fn draw_nine_cell_guides(
    cr: &cairo::Context,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    thickness: f64,
    _radius: f64,
    fg: (f64, f64, f64),
) {
    let t = thickness.max(2.0);
    cr.set_source_rgba(fg.0, fg.1, fg.2, 0.22);
    cr.set_line_width(1.0);
    cr.set_dash(&[3.0, 3.0], 0.0);
    // Vertical splits (left/right edges vs center)
    cr.move_to(x + t, y);
    cr.line_to(x + t, y + h);
    cr.move_to(x + w - t, y);
    cr.line_to(x + w - t, y + h);
    // Horizontal splits
    cr.move_to(x, y + t);
    cr.line_to(x + w, y + t);
    cr.move_to(x, y + h - t);
    cr.line_to(x + w, y + h - t);
    let _ = cr.stroke();
    cr.set_dash(&[], 0.0);
}

