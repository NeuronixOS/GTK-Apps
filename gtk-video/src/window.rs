//! Main application window: video preview, timeline in/out handles, export.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::glib::ControlFlow;
use gtk::prelude::*;

use crate::config::{self, Config};
use crate::crop::{self, CropOverlay, CropRect};
use crate::export::{self, ExportJob};
use crate::timeline::{Timeline, TimelineEvent};

pub struct VideoWindow {
    pub window: gtk::ApplicationWindow,
    picture: gtk::Picture,
    overlay: gtk::Overlay,
    stack: gtk::Stack,
    timeline: Rc<Timeline>,
    crop: Rc<CropOverlay>,
    title_label: gtk::Label,
    status_label: gtk::Label,
    play_btn: gtk::Button,
    loop_toggle: gtk::CheckButton,
    accurate_toggle: gtk::CheckButton,
    path: RefCell<Option<PathBuf>>,
    media: RefCell<Option<gtk::MediaFile>>,
    src_w: Cell<i32>,
    src_h: Cell<i32>,
    rotate_q: Cell<u8>,
    flip_h: Cell<bool>,
    flip_v: Cell<bool>,
}

impl VideoWindow {
    pub fn new(app: &gtk::Application, cfg: &Config) -> Rc<Self> {
        let placeholder = gtk::Label::new(Some(
            "Open a video or drop a file here\nDrag the handles to set in and out points",
        ));
        placeholder.add_css_class("dim-label");
        placeholder.set_justify(gtk::Justification::Center);
        placeholder.set_halign(gtk::Align::Center);
        placeholder.set_valign(gtk::Align::Center);
        placeholder.set_wrap(true);

        let picture = gtk::Picture::new();
        picture.set_hexpand(true);
        picture.set_vexpand(true);
        picture.set_can_shrink(true);
        picture.set_content_fit(gtk::ContentFit::Contain);
        picture.add_css_class("video-stage");

        let stack = gtk::Stack::new();
        stack.set_hexpand(true);
        stack.set_vexpand(true);
        stack.add_css_class("gtk-content");
        stack.add_named(&placeholder, Some("empty"));
        stack.add_named(&picture, Some("video"));
        stack.set_visible_child_name("empty");

        let crop = CropOverlay::new();
        let overlay = gtk::Overlay::new();
        overlay.set_hexpand(true);
        overlay.set_vexpand(true);
        overlay.add_css_class("video-overlay");
        overlay.set_child(Some(&stack));
        overlay.add_overlay(&crop.area);

        let stage = gtk::Box::new(gtk::Orientation::Vertical, 0);
        stage.add_css_class("video-stage");
        stage.set_hexpand(true);
        stage.set_vexpand(true);
        stage.append(&overlay);

        let timeline = Timeline::new();

        let play_btn = gtk::Button::from_icon_name("media-playback-start-symbolic");
        play_btn.set_tooltip_text(Some("Play / Pause (Space)"));
        play_btn.set_action_name(Some("win.play-pause"));

        let to_in = gtk::Button::from_icon_name("go-first-symbolic");
        to_in.set_tooltip_text(Some("Go to in point (Home)"));
        to_in.set_action_name(Some("win.goto-in"));

        let to_out = gtk::Button::from_icon_name("go-last-symbolic");
        to_out.set_tooltip_text(Some("Go to out point (End)"));
        to_out.set_action_name(Some("win.goto-out"));

        let mark_in = gtk::Button::from_icon_name("zoom-original-symbolic");
        mark_in.set_label(" Set In");
        mark_in.set_tooltip_text(Some("Set in point to playhead ([)"));
        mark_in.set_action_name(Some("win.mark-in"));

        let mark_out = gtk::Button::from_icon_name("zoom-original-symbolic");
        mark_out.set_label(" Set Out");
        mark_out.set_tooltip_text(Some("Set out point to playhead (])"));
        mark_out.set_action_name(Some("win.mark-out"));

        let export_btn = gtk_theme::labeled_button("document-save-as-symbolic", "Export");
        export_btn.set_tooltip_text(Some("Export selected section (Ctrl+E)"));
        export_btn.set_action_name(Some("win.export"));
        export_btn.add_css_class("suggested-action");

        let loop_toggle = gtk::CheckButton::with_label("Loop selection");
        loop_toggle.set_active(cfg.loop_selection);

        let accurate_toggle = gtk::CheckButton::with_label("Accurate cut (re-encode)");
        accurate_toggle.set_active(cfg.accurate_export);
        accurate_toggle.set_tooltip_text(Some(
            "Re-encode so in/out points are exact. Uncheck for a fast stream copy.",
        ));

        let transport = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        transport.set_margin_start(12);
        transport.set_margin_end(12);
        transport.set_margin_top(8);
        transport.set_halign(gtk::Align::Fill);
        transport.append(&play_btn);
        transport.append(&to_in);
        transport.append(&to_out);
        transport.append(&mark_in);
        transport.append(&mark_out);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        transport.append(&spacer);
        transport.append(&loop_toggle);
        transport.append(&accurate_toggle);
        transport.append(&export_btn);

        let status_label = gtk::Label::new(Some("No video loaded"));
        status_label.add_css_class("dim-label");
        status_label.set_xalign(0.0);
        status_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        status_label.set_hexpand(true);

        let status_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        status_box.add_css_class("statusbar");
        status_box.set_margin_start(10);
        status_box.set_margin_end(10);
        status_box.set_margin_top(4);
        status_box.set_margin_bottom(4);
        status_box.append(&status_label);

        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        main_box.append(&stage);
        main_box.append(&transport);
        main_box.append(&timeline.area);
        main_box.append(&status_box);

        let title_label = gtk::Label::new(Some("GTK Video"));
        title_label.add_css_class("title");
        title_label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);

