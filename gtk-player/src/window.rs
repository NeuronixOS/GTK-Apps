//! Player window: playlist, transport, and the loop handles on the time bar.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk::gdk;
use gtk::gio;
use gtk::glib::{self, ControlFlow};
use gtk::prelude::*;
use gtk4 as gtk;

use crate::config::{self, Config};
use crate::media;
use crate::playlist::Playlist;
use crate::timeline::{self, Timeline, TimelineEvent};

const END_SLACK_US: i64 = 80_000;

pub struct PlayerWindow {
    pub window: gtk::ApplicationWindow,
    playlist: Rc<Playlist>,
    revealer: gtk::Revealer,
    playlist_toggle: gtk::ToggleButton,
    stack: gtk::Stack,
    picture: gtk::Picture,
    audio_title: gtk::Label,
    title_label: gtk::Label,
    play_btn: gtk::Button,
    loop_track: gtk::CheckButton,
    loop_playlist: gtk::CheckButton,
    volume: gtk::Scale,
    timeline: Rc<Timeline>,
    status_label: gtk::Label,
    time_label: gtk::Label,
    media: RefCell<Option<gtk::MediaFile>>,
    path: RefCell<Option<PathBuf>>,
    /// True after we already handled the end of this pass, so the tick
    /// does not seek or advance on every frame while sitting at the end.
    at_boundary: Cell<bool>,
}

impl PlayerWindow {
    pub fn new(app: &gtk::Application, cfg: &Config) -> Rc<Self> {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("GTK Player")
            .default_width(cfg.window_width)
            .default_height(cfg.window_height)
            .build();

        let playlist = Playlist::new();

        let picture = gtk::Picture::new();
        picture.set_can_shrink(true);
        picture.set_content_fit(gtk::ContentFit::Contain);
        picture.set_hexpand(true);
        picture.set_vexpand(true);
        picture.add_css_class("stage");

        let audio_icon = gtk::Image::from_icon_name("audio-x-generic");
        audio_icon.set_pixel_size(96);
        audio_icon.set_halign(gtk::Align::Center);
        audio_icon.set_valign(gtk::Align::Center);
        let audio_title = gtk::Label::new(Some(""));
        audio_title.set_wrap(true);
        audio_title.set_justify(gtk::Justification::Center);
        audio_title.set_halign(gtk::Align::Center);
        audio_title.set_valign(gtk::Align::Center);
        // Top margin shifts only the title downward, so the icon stays at
        // the center of the black frame.
        audio_title.set_margin_top(168);
        audio_title.add_css_class("title-2");
        let audio_box = gtk::Overlay::new();
        audio_box.set_hexpand(true);
        audio_box.set_vexpand(true);
        audio_box.add_css_class("audio-stage");
        audio_box.set_child(Some(&gtk::Box::new(gtk::Orientation::Vertical, 0)));
        audio_box.add_overlay(&audio_icon);
        audio_box.add_overlay(&audio_title);

        let empty = gtk::Label::new(Some("Open audio or video, or drop files here"));
        empty.add_css_class("title-2");
        empty.set_wrap(true);

        let stack = gtk::Stack::new();
        stack.set_hexpand(true);
        stack.set_vexpand(true);
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&audio_box, Some("audio"));
        stack.add_named(&picture, Some("video"));
        stack.set_visible_child_name("empty");

        let open_btn = icon_button("document-open-symbolic", "Open");
        let add_btn = icon_button("list-add-symbolic", "Add");
        let remove_btn = icon_button("list-remove-symbolic", "Remove");
        let clear_btn = icon_button("edit-clear-symbolic", "Clear");
        let prev_btn = icon_button("media-skip-backward-symbolic", "Previous");
        let play_btn = icon_button("media-playback-start-symbolic", "Play");
        let next_btn = icon_button("media-skip-forward-symbolic", "Next");
        let stop_btn = icon_button("media-playback-stop-symbolic", "Stop");

        let time_label = gtk::Label::new(Some("0:00 / 0:00"));
        time_label.set_margin_start(8);
        time_label.set_margin_end(8);

