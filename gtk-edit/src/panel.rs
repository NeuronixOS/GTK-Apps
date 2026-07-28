//! Side and bottom panel hosts (notebook of plugin pages).

use gtk4 as gtk;
use gtk::prelude::*;

pub struct Panel {
    pub root: gtk::Box,
    pub notebook: gtk::Notebook,
}

impl Panel {
    pub fn new(orientation_label: &str) -> Self {
        Self::with_header(Some(orientation_label))
    }

    /// Side panel without a chrome title — just the notebook tabs.
    pub fn new_untitled() -> Self {
        Self::with_header(None)
    }

    fn with_header(orientation_label: Option<&str>) -> Self {
        let notebook = gtk::Notebook::new();
        notebook.set_scrollable(true);
        notebook.set_vexpand(true);
        notebook.set_hexpand(true);
        notebook.add_css_class("side-panel");
        notebook.add_css_class("gtk-content");

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("side-panel");
        root.add_css_class("gtk-content");
        if let Some(text) = orientation_label {
            let header = gtk::Label::new(Some(text));
            header.add_css_class("dim-label");
            header.set_margin_start(6);
            header.set_margin_top(4);
            root.append(&header);
        }
        root.append(&notebook);

        Self { root, notebook }
    }

    pub fn add_page(&self, title: &str, child: &impl IsA<gtk::Widget>) -> u32 {
        let label = gtk::Label::new(Some(title));
        self.notebook.append_page(child, Some(&label))
    }

    pub fn set_visible_panel(&self, visible: bool) {
        self.root.set_visible(visible);
    }

    /// Stable id for a side-panel tab label (`documents`, `filebrowser`, …).
    pub fn page_id_from_label(label: &str) -> String {
        let lower = label.trim().to_ascii_lowercase();
        if lower.contains("file") && lower.contains("browser") {
            "filebrowser".into()
        } else if lower.contains("document") {
            "documents".into()
        } else {
            lower.replace(' ', "")
        }
    }

    fn tab_label_text(&self, page: u32) -> Option<String> {
        let child = self.notebook.nth_page(Some(page))?;
        let tab = self.notebook.tab_label(&child)?;
        if let Ok(label) = tab.clone().downcast::<gtk::Label>() {
            return Some(label.text().to_string());
        }
        if let Ok(box_) = tab.downcast::<gtk::Box>() {
            let mut child = box_.first_child();
            while let Some(c) = child {
                if let Ok(label) = c.clone().downcast::<gtk::Label>() {
                    return Some(label.text().to_string());
                }
                child = c.next_sibling();
            }
        }
        None
    }

    pub fn current_page_id(&self) -> Option<String> {
        let page = self.notebook.current_page()?;
        self.tab_label_text(page)
            .map(|t| Self::page_id_from_label(&t))
    }

    /// Select the notebook page matching a saved id (e.g. `filebrowser`).
    pub fn restore_page_id(&self, page_id: &str) {
        if page_id.is_empty() {
            return;
        }
        let want = page_id.trim().to_ascii_lowercase();
        let n = self.notebook.n_pages();
        for i in 0..n {
            if let Some(text) = self.tab_label_text(i) {
                if Self::page_id_from_label(&text) == want {
                    self.notebook.set_current_page(Some(i));
                    return;
                }
            }
        }
    }
}