        let header = gtk::HeaderBar::new();
        gtk_theme::prepare_headerbar(&header);
        header.set_title_widget(Some(&title_label));

        let open_btn = gtk::Button::from_icon_name("document-open-symbolic");
        open_btn.set_tooltip_text(Some("Open Video (Ctrl+O)"));
        open_btn.set_action_name(Some("win.open"));
        header.pack_start(&open_btn);

        let header_play = gtk::Button::from_icon_name("media-playback-start-symbolic");
        header_play.set_tooltip_text(Some("Play / Pause (Space)"));
        header_play.set_action_name(Some("win.play-pause"));
        header.pack_start(&header_play);

        let rot_ccw = gtk::Button::from_icon_name("object-rotate-left-symbolic");
        rot_ccw.set_tooltip_text(Some("Rotate counterclockwise (Ctrl+Shift+R)"));
        rot_ccw.set_action_name(Some("win.rotate-ccw"));
        header.pack_start(&rot_ccw);

        let rot_cw = gtk::Button::from_icon_name("object-rotate-right-symbolic");
        rot_cw.set_tooltip_text(Some("Rotate clockwise (Ctrl+R)"));
        rot_cw.set_action_name(Some("win.rotate-cw"));
        header.pack_start(&rot_cw);

        let crop_btn = gtk::ToggleButton::new();
        crop_btn.set_icon_name("edit-select-all-symbolic");
        crop_btn.set_tooltip_text(Some("Crop (Ctrl+Shift+C)"));
        crop_btn.set_action_name(Some("win.toggle-crop"));
        header.pack_start(&crop_btn);

        let header_export = gtk::Button::from_icon_name("document-save-as-symbolic");
        header_export.set_tooltip_text(Some("Export (Ctrl+E)"));
        header_export.set_action_name(Some("win.export"));
        header.pack_end(&header_export);

        let menu_button = build_menu_button();
        header.pack_end(&menu_button);

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("GTK Video")
            .default_width(cfg.window_width)
            .default_height(cfg.window_height)
            .build();
        window.set_titlebar(Some(&header));

        let vw = Rc::new(Self {
            window: window.clone(),
            picture,
            overlay,
            stack,
            timeline,
            crop,
            title_label,
            status_label,
            play_btn,
            loop_toggle,
            accurate_toggle,
            path: RefCell::new(None),
            media: RefCell::new(None),
            src_w: Cell::new(0),
            src_h: Cell::new(0),
            rotate_q: Cell::new(0),
            flip_h: Cell::new(false),
            flip_v: Cell::new(false),
        });

        crate::neuron::attach(&header, &window, &main_box, &vw);

        install_actions(&vw);
        setup_drop_target(&vw);
        wire_timeline(&vw);
        start_playhead_tick(&vw);
        persist_toggles(&vw);

        {
            let vw_c = Rc::clone(&vw);
            vw.crop.connect_changed(move || {
                vw_c.sync_accurate_sensitive();
                vw_c.refresh_status();
            });
        }

        {
            let vw_weak = Rc::downgrade(&vw);
            vw.window.connect_close_request(move |win| {
                if !win.is_maximized() && !win.is_fullscreen() {
                    let w = win.width();
                    let h = win.height();
                    if w > 0 && h > 0 {
                        let mut cfg = config::load();
                        cfg.window_width = w;
                        cfg.window_height = h;
                        if let Some(vw) = vw_weak.upgrade() {
                            cfg.loop_selection = vw.loop_toggle.is_active();
                            cfg.accurate_export = vw.accurate_toggle.is_active();
                        }
                        config::save(&cfg);
                    }
                }
                glib::Propagation::Proceed
            });
        }

