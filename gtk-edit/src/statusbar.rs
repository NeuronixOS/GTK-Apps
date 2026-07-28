//! Status bar with cursor position, language, and tab-width controls.

use gtk4 as gtk;
use gtk::prelude::*;

pub struct Statusbar {
    pub root: gtk::Box,
    pub message: gtk::Label,
    pub position: gtk::Label,
    pub language_btn: gtk::MenuButton,
    pub tab_width_btn: gtk::MenuButton,
    pub encoding_label: gtk::Label,
}

impl Statusbar {
    pub fn new() -> Self {
        let message = gtk::Label::new(None);
        message.set_halign(gtk::Align::Start);
        message.set_hexpand(true);
        message.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let position = gtk::Label::new(Some("Ln 1, Col 1"));
        position.set_margin_start(8);
        position.set_margin_end(8);

        let language_btn = gtk::MenuButton::builder()
            .label("Plain Text")
            .direction(gtk::ArrowType::Up)
            .build();

        let tab_width_btn = gtk::MenuButton::builder()
            .label("Tab Width: 8")
            .direction(gtk::ArrowType::Up)
            .build();

        let encoding_label = gtk::Label::new(Some("UTF-8"));
        encoding_label.set_margin_start(8);
        encoding_label.set_margin_end(8);

        let root = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        root.set_margin_start(6);
        root.set_margin_end(6);
        root.set_margin_top(2);
        root.set_margin_bottom(2);
        root.append(&message);
        root.append(&position);
        root.append(&encoding_label);
        root.append(&language_btn);
        root.append(&tab_width_btn);

        Self {
            root,
            message,
            position,
            language_btn,
            tab_width_btn,
            encoding_label,
        }
    }

    pub fn set_position(&self, line: i32, col: i32) {
        self.position
            .set_text(&format!("Ln {}, Col {}", line + 1, col + 1));
    }

    pub fn set_language_label(&self, name: &str) {
        self.language_btn.set_label(name);
    }

    pub fn set_tab_width(&self, width: u32) {
        self.tab_width_btn
            .set_label(&format!("Tab Width: {width}"));
    }

    pub fn flash(&self, msg: &str) {
        self.message.set_text(msg);
    }
}
