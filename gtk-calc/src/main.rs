//! gtk-calc: a GTK4 calculator written in Rust (GNOME Calculator port).
//!
//! Core feature set:
//!   - Expression entry with Unicode operators (× ÷ − √ ∧ ∨ …)
//!   - Basic / Advanced / Programming / Keyboard modes
//!   - Trig, logs, roots, factorial, bitwise ops, hex/bin literals
//!   - Angle units (deg/rad/grad), number base, word size
//!   - History tape, undo/redo, ans, preferences via TOML

mod buttons;
mod config;
mod engine;
mod equation;
mod window;

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use window::CalcWindow;

const APP_ID: &str = "org.neuronix.GtkCalc";

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    app.connect_startup(|app| {
        load_css();
        gtk_theme::apply_chrome(gtk_theme::load_profile());
        gtk::Window::set_default_icon_name(gtk_theme::app_icons::CALC);
        install_app_actions(app);
        install_accels(app);
    });

    app.connect_activate(|app| {
        let cw = ensure_window(app);
        cw.present();
    });

    app.connect_command_line(|app, cmdline| {
        let args = cmdline.arguments();
        // gtk-calc [--solve EXPR] | [equation…]
        let mut solve_expr: Option<String> = None;
        let mut i = 1;
        while i < args.len() {
            let arg = args[i].to_string_lossy();
            if arg == "--solve" || arg == "-s" {
                if i + 1 < args.len() {
                    solve_expr = Some(args[i + 1].to_string_lossy().into_owned());
                    i += 2;
                    continue;
                }
            } else if !arg.starts_with('-') {
                solve_expr = Some(arg.into_owned());
            }
            i += 1;
        }

        if let Some(expr) = solve_expr {
            let cfg = config::load();
            let ctx = engine::EvalContext {
                angle: cfg.angle_unit,
                ans: 0.0,
                word_size: cfg.word_size,
            };
            match engine::solve(&expr, &ctx) {
                Ok(v) => {
                    let s = engine::format_number(v, cfg.precision, cfg.base);
                    println!("{s}");
                    return 0;
                }
                Err(e) => {
                    eprintln!("gtk-calc: {e}");
                    return 1;
                }
            }
        }

        app.activate();
        0
    });

    app.run()
}

thread_local! {
    static PRIMARY: RefCell<Option<Rc<CalcWindow>>> = const { RefCell::new(None) };
}

fn ensure_window(app: &gtk::Application) -> Rc<CalcWindow> {
    if let Some(cw) = PRIMARY.with(|p| p.borrow().clone()) {
        if cw.window.is_visible() || !app.windows().is_empty() {
            return cw;
        }
    }
    let cfg = config::load();
    let cw = CalcWindow::new(app, &cfg);
    PRIMARY.with(|p| *p.borrow_mut() = Some(Rc::clone(&cw)));
    cw
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        r#"
        .calc-display {
            font-size: 1.75em;
            font-feature-settings: "tnum";
            padding: 10px 8px;
            min-height: 64px;
            border: none;
            box-shadow: none;
            background: transparent;
        }
        .display-container {
            margin: 0;
            min-height: 96px;
        }
        .info-view {
            padding: 0 8px 4px 8px;
            min-height: 14px;
            font-size: 0.85em;
        }
        .history-view {
            background-color: alpha(currentColor, 0.04);
        }
        .history-entry {
            padding: 2px 0;
            font-size: 0.95em;
            font-feature-settings: "tnum";
        }
        .history-entry .answer-label {
            font-weight: bold;
        }
        .statusbar {
            border-top: 1px solid alpha(currentColor, 0.12);
        }
        .math-buttons grid.buttons {
            min-height: calc(40px * 5 + 3px * 4);
        }
        .math-buttons grid.buttons > button {
            font-weight: inherit;
            font-size: 1.05em;
            padding: 0;
            min-height: 40px;
            border-radius: 8px;
        }
        .math-buttons .clear-button {
            font-size: 1.1em;
            font-weight: bolder;
        }
        .math-buttons .number-button {
            font-size: 1.15em;
            font-weight: bolder;
            background-color: alpha(currentColor, 0.10);
        }
        .math-buttons .number-button:hover {
            background-color: alpha(currentColor, 0.16);
        }
        .math-buttons .operator-button,
        .math-buttons .function-button,
        .math-buttons .parenthesis-button,
        .math-buttons .percent-button,
        .math-buttons .numeric-point-button {
            font-size: 1.1em;
        }
        .math-buttons .suggested-action {
            font-size: 1.35em;
            font-weight: bold;
        }
        "#,
    );
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let user = config::config_dir().join("style.css");
    if user.exists() {
        let user_provider = gtk::CssProvider::new();
        user_provider.load_from_path(&user);
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &user_provider,
                gtk::STYLE_PROVIDER_PRIORITY_USER,
            );
        }
    }
}

