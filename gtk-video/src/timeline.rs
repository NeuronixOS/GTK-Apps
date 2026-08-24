//! Draggable in/out handles and playhead on a video timeline.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::cairo;
use gtk::gdk;
use gtk::prelude::*;

use crate::export::{format_timestamp, MIN_SELECTION_US};

const PAD_X: f64 = 18.0;
const TRACK_H: f64 = 12.0;
const HANDLE_W: f64 = 14.0;
const HANDLE_H: f64 = 28.0;
const HIT: f64 = 16.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragKind {
    Start,
    End,
    Playhead,
}

#[derive(Clone, Copy)]
pub enum TimelineEvent {
    /// In-point moved; preview should seek to the new start.
    Start,
    /// Out-point moved; preview should seek to the new end.
    End,
    /// Playhead dragged or clicked; preview should seek there.
    Playhead,
}

pub struct Timeline {
    pub area: gtk::DrawingArea,
    duration_us: Cell<i64>,
    start_us: Cell<i64>,
    end_us: Cell<i64>,
    playhead_us: Cell<i64>,
    drag: Cell<Option<DragKind>>,
    hover: Cell<Option<DragKind>>,
    listeners: RefCell<Vec<Box<dyn Fn(TimelineEvent)>>>,
}

impl Timeline {
    pub fn new() -> Rc<Self> {
        let area = gtk::DrawingArea::new();
        area.set_content_height(84);
        area.set_hexpand(true);
        area.set_vexpand(false);
        area.add_css_class("timeline");
        area.set_cursor_from_name(Some("pointer"));

        let tl = Rc::new(Self {
            area: area.clone(),
            duration_us: Cell::new(0),
            start_us: Cell::new(0),
            end_us: Cell::new(0),
            playhead_us: Cell::new(0),
            drag: Cell::new(None),
            hover: Cell::new(None),
            listeners: RefCell::new(Vec::new()),
        });

        {
            let tl_draw = Rc::clone(&tl);
            area.set_draw_func(move |_, cr, w, h| {
                tl_draw.draw(cr, w, h);
            });
        }

        install_controllers(&tl);
        tl
    }