        vw
    }

    pub fn present(&self) {
        self.window.present();
    }

    pub fn open_path(&self, path: &Path) {
        if !path.is_file() {
            self.set_status(&format!("Not a file: {}", path.display()));
            return;
        }
        if !export::is_supported_video(path) {
            // Still try — GTK/GStreamer may play formats we did not list.
            eprintln!(
                "gtk-video: opening {} (unlisted extension)",
                path.display()
            );
        }

        if let Some(media) = self.media.borrow().as_ref() {
            media.pause();
        }

        let file = gio::File::for_path(path);
        let media = gtk::MediaFile::for_file(&file);
        self.picture.set_paintable(Some(&media));
        self.stack.set_visible_child_name("video");

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        self.title_label.set_text(&name);
        self.window.set_title(Some(&format!("{name} — GTK Video")));
        *self.path.borrow_mut() = Some(path.to_path_buf());
        *self.media.borrow_mut() = Some(media.clone());
        self.rotate_q.set(0);
        self.flip_h.set(false);
        self.flip_v.set(false);
        self.crop.reset();
        self.set_crop_mode(false);
        self.timeline.reset_full(0);
        self.set_status(&format!("Loading {name}…"));
        self.sync_play_icon();

        if let Some(size) = export::probe_size(path) {
            self.src_w.set(size.width);
            self.src_h.set(size.height);
            self.crop.set_video_size(size.width, size.height);
        } else {
            self.src_w.set(0);
            self.src_h.set(0);
        }

        if let Some(dur) = export::probe_duration_us(path) {
            self.timeline.reset_full(dur);
            self.refresh_status();
        }

        let timeline = Rc::clone(&self.timeline);
        media.connect_notify_local(Some("duration"), move |m, _| {
            let dur = m.duration();
            if dur > 0 && timeline.duration_us() <= 0 {
                timeline.set_duration(dur);
            }
        });
    }

    fn media(&self) -> Option<gtk::MediaFile> {
        self.media.borrow().clone()
    }

    pub fn current_path(&self) -> Option<PathBuf> {
        self.path.borrow().clone()
    }

    pub fn current_state(&self) -> serde_json::Value {
        let crop = self.crop.crop();
        serde_json::json!({
            "path": self.current_path().map(|p| p.to_string_lossy().into_owned()),
            "duration_us": self.timeline.duration_us(),
            "in_us": self.timeline.start_us(),
            "out_us": self.timeline.end_us(),
            "playhead_us": self.timeline.playhead_us(),
            "playing": self.media().map(|m| m.is_playing()).unwrap_or(false),
            "width": self.src_w.get(),
            "height": self.src_h.get(),
            "rotate_deg": (self.rotate_q.get() as i32 % 4) * 90,
            "flip_h": self.flip_h.get(),
            "flip_v": self.flip_v.get(),
            "crop": crop.map(|c| serde_json::json!({
                "x": c.x, "y": c.y, "w": c.w, "h": c.h
            })),
        })
    }

    fn set_status(&self, text: &str) {
        self.status_label.set_text(text);
    }

    fn refresh_status(&self) {
        let Some(path) = self.current_path() else {
            self.set_status("No video loaded");
            return;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dur = self.timeline.duration_us();
        let sel = self.timeline.selection_us();
        let play = self.timeline.playhead_us();
        let mut extra = String::new();
        let w = self.src_w.get();
        let h = self.src_h.get();
        if w > 0 && h > 0 {
            extra.push_str(&format!("   {w}×{h}"));
        }
        match self.rotate_q.get() % 4 {
            1 => extra.push_str("   90°"),
            2 => extra.push_str("   180°"),
            3 => extra.push_str("   270°"),
            _ => {}
        }
        if self.flip_h.get() {
            extra.push_str("   flip-h");
        }
        if self.flip_v.get() {
            extra.push_str("   flip-v");
        }
        if let Some(c) = self.crop.crop() {
            extra.push_str(&format!("   crop {}×{}", c.w, c.h));
        }
        self.set_status(&format!(
            "{name}   {} / {}   selection {}{extra}",
            export::format_timestamp(play),
            export::format_timestamp(dur),
            export::format_timestamp(sel)
        ));
    }

    fn filters_active(&self) -> bool {
        self.rotate_q.get() % 4 != 0
            || self.flip_h.get()
            || self.flip_v.get()
            || self.crop.crop().is_some()
    }

    fn apply_preview_css(&self) {
        for c in [
            "video-rot-90",
            "video-rot-180",
            "video-rot-270",
            "video-flip-h",
            "video-flip-v",
        ] {
            self.overlay.remove_css_class(c);
        }
        // Crop handles are in source pixels. Skip CSS transforms while cropping
        // so the overlay stays aligned with the unrotated frame.
        if !self.crop.enabled() {
            match self.rotate_q.get() % 4 {
                1 => self.overlay.add_css_class("video-rot-90"),
                2 => self.overlay.add_css_class("video-rot-180"),
                3 => self.overlay.add_css_class("video-rot-270"),
                _ => {}
            }
            if self.flip_h.get() {
                self.overlay.add_css_class("video-flip-h");
            }
            if self.flip_v.get() {
                self.overlay.add_css_class("video-flip-v");
            }
        }
        self.sync_accurate_sensitive();
        self.refresh_status();
    }

    fn sync_crop_action(&self, on: bool) {
        if let Some(act) = self.window.lookup_action("toggle-crop") {
            act.change_state(&on.to_variant());
        }
    }

    fn set_crop_mode(&self, on: bool) {
        self.apply_crop_mode(on);
        self.sync_crop_action(on);
    }

    fn apply_crop_mode(&self, on: bool) {
        self.crop.set_enabled(on);
        if on && self.crop.crop().is_none() && self.src_w.get() > 0 {
            self.crop
                .set_crop(Some(CropRect::full(self.src_w.get(), self.src_h.get())));
        }
        self.apply_preview_css();
    }

    fn sync_accurate_sensitive(&self) {
        let need = self.filters_active();
        if need {
            self.accurate_toggle.set_active(true);
            self.accurate_toggle.set_sensitive(false);
            self.accurate_toggle.set_tooltip_text(Some(
                "Crop, rotate, or flip requires a re-encode (stream copy is disabled).",
            ));
        } else {
            self.accurate_toggle.set_sensitive(true);
            self.accurate_toggle.set_tooltip_text(Some(
                "Re-encode so in/out points are exact. Uncheck for a fast stream copy.",
            ));
        }
    }

    pub fn rotate_cw(&self) {
        self.rotate_q.set((self.rotate_q.get() + 1) % 4);
        self.apply_preview_css();
    }

    pub fn rotate_ccw(&self) {
        self.rotate_q.set((self.rotate_q.get() + 3) % 4);
        self.apply_preview_css();
    }

    pub fn flip_horizontal(&self) {
        self.flip_h.set(!self.flip_h.get());
        self.apply_preview_css();
    }

    pub fn flip_vertical(&self) {
        self.flip_v.set(!self.flip_v.get());
        self.apply_preview_css();
    }

    pub fn set_playing(&self, playing: bool) {
        let Some(media) = self.media() else {
            return;
        };
        if playing {
            if !media.is_playing() {
                let _ = media.play();
                self.sync_play_icon();
            }
        } else if media.is_playing() {
            media.pause();
            self.sync_play_icon();
        }
    }

    pub fn seek_us(&self, us: i64) {
        self.seek_to(us);
    }

    pub fn set_in_us(&self, us: Option<i64>) {
        if self.timeline.duration_us() <= 0 {
            return;
        }
        let pos = us.unwrap_or_else(|| {
            self.media()
                .map(|m| m.timestamp())
                .unwrap_or_else(|| self.timeline.playhead_us())
        });
        let end = self.timeline.end_us();
        let clamped = pos.clamp(0, (end - export::MIN_SELECTION_US).max(0));
        self.timeline.set_range(clamped, end);
        self.refresh_status();
    }

    pub fn set_out_us(&self, us: Option<i64>) {
        if self.timeline.duration_us() <= 0 {
            return;
        }
        let pos = us.unwrap_or_else(|| {
            self.media()
                .map(|m| m.timestamp())
                .unwrap_or_else(|| self.timeline.playhead_us())
        });
        let start = self.timeline.start_us();
        let dur = self.timeline.duration_us();
        let clamped = pos.clamp((start + export::MIN_SELECTION_US).min(dur), dur);
        self.timeline.set_range(start, clamped);
        self.refresh_status();
    }

    pub fn set_range_us(&self, start: i64, end: i64) {
        if self.timeline.duration_us() <= 0 {
            return;
        }
        self.timeline.set_range(start, end);
        self.refresh_status();
    }

    pub fn apply_crop_preset(&self, preset: &str) -> Result<(), String> {
        let vw = self.src_w.get();
        let vh = self.src_h.get();
        if vw <= 0 || vh <= 0 {
            return Err("no video loaded".into());
        }
        let rect = crop::apply_preset(preset, vw, vh)?;
        if rect.is_full(vw, vh) {
            self.crop.set_crop(None);
        } else {
            self.crop.set_crop(Some(rect));
        }
        self.set_crop_mode(true);
        Ok(())
    }

    pub fn set_crop_rect(&self, x: i32, y: i32, w: i32, h: i32) -> Result<(), String> {
        let vw = self.src_w.get();
        let vh = self.src_h.get();
        if vw <= 0 || vh <= 0 {
            return Err("no video loaded".into());
        }
        self.crop.set_crop(Some(CropRect { x, y, w, h }.clamp(vw, vh)));
        self.set_crop_mode(true);
        Ok(())
    }

    pub fn reset_crop(&self) {
        self.crop.reset();
        self.sync_accurate_sensitive();
        self.refresh_status();
    }

    fn make_export_job(&self, input: PathBuf, output: PathBuf) -> ExportJob {
        ExportJob {
            input,
            output,
            start_us: self.timeline.start_us(),
            end_us: self.timeline.end_us(),
            accurate: self.accurate_toggle.is_active() || self.filters_active(),
            rotate_q: self.rotate_q.get(),
            flip_h: self.flip_h.get(),
            flip_v: self.flip_v.get(),
            crop: self.crop.crop().map(|c| c.as_tuple()),
            src_width: self.src_w.get(),
            src_height: self.src_h.get(),
        }
    }

    pub fn export_to(&self, output: PathBuf) -> Result<(), String> {
        let input = self.current_path().ok_or("no video loaded")?;
        if self.timeline.duration_us() <= 0 {
            return Err("wait for the video to finish loading".into());
        }
        if !export::ffmpeg_available() {
            return Err("ffmpeg is not installed".into());
        }
        if self.timeline.end_us() - self.timeline.start_us() < export::MIN_SELECTION_US {
            return Err("selection is too short to export".into());
        }
        if let Some(media) = self.media() {
            media.pause();
            self.sync_play_icon();
        }
        run_export_async(self, self.make_export_job(input, output));
        Ok(())
    }

    fn sync_play_icon(&self) {
        let playing = self
            .media()
            .map(|m| m.is_playing())
            .unwrap_or(false);
        let icon = if playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        };
        self.play_btn.set_icon_name(icon);
    }

    fn seek_to(&self, us: i64) {
        let Some(media) = self.media() else {
            return;
        };
        media.seek(us.max(0));
        self.timeline.set_playhead(us);
        self.refresh_status();
    }

    pub fn toggle_play(&self) {
        let Some(media) = self.media() else {
            return;
        };
        if media.is_playing() {
            media.pause();
        } else {
            let start = self.timeline.start_us();
            let end = self.timeline.end_us();
            let pos = media.timestamp();
            if pos < start || pos >= end.saturating_sub(20_000) {
                media.seek(start);
                self.timeline.set_playhead(start);
            }
            media.play();
        }
        self.sync_play_icon();
        self.refresh_status();
    }

    fn mark_in(&self) {
        if self.timeline.duration_us() <= 0 {
            return;
        }
        let pos = self
            .media()
            .map(|m| m.timestamp())
            .unwrap_or_else(|| self.timeline.playhead_us());
        // Drive the start handle via the same clamp the drag path uses.
        let end = self.timeline.end_us();
        let us = pos.clamp(0, (end - export::MIN_SELECTION_US).max(0));
        self.timeline.set_range(us, end);
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
        let us = pos.clamp((start + export::MIN_SELECTION_US).min(dur), dur);
        self.timeline.set_range(start, us);
        self.refresh_status();
    }

    fn goto_in(&self) {
        self.seek_to(self.timeline.start_us());
    }

    fn goto_out(&self) {
        let media = self.media();
        if let Some(m) = &media {
            m.pause();
        }
        self.seek_to(self.timeline.end_us());
        self.sync_play_icon();
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
}

