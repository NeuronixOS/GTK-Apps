//! gtk-edit: GTK4 + GtkSourceView 5 text editor (gedit feature port).

mod config;
mod document;
mod documents_panel;
#[allow(dead_code)]
mod encodings;
mod io;
mod markdown_preview;
mod panel;
// Plugin API keeps scaffolding for future/external plugins.
#[allow(dead_code)]
mod plugin;
#[allow(dead_code)]
mod plugins;
mod prefs;
mod print;
mod replace;
mod search;
mod statusbar;
mod tab;
mod window;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use config::Config;
use plugin::PluginEngine;
use window::{open_files, start_autosave, EditorWindow};

const APP_ID: &str = "org.neuronix.GtkEdit";

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN | gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    app.connect_startup(|app| {
        // Register SourceView types
        sourceview5::View::static_type();
        install_app_actions(app);
        gtk::Window::set_default_icon_name(gtk_theme::app_icons::EDIT);
        gtk_theme::apply_chrome(gtk_theme::load_profile());
    });

    app.connect_activate(|app| {
        let config = Rc::new(RefCell::new(Config::load()));
        let engine = PluginEngine::new(&config.borrow());
        let ew = EditorWindow::new(app, Rc::clone(&config), Rc::clone(&engine));

        // Session restore
        if config.borrow().session.restore_on_startup {
            let files: Vec<PathBuf> = config
                .borrow()
                .session
                .open_files
                .iter()
                .map(PathBuf::from)
                .filter(|p| p.exists())
                .collect();
            if !files.is_empty() {
                open_files(&ew, &files);
            }
        }

        start_autosave(Rc::clone(&ew));
        ew.present();
    });

    app.connect_open(|app, files, _| {
        let ew = ensure_window(app);
        let paths: Vec<PathBuf> = files.iter().filter_map(|f| f.path()).collect();
        open_files(&ew, &paths);
        ew.present();
    });

    app.connect_command_line(|app, cmdline| {
        let args = cmdline.arguments();
        let mut files = Vec::new();
        let mut encoding: Option<String> = None;
        let mut line: Option<i32> = None;
        let mut i = 1;
        while i < args.len() {
            let a = args[i].to_string_lossy();
            if a == "--encoding" && i + 1 < args.len() {
                encoding = Some(args[i + 1].to_string_lossy().to_string());
                i += 2;
                continue;
            }
            if (a == "+" || a.starts_with('+')) && a != "+" {
                if let Ok(n) = a.trim_start_matches('+').parse::<i32>() {
                    line = Some(n);
                }
                i += 1;
                continue;
            }
            if a.starts_with('-') {
                i += 1;
                continue;
            }
            files.push(PathBuf::from(a.as_ref()));
            i += 1;
        }

        app.activate();
        let ew = ensure_window(app);
        if !files.is_empty() {
            open_files(&ew, &files);
        }
        if let Some(enc) = encoding {
            if let Some(tab) = ew.current_tab() {
                *tab.document.encoding.borrow_mut() = enc;
            }
        }
        if let Some(line) = line {
            if let Some(tab) = ew.current_tab() {
                if let Some(iter) = tab.document.buffer.iter_at_line((line - 1).max(0)) {
                    tab.document.buffer.place_cursor(&iter);
                    tab.view.scroll_to_iter(&mut iter.clone(), 0.2, true, 0.0, 0.5);
                }
            }
        }
        ew.present();
        0
    });

    app.run()
}

fn install_app_actions(app: &gtk::Application) {
    let quit = gio::SimpleAction::new("quit", None);
    {
        let app = app.clone();
        quit.connect_activate(move |_, _| app.quit());
    }
    app.add_action(&quit);

    let new_window = gio::SimpleAction::new("new-window", None);
    {
        let app = app.clone();
        new_window.connect_activate(move |_, _| {
            let config = Rc::new(RefCell::new(Config::load()));
            let engine = PluginEngine::new(&config.borrow());
            let ew = EditorWindow::new(&app, config, engine);
            start_autosave(Rc::clone(&ew));
            ew.present();
        });
    }
    app.add_action(&new_window);
}

fn ensure_window(app: &gtk::Application) -> Rc<EditorWindow> {
    if let Some(win) = app.active_window() {
        if let Ok(aw) = win.downcast::<gtk::ApplicationWindow>() {
            if let Some(ew) = window::current_from_window(&aw) {
                return ew;
            }
        }
    }
    // Fallback: create
    let config = Rc::new(RefCell::new(Config::load()));
    let engine = PluginEngine::new(&config.borrow());
    let ew = EditorWindow::new(app, config, engine);
    start_autosave(Rc::clone(&ew));
    ew
}
