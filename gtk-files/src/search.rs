//! In-folder search bar.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::prelude::*;

pub struct SearchBar {
    pub root: gtk::Box,
    entry: gtk::SearchEntry,
    on_changed: RefCell<Option<Rc<dyn Fn(String)>>>,
}

impl SearchBar {
    pub fn new() -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        root.set_margin_start(6);
        root.set_margin_end(6);
        root.set_margin_top(4);
        root.set_margin_bottom(4);
        root.add_css_class("search-bar");
        root.set_visible(false);

        let entry = gtk::SearchEntry::builder()
            .hexpand(true)
            .placeholder_text("Search current folder…")
            .build();

        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.set_tooltip_text(Some("Close search"));
        close.add_css_class("flat");

        root.append(&entry);
        root.append(&close);

        let bar = Rc::new(Self {
            root: root.clone(),
            entry: entry.clone(),
            on_changed: RefCell::new(None),
        });

        {
            let bar2 = Rc::clone(&bar);
            entry.connect_search_changed(move |e| {
                let q = e.text().to_string();
                if let Some(cb) = bar2.on_changed.borrow().as_ref() {
                    cb(q);
                }
            });
        }

        {
            let bar2 = Rc::clone(&bar);
            close.connect_clicked(move |_| {
                bar2.hide();
            });
        }

        {
            let bar2 = Rc::clone(&bar);
            entry.connect_stop_search(move |_| {
                bar2.hide();
            });
        }

        bar
    }

    pub fn set_on_changed<F: Fn(String) + 'static>(&self, f: F) {
        *self.on_changed.borrow_mut() = Some(Rc::new(f));
    }

    pub fn show(&self) {
        self.root.set_visible(true);
        self.entry.grab_focus();
    }

    pub fn hide(&self) {
        self.root.set_visible(false);
        self.entry.set_text("");
        if let Some(cb) = self.on_changed.borrow().as_ref() {
            cb(String::new());
        }
    }

    pub fn toggle(&self) {
        if self.root.is_visible() {
            self.hide();
        } else {
            self.show();
        }
    }

    #[allow(dead_code)]
    pub fn is_visible(&self) -> bool {
        self.root.is_visible()
    }
}
