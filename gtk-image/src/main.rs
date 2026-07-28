//! gtk-image: a GTK4 image viewer written in Rust (Eye of GNOME / eog port).
//!
//! Core feature set:
//!   - Open file / folder; CLI and drag-and-drop
//!   - Directory sibling navigation (prev / next / first / last)
//!   - Best-fit and free zoom (in / out / 100%), scroll-wheel zoom, drag pan
//!   - Rotate CW/CCW, flip horizontal/vertical
//!   - Fullscreen, copy, trash, save as
//!   - Header bar, status bar, gear menu, context menu, shortcuts help

mod config;
mod image_list;
mod image_view;
mod window;

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use window::ImageWindow;

const APP_ID: &str = "org.neuronix.GtkImage";

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    app.connect_startup(|app| {
        load_css();
        gtk::Window::set_default_icon_name(gtk_theme::app_icons::IMAGE);
        gtk_theme::apply_chrome(gtk_theme::load_profile());
        install_app_actions(app);
        install_accels(app);
    });

    app.connect_activate(|app| {
        let iw = ensure_window(app);
        iw.present();
    });

    app.connect_open(|app, files, _| {
        let iw = ensure_window(app);
        window::open_files(&iw, files);
        iw.present();
    });

    app.run()
}

thread_local! {
    static PRIMARY: RefCell<Option<Rc<ImageWindow>>> = const { RefCell::new(None) };
}

fn ensure_window(app: &gtk::Application) -> Rc<ImageWindow> {
    if let Some(iw) = PRIMARY.with(|p| p.borrow().clone()) {
        if iw.window.is_visible() || !app.windows().is_empty() {
            return iw;
        }
    }
    let cfg = config::load();
    let iw = ImageWindow::new(app, &cfg);
    PRIMARY.with(|p| *p.borrow_mut() = Some(Rc::clone(&iw)));
    iw
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        "
        .statusbar {
            border-top: 1px solid alpha(@borders, 0.6);
        }
        .image-view {
            background: transparent;
        }
        ",
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
    app.set_accels_for_action("win.open", &["<Ctrl>o"]);
    app.set_accels_for_action("win.open-folder", &["<Ctrl><Shift>o"]);
    app.set_accels_for_action("win.save-as", &["<Ctrl><Shift>s"]);
    app.set_accels_for_action("win.copy", &["<Ctrl>c"]);
    app.set_accels_for_action("win.trash", &["Delete"]);

    app.set_accels_for_action("win.go-previous", &["Left", "BackSpace"]);
    app.set_accels_for_action("win.go-next", &["Right", "space"]);
    app.set_accels_for_action("win.go-first", &["Home"]);
    app.set_accels_for_action("win.go-last", &["End"]);

    app.set_accels_for_action("win.zoom-in", &["plus", "equal", "<Ctrl>plus", "<Ctrl>equal"]);
    app.set_accels_for_action("win.zoom-out", &["minus", "<Ctrl>minus"]);
    app.set_accels_for_action("win.zoom-normal", &["1", "<Ctrl>0"]);
    app.set_accels_for_action("win.zoom-fit", &["f"]);

    app.set_accels_for_action("win.rotate-cw", &["<Ctrl>r"]);
    app.set_accels_for_action("win.rotate-ccw", &["<Ctrl><Shift>r"]);

    app.set_accels_for_action("win.fullscreen", &["F11"]);
    app.set_accels_for_action("app.shortcuts", &["<Ctrl>question"]);
    app.set_accels_for_action("app.quit", &["<Ctrl>q"]);
}

fn show_about(app: &gtk::Application) {
    let about = gtk::AboutDialog::builder()
        .program_name("GTK Image")
        .version(env!("CARGO_PKG_VERSION"))
        .comments(
            "gtk-image standalone image viewer application, in Rust that browses, zooms, and transforms images.",
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
  File\n\
  Ctrl+O                Open image\n\
  Ctrl+Shift+O          Open folder\n\
  Ctrl+Shift+S          Save As\n\
  Delete                Move to Trash\n\
  Ctrl+C                Copy image\n\
  Ctrl+Q                Quit\n\n\
  Navigation\n\
  Left / Backspace      Previous image\n\
  Right / Space         Next image\n\
  Home / End            First / last image\n\n\
  View\n\
  + / =                 Zoom in\n\
  -                     Zoom out\n\
  1 / Ctrl+0            Normal size (100%)\n\
  F                     Best fit\n\
  Scroll wheel          Zoom in / out\n\
  Drag                  Pan when zoomed\n\
  F11                   Toggle full screen\n\n\
  Transform\n\
  Ctrl+R                Rotate clockwise\n\
  Ctrl+Shift+R          Rotate counterclockwise\n\n\
  Ctrl+?                This shortcuts window\n\n\
Config\n\
  ~/.config/gtk-apps/gtk-image/config.toml\n\
  ~/.config/gtk-apps/gtk-image/style.css";

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
        .min_content_height(440)
        .build();

    let window = gtk::Window::builder()
        .title("Keyboard Shortcuts — GTK Image")
        .modal(true)
        .resizable(true)
        .default_width(460)
        .default_height(520)
        .child(&scroller)
        .build();
    if let Some(win) = app.active_window() {
        window.set_transient_for(Some(&win));
    }
    window.present();
}