fn persist_toggles(vw: &Rc<VideoWindow>) {
    let save = {
        let vw = Rc::clone(vw);
        move || {
            let mut cfg = config::load();
            cfg.loop_selection = vw.loop_toggle.is_active();
            cfg.accurate_export = vw.accurate_toggle.is_active();
            config::save(&cfg);
        }
    };
    {
        let save = save.clone();
        vw.loop_toggle.connect_toggled(move |_| save());
    }
    {
        let save = save.clone();
        vw.accurate_toggle.connect_toggled(move |_| save());
    }
}

fn wire_timeline(vw: &Rc<VideoWindow>) {
    let vw_cb = Rc::clone(vw);
    vw.timeline.connect_changed(move |ev| {
        match ev {
            TimelineEvent::Start | TimelineEvent::End | TimelineEvent::Playhead => {
                if let Some(media) = vw_cb.media() {
                    media.pause();
                    media.seek(vw_cb.timeline.playhead_us());
                }
                vw_cb.sync_play_icon();
                vw_cb.refresh_status();
            }
        }
    });
}

fn start_playhead_tick(vw: &Rc<VideoWindow>) {
    let vw = Rc::downgrade(vw);
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        let Some(vw) = vw.upgrade() else {
            return ControlFlow::Break;
        };
        let Some(media) = vw.media() else {
            return ControlFlow::Continue;
        };

        // Pick up duration once the backend knows it.
        let backend_dur = media.duration();
        if backend_dur > 0 && vw.timeline.duration_us() <= 0 {
            vw.timeline.set_duration(backend_dur);
        } else if backend_dur > 0 {
            let current = vw.timeline.duration_us();
            // If ffprobe was missing, fill in; don't shrink a better probe.
            if current <= 0 {
                vw.timeline.set_duration(backend_dur);
            }
        }

        if let Some(err) = media.error() {
            vw.set_status(&format!("Playback error: {err}"));
        }

        if media.is_playing() {
            let pos = media.timestamp();
            vw.timeline.set_playhead(pos);
            let end = vw.timeline.end_us();
            let start = vw.timeline.start_us();
            if end > 0 && pos >= end.saturating_sub(20_000) {
                if vw.loop_toggle.is_active() {
                    media.seek(start);
                    vw.timeline.set_playhead(start);
                } else {
                    media.pause();
                    media.seek(end);
                    vw.timeline.set_playhead(end);
                    vw.sync_play_icon();
                }
            }
            vw.refresh_status();
            vw.sync_play_icon();
        }
        ControlFlow::Continue
    });
}

