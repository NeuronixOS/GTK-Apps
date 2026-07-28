//! gtk-term: a minimal, themeable GTK4 + VTE terminal emulator.
//!
//! Implements the core feature set of gnome-terminal:
//!   - Tabbed interface with reorderable, closable tabs
//!   - Find bar (Ctrl+Shift+F) with regex, match case, whole words
//!   - Right-click context menu (copy, paste, select all, open link, …)
//!   - Clickable URL/link detection
//!   - Tab navigation: Ctrl+PgUp/PgDn, Alt+1–9
//!   - Tab reordering: Ctrl+Shift+PgUp/PgDn
//!   - Detach tab to new window
//!   - Set custom tab title
//!   - Reset / Reset+Clear
//!   - Terminal size presets (80×24, 80×43, 132×24, 132×43)
//!   - Confirm-close dialog for tabs/windows with child processes
//!   - Zoom, fullscreen, read-only, built-in color profiles

mod config;
mod prefs;
mod search;
mod terminal;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::{gdk, gio, glib, pango};
use gtk::prelude::*;
use vte4::prelude::*;

use config::Config;

const APP_ID: &str = "org.neuronix.GtkTerm";

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_startup(|app| {
        load_css(config::config_dir().join("style.css").as_path());
        gtk::Window::set_default_icon_name(gtk_theme::app_icons::TERM);
        install_app_actions(app);
    });
    app.connect_activate(build_ui);

    // Tab management
    app.set_accels_for_action("win.new-tab", &["<Ctrl><Shift>t"]);
    app.set_accels_for_action("win.close-tab", &["<Ctrl><Shift>w"]);
    app.set_accels_for_action("win.detach-tab", &["<Ctrl><Shift>d"]);
    app.set_accels_for_action("win.set-title", &["<Ctrl><Shift>i"]);

    // Tab navigation
    app.set_accels_for_action("win.next-tab", &["<Ctrl>Page_Down"]);
    app.set_accels_for_action("win.prev-tab", &["<Ctrl>Page_Up"]);
    app.set_accels_for_action("win.move-tab-left", &["<Ctrl><Shift>Page_Up"]);
    app.set_accels_for_action("win.move-tab-right", &["<Ctrl><Shift>Page_Down"]);

    // Alt+1–9 switch to tab N
    for i in 1..=9u32 {
        let action = format!("win.switch-to-tab-{i}");
        let accel = format!("<Alt>{i}");
        app.set_accels_for_action(&action, &[&accel]);
    }

    // Clipboard & selection
    app.set_accels_for_action("win.copy", &["<Ctrl><Shift>c"]);
    app.set_accels_for_action("win.paste", &["<Ctrl><Shift>v"]);
    app.set_accels_for_action("win.select-all", &["<Ctrl><Shift>a"]);

    // Search
    app.set_accels_for_action("win.find", &["<Ctrl><Shift>f"]);

    // Zoom
    app.set_accels_for_action("win.zoom-in", &["<Ctrl>plus", "<Ctrl>equal"]);
    app.set_accels_for_action("win.zoom-out", &["<Ctrl>minus"]);
    app.set_accels_for_action("win.zoom-reset", &["<Ctrl>0"]);

    // Window
    app.set_accels_for_action("win.fullscreen", &["F11"]);
    app.set_accels_for_action("app.new-window", &["<Ctrl><Shift>n"]);

    app.run()
}

fn load_css(css_path: &Path) {
    if !css_path.exists() {
        return;
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_path(css_path);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_USER,
        );
    }
}

// ---------------------------------------------------------------------------
// Application-scoped actions (shared across all windows).
// ---------------------------------------------------------------------------

fn install_app_actions(app: &gtk::Application) {
    let new_window = gio::SimpleAction::new("new-window", None);
    {
        let app = app.clone();
        new_window.connect_activate(move |_, _| build_ui(&app));
    }
    app.add_action(&new_window);

    let about = gio::SimpleAction::new("about", None);
    {
        let app = app.clone();
        about.connect_activate(move |_, _| show_about(&app));
    }
    app.add_action(&about);
}

fn show_about(app: &gtk::Application) {
    let about = gtk::AboutDialog::builder()
        .program_name("GTK Term")
        .version(env!("CARGO_PKG_VERSION"))
        .comments(
            "gtk-term standalone terminal application, in Rust that provides a themeable VTE terminal emulator.",
        )
        .authors(["Created by Kevin Hinds"])
        .website("https://github.com/khinds10-Neuronix/GTK-Apps")
        .website_label("github.com/khinds10-Neuronix/GTK-Apps")
        .license_type(gtk::License::MitX11)
        .build();
    if let Some(win) = app.active_window() {
        about.set_transient_for(Some(&win));
    }
    about.set_modal(true);
    about.present();
}

// ---------------------------------------------------------------------------
// Window construction.
// ---------------------------------------------------------------------------

fn build_ui(app: &gtk::Application) {
    let cfg = Rc::new(RefCell::new(config::load()));
    let mut state = config::load_state();
    // Prefer prefs grid (columns×rows) over last pixel size so “Initial
    // terminal size” actually controls the next launch.
    if !state.maximized {
        let c = cfg.borrow();
        let (w, h) = estimate_grid_window_pixels(&c, c.columns, c.rows);
        state.window_width = w;
        state.window_height = h;
    }
    let (window, notebook, zoom_label) = build_window_shell(app, Rc::clone(&cfg), &state, true);

    create_tab(&window, &notebook, &cfg.borrow());

    // Restore saved zoom level and profile.
    if state.zoom != 1.0 {
        if let Some(term) = current_terminal(&notebook) {
            term.set_font_scale(state.zoom);
            refresh_zoom_label(&notebook, &zoom_label);
        }
    }
    config::migrate_theme_from_state(&state);
    let theme_id = gtk_theme::load_theme_id();
    let profile = gtk_theme::load_profile();
    gtk_theme::apply_chrome(profile);
    apply_profile_to_notebook(&notebook, profile);
    if let Some(action) = window.lookup_action("theme") {
        action
            .downcast_ref::<gio::SimpleAction>()
            .map(|a| a.set_state(&theme_id.to_variant()));
    }
    watch_shared_theme(&window, &notebook);

    // Once VTE has real cell metrics, snap exactly to the configured grid.
    if !state.maximized {
        let window_weak = window.downgrade();
        let notebook_weak = notebook.downgrade();
        let cols = cfg.borrow().columns;
        let rows = cfg.borrow().rows;
        let applied = Rc::new(std::cell::Cell::new(false));
        window.connect_map(move |_| {
            if applied.get() {
                return;
            }
            applied.set(true);
            let Some(win) = window_weak.upgrade() else { return };
            let Some(nb) = notebook_weak.upgrade() else { return };
            let Some(term) = current_terminal(&nb) else { return };
            let win = win.clone();
            let term = term.clone();
            glib::idle_add_local_once(move || {
                resize_window_to_grid(&win, &term, cols, rows);
            });
        });
    }

    window.present();
}