        let loop_track = gtk::CheckButton::with_label("Loop track");
        loop_track.set_active(cfg.loop_track);
        loop_track.set_tooltip_text(Some(
            "Repeat the section between the two handles on the time bar",
        ));
        let loop_playlist = gtk::CheckButton::with_label("Loop playlist");
        loop_playlist.set_active(cfg.loop_playlist);
        loop_playlist.set_tooltip_text(Some("After the last item, play the first again"));

        let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.05);
        volume.set_value(cfg.volume);
        volume.set_draw_value(false);
        volume.set_width_request(110);
        volume.set_hexpand(false);
        volume.set_tooltip_text(Some("Volume"));

        let transport = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        transport.set_margin_top(6);
        transport.set_margin_bottom(4);
        transport.set_margin_start(8);
        transport.set_margin_end(8);
        transport.append(&prev_btn);
        transport.append(&play_btn);
        transport.append(&next_btn);
        transport.append(&stop_btn);
        transport.append(&time_label);
        transport.append(&loop_track);
        transport.append(&loop_playlist);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        transport.append(&spacer);
        let vol_icon = gtk::Image::from_icon_name("audio-volume-high-symbolic");
        transport.append(&vol_icon);
        transport.append(&volume);

        let panel = gtk::Box::new(gtk::Orientation::Vertical, 6);
        panel.add_css_class("playlist-panel");
        panel.set_width_request(168);
        panel.set_hexpand(false);
        panel.set_vexpand(true);
        panel.set_margin_top(6);
        panel.set_margin_bottom(6);
        panel.set_margin_start(6);
        panel.set_margin_end(4);
        panel.append(&icon_row(&[&open_btn, &add_btn, &remove_btn, &clear_btn]));
        panel.append(&playlist.root);

        let revealer = gtk::Revealer::new();
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideRight);
        revealer.set_reveal_child(cfg.playlist_open);
        revealer.set_hexpand(false);
        revealer.set_vexpand(true);
        revealer.set_child(Some(&panel));

        let timeline = Timeline::new();

        let status_label = gtk::Label::new(Some("No media"));
        status_label.set_halign(gtk::Align::Start);
        status_label.set_hexpand(true);
        status_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        status_label.set_margin_top(4);
        status_label.set_margin_bottom(4);
        status_label.set_margin_start(10);
        status_label.set_margin_end(10);
        let status = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        status.add_css_class("statusbar");
        status.append(&status_label);

        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        main_box.set_hexpand(true);
        main_box.append(&stack);
        main_box.append(&transport);
        main_box.append(&timeline.area);
        main_box.append(&status);

        let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        body.append(&revealer);
        body.append(&main_box);

        let title_label = gtk::Label::new(Some("GTK Player"));
        title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title_label.add_css_class("title");

        let playlist_toggle = gtk::ToggleButton::new();
        playlist_toggle.set_icon_name("view-list-symbolic");
        playlist_toggle.set_tooltip_text(Some("Playlist"));
        playlist_toggle.set_active(cfg.playlist_open);

        let header = gtk::HeaderBar::new();
        gtk_theme::prepare_headerbar(&header);
        header.pack_start(&playlist_toggle);
        header.set_title_widget(Some(&title_label));
        header.pack_end(&build_menu_button());
        window.set_titlebar(Some(&header));
        window.set_child(Some(&body));

        let player = Rc::new(Self {
            window,
            playlist,
            revealer,
            playlist_toggle,
            stack,
            picture,
            audio_title,
            title_label,
            play_btn,
            loop_track,
            loop_playlist,
            volume,
            timeline,
            status_label,
            time_label,
            media: RefCell::new(None),
            path: RefCell::new(None),
            at_boundary: Cell::new(false),
        });

        wire(&player, &open_btn, &add_btn, &remove_btn, &clear_btn, &prev_btn, &next_btn, &stop_btn);
        install_actions(&player);
        install_drop(&player);
        start_tick(&player);
        persist_on_close(&player);
        player
    }

    pub fn present(&self) {
        self.window.present();
    }

    pub fn open_paths(&self, paths: Vec<PathBuf>, replace: bool) {
        let paths = media::collect(paths);
        if paths.is_empty() {
            self.set_status("Nothing to open");
            return;
        }
        if replace || self.playlist.is_empty() {
            self.playlist.replace(paths);
            self.play_index(0, true);
        } else {
            let start = self.playlist.len();
            self.playlist.extend(paths);
            if self.media.borrow().is_none() {
                self.play_index(start, true);
            } else {
                self.refresh_status();
            }
        }
        self.revealer.set_reveal_child(true);
        self.playlist_toggle.set_active(true);
    }

    fn play_index(&self, index: usize, autostart: bool) {
        let Some(path) = self.playlist.path_at(index) else {
            return;
        };
        if !path.is_file() {
            self.set_status(&format!("Not a file: {}", path.display()));
            return;
        }
        if let Some(media) = self.media.borrow().as_ref() {
            media.pause();
        }

        let file = gio::File::for_path(&path);
        let media = gtk::MediaFile::for_file(&file);
        media.set_volume(self.volume.value());
        self.picture.set_paintable(Some(&media));

        let name = file_name(&path);
        self.audio_title.set_text(&name);
        self.title_label.set_text(&name);
        self.window
            .set_title(Some(&format!("{name} — GTK Player")));
        if media::looks_like_audio(&path) {
            self.stack.set_visible_child_name("audio");
        } else {
            self.stack.set_visible_child_name("video");
        }

        *self.path.borrow_mut() = Some(path.clone());
        *self.media.borrow_mut() = Some(media.clone());
        self.playlist.set_current(Some(index));
        self.timeline.reset_full(0);
        self.at_boundary.set(false);
        self.sync_play_icon();
        self.set_status(&format!("Loading {name}…"));

        let timeline = Rc::clone(&self.timeline);
        media.connect_notify_local(Some("duration"), move |m, _| {
            let dur = m.duration();
            if dur > 0 && timeline.duration_us() <= 0 {
                timeline.set_duration(dur);
            }
        });

        if autostart {
            media.play();
            self.sync_play_icon();
        }
        self.refresh_status();
    }

    fn toggle_play(&self) {
        let Some(media) = self.media() else {
            if self.playlist.len() > 0 {
                let index = self.playlist.current().unwrap_or(0);
                self.play_index(index, true);
            }
            return;
        };
        if media.is_playing() {
            media.pause();
        } else {
            let (start, end) = self.play_bounds();
            let pos = media.timestamp();
            if end > 0 && (pos < start || pos >= end.saturating_sub(20_000) || media.is_ended()) {
                media.seek(start);
                self.timeline.set_playhead(start);
            }
            self.at_boundary.set(false);
            media.play();
        }
        self.sync_play_icon();
        self.refresh_status();
    }

    fn stop(&self) {
        let (start, _) = self.play_bounds();
        if let Some(media) = self.media() {
            media.pause();
            media.seek(start);
        }
        self.timeline.set_playhead(start);
        self.at_boundary.set(false);
        self.sync_play_icon();
        self.refresh_status();
    }

    fn previous(&self) {
        let pos = self.media().map(|m| m.timestamp()).unwrap_or(0);
        let (start, _) = self.play_bounds();
        if pos > start + 3_000_000 {
            self.seek_to(start);
            return;
        }
        let len = self.playlist.len();
        if len == 0 {
            return;
        }
        let index = self.playlist.current().unwrap_or(0);
        if index > 0 {
            self.play_index(index - 1, true);
        } else if self.loop_playlist.is_active() {
            self.play_index(len - 1, true);
        } else {
            self.seek_to(start);
        }
    }

    fn next_item(&self) {
        let len = self.playlist.len();
        if len == 0 {
            return;
        }
        let index = self.playlist.current().unwrap_or(0);
        if index + 1 < len {
            self.play_index(index + 1, true);
        } else if self.loop_playlist.is_active() {
            self.play_index(0, true);
        }
    }

    fn advance_after_end(&self) {
        if self.loop_track.is_active() {
            let start = self.timeline.start_us();
            if let Some(media) = self.media() {
                media.seek(start);
                self.timeline.set_playhead(start);
                if !media.is_playing() {
                    media.play();
                }
            }
            self.at_boundary.set(false);
            return;
        }
        let len = self.playlist.len();
        let index = self.playlist.current().unwrap_or(0);
        if index + 1 < len {
            self.play_index(index + 1, true);
        } else if self.loop_playlist.is_active() && len > 0 {
            if len == 1 {
                if let Some(media) = self.media() {
                    media.seek(0);
                    self.timeline.set_playhead(0);
                    media.play();
                }
                self.at_boundary.set(false);
            } else {
                self.play_index(0, true);
            }
        } else if let Some(media) = self.media() {
            media.pause();
            let end = self.timeline.duration_us();
            if end > 0 {
                media.seek(end);
                self.timeline.set_playhead(end);
            }
            self.at_boundary.set(true);
            self.sync_play_icon();
        }
    }

    fn seek_to(&self, us: i64) {
        let Some(media) = self.media() else {
            return;
        };
        let us = us.clamp(0, self.timeline.duration_us().max(0));
        media.seek(us);
        self.timeline.set_playhead(us);
        self.at_boundary.set(false);
        self.refresh_status();
    }

    fn nudge(&self, delta_us: i64) {
        let dur = self.timeline.duration_us();
        if dur <= 0 {
            return;
        }
        let pos = self
            .media()
            .map(|m| m.timestamp())
            .unwrap_or_else(|| self.timeline.playhead_us());
        self.seek_to((pos + delta_us).clamp(0, dur));
    }

    fn mark_in(&self) {
        if self.timeline.duration_us() <= 0 {
            return;
        }
        let pos = self
            .media()
            .map(|m| m.timestamp())
            .unwrap_or_else(|| self.timeline.playhead_us());
        let end = self.timeline.end_us();
        let us = pos.clamp(0, (end - 200_000).max(0));
        self.timeline.set_range(us, end);
        self.loop_track.set_active(true);
        self.refresh_status();
    }

    fn mark_out(&self) {
        if self.timeline.duration_us() <= 0 {
            return;
        }
        let pos = self
            .media()
            .map(|m| m.timestamp())
            .unwrap_or_else(|| self.timeline.playhead_us());
        let start = self.timeline.start_us();
        let dur = self.timeline.duration_us();
        let us = pos.clamp((start + 200_000).min(dur), dur);
        self.timeline.set_range(start, us);
        self.loop_track.set_active(true);
        self.refresh_status();
    }

    fn remove_selected(&self) {
        let was_current = self.playlist.current();
        let selected = self.playlist.selected().or(was_current);
        if selected.is_none() {
            return;
        }
        let removed_current = selected == was_current;
        let Some(index) = selected else {
            return;
        };
        let next = self.playlist.remove_at(index);
        if self.playlist.is_empty() {
            self.clear_media();
            return;
        }
        if removed_current {
            if let Some(index) = next {
                self.play_index(index, true);
            }
        } else {
            self.refresh_status();
        }
    }

    fn clear_playlist(&self) {
        self.playlist.clear();
        self.clear_media();
    }

    fn clear_media(&self) {
        if let Some(media) = self.media.borrow().as_ref() {
            media.pause();
        }
        *self.media.borrow_mut() = None;
        *self.path.borrow_mut() = None;
        self.picture.set_paintable(None::<&gtk::MediaFile>);
        self.stack.set_visible_child_name("empty");
        self.timeline.reset_full(0);
        self.title_label.set_text("GTK Player");
        self.window.set_title(Some("GTK Player"));
        self.audio_title.set_text("");
        self.at_boundary.set(false);
        self.sync_play_icon();
        self.refresh_status();
    }

    fn media(&self) -> Option<gtk::MediaFile> {
        self.media.borrow().clone()
    }

    /// When "Loop track" is on, playback stays between the two handles.
    /// Otherwise the whole file plays.
    fn play_bounds(&self) -> (i64, i64) {
        if self.loop_track.is_active() {
            (self.timeline.start_us(), self.timeline.end_us())
        } else {
            (0, self.timeline.duration_us())
        }
    }

    fn set_status(&self, text: &str) {
        self.status_label.set_text(text);
    }

    fn refresh_status(&self) {
        let dur = self.timeline.duration_us();
        let pos = self
            .media()
            .map(|m| m.timestamp())
            .unwrap_or_else(|| self.timeline.playhead_us());
        self.time_label.set_text(&format!(
            "{} / {}",
            timeline::format_timestamp(pos),
            timeline::format_timestamp(dur)
        ));

        let len = self.playlist.len();
        if len == 0 {
            self.set_status("No media");
            return;
        }
        let index = self.playlist.current().unwrap_or(0);
        let name = self
            .path
            .borrow()
            .as_ref()
            .map(|p| file_name(p))
            .unwrap_or_default();
        let loop_note = if self.loop_track.is_active() {
            format!(
                "   loop {}–{}",
                timeline::format_timestamp(self.timeline.start_us()),
                timeline::format_timestamp(self.timeline.end_us())
            )
        } else {
            String::new()
        };
        self.set_status(&format!(
            "{} / {len}   {name}{loop_note}",
            index + 1
        ));
    }

    fn sync_play_icon(&self) {
        let playing = self.media().map(|m| m.is_playing()).unwrap_or(false);
        let icon = if playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        };
        self.play_btn.set_icon_name(icon);
    }

    fn update_stage(&self) {
        let Some(media) = self.media() else {
            return;
        };
        if media.has_video() {
            self.stack.set_visible_child_name("video");
        } else if media.has_audio() && media.duration() > 0 {
            self.stack.set_visible_child_name("audio");
        }
    }

    fn save_config(&self) {
        let mut cfg = config::load();
        cfg.window_width = self.window.width();
        cfg.window_height = self.window.height();
        cfg.loop_track = self.loop_track.is_active();
        cfg.loop_playlist = self.loop_playlist.is_active();
        cfg.playlist_open = self.playlist_toggle.is_active();
        cfg.volume = self.volume.value();
        config::save(&cfg);
    }
}

