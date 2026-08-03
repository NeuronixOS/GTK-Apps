//! Non-blocking copy/move progress docked at the bottom of the Places sidebar.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gtk4 as gtk;
use gtk::prelude::*;

thread_local! {
    static ACTIVE: RefCell<Option<Rc<TransferPanel>>> = const { RefCell::new(None) };
}

pub fn set_active(panel: &Rc<TransferPanel>) {
    ACTIVE.with(|a| *a.borrow_mut() = Some(Rc::clone(panel)));
}

pub fn active() -> Option<Rc<TransferPanel>> {
    ACTIVE.with(|a| a.borrow().clone())
}

pub struct TransferPanel {
    pub root: gtk::Box,
    title: gtk::Label,
    detail: gtk::Label,
    bar: gtk::ProgressBar,
    cancel_btn: gtk::Button,
    cancelled: RefCell<Option<Arc<AtomicBool>>>,
    busy: Cell<bool>,
}

impl TransferPanel {
    pub fn new() -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.add_css_class("transfer-panel");
        root.set_hexpand(true);
        root.set_vexpand(false);
        root.set_margin_start(8);
        root.set_margin_end(8);
        root.set_margin_top(6);
        root.set_margin_bottom(8);
        root.set_visible(false);

        let title = gtk::Label::new(None);
        title.set_xalign(0.0);
        title.add_css_class("heading");
        title.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        title.set_wrap(true);
        title.set_max_width_chars(28);

        let detail = gtk::Label::new(None);
        detail.set_xalign(0.0);
        detail.add_css_class("dim-label");
        detail.add_css_class("caption");
        detail.set_wrap(true);
        detail.set_max_width_chars(28);

        let bar = gtk::ProgressBar::new();
        bar.set_show_text(true);
        bar.set_fraction(0.0);
        bar.set_text(Some("0%"));
        bar.set_hexpand(true);

        let cancel_btn = gtk::Button::with_label("Cancel");
        cancel_btn.set_halign(gtk::Align::End);
        cancel_btn.add_css_class("flat");

        root.append(&title);
        root.append(&detail);
        root.append(&bar);
        root.append(&cancel_btn);

        let panel = Rc::new(Self {
            root,
            title,
            detail,
            bar,
            cancel_btn: cancel_btn.clone(),
            cancelled: RefCell::new(None),
            busy: Cell::new(false),
        });

        {
            let panel2 = Rc::clone(&panel);
            cancel_btn.connect_clicked(move |_| {
                if let Some(flag) = panel2.cancelled.borrow().as_ref() {
                    flag.store(true, Ordering::SeqCst);
                }
                panel2.title.set_text("Cancelling…");
                panel2.cancel_btn.set_sensitive(false);
            });
        }

        panel
    }

    #[allow(dead_code)]
    pub fn is_busy(&self) -> bool {
        self.busy.get()
    }

    /// Show the dock and prepare for a new job. Returns `None` if another transfer
    /// is already running.
    pub fn begin(&self, move_files: bool) -> Option<Arc<AtomicBool>> {
        if self.busy.get() {
            return None;
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        *self.cancelled.borrow_mut() = Some(Arc::clone(&cancelled));
        self.busy.set(true);
        self.cancel_btn.set_sensitive(true);
        self.title.set_text(if move_files {
            "Moving…"
        } else {
            "Copying…"
        });
        self.detail.set_text("Preparing…");
        self.bar.set_fraction(0.0);
        self.bar.set_text(Some("0%"));
        self.root.set_visible(true);
        Some(cancelled)
    }

    pub fn update(&self, title: &str, detail: &str, fraction: f64) {
        self.title.set_text(title);
        self.detail.set_text(detail);
        let fraction = fraction.clamp(0.0, 1.0);
        self.bar.set_fraction(fraction);
        self.bar.set_text(Some(&format!("{:.0}%", fraction * 100.0)));
    }

    pub fn finish(&self) {
        self.busy.set(false);
        *self.cancelled.borrow_mut() = None;
        self.cancel_btn.set_sensitive(true);
        self.root.set_visible(false);
        self.title.set_text("");
        self.detail.set_text("");
        self.bar.set_fraction(0.0);
    }
}