fn build_menu_button() -> gtk::MenuButton {
    let mut icons = gtk_theme::IconMenu::new();
    let menu = gio::Menu::new();

    let file = gio::Menu::new();
    icons.append_action(&file, "Open…", "win.open");
    icons.append_action(&file, "Export Selection…", "win.export");
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
        "Go to In Point",
        "win.goto-in",
        "go-first-symbolic",
    );
    icons.append(
        &playback,
        "Go to Out Point",
        "win.goto-out",
        "go-last-symbolic",
    );
    icons.append(
        &playback,
        "Set In Point",
        "win.mark-in",
        "list-add-symbolic",
    );
    icons.append(
        &playback,
        "Set Out Point",
        "win.mark-out",
        "list-add-symbolic",
    );
    menu.append_section(None, &playback);

    let transform = gio::Menu::new();
    icons.append(
        &transform,
        "Rotate Clockwise",
        "win.rotate-cw",
        "object-rotate-right-symbolic",
    );
    icons.append(
        &transform,
        "Rotate Counterclockwise",
        "win.rotate-ccw",
        "object-rotate-left-symbolic",
    );
    icons.append_action(&transform, "Flip Horizontal", "win.flip-horizontal");
    icons.append(
        &transform,
        "Flip Vertical",
        "win.flip-vertical",
        "object-flip-vertical-symbolic",
    );
    menu.append_section(None, &transform);

    let crop = gio::Menu::new();
    icons.append(
        &crop,
        "Crop Mode",
        "win.toggle-crop",
        "edit-select-all-symbolic",
    );
    icons.append_action(&crop, "Reset Crop", "win.crop-reset");
    icons.append_action(&crop, "Crop 16:9", "win.crop-16-9");
    icons.append_action(&crop, "Crop 9:16", "win.crop-9-16");
    icons.append_action(&crop, "Crop 1:1", "win.crop-1-1");
    icons.append_action(&crop, "Crop 4:3", "win.crop-4-3");
    menu.append_section(None, &crop);

    let view = gio::Menu::new();
    gtk_neuron::append_driving_menu_item(&mut icons, &view);
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