pub fn open_files(player: &Rc<PlayerWindow>, files: &[gio::File]) {
    let paths = files.iter().filter_map(|f| f.path()).collect();
    let replace = player.playlist.is_empty();
    player.open_paths(paths, replace);
}

fn wire(
    player: &Rc<PlayerWindow>,
    open_btn: &gtk::Button,
    add_btn: &gtk::Button,
    remove_btn: &gtk::Button,
    clear_btn: &gtk::Button,
    prev_btn: &gtk::Button,
    next_btn: &gtk::Button,
    stop_btn: &gtk::Button,
) {
    {
        let player = Rc::clone(player);
        open_btn.connect_clicked(move |_| open_dialog(&player, true));
    }
    {
        let player = Rc::clone(player);
        add_btn.connect_clicked(move |_| open_dialog(&player, false));
    }
    {
        let player = Rc::clone(player);
        remove_btn.connect_clicked(move |_| player.remove_selected());
    }
    {
        let player = Rc::clone(player);
        clear_btn.connect_clicked(move |_| player.clear_playlist());
    }
    {
        let player = Rc::clone(player);
        prev_btn.connect_clicked(move |_| player.previous());
    }
    {
        let player = Rc::clone(player);
        let play_btn = player.play_btn.clone();
        play_btn.connect_clicked(move |_| player.toggle_play());
    }
    {
        let player = Rc::clone(player);
        next_btn.connect_clicked(move |_| player.next_item());
    }
    {
        let player = Rc::clone(player);
        stop_btn.connect_clicked(move |_| player.stop());
    }
    {
        let player = Rc::clone(player);
        let toggle = player.playlist_toggle.clone();
        toggle.connect_toggled(move |btn| {
            player.revealer.set_reveal_child(btn.is_active());
            player.save_config();
        });
    }
    {
        let player = Rc::clone(player);
        let loop_track = player.loop_track.clone();
        loop_track.connect_toggled(move |_| {
            player.at_boundary.set(false);
            player.save_config();
            player.refresh_status();
        });
    }
    {
        let player = Rc::clone(player);
        let loop_playlist = player.loop_playlist.clone();
        loop_playlist.connect_toggled(move |_| player.save_config());
    }
    {
        let player = Rc::clone(player);
        let volume = player.volume.clone();
        volume.connect_value_changed(move |scale| {
            if let Some(media) = player.media() {
                media.set_volume(scale.value());
            }
            player.save_config();
        });
    }
    {
        let player = Rc::clone(player);
        let playlist = Rc::clone(&player.playlist);
        playlist.connect_activated(move |index| {
            player.play_index(index, true);
        });
    }

    let player_tl = Rc::clone(player);
    let timeline = Rc::clone(&player.timeline);
    timeline.connect_changed(move |ev| match ev {
        TimelineEvent::Playhead => {
            player_tl.seek_to(player_tl.timeline.playhead_us());
        }
        TimelineEvent::Start | TimelineEvent::End => {
            player_tl.loop_track.set_active(true);
            player_tl.at_boundary.set(false);
            player_tl.refresh_status();
        }
    });
}