fn install_app_actions(app: &gtk::Application) {
    let about = gio::SimpleAction::new("about", None);
    {
        let app = app.clone();
        about.connect_activate(move |_, _| show_about(&app));
    }
    app.add_action(&about);

    let shortcuts = gio::SimpleAction::new("shortcuts", None);
    {
        let app = app.clone();
        shortcuts.connect_activate(move |_, _| show_shortcuts(&app));
    }
    app.add_action(&shortcuts);

    let quit = gio::SimpleAction::new("quit", None);
    {
        let app = app.clone();
        quit.connect_activate(move |_, _| app.quit());
    }
    app.add_action(&quit);
}

fn install_accels(app: &gtk::Application) {
    app.set_accels_for_action("win.undo", &["<Ctrl>z"]);
    app.set_accels_for_action("win.redo", &["<Ctrl><Shift>z", "<Ctrl>y"]);
    app.set_accels_for_action("win.clear", &["Escape"]);
    app.set_accels_for_action("win.solve", &["Return", "KP_Enter"]);
    app.set_accels_for_action("win.mode-basic", &["<Ctrl><Alt>b"]);
    app.set_accels_for_action("win.mode-advanced", &["<Ctrl><Alt>a"]);
    app.set_accels_for_action("win.mode-programming", &["<Ctrl><Alt>p"]);
    app.set_accels_for_action("win.mode-keyboard", &["<Ctrl><Alt>k"]);
    app.set_accels_for_action("win.base-2", &["<Ctrl>b"]);
    app.set_accels_for_action("win.base-8", &["<Ctrl>o"]);
    app.set_accels_for_action("win.base-10", &["<Ctrl>d"]);
    app.set_accels_for_action("win.base-16", &["<Ctrl>h"]);
    app.set_accels_for_action("app.shortcuts", &["<Ctrl>question"]);
    app.set_accels_for_action("app.quit", &["<Ctrl>q"]);
}

fn show_about(app: &gtk::Application) {
    let about = gtk::AboutDialog::builder()
        .program_name("GTK Calc")
        .version(env!("CARGO_PKG_VERSION"))
        .comments(
            "gtk-calc standalone calculator application, in Rust that evaluates expressions across basic, advanced, and programming modes.",
        )
        .authors(["Created by Kevin Hinds"])
        .website("https://github.com/khinds10-Neuronix/GTK-Apps")
        .website_label("github.com/khinds10-Neuronix/GTK-Apps")
        .license_type(gtk::License::Gpl30)
        .build();
    if let Some(win) = app.active_window() {
        about.set_transient_for(Some(&win));
    }
    about.set_modal(true);
    about.present();
}

fn show_shortcuts(app: &gtk::Application) {
    let text = "\
Keyboard shortcuts\n\n\
  Calculation\n\
  Enter                 Solve\n\
  Escape                Clear\n\
  Ctrl+Z                Undo\n\
  Ctrl+Shift+Z / Ctrl+Y Redo\n\n\
  Modes\n\
  Ctrl+Alt+B            Basic\n\
  Ctrl+Alt+A            Advanced\n\
  Ctrl+Alt+P            Programming\n\
  Ctrl+Alt+K            Keyboard\n\n\
  Programming\n\
  Ctrl+B / O / D / H    Base 2 / 8 / 10 / 16\n\n\
  App\n\
  Ctrl+?                This shortcuts window\n\
  Ctrl+Q                Quit\n\n\
Config\n\
  ~/.config/gtk-apps/gtk-calc/config.toml\n\
  ~/.config/gtk-apps/gtk-calc/style.css\n\n\
CLI\n\
  gtk-calc --solve '2+3×4'";

    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_margin_top(16);
    label.set_margin_bottom(16);
    label.set_margin_start(20);
    label.set_margin_end(20);
    label.set_selectable(true);

    let scroller = gtk::ScrolledWindow::builder()
        .child(&label)
        .min_content_width(420)
        .min_content_height(400)
        .build();

    let window = gtk::Window::builder()
        .title("Keyboard Shortcuts — GTK Calc")
        .modal(true)
        .resizable(true)
        .default_width(460)
        .default_height(480)
        .child(&scroller)
        .build();
    if let Some(win) = app.active_window() {
        window.set_transient_for(Some(&win));
    }
    window.present();
}
