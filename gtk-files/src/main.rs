//! gtk-files: a GTK4 file manager written in Rust (Nautilus / GNOME Files port).

mod clipboard;
mod config;
mod dnd;
mod file_ops;
mod find_in_files;
mod network;
mod open_with;
mod pathbar;
mod places;
mod prefs;
mod properties;
mod scripts;
mod search;
mod sidebar;
mod sync_setup;
mod sync_status;
mod tab;
mod templates;
mod terminal_panel;
mod thumbnails;
mod transfer_panel;
mod util;
mod window;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use clipboard::SharedClipboard;
use config::Config;
use window::FilesWindow;

const APP_ID: &str = "org.neuronix.GtkFiles";

/// Sentinel rewritten from `--new-window` / `-n` so GApplication accepts it
/// (and forwards it to a running primary instance over D-Bus).
const NEW_WINDOW_SENTINEL: &str = "__GTK_FILES_NEW_WINDOW__";

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN | gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    app.connect_startup(|app| {
        gtk::Window::set_default_icon_name(gtk_theme::app_icons::FILES);
        install_app_actions(app);
        install_accels(app);
        load_css();
        gtk_theme::apply_chrome(gtk_theme::load_profile());
    });

    app.connect_activate(|app| {
        ensure_window(app);
    });

    app.connect_open(|app, files, _| {
        let fw = ensure_window(app);
        open_locations(&fw, files);
        fw.present();
    });

    app.connect_command_line(|app, cmdline| {
        let (new_window, paths) = parse_command_line(cmdline);

        let fw = if new_window {
            open_new_window(app)
        } else {
            ensure_window(app)
        };

        if !paths.is_empty() {
            let files: Vec<gio::File> = paths.iter().map(gio::File::for_path).collect();
            open_locations(&fw, &files);
        }
        fw.present();
        0
    });

    // Rewrite -n/--new-window to a path-like sentinel before GApplication's
    // option parser sees them (add_main_option is unreliable here).
    let args: Vec<String> = std::env::args()
        .map(|a| {
            if a == "--new-window" || a == "-n" {
                NEW_WINDOW_SENTINEL.to_string()
            } else {
                a
            }
        })
        .collect();
    app.run_with_args(&args)
}

fn ensure_window(app: &gtk::Application) -> Rc<FilesWindow> {
    if let Some(fw) = get_primary_window(app) {
        // Keep window map in sync: if the stored window was closed, recreate.
        if fw.window.is_visible() || !app.windows().is_empty() {
            fw.present();
            return fw;
        }
    }
    open_new_window(app)
}

/// Always create a fresh window (shared config + clipboard with existing ones).
fn open_new_window(app: &gtk::Application) -> Rc<FilesWindow> {
    let config = if let Some(existing) = get_primary_window(app) {
        Rc::clone(&existing.config)
    } else {
        Rc::new(RefCell::new(Config::load()))
    };
    let clipboard: SharedClipboard = if let Some(existing) = get_primary_window(app) {
        Rc::clone(&existing.clipboard)
    } else {
        clipboard::new_shared()
    };
    let fw = FilesWindow::new(app, config, clipboard);
    // First window becomes the “primary” for activate/open reuse.
    if get_primary_window(app).is_none() {
        store_primary_window(app, &fw);
    }
    fw.present();
    fw
}

fn open_locations(fw: &Rc<FilesWindow>, files: &[gio::File]) {
    for file in files {
        if let Some(path) = file.path() {
            if path.is_dir() {
                fw.add_tab(Some(gio::File::for_path(&path)));
            } else if let Some(parent) = path.parent() {
                fw.add_tab(Some(gio::File::for_path(parent)));
            }
        } else {
            // Remote / URI location
            fw.add_tab(Some(file.clone()));
        }
    }
}

