//! gtk-video: a GTK4 video trimmer.
//!
//! Core feature set:
//!   - Open a video (dialog, CLI, drag-and-drop)
//!   - Preview playback with play / pause and seeking
//!   - Drag in/out handles on the timeline to select a range
//!   - Crop, rotate, and flip before export
//!   - Export the selected section with ffmpeg (accurate re-encode or stream copy)

mod config;
mod crop;
mod export;
mod neuron;
mod timeline;
mod window;

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use window::VideoWindow;

const APP_ID: &str = "org.neuronix.GtkVideo";

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    app.connect_startup(|app| {
        load_css();
        gtk::Window::set_default_icon_name(gtk_theme::app_icons::VIDEO);
        gtk_theme::apply_chrome(gtk_theme::load_profile());
        install_app_actions(app);
        install_accels(app);
    });

    app.connect_activate(|app| {
        let vw = ensure_window(app);
        vw.present();
    });

    app.connect_open(|app, files, _| {
        let vw = ensure_window(app);
        window::open_files(&vw, files);
        vw.present();
    });

    app.run()
}

thread_local! {
    static PRIMARY: RefCell<Option<Rc<VideoWindow>>> = const { RefCell::new(None) };
}

fn ensure_window(app: &gtk::Application) -> Rc<VideoWindow> {
    if let Some(vw) = PRIMARY.with(|p| p.borrow().clone()) {
        if vw.window.is_visible() || !app.windows().is_empty() {
            return vw;
        }
    }
    let cfg = config::load();
    let vw = VideoWindow::new(app, &cfg);
    PRIMARY.with(|p| *p.borrow_mut() = Some(Rc::clone(&vw)));
    vw
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        "
        .statusbar {
            border-top: 1px solid alpha(@borders, 0.6);
        }
        .video-stage {
            background: #000000;
            min-height: 240px;
        }
        .timeline {
            min-height: 84px;
        }
        .video-overlay {
            transform-origin: center center;
        }
        .video-rot-90 { transform: rotate(90deg); }
        .video-rot-180 { transform: rotate(180deg); }
        .video-rot-270 { transform: rotate(270deg); }
        .video-flip-h { transform: scaleX(-1); }
        .video-flip-v { transform: scaleY(-1); }
        .video-rot-90.video-flip-h { transform: rotate(90deg) scaleX(-1); }
        .video-rot-90.video-flip-v { transform: rotate(90deg) scaleY(-1); }
        .video-rot-90.video-flip-h.video-flip-v { transform: rotate(90deg) scaleX(-1) scaleY(-1); }
        .video-rot-180.video-flip-h { transform: rotate(180deg) scaleX(-1); }
        .video-rot-180.video-flip-v { transform: rotate(180deg) scaleY(-1); }
        .video-rot-180.video-flip-h.video-flip-v { transform: rotate(180deg) scaleX(-1) scaleY(-1); }
        .video-rot-270.video-flip-h { transform: rotate(270deg) scaleX(-1); }
        .video-rot-270.video-flip-v { transform: rotate(270deg) scaleY(-1); }
        .video-rot-270.video-flip-h.video-flip-v { transform: rotate(270deg) scaleX(-1) scaleY(-1); }
        .video-flip-h.video-flip-v { transform: scaleX(-1) scaleY(-1); }
        .crop-overlay {
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
    app.set_accels_for_action("win.export", &["<Ctrl>e", "<Ctrl>s"]);
    app.set_accels_for_action("win.play-pause", &["space"]);
    app.set_accels_for_action("win.goto-in", &["Home"]);
    app.set_accels_for_action("win.goto-out", &["End"]);
    app.set_accels_for_action("win.mark-in", &["bracketleft"]);
    app.set_accels_for_action("win.mark-out", &["bracketright"]);
    app.set_accels_for_action("win.seek-back", &["Left"]);
    app.set_accels_for_action("win.seek-forward", &["Right"]);
    app.set_accels_for_action("win.seek-back-fine", &["<Shift>Left"]);
    app.set_accels_for_action("win.seek-forward-fine", &["<Shift>Right"]);
    app.set_accels_for_action("win.rotate-cw", &["<Ctrl>r"]);
    app.set_accels_for_action("win.rotate-ccw", &["<Ctrl><Shift>r"]);
    app.set_accels_for_action("win.toggle-crop", &["<Ctrl><Shift>c"]);
    app.set_accels_for_action("app.shortcuts", &["<Ctrl>question"]);
    app.set_accels_for_action("app.quit", &["<Ctrl>q"]);
}

fn show_about(app: &gtk::Application) {
    let about = gtk::AboutDialog::builder()
        .program_name("GTK Video")
        .version(env!("CARGO_PKG_VERSION"))
        .comments(
            "gtk-video standalone trimmer: load a clip, set in/out points, crop/rotate/flip, and export the selection.",
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
Keyboard shortcuts\n\n\
  File\n\
  Ctrl+O                Open video\n\
  Ctrl+E / Ctrl+S       Export selection\n\
  Ctrl+Q                Quit\n\n\
  Playback\n\
  Space                 Play / Pause\n\
  Left / Right          Seek 1 second\n\
  Shift+Left / Right    Seek 0.2 seconds\n\
  Home                  Go to in point\n\
  End                   Go to out point\n\n\
  Selection\n\
  [                     Set in point to playhead\n\
  ]                     Set out point to playhead\n\
  Drag handles          Set in / out on the timeline\n\
  Drag playhead         Seek\n\n\
  Transform\n\
  Ctrl+R                Rotate clockwise\n\
  Ctrl+Shift+R          Rotate counterclockwise\n\
  Ctrl+Shift+C          Toggle crop overlay\n\n\
  Ctrl+D                Self Driving\n\
  Ctrl+?                This shortcuts window\n\n\
Config\n\
  ~/.config/gtk-apps/gtk-video/config.toml\n\
  ~/.config/gtk-apps/gtk-video/style.css";

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
        .title("Keyboard Shortcuts — GTK Video")
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