/// Open a new window that hosts an already-running terminal tab (detached).
fn build_ui_with_existing_tab(
    app: &gtk::Application,
    config: Rc<RefCell<Config>>,
    scroller: gtk::ScrolledWindow,
) {
    let mut state = config::load_state();
    state.window_width = 800;
    state.window_height = 500;
    state.maximized = false;
    state.zoom = terminal_from_scroller(&scroller)
        .map(|t| t.font_scale())
        .unwrap_or(1.0);
    let (window, notebook, zoom_label) =
        build_window_shell(app, Rc::clone(&config), &state, false);
    attach_existing_tab(&window, &notebook, scroller, &config.borrow());
    refresh_zoom_label(&notebook, &zoom_label);
    if let Some(term) = current_terminal(&notebook) {
        if let Some(title) = term.window_title().filter(|t| !t.is_empty()) {
            window.set_title(Some(title.as_str()));
        }
    }
    apply_profile_to_notebook(&notebook, gtk_theme::load_profile());
    watch_shared_theme(&window, &notebook);
    window.present();
    if let Some(term) = current_terminal(&notebook) {
        term.grab_focus();
    }
}

fn build_window_shell(
    app: &gtk::Application,
    cfg: Rc<RefCell<Config>>,
    state: &config::State,
    apply_saved_maximize: bool,
) -> (gtk::ApplicationWindow, gtk::Notebook, gtk::Label) {
    let notebook = gtk::Notebook::new();
    notebook.set_scrollable(true);
    notebook.set_show_border(false);

    let (find_revealer, find_bar_box) = search::build_find_bar();

    let main_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    main_box.append(&find_revealer);
    main_box.append(&notebook);

    let header = gtk::HeaderBar::new();

    let new_tab_btn = gtk::Button::from_icon_name("tab-new-symbolic");
    new_tab_btn.set_tooltip_text(Some("New Tab (Ctrl+Shift+T)"));
    header.pack_start(&new_tab_btn);

    let zoom_label = gtk::Label::new(Some("100%"));
    let menu_button = build_menu_button(&zoom_label, &notebook);
    header.pack_end(&menu_button);

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("GTK Term")
        .default_width(state.window_width)
        .default_height(state.window_height)
        .build();
    if apply_saved_maximize && state.maximized {
        window.maximize();
    }
    window.set_titlebar(Some(&header));
    window.set_child(Some(&main_box));

    {
        let notebook_weak = notebook.downgrade();
        search::connect_find_bar(&find_bar_box, &find_revealer, move || {
            notebook_weak.upgrade().and_then(|n| current_terminal(&n))
        });
    }

    install_actions(
        &window,
        &notebook,
        Rc::clone(&cfg),
        &zoom_label,
        &find_revealer,
    );

    {
        let window_weak = window.downgrade();
        let notebook_weak = notebook.downgrade();
        let cfg = Rc::clone(&cfg);
        new_tab_btn.connect_clicked(move |_| {
            if let (Some(window), Some(notebook)) =
                (window_weak.upgrade(), notebook_weak.upgrade())
            {
                create_tab(&window, &notebook, &cfg.borrow());
            }
        });
    }

    {
        let window_weak = window.downgrade();
        notebook.connect_switch_page(move |_notebook, page_widget, _num| {
            if let Some(window) = window_weak.upgrade() {
                let title = page_widget
                    .clone()
                    .downcast::<gtk::ScrolledWindow>()
                    .ok()
                    .and_then(|s| s.child())
                    .and_then(|c| c.downcast::<vte4::Terminal>().ok())
                    .and_then(|t| t.window_title())
                    .map(|t| t.to_string())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| "GTK Term".to_string());
                window.set_title(Some(&title));
            }
        });
    }

    {
        let notebook_weak = notebook.downgrade();
        window.connect_close_request(move |win| {
            if let Some(notebook) = notebook_weak.upgrade() {
                save_window_state(win, &notebook);
                if notebook.n_pages() > 1 {
                    show_confirm_close_dialog(win, &notebook, None);
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
    }

    (window, notebook, zoom_label)
}

fn save_window_state(window: &gtk::ApplicationWindow, notebook: &gtk::Notebook) {
    let zoom = current_terminal(notebook)
        .map(|t| t.font_scale())
        .unwrap_or(1.0);

    let profile = window
        .lookup_action("theme")
        .and_then(|a| a.downcast_ref::<gio::SimpleAction>().map(|sa| sa.state()))
        .flatten()
        .and_then(|v| v.get::<String>())
        .unwrap_or_else(|| gtk_theme::default_profile_id().to_string());

    let maximized = window.is_maximized();
    let width = window.width();
    let height = window.height();

    let state = config::State {
        window_width: if maximized || width <= 0 { 900 } else { width },
        window_height: if maximized || height <= 0 { 560 } else { height },
        zoom,
        profile,
        maximized,
    };
    config::save_state(&state);
}

fn build_menu_button(zoom_label: &gtk::Label, notebook: &gtk::Notebook) -> gtk::MenuButton {
    let mut icons = gtk_theme::IconMenu::new();
    let menu = gio::Menu::new();

    // Zoom row (custom widget).
    let zoom_section = gio::Menu::new();
    let zoom_item = gio::MenuItem::new(None, None);
    zoom_item.set_attribute_value("custom", Some(&"zoom".to_variant()));
    zoom_section.append_item(&zoom_item);
    menu.append_section(None, &zoom_section);

    // Window / full screen.
    let window_section = gio::Menu::new();
    icons.append_action(&window_section, "New Window", "app.new-window");
    icons.append_action(&window_section, "Full Screen", "win.fullscreen");
    menu.append_section(None, &window_section);

    // Read-only toggle.
    let ro_section = gio::Menu::new();
    icons.append_action(&ro_section, "Read-Only", "win.read-only");
    icons.append_action(&ro_section, "Set Title…", "win.set-title");
    menu.append_section(None, &ro_section);

    // Profiles submenu (shared suite themes).
    let profiles_section = gio::Menu::new();
    gtk_theme::append_profile_menu(&profiles_section, "win.theme");
    menu.append_section(None, &profiles_section);

    // Advanced submenu (reset, size presets).
    let advanced_menu = gio::Menu::new();

    let reset_section = gio::Menu::new();
    icons.append_action(&reset_section, "Reset", "win.reset");
    icons.append_action(&reset_section, "Reset and Clear", "win.reset-and-clear");
    advanced_menu.append_section(None, &reset_section);

    // Size presets stay plain so radio/state indicators remain visible.
    let size_section = gio::Menu::new();
    let sizes = [("80×24", "80x24"), ("80×43", "80x43"), ("132×24", "132x24"), ("132×43", "132x43")];
    for (label, target) in sizes {
        let item = gio::MenuItem::new(Some(label), None);
        item.set_action_and_target_value(Some("win.size-to"), Some(&target.to_variant()));
        size_section.append_item(&item);
    }
    advanced_menu.append_section(None, &size_section);

    let advanced_section = gio::Menu::new();
    icons.append_submenu(
        &advanced_section,
        "Advanced",
        &advanced_menu,
        "preferences-system-symbolic",
    );
    menu.append_section(None, &advanced_section);

    let prefs_section = gio::Menu::new();
    icons.append_action(&prefs_section, "Preferences…", "win.preferences");
    icons.append_action(&prefs_section, "About", "app.about");
    menu.append_section(None, &prefs_section);

    let menu_button = gtk::MenuButton::new();
    menu_button.set_icon_name("open-menu-symbolic");
    menu_button.set_tooltip_text(Some("Menu"));
    menu_button.set_menu_model(Some(&menu));
    icons.bind_menu_button(&menu_button);

    // Custom zoom row.
    let zoom_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    zoom_box.add_css_class("linked");
    zoom_box.set_margin_top(6);
    zoom_box.set_margin_bottom(6);
    zoom_box.set_margin_start(6);
    zoom_box.set_margin_end(6);

    let zoom_out = gtk::Button::from_icon_name("zoom-out-symbolic");
    zoom_out.set_action_name(Some("win.zoom-out"));
    zoom_out.set_tooltip_text(Some("Zoom out"));

    let zoom_reset = gtk::Button::builder().child(zoom_label).hexpand(true).build();
    zoom_reset.set_action_name(Some("win.zoom-reset"));
    zoom_reset.set_tooltip_text(Some("Reset zoom"));

    let zoom_in = gtk::Button::from_icon_name("zoom-in-symbolic");
    zoom_in.set_action_name(Some("win.zoom-in"));
    zoom_in.set_tooltip_text(Some("Zoom in"));

    zoom_box.append(&zoom_out);
    zoom_box.append(&zoom_reset);
    zoom_box.append(&zoom_in);

    if let Some(popover) = menu_button.popover().and_downcast::<gtk::PopoverMenu>() {
        popover.add_child(&zoom_box, "zoom");
        let notebook_weak = notebook.downgrade();
        let zoom_label = zoom_label.clone();
        popover.connect_visible_notify(move |pop| {
            if pop.is_visible() {
                if let Some(notebook) = notebook_weak.upgrade() {
                    refresh_zoom_label(&notebook, &zoom_label);
                }
            }
        });
    }

    menu_button
}

// ---------------------------------------------------------------------------
// Window-scoped actions.
// ---------------------------------------------------------------------------

fn install_actions(
    window: &gtk::ApplicationWindow,
    notebook: &gtk::Notebook,
    cfg: Rc<RefCell<Config>>,
    zoom_label: &gtk::Label,
    find_revealer: &gtk::Revealer,
) {
    // ---- new-tab ----
    let act_new = gio::SimpleAction::new("new-tab", None);
    {
        let window_weak = window.downgrade();
        let notebook_weak = notebook.downgrade();
        let cfg = Rc::clone(&cfg);
        act_new.connect_activate(move |_, _| {
            if let (Some(window), Some(notebook)) =
                (window_weak.upgrade(), notebook_weak.upgrade())
            {
                create_tab(&window, &notebook, &cfg.borrow());
            }
        });
    }
    window.add_action(&act_new);

    // ---- close-tab ----
    let act_close = gio::SimpleAction::new("close-tab", None);
    {
        let notebook_weak = notebook.downgrade();
        let window_weak = window.downgrade();
        act_close.connect_activate(move |_, _| {
            if let (Some(notebook), Some(window)) =
                (notebook_weak.upgrade(), window_weak.upgrade())
            {
                if let Some(page) = notebook.current_page() {
                    remove_tab(&notebook, &window, page);
                }
            }
        });
    }
    window.add_action(&act_close);

    // ---- copy ----
    let act_copy = gio::SimpleAction::new("copy", None);
    {
        let notebook_weak = notebook.downgrade();
        act_copy.connect_activate(move |_, _| {
            if let Some(term) = notebook_weak.upgrade().and_then(|n| current_terminal(&n)) {
                term.copy_clipboard_format(vte4::Format::Text);
            }
        });
    }
    window.add_action(&act_copy);

    // ---- paste ----
    let act_paste = gio::SimpleAction::new("paste", None);
    {
        let notebook_weak = notebook.downgrade();
        act_paste.connect_activate(move |_, _| {
            if let Some(term) = notebook_weak.upgrade().and_then(|n| current_terminal(&n)) {
                term.paste_clipboard();
            }
        });
    }
    window.add_action(&act_paste);

    // ---- select-all ----
    let act_sel = gio::SimpleAction::new("select-all", None);
    {
        let notebook_weak = notebook.downgrade();
        act_sel.connect_activate(move |_, _| {
            if let Some(term) = notebook_weak.upgrade().and_then(|n| current_terminal(&n)) {
                term.select_all();
            }
        });
    }
    window.add_action(&act_sel);

    // ---- find (toggle search bar) ----
    let act_find = gio::SimpleAction::new("find", None);
    {
        let revealer_weak = find_revealer.downgrade();
        act_find.connect_activate(move |_, _| {
            if let Some(rev) = revealer_weak.upgrade() {
                let showing = rev.reveals_child();
                rev.set_reveal_child(!showing);
                if !showing {
                    rev.child().map(|c| c.grab_focus());
                }
            }
        });
    }
    window.add_action(&act_find);

    // ---- reset ----
    let act_reset = gio::SimpleAction::new("reset", None);
    {
        let notebook_weak = notebook.downgrade();
        act_reset.connect_activate(move |_, _| {
            if let Some(term) = notebook_weak.upgrade().and_then(|n| current_terminal(&n)) {
                term.reset(true, false);
            }
        });
    }
    window.add_action(&act_reset);

    // ---- reset-and-clear ----
    let act_reset_clear = gio::SimpleAction::new("reset-and-clear", None);
    {
        let notebook_weak = notebook.downgrade();
        act_reset_clear.connect_activate(move |_, _| {
            if let Some(term) = notebook_weak.upgrade().and_then(|n| current_terminal(&n)) {
                term.reset(true, true);
            }
        });
    }
    window.add_action(&act_reset_clear);

    // ---- next-tab / prev-tab ----
    let act_next = gio::SimpleAction::new("next-tab", None);
    {
        let notebook_weak = notebook.downgrade();
        act_next.connect_activate(move |_, _| {
            if let Some(nb) = notebook_weak.upgrade() {
                if nb.n_pages() > 1 {
                    let next = nb.current_page().map(|p| p + 1).unwrap_or(0);
                    if next >= nb.n_pages() as u32 {
                        nb.set_current_page(Some(0));
                    } else {
                        nb.set_current_page(Some(next));
                    }
                }
            }
        });
    }
    window.add_action(&act_next);

    let act_prev = gio::SimpleAction::new("prev-tab", None);
    {
        let notebook_weak = notebook.downgrade();
        act_prev.connect_activate(move |_, _| {
            if let Some(nb) = notebook_weak.upgrade() {
                if nb.n_pages() > 1 {
                    let cur = nb.current_page().unwrap_or(0);
                    if cur == 0 {
                        nb.set_current_page(Some(nb.n_pages() as u32 - 1));
                    } else {
                        nb.set_current_page(Some(cur - 1));
                    }
                }
            }
        });
    }
    window.add_action(&act_prev);

    // ---- move-tab-left / move-tab-right ----
    let act_move_left = gio::SimpleAction::new("move-tab-left", None);
    {
        let notebook_weak = notebook.downgrade();
        act_move_left.connect_activate(move |_, _| {
            if let Some(nb) = notebook_weak.upgrade() {
                if let Some(page_num) = nb.current_page() {
                    if let Some(child) = nb.nth_page(Some(page_num)) {
                        if page_num > 0 {
                            nb.reorder_child(&child, Some(page_num as u32 - 1));
                        }
                    }
                }
            }
        });
    }
    window.add_action(&act_move_left);

    let act_move_right = gio::SimpleAction::new("move-tab-right", None);
    {
        let notebook_weak = notebook.downgrade();
        act_move_right.connect_activate(move |_, _| {
            if let Some(nb) = notebook_weak.upgrade() {
                if let Some(page_num) = nb.current_page() {
                    if let Some(child) = nb.nth_page(Some(page_num)) {
                        let max = nb.n_pages() as u32 - 1;
                        if page_num < max {
                            nb.reorder_child(&child, Some(page_num as u32 + 1));
                        }
                    }
                }
            }
        });
    }
    window.add_action(&act_move_right);

    // ---- switch-to-tab-N (Alt+1 … Alt+9) ----
    for i in 1..=9u32 {
        let action_name = format!("switch-to-tab-{i}");
        let act = gio::SimpleAction::new(&action_name, None);
        let notebook_weak = notebook.downgrade();
        act.connect_activate(move |_, _| {
            if let Some(nb) = notebook_weak.upgrade() {
                let target = i - 1;
                if target < nb.n_pages() as u32 {
                    nb.set_current_page(Some(target));
                }
            }
        });
        window.add_action(&act);
    }

    // ---- detach-tab ----
    let act_detach = gio::SimpleAction::new("detach-tab", None);
    {
        let window_weak = window.downgrade();
        let notebook_weak = notebook.downgrade();
        let cfg = Rc::clone(&cfg);
        act_detach.connect_activate(move |_, _| {
            if let (Some(old_win), Some(notebook)) =
                (window_weak.upgrade(), notebook_weak.upgrade())
            {
                detach_current_tab(&old_win, &notebook, Rc::clone(&cfg));
            }
        });
    }
    window.add_action(&act_detach);

    // ---- preferences ----
    let act_prefs = gio::SimpleAction::new("preferences", None);
    {
        let window_weak = window.downgrade();
        let notebook_weak = notebook.downgrade();
        let cfg = Rc::clone(&cfg);
        act_prefs.connect_activate(move |_, _| {
            let Some(window) = window_weak.upgrade() else { return };
            let notebook_weak = notebook_weak.clone();
            let window_weak = window.downgrade();
            prefs::show_preferences(&window, Rc::clone(&cfg), move |c| {
                if let Some(nb) = notebook_weak.upgrade() {
                    apply_config_to_notebook(&nb, c);
                    if let (Some(win), Some(term)) =
                        (window_weak.upgrade(), current_terminal(&nb))
                    {
                        resize_window_to_grid(&win, &term, c.columns, c.rows);
                    }
                }
            });
        });
    }
    window.add_action(&act_prefs);

    // ---- set-title ----
    let act_title = gio::SimpleAction::new("set-title", None);
    {
        let window_weak = window.downgrade();
        let notebook_weak = notebook.downgrade();
        act_title.connect_activate(move |_, _| {
            if let (Some(window), Some(notebook)) =
                (window_weak.upgrade(), notebook_weak.upgrade())
            {
                show_set_title_dialog(&window, &notebook);
            }
        });
    }
    window.add_action(&act_title);

    // ---- size-to (terminal size presets) ----
    let act_size = gio::SimpleAction::new("size-to", Some(glib::VariantTy::STRING));
    {
        let notebook_weak = notebook.downgrade();
        let window_weak = window.downgrade();
        let cfg = Rc::clone(&cfg);
        act_size.connect_activate(move |_, param| {
            let Some(s) = param.and_then(|p| p.get::<String>()) else { return };
            let parts: Vec<&str> = s.split('x').collect();
            if parts.len() != 2 {
                return;
            }
            let Ok(cols) = parts[0].parse::<i64>() else { return };
            let Ok(rows) = parts[1].parse::<i64>() else { return };
            if let (Some(nb), Some(win)) = (notebook_weak.upgrade(), window_weak.upgrade()) {
                if let Some(term) = current_terminal(&nb) {
                    resize_window_to_grid(&win, &term, cols, rows);
                    {
                        let mut c = cfg.borrow_mut();
                        c.columns = cols;
                        c.rows = rows;
                    }
                    config::save(&cfg.borrow());
                }
            }
        });
    }
    window.add_action(&act_size);

    // ---- zoom in/out/reset ----
    add_zoom_action(window, notebook, zoom_label, "zoom-in", 0.1);
    add_zoom_action(window, notebook, zoom_label, "zoom-out", -0.1);
    add_zoom_action(window, notebook, zoom_label, "zoom-reset", 0.0);

    // ---- fullscreen (stateful toggle) ----
    let act_fs = gio::SimpleAction::new_stateful("fullscreen", None, &false.to_variant());
    {
        let window_weak = window.downgrade();
        act_fs.connect_change_state(move |action, state| {
            let active = state.and_then(|s| s.get::<bool>()).unwrap_or(false);
            if let Some(window) = window_weak.upgrade() {
                if active {
                    window.fullscreen();
                } else {
                    window.unfullscreen();
                }
            }
            action.set_state(&active.to_variant());
        });
    }
    window.add_action(&act_fs);

    // ---- read-only (stateful toggle) ----
    let act_ro = gio::SimpleAction::new_stateful("read-only", None, &false.to_variant());
    {
        let notebook_weak = notebook.downgrade();
        act_ro.connect_change_state(move |action, state| {
            let active = state.and_then(|s| s.get::<bool>()).unwrap_or(false);
            if let Some(term) = notebook_weak.upgrade().and_then(|n| current_terminal(&n)) {
                term.set_input_enabled(!active);
            }
            action.set_state(&active.to_variant());
        });
    }
    window.add_action(&act_ro);

    // ---- theme / profile (stateful string; shared across gtk-* apps) ----
    let act_theme = gio::SimpleAction::new_stateful(
        "theme",
        Some(glib::VariantTy::STRING),
        &gtk_theme::load_theme_id().to_variant(),
    );
    {
        let notebook_weak = notebook.downgrade();
        act_theme.connect_activate(move |action, param| {
            let Some(id) = param.and_then(|p| p.get::<String>()) else {
                return;
            };
            gtk_theme::select_theme(&id, |profile| {
                if let Some(notebook) = notebook_weak.upgrade() {
                    apply_profile_to_notebook(&notebook, profile);
                }
            });
            action.set_state(&id.to_variant());
        });
    }
    window.add_action(&act_theme);
    gtk_theme::install_open_theme_editor_action(window);
}

fn apply_profile_to_notebook(notebook: &gtk::Notebook, profile: &gtk_theme::Profile) {
    let n = notebook.n_pages();
    for i in 0..n {
        let Some(child) = notebook.nth_page(Some(i)) else {
            continue;
        };
        let Ok(scroller) = child.downcast::<gtk::ScrolledWindow>() else {
            continue;
        };
        if let Some(term) = terminal_from_scroller(&scroller) {
            terminal::apply_profile(&term, profile);
        }
    }
}

fn watch_shared_theme(window: &gtk::ApplicationWindow, notebook: &gtk::Notebook) {
    let notebook = notebook.clone();
    let window = window.clone();
    gtk_theme::watch_theme(move |profile| {
        apply_profile_to_notebook(&notebook, profile);
        if let Some(action) = window.lookup_action("theme") {
            action
                .downcast_ref::<gio::SimpleAction>()
                .map(|a| a.set_state(&profile.id.to_variant()));
        }
    });
}

fn add_zoom_action(
    window: &gtk::ApplicationWindow,
    notebook: &gtk::Notebook,
    zoom_label: &gtk::Label,
    name: &str,
    delta: f64,
) {
    let action = gio::SimpleAction::new(name, None);
    let notebook_weak = notebook.downgrade();
    let label_weak = zoom_label.downgrade();
    action.connect_activate(move |_, _| {
        if let Some(notebook) = notebook_weak.upgrade() {
            if let Some(term) = current_terminal(&notebook) {
                let scale = if delta == 0.0 {
                    1.0
                } else {
                    (term.font_scale() + delta).clamp(0.3, 5.0)
                };
                term.set_font_scale(scale);
            }
            if let Some(label) = label_weak.upgrade() {
                refresh_zoom_label(&notebook, &label);
            }
        }
    });
    window.add_action(&action);
}

fn refresh_zoom_label(notebook: &gtk::Notebook, label: &gtk::Label) {
    let pct = current_terminal(notebook)
        .map(|t| (t.font_scale() * 100.0).round() as i32)
        .unwrap_or(100);
    label.set_text(&format!("{pct}%"));
}

fn current_terminal(notebook: &gtk::Notebook) -> Option<vte4::Terminal> {
    let page = notebook.current_page()?;
    let child = notebook.nth_page(Some(page))?;
    let scroller = child.downcast::<gtk::ScrolledWindow>().ok()?;
    scroller.child()?.downcast::<vte4::Terminal>().ok()
}

// ---------------------------------------------------------------------------
// Tab management.
// ---------------------------------------------------------------------------

fn create_tab(window: &gtk::ApplicationWindow, notebook: &gtk::Notebook, config: &Config) {
    let terminal = terminal::build_terminal(config);
    terminal::apply_profile(&terminal, gtk_theme::load_profile());

    let scroller = gtk::ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .child(&terminal)
        .build();
    scroller.add_css_class("gtk-content");
    apply_scrollbar_policy(&scroller, config.show_scrollbar);

    attach_existing_tab(window, notebook, scroller, config);
    setup_context_menu(&terminal, notebook, window);
    terminal.grab_focus();
}

fn apply_scrollbar_policy(scroller: &gtk::ScrolledWindow, show: bool) {
    let vpolicy = if show {
        gtk::PolicyType::Automatic
    } else {
        gtk::PolicyType::Never
    };
    scroller.set_policy(gtk::PolicyType::Automatic, vpolicy);
}

fn apply_config_to_notebook(notebook: &gtk::Notebook, config: &Config) {
    let n = notebook.n_pages();
    for i in 0..n {
        let Some(child) = notebook.nth_page(Some(i)) else {
            continue;
        };
        let Ok(scroller) = child.downcast::<gtk::ScrolledWindow>() else {
            continue;
        };
        apply_scrollbar_policy(&scroller, config.show_scrollbar);
        if let Some(term) = terminal_from_scroller(&scroller) {
            terminal::apply_settings(&term, config);
        }
    }
}

/// Rough pixel size for a cols×rows grid before VTE cell metrics exist.
fn estimate_grid_window_pixels(config: &Config, cols: i64, rows: i64) -> (i32, i32) {
    let font_px = parse_font_pixel_size(&config.font).unwrap_or(12.0);
    let cw = (font_px * 0.6 * config.cell_width_scale)
        .ceil()
        .clamp(6.0, 40.0) as i32;
    let ch = (font_px * 1.25 * config.cell_height_scale)
        .ceil()
        .clamp(10.0, 60.0) as i32;
    let cols = cols.max(1) as i32;
    let rows = rows.max(1) as i32;
    // Header + notebook tab chrome.
    ((cols * cw + 48).max(200), (rows * ch + 96).max(120))
}

fn parse_font_pixel_size(font: &str) -> Option<f64> {
    font.split_whitespace()
        .rev()
        .find_map(|tok| tok.parse::<f64>().ok())
}

/// Resize the toplevel so the active VTE shows exactly `cols`×`rows` cells
/// (gnome-terminal “size to” behaviour).
fn resize_window_to_grid(
    window: &gtk::ApplicationWindow,
    term: &vte4::Terminal,
    cols: i64,
    rows: i64,
) {
    if window.is_maximized() || window.is_fullscreen() {
        term.set_size(cols, rows);
        return;
    }

    let cols = cols.max(1);
    let rows = rows.max(1);
    term.set_size(cols, rows);

    let cw = term.char_width().max(1) as i32;
    let ch = term.char_height().max(1) as i32;
    let alloc = term.allocation();
    let win_w = window.width();
    let win_h = window.height();

    let (pixel_w, pixel_h) = if win_w > 0 && win_h > 0 && alloc.width() > 0 && alloc.height() > 0 {
        let chrome_w = (win_w - alloc.width()).max(0);
        let chrome_h = (win_h - alloc.height()).max(0);
        (
            chrome_w + cols as i32 * cw,
            chrome_h + rows as i32 * ch,
        )
    } else {
        // Before realize: rough chrome allowance for header + notebook tabs.
        (cols as i32 * cw + 48, rows as i32 * ch + 96)
    };

    let pixel_w = pixel_w.max(200);
    let pixel_h = pixel_h.max(120);
    window.set_default_size(pixel_w, pixel_h);

    // GTK4 ignores set_default_size on an already-mapped window. Temporarily
    // pin the terminal’s size request so the toplevel actually grows/shrinks.
    if window.is_visible() {
        let req_w = (cols as i32 * cw).max(1);
        let req_h = (rows as i32 * ch).max(1);
        term.set_size_request(req_w, req_h);
        window.queue_resize();
        let term = term.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(100), move || {
            term.set_size_request(-1, -1);
        });
    }
}

/// Attach an existing terminal scroller as a notebook tab (new or detached).
fn attach_existing_tab(
    window: &gtk::ApplicationWindow,
    notebook: &gtk::Notebook,
    scroller: gtk::ScrolledWindow,
    config: &Config,
) {
    let terminal = match terminal_from_scroller(&scroller) {
        Some(t) => t,
        None => return,
    };

    let initial_title = terminal
        .window_title()
        .map(|t| t.to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Terminal".to_string());

    let label = gtk::Label::new(Some(&initial_title));
    label.set_ellipsize(pango::EllipsizeMode::End);
    label.set_hexpand(true);
    label.set_xalign(0.5);
    label.set_tooltip_text(Some(&initial_title));

    let close_btn = gtk::Button::from_icon_name("window-close-symbolic");
    close_btn.add_css_class("flat");
    close_btn.set_tooltip_text(Some("Close Tab (Ctrl+Shift+W)"));

    let tab_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    tab_box.set_hexpand(true);
    tab_box.append(&label);
    tab_box.append(&close_btn);

    let page = notebook.append_page(&scroller, Some(&tab_box));
    notebook.set_tab_reorderable(&scroller, true);
    notebook.set_current_page(Some(page));
    notebook.set_show_tabs(notebook.n_pages() > 1);

    let page_obj = notebook.page(&scroller);
    page_obj.set_property("tab-expand", true);
    page_obj.set_property("tab-fill", true);

    {
        let notebook_weak = notebook.downgrade();
        let scroller_weak = scroller.downgrade();
        let window_weak = window.downgrade();
        close_btn.connect_clicked(move |_| {
            close_tab(&notebook_weak, &scroller_weak, &window_weak);
        });
    }

    install_tab_context_menu(&tab_box, notebook, window, &scroller, config);

    {
        let label = label.clone();
        let window_weak = window.downgrade();
        let notebook_weak = notebook.downgrade();
        let scroller_weak = scroller.downgrade();
        terminal.connect_window_title_notify(move |term| {
            let title = term
                .window_title()
                .map(|t| t.to_string())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| "Terminal".to_string());
            if label.widget_name() != "custom-title" {
                label.set_text(&title);
                label.set_tooltip_text(Some(&title));
            }
            if let (Some(window), Some(notebook), Some(scroller)) = (
                window_weak.upgrade(),
                notebook_weak.upgrade(),
                scroller_weak.upgrade(),
            ) {
                if notebook.current_page() == notebook.page_num(&scroller) {
                    window.set_title(Some(&title));
                }
            }
        });
    }

    {
        let notebook_weak = notebook.downgrade();
        let scroller_weak = scroller.downgrade();
        let window_weak = window.downgrade();
        terminal.connect_child_exited(move |_term, _status| {
            close_tab(&notebook_weak, &scroller_weak, &window_weak);
        });
    }
}

fn terminal_from_scroller(scroller: &gtk::ScrolledWindow) -> Option<vte4::Terminal> {
    scroller
        .child()
        .and_then(|c| c.downcast::<vte4::Terminal>().ok())
}

fn close_tab(
    notebook_weak: &glib::WeakRef<gtk::Notebook>,
    scroller_weak: &glib::WeakRef<gtk::ScrolledWindow>,
    window_weak: &glib::WeakRef<gtk::ApplicationWindow>,
) {
    if let (Some(notebook), Some(scroller)) = (notebook_weak.upgrade(), scroller_weak.upgrade()) {
        if let Some(num) = notebook.page_num(&scroller) {
            notebook.remove_page(Some(num));
        }
        notebook.set_show_tabs(notebook.n_pages() > 1);
        if notebook.n_pages() == 0 {
            if let Some(window) = window_weak.upgrade() {
                window.close();
            }
        }
    }
}

fn remove_tab(notebook: &gtk::Notebook, window: &gtk::ApplicationWindow, page: u32) {
    notebook.remove_page(Some(page));
    notebook.set_show_tabs(notebook.n_pages() > 1);
    if notebook.n_pages() == 0 {
        window.close();
    }
}

// ---------------------------------------------------------------------------
// Right-click context menu (mirrors gnome-terminal).
// ---------------------------------------------------------------------------

fn setup_context_menu(
    terminal: &vte4::Terminal,
    _notebook: &gtk::Notebook,
    _window: &gtk::ApplicationWindow,
) {
    let mut icons = gtk_theme::IconMenu::new();
    let menu = gio::Menu::new();

    let clip_section = gio::Menu::new();
    icons.append_action(&clip_section, "Copy", "win.copy");
    icons.append_action(&clip_section, "Paste", "win.paste");
    icons.append_action(&clip_section, "Select All", "win.select-all");
    menu.append_section(None, &clip_section);

    let search_section = gio::Menu::new();
    icons.append_action(&search_section, "Find…", "win.find");
    menu.append_section(None, &search_section);

    let term_section = gio::Menu::new();
    icons.append_action(&term_section, "Reset", "win.reset");
    icons.append_action(&term_section, "Reset and Clear", "win.reset-and-clear");
    menu.append_section(None, &term_section);

    let tab_section = gio::Menu::new();
    icons.append_action(&tab_section, "New Tab", "win.new-tab");
    icons.append_action(&tab_section, "Detach Tab", "win.detach-tab");
    icons.append_action(&tab_section, "Set Title…", "win.set-title");
    icons.append_action(&tab_section, "Close Tab", "win.close-tab");
    menu.append_section(None, &tab_section);

    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    icons.bind_popover(&popover);
    popover.set_parent(terminal);
    popover.set_has_arrow(false);

    // Unparent the popover before the terminal is finalized.
    {
        let popover_weak = popover.downgrade();
        terminal.connect_destroy(move |_| {
            if let Some(p) = popover_weak.upgrade() {
                p.unparent();
            }
        });
    }

    let gesture = gtk::GestureClick::new();
    gesture.set_button(3); // right-click
    {
        let popover = popover.clone();
        gesture.connect_pressed(move |gesture, _n_press, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let rect = gdk::Rectangle::new(x as i32, y as i32, 1, 1);
            popover.set_pointing_to(Some(&rect));
            popover.popup();
        });
    }
    terminal.add_controller(gesture);

    // Ctrl+click on links to open them.
    let link_gesture = gtk::GestureClick::new();
    link_gesture.set_button(1); // left-click
    {
        let terminal_weak = terminal.downgrade();
        link_gesture.connect_pressed(move |gesture, _n, x, y| {
            let modifiers = gesture
                .current_event()
                .map(|e| e.modifier_state())
                .unwrap_or(gdk::ModifierType::empty());
            if !modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
                return;
            }
            if let Some(term) = terminal_weak.upgrade() {
                let (url, _tag) = term.check_match_at(x, y);
                if let Some(ref url) = url {
                    open_url(url);
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                }
            }
        });
    }
    terminal.add_controller(link_gesture);
}