fn install_actions(vw: &Rc<VideoWindow>) {
    let window = &vw.window;

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

    {
        let vw = Rc::clone(vw);
        bind(window, "open", move |_| open_dialog(&vw));
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "export", move |_| export_dialog(&vw));
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "play-pause", move |_| vw.toggle_play());
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "goto-in", move |_| vw.goto_in());
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "goto-out", move |_| vw.goto_out());
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "mark-in", move |_| vw.mark_in());
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "mark-out", move |_| vw.mark_out());
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "seek-back", move |_| vw.nudge(-1_000_000));
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "seek-forward", move |_| vw.nudge(1_000_000));
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "seek-back-fine", move |_| vw.nudge(-200_000));
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "seek-forward-fine", move |_| vw.nudge(200_000));
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "rotate-cw", move |_| vw.rotate_cw());
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "rotate-ccw", move |_| vw.rotate_ccw());
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "flip-horizontal", move |_| vw.flip_horizontal());
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "flip-vertical", move |_| vw.flip_vertical());
    }
    {
        let vw = Rc::clone(vw);
        let act = gio::SimpleAction::new_stateful("toggle-crop", None, &false.to_variant());
        act.connect_activate(move |action, _| {
            let on = !action
                .state()
                .and_then(|s| s.get::<bool>())
                .unwrap_or(false);
            action.set_state(&on.to_variant());
            vw.apply_crop_mode(on);
        });
        window.add_action(&act);
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "crop-reset", move |_| vw.reset_crop());
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "crop-16-9", move |_| {
            if let Err(e) = vw.apply_crop_preset("16:9") {
                vw.set_status(&e);
            }
        });
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "crop-9-16", move |_| {
            if let Err(e) = vw.apply_crop_preset("9:16") {
                vw.set_status(&e);
            }
        });
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "crop-1-1", move |_| {
            if let Err(e) = vw.apply_crop_preset("1:1") {
                vw.set_status(&e);
            }
        });
    }
    {
        let vw = Rc::clone(vw);
        bind(window, "crop-4-3", move |_| {
            if let Err(e) = vw.apply_crop_preset("4:3") {
                vw.set_status(&e);
            }
        });
    }
}

