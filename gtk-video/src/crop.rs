//! Crop rectangle in source video pixels, plus an overlay drawing area.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::cairo;
use gtk::gdk;
use gtk::prelude::*;

use crate::export::even_crop;

const HANDLE: f64 = 8.0;
const HIT: f64 = 12.0;
const MIN_CROP: i32 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CropRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl CropRect {
    pub fn full(vw: i32, vh: i32) -> Self {
        Self {
            x: 0,
            y: 0,
            w: vw.max(2),
            h: vh.max(2),
        }
    }

    pub fn is_full(self, vw: i32, vh: i32) -> bool {
        self.x <= 0 && self.y <= 0 && self.w >= vw && self.h >= vh
    }

    pub fn clamp(self, vw: i32, vh: i32) -> Self {
        let (x, y, w, h) = even_crop(self.x, self.y, self.w.max(MIN_CROP), self.h.max(MIN_CROP), vw, vh);
        Self { x, y, w, h }
    }

    pub fn as_tuple(self) -> (i32, i32, i32, i32) {
        (self.x, self.y, self.w, self.h)
    }
}

/// Aspect presets. `reset` restores the full frame.
pub fn apply_preset(preset: &str, vw: i32, vh: i32) -> Result<CropRect, String> {
    let vw = vw.max(2);
    let vh = vh.max(2);
    let (rw, rh) = match preset {
        "reset" | "full" => return Ok(CropRect::full(vw, vh)),
        "16:9" => (16.0, 9.0),
        "9:16" => (9.0, 16.0),
        "1:1" | "square" => (1.0, 1.0),
        "4:3" => (4.0, 3.0),
        other => return Err(format!("unknown crop preset {other}")),
    };
    let src_a = vw as f64 / vh as f64;
    let want_a = rw / rh;
    let (w, h) = if src_a > want_a {
        let h = vh;
        let w = ((h as f64) * want_a).round() as i32;
        (w.min(vw), h)
    } else {
        let w = vw;
        let h = ((w as f64) / want_a).round() as i32;
        (w, h.min(vh))
    };
    let x = ((vw - w) / 2).max(0);
    let y = ((vh - h) / 2).max(0);
    Ok(CropRect { x, y, w, h }.clamp(vw, vh))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_fit_inside_frame() {
        let r = apply_preset("16:9", 1920, 1080).unwrap();
        assert!(r.is_full(1920, 1080));
        let r = apply_preset("1:1", 1920, 1080).unwrap();
        assert_eq!(r.w, r.h);
        assert!(r.x + r.w <= 1920);
        let r = apply_preset("9:16", 1920, 1080).unwrap();
        assert!(r.h <= 1080 && r.w < 1920);
        assert!(apply_preset("free", 1920, 1080).is_err());
    }
}

/// Letterboxed video rectangle inside a widget (`ContentFit::Contain`).
pub fn letterbox(ww: f64, wh: f64, vw: i32, vh: i32) -> (f64, f64, f64, f64) {
    if vw <= 0 || vh <= 0 || ww <= 1.0 || wh <= 1.0 {
        return (0.0, 0.0, ww.max(1.0), wh.max(1.0));
    }
    let src_a = vw as f64 / vh as f64;
    let box_a = ww / wh;
    if box_a > src_a {
        let h = wh;
        let w = h * src_a;
        ((ww - w) * 0.5, 0.0, w, h)
    } else {
        let w = ww;
        let h = w / src_a;
        (0.0, (wh - h) * 0.5, w, h)
    }
}

fn video_to_widget(x: i32, y: i32, lb: (f64, f64, f64, f64), vw: i32, vh: i32) -> (f64, f64) {
    let (lx, ly, lw, lh) = lb;
    (
        lx + (x as f64 / vw.max(1) as f64) * lw,
        ly + (y as f64 / vh.max(1) as f64) * lh,
    )
}

