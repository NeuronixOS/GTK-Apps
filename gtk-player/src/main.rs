//! gtk-player: GTK4 audio and video player.
//!
//! Playback goes through GTK's GStreamer media backend, so any format the
//! installed plugins can decode (H.264, H.265, MP3, FLAC, and the rest)
//! plays without an in-app decoder list.

mod config;
mod media;
mod playlist;
mod timeline;
mod window;

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;

use window::PlayerWindow;

const APP_ID: &str = "org.neuronix.GtkPlayer";

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
        ensure_window(app).present();
    });

    app.connect_open(|app, files, _| {
        let player = ensure_window(app);
        window::open_files(&player, files);
        player.present();
    });

    app.run()
}

thread_local! {
    static PRIMARY: RefCell<Option<Rc<PlayerWindow>>> = const { RefCell::new(None) };
}

fn ensure_window(app: &gtk::Application) -> Rc<PlayerWindow> {
    if let Some(player) = PRIMARY.with(|p| p.borrow().clone()) {
        if player.window.is_visible() || !app.windows().is_empty() {
            return player;
        }
    }
    let cfg = config::load();
    let player = PlayerWindow::new(app, &cfg);
    PRIMARY.with(|p| *p.borrow_mut() = Some(Rc::clone(&player)));
    player
}

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        "
        .statusbar {
            border-top: 1px solid alpha(@borders, 0.6);
        }
        .stage {
            background: #000000;
            min-height: 240px;
        }
        .audio-stage {
            background: #000000;
            min-height: 240px;
        }
        .playlist-panel {
            border-right: 1px solid alpha(@borders, 0.7);
        }
        .timeline {
            min-height: 84px;
        }
        row.current {
            background: alpha(@theme_selected_bg_color, 0.35);
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
    app.set_accels_for_action("win.add", &["<Ctrl><Shift>o"]);
    app.set_accels_for_action("win.play-pause", &["space"]);
    app.set_accels_for_action("win.stop", &["<Ctrl>period"]);
    app.set_accels_for_action("win.prev", &["p", "<Ctrl>Left"]);
    app.set_accels_for_action("win.next", &["n", "<Ctrl>Right"]);
    app.set_accels_for_action("win.seek-back", &["Left"]);
    app.set_accels_for_action("win.seek-forward", &["Right"]);
    app.set_accels_for_action("win.seek-back-fine", &["<Shift>Left"]);
    app.set_accels_for_action("win.seek-forward-fine", &["<Shift>Right"]);
    app.set_accels_for_action("win.goto-in", &["Home"]);
    app.set_accels_for_action("win.goto-out", &["End"]);
    app.set_accels_for_action("win.mark-in", &["bracketleft"]);
    app.set_accels_for_action("win.mark-out", &["bracketright"]);
    app.set_accels_for_action("win.loop-track", &["l"]);
    app.set_accels_for_action("win.loop-playlist", &["<Shift>l"]);
    app.set_accels_for_action("win.playlist", &["F9", "<Ctrl>l"]);
    app.set_accels_for_action("win.remove", &["Delete"]);
    app.set_accels_for_action("app.shortcuts", &["<Ctrl>question"]);
    app.set_accels_for_action("app.quit", &["<Ctrl>q"]);
}

fn show_about(app: &gtk::Application) {
    let about = gtk::AboutDialog::builder()
        .program_name("GTK Player")
        .version(env!("CARGO_PKG_VERSION"))
        .comments(
            "Play video and audio through GStreamer, with a playlist and a loop region on the time bar.",
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
  Ctrl+O                Open (replaces the playlist)\n\
  Ctrl+Shift+O          Add to the playlist\n\
  Delete                Remove the selected playlist item\n\
  F9 / Ctrl+L           Show or hide the playlist\n\
  Ctrl+Q                Quit\n\n\
  Playback\n\
  Space                 Play / Pause\n\
  P / Ctrl+Left         Previous\n\
  N / Ctrl+Right        Next\n\
  Left / Right          Seek 5 seconds\n\
  Shift+Left / Right    Seek 1 second\n\
  Home                  Go to loop start\n\
  End                   Go to loop end\n\n\
  Loop\n\
  Drag the handles      Set the section that loops\n\
  Drag the playhead     Seek\n\
  [ / ]                 Set loop start / end at the playhead\n\
  L                     Loop this track\n\
  Shift+L               Loop the playlist\n\n\
  Ctrl+?                This shortcuts window\n\n\
Config\n\
  ~/.config/gtk-apps/gtk-player/config.toml\n\
  ~/.config/gtk-apps/gtk-player/style.css";

    let label = gtk::Label::new(Some(text));
    label.set_halign(gtk::Align::Start);
    label.set_margin_top(12);
    label.set_margin_bottom(12);
    label.set_margin_start(16);
    label.set_margin_end(16);
    label.set_selectable(true);

    let dialog = gtk::Window::builder()
        .title("Keyboard Shortcuts")
        .modal(true)
        .build();
    if let Some(win) = app.active_window() {
        dialog.set_transient_for(Some(&win));
    }
    dialog.set_child(Some(&label));
    dialog.present();
}