fn parse_command_line(cmdline: &gio::ApplicationCommandLine) -> (bool, Vec<PathBuf>) {
    let mut new_window = false;
    let mut paths = Vec::new();
    for a in cmdline.arguments().into_iter().skip(1) {
        let s = a.to_string_lossy();
        if s.is_empty() || s.starts_with('-') {
            continue;
        }
        if s == NEW_WINDOW_SENTINEL {
            new_window = true;
            continue;
        }
        let path = PathBuf::from(s.as_ref());
        // Resolve relative to the invoking process cwd (remote launches).
        let path = if path.is_absolute() {
            path
        } else {
            cmdline
                .cwd()
                .map(|cwd| cwd.join(&path))
                .unwrap_or(path)
        };
        paths.push(path);
    }
    (new_window, paths)
}

thread_local! {
    static PRIMARY: RefCell<Option<Rc<FilesWindow>>> = const { RefCell::new(None) };
}

fn store_primary_window(_app: &gtk::Application, fw: &Rc<FilesWindow>) {
    PRIMARY.with(|p| *p.borrow_mut() = Some(Rc::clone(fw)));
}

fn get_primary_window(_app: &gtk::Application) -> Option<Rc<FilesWindow>> {
    PRIMARY.with(|p| p.borrow().clone())
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
    app.set_accels_for_action("win.new-tab", &["<Ctrl>t"]);
    app.set_accels_for_action("win.close-tab", &["<Ctrl>w"]);
    app.set_accels_for_action("win.new-window", &["<Ctrl>n"]);
    app.set_accels_for_action("win.new-folder", &["<Ctrl><Shift>n"]);
    app.set_accels_for_action("win.go-back", &["<Alt>Left", "Back"]);
    app.set_accels_for_action("win.go-forward", &["<Alt>Right", "Forward"]);
    app.set_accels_for_action("win.go-up", &["<Alt>Up"]);
    app.set_accels_for_action("win.go-home", &["<Alt>Home"]);
    app.set_accels_for_action("win.connect-server", &["<Primary><Alt>s"]);
    app.set_accels_for_action("win.reload", &["<Ctrl>r", "F5"]);
    app.set_accels_for_action("win.edit-location", &["<Ctrl>l"]);
    app.set_accels_for_action("win.search", &["<Ctrl>f"]);
    app.set_accels_for_action("win.find-in-files", &["<Ctrl><Shift>f"]);
    app.set_accels_for_action("win.toggle-view", &["<Ctrl>1", "<Ctrl>2"]);
    app.set_accels_for_action("win.show-hidden", &["<Ctrl>h"]);
    // Ctrl+C/X/V/A are handled by a window ShortcutController in window.rs
    // (app accels fight the embedded terminal and were unreliable).
    app.set_accels_for_action("win.trash", &["Delete"]);
    app.set_accels_for_action("win.delete", &["<Shift>Delete"]);
    app.set_accels_for_action("win.rename", &["F2"]);
    app.set_accels_for_action("win.properties", &["<Alt>Return"]);
    app.set_accels_for_action("app.quit", &["<Ctrl>q"]);
    app.set_accels_for_action("app.shortcuts", &["<Ctrl>question"]);
}

