//! Zoomable / pannable image canvas — rusty counterpart to eog's EogScrollView.
//!
//! Displays a Pixbuf via `gdk::Texture` + `gtk::Picture` inside a scrolled
//! window. Best-fit uses `ContentFit::Contain`; free zoom sets an explicit
//! size request so the user can pan with scrollbars or drag.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gdk_pixbuf::{Pixbuf, PixbufRotation};
use gtk::glib;
use gtk::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZoomMode {
    BestFit,
    Free,
}

#[derive(Clone)]
pub struct ImageView {
    pub root: gtk::ScrolledWindow,
    picture: gtk::Picture,
    pixbuf: Rc<RefCell<Option<Pixbuf>>>,
    mode: Rc<Cell<ZoomMode>>,
    zoom: Rc<Cell<f64>>,
    zoom_min: f64,
    zoom_max: f64,
    zoom_step: f64,
    status: gtk::Label,
    panning: Rc<Cell<bool>>,
}

impl ImageView {
    pub fn new(zoom_min: f64, zoom_max: f64, zoom_step: f64, best_fit: bool) -> Self {
        let picture = gtk::Picture::new();
        picture.set_can_shrink(true);
        picture.set_content_fit(gtk::ContentFit::Contain);
        picture.set_halign(gtk::Align::Center);
        picture.set_valign(gtk::Align::Center);
        picture.add_css_class("image-view");
        picture.set_hexpand(true);
        picture.set_vexpand(true);

        let root = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .kinetic_scrolling(false)
            .child(&picture)
            .build();
        root.add_css_class("image-scroller");

        let status = gtk::Label::new(None);
        status.add_css_class("dim-label");
        status.set_xalign(0.0);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let view = Self {
            root,
            picture,
            pixbuf: Rc::new(RefCell::new(None)),
            mode: Rc::new(Cell::new(if best_fit {
                ZoomMode::BestFit
            } else {
                ZoomMode::Free
            })),
            zoom: Rc::new(Cell::new(1.0)),
            zoom_min,
            zoom_max,
            zoom_step,
            status,
            panning: Rc::new(Cell::new(false)),
        };

        view.connect_best_fit_tracking();
        view.connect_scroll_zoom();
        view.connect_drag_pan();
        view
    }

    pub fn status_label(&self) -> &gtk::Label {
        &self.status
    }

    pub fn zoom_percent(&self) -> i32 {
        (self.zoom.get() * 100.0).round() as i32
    }

    pub fn has_image(&self) -> bool {
        self.pixbuf.borrow().is_some()
    }

    pub fn clear(&self) {
        *self.pixbuf.borrow_mut() = None;
        self.picture.set_paintable(None::<&gdk::Paintable>);
        self.picture.set_size_request(-1, -1);
        self.zoom.set(1.0);
        self.status.set_text("");
    }

    pub fn set_pixbuf(&self, pixbuf: Pixbuf) {
        *self.pixbuf.borrow_mut() = Some(pixbuf);
        if self.mode.get() == ZoomMode::BestFit {
            self.apply_best_fit();
        } else {
            self.apply_free_zoom(self.zoom.get());
        }
    }

    pub fn zoom_in(&self) {
        self.mode.set(ZoomMode::Free);
        let z = (self.zoom.get() * self.zoom_step).clamp(self.zoom_min, self.zoom_max);
        self.apply_free_zoom(z);
    }

    pub fn zoom_out(&self) {
        self.mode.set(ZoomMode::Free);
        let z = (self.zoom.get() / self.zoom_step).clamp(self.zoom_min, self.zoom_max);
        self.apply_free_zoom(z);
    }

    pub fn zoom_normal(&self) {
        self.mode.set(ZoomMode::Free);
        self.apply_free_zoom(1.0);
    }

    pub fn zoom_best_fit(&self) {
        self.mode.set(ZoomMode::BestFit);
        self.apply_best_fit();
    }

    pub fn rotate_cw(&self) {
        self.transform(|pb| pb.rotate_simple(PixbufRotation::Clockwise));
    }