fn start_tick(player: &Rc<PlayerWindow>) {
    let player = Rc::downgrade(player);
    glib::timeout_add_local(Duration::from_millis(50), move || {
        let Some(player) = player.upgrade() else {
            return ControlFlow::Break;
        };
        let Some(media) = player.media() else {
            return ControlFlow::Continue;
        };

        let backend_dur = media.duration();
        if backend_dur > 0 && player.timeline.duration_us() <= 0 {
            player.timeline.set_duration(backend_dur);
        }
        player.update_stage();

        if let Some(err) = media.error() {
            player.set_status(&format!("Playback error: {err}"));
            return ControlFlow::Continue;
        }

        if media.is_playing() {
            let pos = media.timestamp();
            player.timeline.set_playhead(pos);
            let (start, end) = player.play_bounds();
            let hit_end = end > 0
                && (pos >= end.saturating_sub(END_SLACK_US) || media.is_ended());
            if hit_end {
                if !player.at_boundary.get() {
                    player.at_boundary.set(true);
                    player.advance_after_end();
                }
            } else if pos + END_SLACK_US < end || pos < start {
                player.at_boundary.set(false);
                if player.loop_track.is_active() && pos < start {
                    media.seek(start);
                    player.timeline.set_playhead(start);
                }
            }
            player.refresh_status();
            player.sync_play_icon();
        }
        ControlFlow::Continue
    });
}