    pub fn connect_changed<F: Fn(TimelineEvent) + 'static>(&self, f: F) {
        self.listeners.borrow_mut().push(Box::new(f));
    }

    pub fn duration_us(&self) -> i64 {
        self.duration_us.get()
    }

    pub fn start_us(&self) -> i64 {
        self.start_us.get()
    }

    pub fn end_us(&self) -> i64 {
        self.end_us.get()
    }

    pub fn playhead_us(&self) -> i64 {
        self.playhead_us.get()
    }

    pub fn selection_us(&self) -> i64 {
        (self.end_us.get() - self.start_us.get()).max(0)
    }

    pub fn reset_full(&self, duration_us: i64) {
        let dur = duration_us.max(0);
        self.duration_us.set(dur);
        self.start_us.set(0);
        self.end_us.set(dur);
        self.playhead_us.set(0);
        self.area.queue_draw();
    }

    pub fn set_duration(&self, duration_us: i64) {
        let dur = duration_us.max(0);
        self.duration_us.set(dur);
        if self.end_us.get() <= 0 || self.end_us.get() > dur {
            self.end_us.set(dur);
        }
        if self.start_us.get() > dur.saturating_sub(MIN_SELECTION_US) {
            self.start_us.set(0);
        }
        self.clamp_all();
        self.area.queue_draw();
    }

    pub fn set_playhead(&self, us: i64) {
        let us = us.clamp(0, self.duration_us.get().max(0));
        if self.playhead_us.get() != us {
            self.playhead_us.set(us);
            self.area.queue_draw();
        }
    }

    /// Set in/out points (clamped to duration and minimum selection length).
    pub fn set_range(&self, start_us: i64, end_us: i64) {
        self.start_us.set(start_us);
        self.end_us.set(end_us);
        self.clamp_all();
        self.area.queue_draw();
    }

    fn emit(&self, ev: TimelineEvent) {
        for cb in self.listeners.borrow().iter() {
            cb(ev);
        }
    }

    fn clamp_all(&self) {
        let dur = self.duration_us.get().max(0);
        let mut start = self.start_us.get().clamp(0, dur);
        let mut end = self.end_us.get().clamp(0, dur);
        if end - start < MIN_SELECTION_US {
            if end < MIN_SELECTION_US {
                end = MIN_SELECTION_US.min(dur);
                start = 0;
            } else {
                start = (end - MIN_SELECTION_US).max(0);
            }
        }
        self.start_us.set(start);
        self.end_us.set(end);
        self.playhead_us
            .set(self.playhead_us.get().clamp(0, dur));
    }

    fn track_geom(&self, width: i32) -> (f64, f64) {
        let w = width as f64;
        let x0 = PAD_X;
        let x1 = (w - PAD_X).max(x0 + 1.0);
        (x0, x1)
    }

    fn x_from_us(&self, us: i64, width: i32) -> f64 {
        let (x0, x1) = self.track_geom(width);
        let dur = self.duration_us.get().max(1) as f64;
        x0 + (us.max(0) as f64 / dur) * (x1 - x0)
    }

    fn us_from_x(&self, x: f64, width: i32) -> i64 {
        let (x0, x1) = self.track_geom(width);
        let dur = self.duration_us.get().max(0);
        if x1 <= x0 {
            return 0;
        }
        let t = ((x - x0) / (x1 - x0)).clamp(0.0, 1.0);
        (t * dur as f64).round() as i64
    }

    fn hit(&self, x: f64, y: f64, width: i32, height: i32) -> Option<DragKind> {
        let cy = height as f64 * 0.42;
        if (y - cy).abs() > HANDLE_H {
            return None;
        }
        let sx = self.x_from_us(self.start_us.get(), width);
        let ex = self.x_from_us(self.end_us.get(), width);
        let px = self.x_from_us(self.playhead_us.get(), width);
        let ds = (x - sx).abs();
        let de = (x - ex).abs();
        let dp = (x - px).abs();
        // Prefer the closer handle when they overlap.
        if ds <= HIT && ds <= de && ds <= dp + 2.0 {
            return Some(DragKind::Start);
        }
        if de <= HIT && de <= ds && de <= dp + 2.0 {
            return Some(DragKind::End);
        }
        if dp <= HIT {
            return Some(DragKind::Playhead);
        }
        let (x0, x1) = self.track_geom(width);
        if x >= x0 && x <= x1 {
            Some(DragKind::Playhead)
        } else {
            None
        }
    }

    fn apply_drag(&self, kind: DragKind, x: f64, width: i32) {
        let mut us = self.us_from_x(x, width);
        let dur = self.duration_us.get().max(0);
        match kind {
            DragKind::Start => {
                let end = self.end_us.get();
                us = us.clamp(0, (end - MIN_SELECTION_US).max(0));
                self.start_us.set(us);
                self.playhead_us.set(us);
            }
            DragKind::End => {
                let start = self.start_us.get();
                us = us.clamp((start + MIN_SELECTION_US).min(dur), dur);
                self.end_us.set(us);
                self.playhead_us.set(us);
            }
            DragKind::Playhead => {
                self.playhead_us.set(us.clamp(0, dur));
            }
        }
        self.area.queue_draw();
        self.emit(match kind {
            DragKind::Start => TimelineEvent::Start,
            DragKind::End => TimelineEvent::End,
            DragKind::Playhead => TimelineEvent::Playhead,
        });
    }

    fn draw(&self, cr: &cairo::Context, width: i32, height: i32) {
        if width <= 0 || height <= 0 {
            return;
        }
        let profile = gtk_theme::load_profile();
        let accent = parse_rgba(profile.accent(), 0.35, 0.55, 0.90);
        let fg = parse_rgba(profile.foreground, 0.92, 0.86, 0.70);
        let bg = parse_rgba(profile.background, 0.16, 0.16, 0.16);

        let w = width as f64;
        let h = height as f64;
        cr.set_source_rgba(bg.0, bg.1, bg.2, 1.0);
        let _ = cr.paint();

        let (x0, x1) = self.track_geom(width);
        let cy = h * 0.42;
        let track_top = cy - TRACK_H / 2.0;

        // Track
        rounded_rect(cr, x0, track_top, x1 - x0, TRACK_H, 4.0);
        cr.set_source_rgba(fg.0, fg.1, fg.2, 0.18);
        let _ = cr.fill();

        let dur = self.duration_us.get();
        if dur <= 0 {
            cr.set_source_rgba(fg.0, fg.1, fg.2, 0.55);
            cr.select_font_face(
                "sans-serif",
                cairo::FontSlant::Normal,
                cairo::FontWeight::Normal,
            );
            cr.set_font_size(12.0);
            cr.move_to(x0, cy + 28.0);
            let _ = cr.show_text("Open a video to set in and out points");
            return;
        }

        let sx = self.x_from_us(self.start_us.get(), width);
        let ex = self.x_from_us(self.end_us.get(), width);
        let px = self.x_from_us(self.playhead_us.get(), width);

        // Selected range
        rounded_rect(cr, sx, track_top, (ex - sx).max(2.0), TRACK_H, 4.0);
        cr.set_source_rgba(accent.0, accent.1, accent.2, 0.85);
        let _ = cr.fill();

        // Tick marks
        cr.set_source_rgba(fg.0, fg.1, fg.2, 0.28);
        cr.set_line_width(1.0);
        let ticks = 8;
        for i in 0..=ticks {
            let t = i as f64 / ticks as f64;
            let x = x0 + t * (x1 - x0);
            cr.move_to(x, track_top + TRACK_H + 4.0);
            cr.line_to(x, track_top + TRACK_H + 10.0);
            let _ = cr.stroke();
        }

        draw_handle(cr, sx, cy, &accent, &fg);
        draw_handle(cr, ex, cy, &accent, &fg);

        // Playhead
        cr.set_source_rgba(fg.0, fg.1, fg.2, 0.95);
        cr.set_line_width(2.0);
        cr.move_to(px, cy - HANDLE_H / 2.0 - 4.0);
        cr.line_to(px, cy + HANDLE_H / 2.0 + 4.0);
        let _ = cr.stroke();
        cr.new_path();
        cr.move_to(px, cy - HANDLE_H / 2.0 - 4.0);
        cr.line_to(px - 5.0, cy - HANDLE_H / 2.0 - 12.0);
        cr.line_to(px + 5.0, cy - HANDLE_H / 2.0 - 12.0);
        cr.close_path();
        let _ = cr.fill();

        cr.set_source_rgba(fg.0, fg.1, fg.2, 0.78);
        cr.select_font_face(
            "sans-serif",
            cairo::FontSlant::Normal,
            cairo::FontWeight::Normal,
        );
        cr.set_font_size(11.5);

        let in_label = format!("In  {}", format_timestamp(self.start_us.get()));
        let out_label = format!("Out  {}", format_timestamp(self.end_us.get()));
        cr.move_to(x0, h - 10.0);
        let _ = cr.show_text(&in_label);

        if let Ok(ext) = cr.text_extents(&out_label) {
            cr.move_to((w - PAD_X - ext.width()).max(x0), h - 10.0);
            let _ = cr.show_text(&out_label);
        }
    }
}