fn bind<F>(window: &gtk::ApplicationWindow, name: &str, f: F)
where
    F: Fn(Option<&glib::Variant>) + 'static,
{
    let action = gio::SimpleAction::new(name, None);
    action.connect_activate(move |_, param| f(param));
    window.add_action(&action);
}

fn video_filter() -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Videos"));
    filter.add_mime_type("video/*");
    for ext in [
        "mp4", "m4v", "mkv", "webm", "mov", "avi", "mpeg", "mpg", "wmv", "flv", "ogv", "3gp",
        "ts", "mts", "m2ts",
    ] {
        filter.add_pattern(&format!("*.{ext}"));
    }
    filter
}

fn export_filter() -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Video files"));
    for ext in ["mp4", "mkv", "webm", "mov"] {
        filter.add_pattern(&format!("*.{ext}"));
    }
    filter
}

fn file_to_path(file: Option<gio::File>) -> Option<PathBuf> {
    let file = file?;
    if let Some(path) = file.path() {
        return Some(path);
    }
    let uri = file.uri();
    if let Ok(url) = glib::Uri::parse(&uri, glib::UriFlags::PARSE_RELAXED) {
        if url.scheme().as_str() == "file" {
            let path = url.path();
            if !path.is_empty() {
                return Some(PathBuf::from(path.as_str()));
            }
        }
    }
    None
}

fn open_dialog(vw: &Rc<VideoWindow>) {
    let parent = vw.window.clone();
    let vw = Rc::clone(vw);
    let filter = video_filter();
    gtk_theme::present_file_chooser(
        Some(&parent),
        "Open Video",
        gtk::FileChooserAction::Open,
        "Open",
        Some(&filter),
        None,
        move |file| {
            if let Some(path) = file_to_path(file) {
                vw.open_path(&path);
            }
        },
    );
}

fn export_dialog(vw: &Rc<VideoWindow>) {
    let Some(input) = vw.current_path() else {
        vw.set_status("Open a video before exporting");
        return;
    };
    if vw.timeline.duration_us() <= 0 {
        vw.set_status("Wait for the video to finish loading");
        return;
    }
    if !export::ffmpeg_available() {
        show_message(
            &vw.window,
            "ffmpeg required",
            "Export uses ffmpeg to write the selected section.\n\nInstall it with:\n  sudo apt install ffmpeg",
        );
        return;
    }

    let start = vw.timeline.start_us();
    let end = vw.timeline.end_us();
    if end - start < export::MIN_SELECTION_US {
        vw.set_status("Selection is too short to export");
        return;
    }

    if let Some(media) = vw.media() {
        media.pause();
        vw.sync_play_icon();
    }

    let suggested = export::suggested_export_name(&input);
    let parent = vw.window.clone();
    let vw = Rc::clone(vw);
    let filter = export_filter();
    gtk_theme::present_file_chooser(
        Some(&parent),
        "Export Selection",
        gtk::FileChooserAction::Save,
        "Export",
        Some(&filter),
        Some(&suggested),
        move |file| {
            let Some(output) = file_to_path(file) else {
                return;
            };
            run_export_async(&vw, vw.make_export_job(input.clone(), output));
        },
    );
}