    pub fn rotate_ccw(&self) {
        self.transform(|pb| pb.rotate_simple(PixbufRotation::Counterclockwise));
    }

    pub fn flip_horizontal(&self) {
        self.transform(|pb| pb.flip(true));
    }

    pub fn flip_vertical(&self) {
        self.transform(|pb| pb.flip(false));
    }

    pub fn current_pixbuf(&self) -> Option<Pixbuf> {
        self.pixbuf.borrow().clone()
    }

    fn transform<F>(&self, f: F)
    where
        F: FnOnce(&Pixbuf) -> Option<Pixbuf>,
    {
        let next = {
            let borrow = self.pixbuf.borrow();
            let Some(pb) = borrow.as_ref() else {
                return;
            };
            f(pb)
        };
        if let Some(next) = next {
            *self.pixbuf.borrow_mut() = Some(crate::image_list::normalize_pixbuf(&next));
            if self.mode.get() == ZoomMode::BestFit {
                self.apply_best_fit();
            } else {
                self.apply_free_zoom(self.zoom.get());
            }
        }
    }

    fn apply_best_fit(&self) {
        let borrow = self.pixbuf.borrow();
        let Some(pb) = borrow.as_ref() else {
            return;
        };

        self.picture.set_size_request(-1, -1);
        self.picture.set_can_shrink(true);
        self.picture.set_hexpand(true);
        self.picture.set_vexpand(true);
        self.picture.set_halign(gtk::Align::Center);
        self.picture.set_valign(gtk::Align::Center);
        self.picture.set_content_fit(gtk::ContentFit::Contain);
        self.picture.set_paintable(Some(&gdk::Texture::for_pixbuf(pb)));

        let scale = self.compute_fit_scale(pb);
        self.zoom.set(scale);
        self.update_status(pb);
    }

    fn apply_free_zoom(&self, zoom: f64) {
        let borrow = self.pixbuf.borrow();
        let Some(pb) = borrow.as_ref() else {
            return;
        };

        let zoom = zoom.clamp(self.zoom_min, self.zoom_max);
        // Keep the widget on-screen-sized. Scaling the full pixbuf to zoom
        // (old path) allocated width*height*zoom² bytes and froze the UI.
        let max_edge = 8192.0;
        let max_z = (max_edge / (pb.width().max(1) as f64))
            .min(max_edge / (pb.height().max(1) as f64))
            .max(self.zoom_min);
        let zoom = zoom.min(max_z);
        self.zoom.set(zoom);
        self.mode.set(ZoomMode::Free);

        let w = ((pb.width() as f64) * zoom).round().max(1.0) as i32;
        let h = ((pb.height() as f64) * zoom).round().max(1.0) as i32;

        // Start alignment: Center inside a ScrolledWindow reflows as you pan
        // and makes middle-drag snag/flicker.
        self.picture.set_can_shrink(false);
        self.picture.set_hexpand(false);
        self.picture.set_vexpand(false);
        self.picture.set_halign(gtk::Align::Start);
        self.picture.set_valign(gtk::Align::Start);
        self.picture.set_content_fit(gtk::ContentFit::Fill);
        self.picture.set_size_request(w, h);
        self.picture.set_paintable(Some(&gdk::Texture::for_pixbuf(pb)));
        self.update_status(pb);
    }

    fn compute_fit_scale(&self, pb: &Pixbuf) -> f64 {
        let alloc = self.root.allocation();
        let vw = (alloc.width() as f64).max(1.0);
        let vh = (alloc.height() as f64).max(1.0);
        let iw = pb.width() as f64;
        let ih = pb.height() as f64;
        if iw <= 0.0 || ih <= 0.0 {
            return 1.0;
        }
        (vw / iw).min(vh / ih).min(1.0)
    }

    fn update_status(&self, pb: &Pixbuf) {
        let mode = match self.mode.get() {
            ZoomMode::BestFit => "Best Fit",
            ZoomMode::Free => "Zoom",
        };
        self.status.set_text(&format!(
            "{}×{} · {}% · {mode}",
            pb.width(),
            pb.height(),
            self.zoom_percent()
        ));
    }

