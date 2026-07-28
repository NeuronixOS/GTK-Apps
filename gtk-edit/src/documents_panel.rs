//! Documents list in the side panel.

use std::rc::Rc;

use gtk4 as gtk;
use gtk::prelude::*;

pub struct DocumentsPanel {
    pub root: gtk::ScrolledWindow,
    pub list: gtk::ListBox,
}

impl DocumentsPanel {
    pub fn new() -> Rc<Self> {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.add_css_class("navigation-sidebar");

        let root = gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .hexpand(true)
            .build();

        Rc::new(Self { root, list })
    }

    pub fn clear(&self) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
    }

    pub fn add_document(&self, title: &str, path: Option<&str>) -> gtk::ListBoxRow {
        let row = gtk::ListBoxRow::new();
        let box_ = gtk::Box::new(gtk::Orientation::Vertical, 2);
        box_.set_margin_top(4);
        box_.set_margin_bottom(4);
        box_.set_margin_start(8);
        box_.set_margin_end(8);
        let name = gtk::Label::new(Some(title));
        name.set_halign(gtk::Align::Start);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        box_.append(&name);
        if let Some(p) = path {
            let sub = gtk::Label::new(Some(p));
            sub.set_halign(gtk::Align::Start);
            sub.add_css_class("dim-label");
            sub.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            box_.append(&sub);
        }
        row.set_child(Some(&box_));
        self.list.append(&row);
        row
    }
}