fn install_controllers(tl: &Rc<Timeline>) {
    let drag = gtk::GestureDrag::new();
    drag.set_button(1);
    {
        let tl = Rc::clone(tl);
        drag.connect_drag_begin(move |g, x, y| {
            let kind = tl.hit(x, y, tl.area.width(), tl.area.height());
            tl.drag.set(kind);
            if kind.is_some() {
                g.set_state(gtk::EventSequenceState::Claimed);
            }
        });
    }
    {
        let tl = Rc::clone(tl);
        drag.connect_drag_update(move |g, dx, dy| {
            let Some(kind) = tl.drag.get() else {
                return;
            };
            let Some((sx, _sy)) = g.start_point() else {
                let _ = dy;
                return;
            };
            tl.apply_drag(kind, sx + dx, tl.area.width());
        });
    }
    {
        let tl = Rc::clone(tl);
        drag.connect_drag_end(move |g, dx, _dy| {
            if let Some(kind) = tl.drag.take() {
                if let Some((sx, _)) = g.start_point() {
                    tl.apply_drag(kind, sx + dx, tl.area.width());
                }
            }
        });
    }
    tl.area.add_controller(drag);

    // Click (no drag) still seeks / grabs a handle.
    let click = gtk::GestureClick::new();
    click.set_button(1);
    {
        let tl = Rc::clone(tl);
        click.connect_pressed(move |g, _n, x, y| {
            if let Some(kind) = tl.hit(x, y, tl.area.width(), tl.area.height()) {
                tl.drag.set(Some(kind));
                tl.apply_drag(kind, x, tl.area.width());
                g.set_state(gtk::EventSequenceState::Claimed);
            }
        });
    }
    tl.area.add_controller(click);

    let motion = gtk::EventControllerMotion::new();
    {
        let tl = Rc::clone(tl);
        motion.connect_motion(move |_, x, y| {
            let hit = tl.hit(x, y, tl.area.width(), tl.area.height());
            if tl.hover.get() != hit {
                tl.hover.set(hit);
                let name = match hit {
                    Some(DragKind::Start) | Some(DragKind::End) => "col-resize",
                    Some(DragKind::Playhead) => "ew-resize",
                    None => "default",
                };
                tl.area.set_cursor_from_name(Some(name));
            }
        });
    }
    {
        let tl = Rc::clone(tl);
        motion.connect_leave(move |_| {
            tl.hover.set(None);
            tl.area.set_cursor_from_name(Some("default"));
        });
    }
    tl.area.add_controller(motion);
}

fn parse_rgba(hex: &str, r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    gdk::RGBA::parse(hex)
        .map(|c| (c.red() as f64, c.green() as f64, c.blue() as f64))
        .unwrap_or((r, g, b))
}

fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
    cr.close_path();
}

fn draw_handle(
    cr: &cairo::Context,
    x: f64,
    cy: f64,
    accent: &(f64, f64, f64),
    fg: &(f64, f64, f64),
) {
    let w = HANDLE_W;
    let h = HANDLE_H;
    rounded_rect(cr, x - w / 2.0, cy - h / 2.0, w, h, 3.0);
    cr.set_source_rgba(accent.0, accent.1, accent.2, 1.0);
    let _ = cr.fill_preserve();
    cr.set_source_rgba(fg.0, fg.1, fg.2, 0.9);
    cr.set_line_width(1.2);
    let _ = cr.stroke();

    // Grip lines
    cr.set_source_rgba(fg.0, fg.1, fg.2, 0.85);
    cr.set_line_width(1.0);
    for dx in [-2.5, 0.0, 2.5] {
        cr.move_to(x + dx, cy - 7.0);
        cr.line_to(x + dx, cy + 7.0);
        let _ = cr.stroke();
    }
}