    fn connect_best_fit_tracking(&self) {
        // Periodically refresh the best-fit zoom percentage after layout settles.
        let mode = Rc::clone(&self.mode);
        let pixbuf = Rc::clone(&self.pixbuf);
        let zoom = Rc::clone(&self.zoom);
        let status = self.status.clone();
        let root = self.root.clone();

        self.root.connect_map(move |_| {
            let mode = Rc::clone(&mode);
            let pixbuf = Rc::clone(&pixbuf);
            let zoom = Rc::clone(&zoom);
            let status = status.clone();
            let root = root.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
                if mode.get() != ZoomMode::BestFit {
                    return glib::ControlFlow::Continue;
                }
                let borrow = pixbuf.borrow();
                let Some(pb) = borrow.as_ref() else {
                    return glib::ControlFlow::Continue;
                };
                let alloc = root.allocation();
                let vw = (alloc.width() as f64).max(1.0);
                let vh = (alloc.height() as f64).max(1.0);
                let iw = pb.width() as f64;
                let ih = pb.height() as f64;
                if iw <= 0.0 || ih <= 0.0 || vw <= 1.0 || vh <= 1.0 {
                    return glib::ControlFlow::Continue;
                }
                let scale = (vw / iw).min(vh / ih).min(1.0);
                if (scale - zoom.get()).abs() >= 0.005 {
                    zoom.set(scale);
                    status.set_text(&format!(
                        "{}×{} · {}% · Best Fit",
                        pb.width(),
                        pb.height(),
                        (scale * 100.0).round() as i32
                    ));
                }
                glib::ControlFlow::Continue
            });
        });
    }

    fn connect_scroll_zoom(&self) {
        let controller = gtk::EventControllerScroll::new(
            gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::DISCRETE,
        );
        let view = self.clone();
        let panning = Rc::clone(&self.panning);
        controller.connect_scroll(move |_controller, _dx, dy| {
            if panning.get() || !view.has_image() {
                return glib::Propagation::Proceed;
            }
            if dy < 0.0 {
                view.zoom_in();
            } else if dy > 0.0 {
                view.zoom_out();
            }
            glib::Propagation::Stop
        });
        self.root.add_controller(controller);
    }

    fn connect_drag_pan(&self) {
        // Pan in the scroller's stable coordinates. GestureDrag on the Picture
        // used widget space that moved as we scrolled, so the image snagged.
        let click = gtk::GestureClick::new();
        click.set_button(2);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);

        let panning = Rc::clone(&self.panning);
        let last = Rc::new(Cell::new((0.0, 0.0)));
        let root = self.root.clone();

        {
            let panning = Rc::clone(&panning);
            let last = Rc::clone(&last);
            let root = root.clone();
            click.connect_pressed(move |gesture, _n, x, y| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                panning.set(true);
                last.set((x, y));
                root.set_cursor_from_name(Some("grabbing"));
            });
        }
        {
            let panning = Rc::clone(&panning);
            let root = root.clone();
            click.connect_released(move |_, _n, _x, _y| {
                panning.set(false);
                root.set_cursor_from_name(None);
            });
        }
        {
            let panning = Rc::clone(&panning);
            let root = root.clone();
            click.connect_cancel(move |_, _| {
                panning.set(false);
                root.set_cursor_from_name(None);
            });
        }
        self.root.add_controller(click);

        let motion = gtk::EventControllerMotion::new();
        motion.set_propagation_phase(gtk::PropagationPhase::Capture);
        let hadj = self.root.hadjustment();
        let vadj = self.root.vadjustment();
        motion.connect_motion(move |_, x, y| {
            if !panning.get() {
                return;
            }
            let (lx, ly) = last.get();
            last.set((x, y));
            hadj.set_value(hadj.value() - (x - lx));
            vadj.set_value(vadj.value() - (y - ly));
        });
        self.root.add_controller(motion);
    }
}