fn run_export_async(vw: &VideoWindow, job: ExportJob) {
    let output = job.output.clone();
    let window = vw.window.clone();
    let status = vw.status_label.clone();

    let progress_bar = gtk::ProgressBar::new();
    progress_bar.set_show_text(true);
    progress_bar.set_text(Some("Starting ffmpeg…"));
    progress_bar.set_hexpand(true);
    progress_bar.set_margin_start(16);
    progress_bar.set_margin_end(16);
    progress_bar.set_margin_top(16);
    progress_bar.set_margin_bottom(16);

    let dialog = gtk::Window::builder()
        .transient_for(&window)
        .modal(true)
        .title("Exporting…")
        .default_width(420)
        .child(&progress_bar)
        .build();
    gtk_theme::style_dialog(&dialog);
    dialog.present();

    let frac = Arc::new(Mutex::new(0.0_f64));
    let done: Arc<Mutex<Option<Result<(), String>>>> = Arc::new(Mutex::new(None));

    {
        let frac = Arc::clone(&frac);
        let done = Arc::clone(&done);
        std::thread::spawn(move || {
            let result = export::run_export(&job, |p| {
                if let Ok(mut g) = frac.lock() {
                    *g = p;
                }
            });
            if let Ok(mut g) = done.lock() {
                *g = Some(result);
            }
        });
    }

    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        if let Ok(p) = frac.lock() {
            progress_bar.set_fraction(*p);
            progress_bar.set_text(Some(&format!("Encoding… {:.0}%", *p * 100.0)));
        }
        let finished = done.lock().ok().and_then(|mut g| g.take());
        if let Some(result) = finished {
            dialog.close();
            match result {
                Ok(()) => {
                    status.set_text(&format!("Exported {}", output.display()));
                    show_message(
                        &window,
                        "Export complete",
                        &format!("Wrote:\n{}", output.display()),
                    );
                }
                Err(e) => {
                    status.set_text("Export failed");
                    show_message(&window, "Export failed", &e);
                }
            }
            ControlFlow::Break
        } else {
            ControlFlow::Continue
        }
    });
}

fn show_message(parent: &gtk::ApplicationWindow, title: &str, body: &str) {
    let label = gtk::Label::new(Some(body));
    label.set_wrap(true);
    label.set_xalign(0.0);
    label.set_margin_start(16);
    label.set_margin_end(16);
    label.set_margin_top(16);
    label.set_selectable(true);

    let close = gtk_theme::labeled_button(gtk_theme::icon_for_label("Close"), "Close");
    close.add_css_class("suggested-action");
    close.set_halign(gtk::Align::End);
    close.set_margin_start(16);
    close.set_margin_end(16);
    close.set_margin_top(8);
    close.set_margin_bottom(12);

    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 0);
    box_.append(&label);
    box_.append(&close);

    let win = gtk::Window::builder()
        .transient_for(parent)
        .modal(true)
        .title(title)
        .default_width(420)
        .child(&box_)
        .build();
    gtk_theme::style_dialog(&win);
    {
        let win = win.clone();
        close.connect_clicked(move |_| win.close());
    }
    win.present();
}

fn setup_drop_target(vw: &Rc<VideoWindow>) {
    let drop =
        gtk::DropTarget::new(glib::Type::INVALID, gdk::DragAction::COPY | gdk::DragAction::MOVE);
    drop.set_types(&[gdk::FileList::static_type(), gio::File::static_type()]);
    drop.set_preload(true);
    drop.set_propagation_phase(gtk::PropagationPhase::Capture);

    let vw_drop = Rc::clone(vw);
    drop.connect_drop(move |_t, value, _x, _y| {
        let paths = paths_from_drop_value(value);
        let Some(path) = paths.into_iter().find(|p| p.is_file()) else {
            return false;
        };
        vw_drop.open_path(&path);
        true
    });
    vw.window.add_controller(drop);
}

fn paths_from_drop_value(value: &glib::Value) -> Vec<PathBuf> {
    if let Ok(list) = value.get::<gdk::FileList>() {
        return list.files().into_iter().filter_map(|f| f.path()).collect();
    }
    if let Ok(file) = value.get::<gio::File>() {
        if let Some(p) = file.path() {
            return vec![p];
        }
    }
    Vec::new()
}

/// Open paths from the command line / HANDLES_OPEN into this window.
pub fn open_files(vw: &VideoWindow, files: &[gio::File]) {
    let paths: Vec<PathBuf> = files.iter().filter_map(|f| f.path()).collect();
    if let Some(path) = paths.into_iter().find(|p| p.is_file()) {
        vw.open_path(&path);
    }
}