fn persist_on_close(player: &Rc<PlayerWindow>) {
    let player_cb = Rc::clone(player);
    player.window.connect_close_request(move |_| {
        player_cb.save_config();
        glib::Propagation::Proceed
    });
}

fn install_drop(player: &Rc<PlayerWindow>) {
    let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    let player_cb = Rc::clone(player);
    drop.connect_drop(move |_, value, _, _| {
        let Ok(list) = value.get::<gdk::FileList>() else {
            return false;
        };
        let paths = list.files().into_iter().filter_map(|f| f.path()).collect();
        player_cb.open_paths(paths, false);
        true
    });
    player.window.add_controller(drop);
}

fn install_actions(player: &Rc<PlayerWindow>) {
    let window = &player.window;
    {
        let act = gio::SimpleAction::new_stateful(
            "theme",
            Some(glib::VariantTy::STRING),
            &gtk_theme::load_theme_id().to_variant(),
        );
        act.connect_activate(move |action, param| {
            let Some(id) = param.and_then(|p| p.get::<String>()) else {
                return;
            };
            gtk_theme::select_theme(&id, |_| {});
            action.set_state(&id.to_variant());
        });
        window.add_action(&act);
        gtk_theme::watch_theme_sync_action(window, "theme");
        gtk_theme::install_open_theme_editor_action(window);
    }

    bind(player, "open", |p| open_dialog(p, true));
    bind(player, "add", |p| open_dialog(p, false));
    bind(player, "play-pause", |p| p.toggle_play());
    bind(player, "stop", |p| p.stop());
    bind(player, "prev", |p| p.previous());
    bind(player, "next", |p| p.next_item());
    bind(player, "seek-back", |p| p.nudge(-5_000_000));
    bind(player, "seek-forward", |p| p.nudge(5_000_000));
    bind(player, "seek-back-fine", |p| p.nudge(-1_000_000));
    bind(player, "seek-forward-fine", |p| p.nudge(1_000_000));
    bind(player, "goto-in", |p| p.seek_to(p.timeline.start_us()));
    bind(player, "goto-out", |p| {
        if let Some(media) = p.media() {
            media.pause();
        }
        p.seek_to(p.timeline.end_us());
        p.sync_play_icon();
    });
    bind(player, "mark-in", |p| p.mark_in());
    bind(player, "mark-out", |p| p.mark_out());
    bind(player, "remove", |p| p.remove_selected());
    bind(player, "loop-track", |p| {
        p.loop_track.set_active(!p.loop_track.is_active());
    });
    bind(player, "loop-playlist", |p| {
        p.loop_playlist.set_active(!p.loop_playlist.is_active());
    });
    bind(player, "playlist", |p| {
        p.playlist_toggle.set_active(!p.playlist_toggle.is_active());
    });
}