fn open_url(url: &str) {
    let url = if !url.contains("://") {
        format!("https://{url}")
    } else {
        url.to_string()
    };
    if let Err(e) = std::process::Command::new("xdg-open").arg(&url).spawn() {
        eprintln!("gtk-term: failed to open URL {url}: {e}");
    }
}

// ---------------------------------------------------------------------------
// Tab right-click menu (Close Tab / Move to New Window).
// ---------------------------------------------------------------------------

fn install_tab_context_menu(
    tab_box: &gtk::Box,
    notebook: &gtk::Notebook,
    window: &gtk::ApplicationWindow,
    scroller: &gtk::ScrolledWindow,
    config: &Config,
) {
    let group = gio::SimpleActionGroup::new();

    {
        let notebook_weak = notebook.downgrade();
        let scroller_weak = scroller.downgrade();
        let window_weak = window.downgrade();
        let close = gio::SimpleAction::new("close", None);
        close.connect_activate(move |_, _| {
            close_tab(&notebook_weak, &scroller_weak, &window_weak);
        });
        group.add_action(&close);
    }
    {
        let notebook_weak = notebook.downgrade();
        let scroller_weak = scroller.downgrade();
        let window_weak = window.downgrade();
        let cfg = Rc::new(RefCell::new(config.clone()));
        let move_act = gio::SimpleAction::new("move-to-window", None);
        move_act.connect_activate(move |_, _| {
            if let (Some(notebook), Some(scroller), Some(window)) = (
                notebook_weak.upgrade(),
                scroller_weak.upgrade(),
                window_weak.upgrade(),
            ) {
                // Focus this tab first so detach operates on the right page.
                if let Some(num) = notebook.page_num(&scroller) {
                    notebook.set_current_page(Some(num));
                }
                detach_tab(&window, &notebook, &scroller, Rc::clone(&cfg));
            }
        });
        group.add_action(&move_act);
    }

    tab_box.insert_action_group("tab", Some(&group));

    let mut icons = gtk_theme::IconMenu::new();
    let menu = gio::Menu::new();
    icons.append_action(&menu, "Move Tab to New Window", "tab.move-to-window");
    icons.append_action(&menu, "Close Tab", "tab.close");

    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    icons.bind_popover(&popover);
    popover.set_parent(tab_box);
    popover.set_has_arrow(false);
    {
        let popover_weak = popover.downgrade();
        tab_box.connect_destroy(move |_| {
            if let Some(p) = popover_weak.upgrade() {
                p.unparent();
            }
        });
    }

    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);
    {
        let popover = popover.clone();
        let notebook_weak = notebook.downgrade();
        let scroller_weak = scroller.downgrade();
        gesture.connect_pressed(move |gesture, _n, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            // Select the right-clicked tab before showing the menu.
            if let (Some(notebook), Some(scroller)) =
                (notebook_weak.upgrade(), scroller_weak.upgrade())
            {
                if let Some(num) = notebook.page_num(&scroller) {
                    notebook.set_current_page(Some(num));
                }
            }
            popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
    }
    tab_box.add_controller(gesture);
}