fn show_about(app: &gtk::Application) {
    let about = gtk::AboutDialog::builder()
        .program_name("GTK Files")
        .version(env!("CARGO_PKG_VERSION"))
        .comments(
            "gtk-files standalone file manager application, in Rust that browses and manages files and folders.",
        )
        .authors(["Created by Kevin Hinds"])
        .website("https://github.com/NeuronixOS/GTK-Apps")
        .website_label("github.com/NeuronixOS/GTK-Apps")
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
Keyboard shortcuts

Navigation
  Alt+Left / Back     Go back
  Alt+Right / Forward Go forward
  Alt+Up              Parent folder
  Alt+Home            Home
  Ctrl+L              Enter location
  Ctrl+R / F5         Reload

Tabs & windows
  Ctrl+T              New tab
  Ctrl+W              Close tab
  Ctrl+N              New window
  Ctrl+Shift+N        New folder

Command line
  gtk-files [PATH…]
  gtk-files -n|--new-window [PATH…]

Files (when the file list is focused)
  Enter               Open
  Ctrl+C / X / V      Copy / Cut / Paste
  Delete              Move to Trash
  Shift+Delete        Delete permanently
  F2                  Rename
  Alt+Enter           Properties
  Ctrl+A              Select all
  Ctrl+H              Show hidden files
  Ctrl+F              Search folder (filter names)
  Ctrl+Shift+F        Find in Files (content search)
  Ctrl+1 / Ctrl+2     Toggle list/grid view

Terminal (when the terminal is focused)
  Ctrl+Shift+C        Copy
  Ctrl+Shift+V        Paste
  Ctrl+Shift+A        Select all
  Right-click         Copy / Paste / Select all
";
    let dialog = gtk::Window::builder()
        .title("Keyboard Shortcuts")
        .default_width(480)
        .default_height(520)
        .modal(true)
        .build();
    gtk_theme::style_dialog(&dialog);
    if let Some(win) = app.active_window() {
        dialog.set_transient_for(Some(&win));
    }
    let scroll = gtk::ScrolledWindow::new();
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_yalign(0.0);
    label.set_margin_start(16);
    label.set_margin_end(16);
    label.set_margin_top(16);
    label.set_margin_bottom(16);
    label.add_css_class("monospace");
    scroll.set_child(Some(&label));
    dialog.set_child(Some(&scroll));
    dialog.present();
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        "
        .pathbar button {
            padding-left: 6px;
            padding-right: 6px;
        }
        .file-list {
            padding: 4px;
        }
        .file-grid {
            padding: 8px;
        }
        .file-row-content {
            padding: 2px 4px;
            min-height: 0;
        }
        .file-row-content:hover {
            background-color: alpha(@theme_fg_color, 0.08);
            border-radius: 6px;
        }
        /* Tree indent: each nesting level steps right of the parent chevron. */
        treeexpander.file-expander {
            border-spacing: 2px;
            margin: 0;
            padding: 0;
        }
        treeexpander.file-expander > expander {
            min-width: 16px;
            min-height: 16px;
            margin: 0;
            padding: 0;
        }
        treeexpander.file-expander > indent {
            /* Per-depth step — was 6px, which lined children up with the chevron. */
            -gtk-icon-size: 18px;
            min-width: 18px;
        }
        treeexpander.file-expander > .file-row-content {
            margin: 0;
            padding-left: 4px;
            padding-right: 4px;
            padding-top: 2px;
            padding-bottom: 2px;
        }
        .search-bar {
            /* Inherit suite chrome / window bg — do not use @theme_bg_color,
             * which stays dark under Adwaita-dark even with a light profile. */
            background-color: transparent;
        }
        .terminal-panel-header {
            min-height: 22px;
        }
        /* Corner badges overlaid on file/folder icons. */
        .symlink-emblem,
        .lock-emblem,
        .sync-emblem {
            background-color: @theme_base_color;
            border-radius: 9999px;
            padding: 1px;
            box-shadow: 0 0 1px alpha(@theme_fg_color, 0.6);
        }
        .file-row-content.sync-deleted {
            opacity: 0.55;
        }
        .file-row-content.clipboard-cut {
            opacity: 0.4;
        }
        .file-row-content.clipboard-cut image,
        .file-row-content.clipboard-cut label {
            opacity: 0.4;
        }
        .sync-header-status {
            margin-end: 4px;
            opacity: 0.9;
            min-width: 220px;
        }
        .sync-header-status label,
        .sync-header-label {
            margin: 0;
            font-family: monospace;
        }
        /* Copy/move progress docked under Places (non-modal). */
        .transfer-panel {
            border-top: 1px solid alpha(@theme_fg_color, 0.15);
            padding-top: 6px;
        }
        .transfer-panel progressbar {
            margin-top: 2px;
            margin-bottom: 2px;
        }
        ",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    // Optional user CSS: ~/.config/gtk-apps/gtk-files/style.css
    let user = config::style_path();
    if user.exists() {
        let user_provider = gtk::CssProvider::new();
        user_provider.load_from_path(&user);
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &user_provider,
                gtk::STYLE_PROVIDER_PRIORITY_USER,
            );
        }
    }
}