fn bind(player: &Rc<PlayerWindow>, name: &str, f: impl Fn(&Rc<PlayerWindow>) + 'static) {
    let action = gio::SimpleAction::new(name, None);
    let player_cb = Rc::clone(player);
    action.connect_activate(move |_, _| f(&player_cb));
    player.window.add_action(&action);
}

fn build_menu_button() -> gtk::MenuButton {
    let mut icons = gtk_theme::IconMenu::new();
    let menu = gio::Menu::new();

    let file = gio::Menu::new();
    icons.append(&file, "Open…", "win.open", "document-open-symbolic");
    icons.append(&file, "Add to Playlist…", "win.add", "list-add-symbolic");
    icons.append(
        &file,
        "Remove from Playlist",
        "win.remove",
        "list-remove-symbolic",
    );
    menu.append_section(None, &file);

    let playback = gio::Menu::new();
    icons.append(
        &playback,
        "Play / Pause",
        "win.play-pause",
        "media-playback-start-symbolic",
    );
    icons.append(
        &playback,
        "Previous",
        "win.prev",
        "media-skip-backward-symbolic",
    );
    icons.append(
        &playback,
        "Next",
        "win.next",
        "media-skip-forward-symbolic",
    );
    icons.append(
        &playback,
        "Go to Loop Start",
        "win.goto-in",
        "go-first-symbolic",
    );
    icons.append(
        &playback,
        "Go to Loop End",
        "win.goto-out",
        "go-last-symbolic",
    );
    menu.append_section(None, &playback);

    let view = gio::Menu::new();
    icons.append_action(&view, "Loop Track", "win.loop-track");
    icons.append_action(&view, "Loop Playlist", "win.loop-playlist");
    icons.append(&view, "Show Playlist", "win.playlist", "view-list-symbolic");
    gtk_theme::append_profile_menu(&view, "win.theme");
    menu.append_section(None, &view);

    let help = gio::Menu::new();
    icons.append_action(&help, "Keyboard Shortcuts", "app.shortcuts");
    icons.append_action(&help, "About", "app.about");
    menu.append_section(None, &help);

    let button = gtk::MenuButton::new();
    button.set_icon_name("open-menu-symbolic");
    button.set_tooltip_text(Some("Menu"));
    button.set_menu_model(Some(&menu));
    icons.bind_menu_button(&button);
    button
}