fn widget_to_video(px: f64, py: f64, lb: (f64, f64, f64, f64), vw: i32, vh: i32) -> (i32, i32) {
    let (lx, ly, lw, lh) = lb;
    let nx = ((px - lx) / lw.max(1.0)).clamp(0.0, 1.0);
    let ny = ((py - ly) / lh.max(1.0)).clamp(0.0, 1.0);
    ((nx * vw as f64).round() as i32, (ny * vh as f64).round() as i32)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Handle {
    Move,
    N,
    S,
    E,
    W,
    Ne,
    Nw,
    Se,
    Sw,
}

pub struct CropOverlay {
    pub area: gtk::DrawingArea,
    video_w: Cell<i32>,
    video_h: Cell<i32>,
    crop: Cell<Option<CropRect>>,
    enabled: Cell<bool>,
    drag: Cell<Option<(Handle, i32, i32, CropRect)>>,
    listeners: RefCell<Vec<Box<dyn Fn()>>>,
}

impl CropOverlay {
    pub fn new() -> Rc<Self> {
        let area = gtk::DrawingArea::new();
        area.set_hexpand(true);
        area.set_vexpand(true);
        area.set_can_target(false);
        area.add_css_class("crop-overlay");

        let overlay = Rc::new(Self {
            area: area.clone(),
            video_w: Cell::new(0),
            video_h: Cell::new(0),
            crop: Cell::new(None),
            enabled: Cell::new(false),
            drag: Cell::new(None),
            listeners: RefCell::new(Vec::new()),
        });

        {
            let o = Rc::clone(&overlay);
            area.set_draw_func(move |_, cr, w, h| o.draw(cr, w as f64, h as f64));
        }
        install_gestures(&overlay);
        overlay
    }

    pub fn connect_changed<F: Fn() + 'static>(&self, f: F) {
        self.listeners.borrow_mut().push(Box::new(f));
    }

    pub fn set_video_size(&self, w: i32, h: i32) {
        self.video_w.set(w);
        self.video_h.set(h);
        if let Some(c) = self.crop.get() {
            self.crop.set(Some(c.clamp(w, h)));
        }
        self.area.queue_draw();
    }

    pub fn reset(&self) {
        self.crop.set(None);
        self.drag.set(None);
        self.area.queue_draw();
        self.emit();
    }

    pub fn crop(&self) -> Option<CropRect> {
        let vw = self.video_w.get();
        let vh = self.video_h.get();
        let c = self.crop.get()?;
        if c.is_full(vw, vh) {
            None
        } else {
            Some(c.clamp(vw, vh))
        }
    }

    pub fn set_crop(&self, rect: Option<CropRect>) {
        let vw = self.video_w.get();
        let vh = self.video_h.get();
        self.crop.set(rect.map(|c| c.clamp(vw.max(2), vh.max(2))));
        self.area.queue_draw();
        self.emit();
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.set(on);
        self.area.set_can_target(on);
        self.area.set_cursor_from_name(if on { Some("crosshair") } else { None });
        self.area.queue_draw();
    }

    pub fn enabled(&self) -> bool {
        self.enabled.get()
    }

    fn emit(&self) {
        for cb in self.listeners.borrow().iter() {
            cb();
        }
    }

    fn current_or_full(&self) -> CropRect {
        let vw = self.video_w.get().max(2);
        let vh = self.video_h.get().max(2);
        self.crop.get().unwrap_or_else(|| CropRect::full(vw, vh))
    }

    fn lb(&self, ww: f64, wh: f64) -> (f64, f64, f64, f64) {
        letterbox(ww, wh, self.video_w.get(), self.video_h.get())
    }

    fn draw(&self, cr: &cairo::Context, ww: f64, wh: f64) {
        let vw = self.video_w.get();
        let vh = self.video_h.get();
        if vw <= 0 || vh <= 0 {
            return;
        }
        let Some(crop) = self.crop.get().filter(|c| !c.is_full(vw, vh)).or_else(|| {
            if self.enabled.get() {
                Some(self.current_or_full())
            } else {
                None
            }
        }) else {
            return;
        };

        let lb = self.lb(ww, wh);
        let (x0, y0) = video_to_widget(crop.x, crop.y, lb, vw, vh);
        let (x1, y1) = video_to_widget(crop.x + crop.w, crop.y + crop.h, lb, vw, vh);
        let (lx, ly, lw, lh) = lb;

        cr.set_source_rgba(0.0, 0.0, 0.0, 0.45);
        cr.rectangle(lx, ly, lw, (y0 - ly).max(0.0));
        cr.rectangle(lx, y1, lw, (ly + lh - y1).max(0.0));
        cr.rectangle(lx, y0, (x0 - lx).max(0.0), (y1 - y0).max(0.0));
        cr.rectangle(x1, y0, (lx + lw - x1).max(0.0), (y1 - y0).max(0.0));
        cr.fill().ok();

        cr.set_source_rgba(1.0, 1.0, 1.0, 0.95);
        cr.set_line_width(1.5);
        cr.rectangle(x0, y0, (x1 - x0).max(1.0), (y1 - y0).max(1.0));
        cr.stroke().ok();

        if self.enabled.get() {
            for (hx, hy) in handle_points(x0, y0, x1, y1) {
                cr.rectangle(hx - HANDLE * 0.5, hy - HANDLE * 0.5, HANDLE, HANDLE);
                cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
                cr.fill_preserve().ok();
                cr.set_source_rgba(0.1, 0.1, 0.1, 0.9);
                cr.set_line_width(1.0);
                cr.stroke().ok();
            }
        }
    }
}

fn handle_points(x0: f64, y0: f64, x1: f64, y1: f64) -> [(f64, f64); 8] {
    let mx = (x0 + x1) * 0.5;
    let my = (y0 + y1) * 0.5;
    [
        (x0, y0),
        (mx, y0),
        (x1, y0),
        (x1, my),
        (x1, y1),
        (mx, y1),
        (x0, y1),
        (x0, my),
    ]
}