// ---------------------------------------------------------------------------
// Detach tab to new window.
// ---------------------------------------------------------------------------

fn detach_current_tab(
    old_window: &gtk::ApplicationWindow,
    notebook: &gtk::Notebook,
    config: Rc<RefCell<Config>>,
) {
    let Some(page_num) = notebook.current_page() else { return };
    let Some(child) = notebook.nth_page(Some(page_num)) else { return };
    let Ok(scroller) = child.downcast::<gtk::ScrolledWindow>() else {
        return;
    };
    detach_tab(old_window, notebook, &scroller, config);
}

fn detach_tab(
    old_window: &gtk::ApplicationWindow,
    notebook: &gtk::Notebook,
    scroller: &gtk::ScrolledWindow,
    config: Rc<RefCell<Config>>,
) {
    if notebook.n_pages() <= 1 {
        return; // don't detach the last tab
    }
    let Some(page_num) = notebook.page_num(scroller) else {
        return;
    };
    let Some(app) = old_window.application() else {
        return;
    };

    // Hold the page so remove_page doesn't destroy the terminal.
    let scroller = scroller.clone();
    notebook.remove_page(Some(page_num));
    notebook.set_show_tabs(notebook.n_pages() > 1);

    // New window that hosts this existing terminal (shell stays alive).
    build_ui_with_existing_tab(&app, config, scroller);
}