#[allow(deprecated)]
fn open_dialog(player: &Rc<PlayerWindow>, replace: bool) {
    let dialog = gtk::FileChooserDialog::new(
        Some(if replace {
            "Open media"
        } else {
            "Add to playlist"
        }),
        Some(&player.window),
        gtk::FileChooserAction::Open,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Open", gtk::ResponseType::Accept),
        ],
    );
    gtk_theme::style_dialog(&dialog);
    dialog.set_modal(true);
    dialog.set_select_multiple(true);

    let media_filter = gtk::FileFilter::new();
    media_filter.set_name(Some("Audio and video"));
    for pattern in media::MEDIA_PATTERNS {
        media_filter.add_pattern(pattern);
    }
    let all = gtk::FileFilter::new();
    all.set_name(Some("All files"));
    all.add_pattern("*");
    dialog.add_filter(&media_filter);
    dialog.add_filter(&all);

    let player = Rc::clone(player);
    dialog.connect_response(move |dialog, resp| {
        let mut paths = Vec::new();
        if resp == gtk::ResponseType::Accept {
            let model = dialog.files();
            for i in 0..model.n_items() {
                if let Some(file) = model.item(i).and_downcast::<gio::File>() {
                    if let Some(path) = file.path() {
                        paths.push(path);
                    }
                }
            }
        }
        dialog.close();
        if !paths.is_empty() {
            player.open_paths(paths, replace);
        }
    });
    dialog.present();
}

fn icon_row(buttons: &[&gtk::Button]) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    row.set_halign(gtk::Align::Center);
    for button in buttons {
        row.append(*button);
    }
    row
}

fn icon_button(icon: &str, tip: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon);
    button.set_tooltip_text(Some(tip));
    button
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