fn hit_handle(px: f64, py: f64, x0: f64, y0: f64, x1: f64, y1: f64) -> Option<Handle> {
    let pts = [
        (Handle::Nw, x0, y0),
        (Handle::N, (x0 + x1) * 0.5, y0),
        (Handle::Ne, x1, y0),
        (Handle::E, x1, (y0 + y1) * 0.5),
        (Handle::Se, x1, y1),
        (Handle::S, (x0 + x1) * 0.5, y1),
        (Handle::Sw, x0, y1),
        (Handle::W, x0, (y0 + y1) * 0.5),
    ];
    for (h, hx, hy) in pts {
        if (px - hx).abs() <= HIT && (py - hy).abs() <= HIT {
            return Some(h);
        }
    }
    if px >= x0 && px <= x1 && py >= y0 && py <= y1 {
        Some(Handle::Move)
    } else {
        None
    }
}

fn install_gestures(overlay: &Rc<CropOverlay>) {
    let drag = gtk::GestureDrag::new();
    drag.set_button(gdk::BUTTON_PRIMARY);
    {
        let o = Rc::clone(overlay);
        drag.connect_drag_begin(move |_, x, y| {
            if !o.enabled.get() {
                return;
            }
            let vw = o.video_w.get();
            let vh = o.video_h.get();
            if vw <= 0 || vh <= 0 {
                return;
            }
            let ww = o.area.width() as f64;
            let wh = o.area.height() as f64;
            let lb = o.lb(ww, wh);
            let crop = o.current_or_full();
            let (x0, y0) = video_to_widget(crop.x, crop.y, lb, vw, vh);
            let (x1, y1) = video_to_widget(crop.x + crop.w, crop.y + crop.h, lb, vw, vh);
            let Some(handle) = hit_handle(x, y, x0, y0, x1, y1) else {
                return;
            };
            let (vx, vy) = widget_to_video(x, y, lb, vw, vh);
            o.drag.set(Some((handle, vx, vy, crop)));
        });
    }
    {
        let o = Rc::clone(overlay);
        drag.connect_drag_update(move |g, dx, dy| {
            let Some((handle, ox, oy, start)) = o.drag.get() else {
                return;
            };
            let vw = o.video_w.get();
            let vh = o.video_h.get();
            let Some((sx, sy)) = g.start_point() else {
                return;
            };
            let ww = o.area.width() as f64;
            let wh = o.area.height() as f64;
            let lb = o.lb(ww, wh);
            let (nx, ny) = widget_to_video(sx + dx, sy + dy, lb, vw, vh);
            let mut c = start;
            let ddx = nx - ox;
            let ddy = ny - oy;
            match handle {
                Handle::Move => {
                    c.x = (start.x + ddx).clamp(0, (vw - start.w).max(0));
                    c.y = (start.y + ddy).clamp(0, (vh - start.h).max(0));
                }
                Handle::N => {
                    let y1 = start.y + start.h;
                    c.y = (start.y + ddy).clamp(0, y1 - MIN_CROP);
                    c.h = y1 - c.y;
                }
                Handle::S => {
                    c.h = (start.h + ddy).clamp(MIN_CROP, vh - start.y);
                }
                Handle::W => {
                    let x1 = start.x + start.w;
                    c.x = (start.x + ddx).clamp(0, x1 - MIN_CROP);
                    c.w = x1 - c.x;
                }
                Handle::E => {
                    c.w = (start.w + ddx).clamp(MIN_CROP, vw - start.x);
                }
                Handle::Nw => {
                    let x1 = start.x + start.w;
                    let y1 = start.y + start.h;
                    c.x = (start.x + ddx).clamp(0, x1 - MIN_CROP);
                    c.y = (start.y + ddy).clamp(0, y1 - MIN_CROP);
                    c.w = x1 - c.x;
                    c.h = y1 - c.y;
                }
                Handle::Ne => {
                    let y1 = start.y + start.h;
                    c.y = (start.y + ddy).clamp(0, y1 - MIN_CROP);
                    c.h = y1 - c.y;
                    c.w = (start.w + ddx).clamp(MIN_CROP, vw - start.x);
                }
                Handle::Sw => {
                    let x1 = start.x + start.w;
                    c.x = (start.x + ddx).clamp(0, x1 - MIN_CROP);
                    c.w = x1 - c.x;
                    c.h = (start.h + ddy).clamp(MIN_CROP, vh - start.y);
                }
                Handle::Se => {
                    c.w = (start.w + ddx).clamp(MIN_CROP, vw - start.x);
                    c.h = (start.h + ddy).clamp(MIN_CROP, vh - start.y);
                }
            }
            o.crop.set(Some(c.clamp(vw, vh)));
            o.area.queue_draw();
            o.emit();
        });
    }
    {
        let o = Rc::clone(overlay);
        drag.connect_drag_end(move |_, _, _| {
            o.drag.set(None);
        });
    }
    overlay.area.add_controller(drag);
}