// ---------------------------------------------------------------------------
// Set tab title dialog.
// ---------------------------------------------------------------------------

fn show_set_title_dialog(window: &gtk::ApplicationWindow, notebook: &gtk::Notebook) {
    let Some(page) = notebook.current_page() else { return };
    let Some(child) = notebook.nth_page(Some(page)) else { return };

    let entry = gtk::Entry::builder()
        .placeholder_text("Tab title")
        .hexpand(true)
        .build();

    // Pre-fill with the current label text.
    if let Some(tab_widget) = notebook.tab_label(&child) {
        if let Ok(tab_box) = tab_widget.downcast::<gtk::Box>() {
            if let Some(first) = tab_box.first_child() {
                if let Ok(label) = first.downcast::<gtk::Label>() {
                    entry.set_text(&label.text());
                }
            }
        }
    }

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(20);
    content.set_margin_end(20);
    let prompt = gtk::Label::new(Some("Enter a title for the current tab:"));
    content.append(&prompt);
    content.append(&entry);

    let btn_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk::Align::End);
    let cancel_btn =
        gtk_theme::labeled_button(gtk_theme::icon_for_label("Cancel"), "Cancel");
    let ok_btn = gtk_theme::labeled_button(gtk_theme::icon_for_label("OK"), "OK");
    ok_btn.add_css_class("suggested-action");
    btn_box.append(&cancel_btn);
    btn_box.append(&ok_btn);
    content.append(&btn_box);

    let dialog = gtk::Window::builder()
        .title("Set Tab Title")
        .modal(true)
        .resizable(false)
        .transient_for(window)
        .child(&content)
        .default_width(320)
        .build();
    gtk_theme::style_dialog(&dialog);

    {
        let dialog_weak = dialog.downgrade();
        cancel_btn.connect_clicked(move |_| {
            if let Some(d) = dialog_weak.upgrade() {
                d.close();
            }
        });
    }

    {
        let dialog_weak = dialog.downgrade();
        let notebook_weak = notebook.downgrade();
        let window_weak = window.downgrade();
        let entry_clone = entry.clone();
        ok_btn.connect_clicked(move |_| {
            if let (Some(d), Some(nb)) = (dialog_weak.upgrade(), notebook_weak.upgrade()) {
                let new_title = entry_clone.text().to_string();
                if let Some(page) = nb.current_page() {
                    if let Some(child) = nb.nth_page(Some(page)) {
                        if let Some(tab_widget) = nb.tab_label(&child) {
                            if let Ok(tab_box) = tab_widget.downcast::<gtk::Box>() {
                                if let Some(first) = tab_box.first_child() {
                                    if let Ok(label) = first.downcast::<gtk::Label>() {
                                        if new_title.is_empty() {
                                            label.set_widget_name("");
                                            // Resume following the shell title.
                                            let shell_title = child
                                                .clone()
                                                .downcast::<gtk::ScrolledWindow>()
                                                .ok()
                                                .and_then(|s| s.child())
                                                .and_then(|c| c.downcast::<vte4::Terminal>().ok())
                                                .and_then(|t| t.window_title())
                                                .map(|t| t.to_string())
                                                .filter(|t| !t.is_empty())
                                                .unwrap_or_else(|| "Terminal".to_string());
                                            label.set_text(&shell_title);
                                            label.set_tooltip_text(Some(&shell_title));
                                            if let Some(window) = window_weak.upgrade() {
                                                window.set_title(Some(&shell_title));
                                            }
                                        } else {
                                            label.set_text(&new_title);
                                            label.set_tooltip_text(Some(&new_title));
                                            label.set_widget_name("custom-title");
                                            if let Some(window) = window_weak.upgrade() {
                                                window.set_title(Some(&new_title));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                d.close();
            }
        });
    }

    // Enter key = OK
    {
        let ok_btn_clone = ok_btn.clone();
        entry.connect_activate(move |_| ok_btn_clone.emit_clicked());
    }

    dialog.present();
    entry.grab_focus();
}

// ---------------------------------------------------------------------------
// Confirm-close dialog.
// ---------------------------------------------------------------------------

fn show_confirm_close_dialog(
    window: &gtk::ApplicationWindow,
    notebook: &gtk::Notebook,
    specific_tab: Option<u32>,
) {
    let msg = if specific_tab.is_some() {
        "Close this tab?"
    } else {
        "There are multiple tabs open.\nClose all tabs?"
    };

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(20);
    content.set_margin_bottom(16);
    content.set_margin_start(24);
    content.set_margin_end(24);

    let icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
    icon.set_pixel_size(48);
    content.append(&icon);

    let label = gtk::Label::new(Some(msg));
    label.set_justify(gtk::Justification::Center);
    content.append(&label);

    let btn_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk::Align::Center);
    let cancel_btn =
        gtk_theme::labeled_button(gtk_theme::icon_for_label("Cancel"), "Cancel");
    let close_btn =
        gtk_theme::labeled_button(gtk_theme::icon_for_label("Close"), "Close");
    close_btn.add_css_class("destructive-action");
    btn_box.append(&cancel_btn);
    btn_box.append(&close_btn);
    content.append(&btn_box);

    let dialog = gtk::Window::builder()
        .title("Confirm Close")
        .modal(true)
        .resizable(false)
        .transient_for(window)
        .child(&content)
        .default_width(340)
        .build();
    gtk_theme::style_dialog(&dialog);

    {
        let dialog_weak = dialog.downgrade();
        cancel_btn.connect_clicked(move |_| {
            if let Some(d) = dialog_weak.upgrade() {
                d.close();
            }
        });
    }

    {
        let dialog_weak = dialog.downgrade();
        let notebook_weak = notebook.downgrade();
        let window_weak = window.downgrade();
        close_btn.connect_clicked(move |_| {
            if let Some(d) = dialog_weak.upgrade() {
                d.close();
            }
            if let (Some(nb), Some(win)) = (notebook_weak.upgrade(), window_weak.upgrade()) {
                if let Some(tab) = specific_tab {
                    remove_tab(&nb, &win, tab);
                } else {
                    save_window_state(&win, &nb);
                    // destroy() bypasses close_request, avoiding an infinite dialog loop
                    win.destroy();
                }
            }
        });
    }

    dialog.present();
}
