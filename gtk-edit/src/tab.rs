//! Editor tab: scrolled SourceView + document (+ Markdown preview split).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::glib;
use gtk::prelude::*;
use gtk::pango;
use sourceview5::prelude::*;

use crate::config::EditorConfig;
use crate::document::{wrap_mode_from_str, Document};
use crate::markdown_preview;

pub struct EditorTab {
    pub page: gtk::Box,
    pub paned: gtk::Paned,
    pub view: sourceview5::View,
    #[allow(dead_code)]
    pub scrolled: gtk::ScrolledWindow,
    pub preview_scroll: gtk::ScrolledWindow,
    pub preview_buffer: gtk::TextBuffer,
    pub document: Rc<Document>,
    pub label: gtk::Label,
    pub tab_box: gtk::Box,
    pub close_btn: gtk::Button,
    /// Avoid resetting the paned position on every keystroke.
    preview_positioned: Rc<Cell<bool>>,
}

impl EditorTab {
    pub fn new(cfg: &EditorConfig) -> Rc<Self> {
        let document = Document::new(cfg);
        let view = sourceview5::View::with_buffer(&document.buffer);
        view.set_monospace(true);
        view.set_vexpand(true);
        view.set_hexpand(true);
        apply_view_config(&view, cfg);

        view.add_css_class("editor-view");
        view.add_css_class("gtk-content");

        let scrolled = gtk::ScrolledWindow::builder()
            .child(&view)
            .vexpand(true)
            .hexpand(true)
            .build();
        scrolled.add_css_class("editor-view");
        scrolled.add_css_class("gtk-content");

        let preview_buffer = gtk::TextBuffer::new(None::<&gtk::TextTagTable>);
        let preview_view = gtk::TextView::with_buffer(&preview_buffer);
        preview_view.set_editable(false);
        preview_view.set_cursor_visible(false);
        preview_view.set_wrap_mode(gtk::WrapMode::Word);
        preview_view.set_left_margin(12);
        preview_view.set_right_margin(12);
        preview_view.set_top_margin(10);
        preview_view.set_bottom_margin(10);
        preview_view.set_monospace(false);
        preview_view.add_css_class("gtk-content");
        preview_view.add_css_class("markdown-preview");

        let preview_scroll = gtk::ScrolledWindow::builder()
            .child(&preview_view)
            .vexpand(true)
            .hexpand(true)
            .build();
        preview_scroll.add_css_class("gtk-content");
        preview_scroll.add_css_class("markdown-preview");
        preview_scroll.set_visible(false);

        let paned = gtk::Paned::new(gtk::Orientation::Vertical);
        paned.set_vexpand(true);
        paned.set_hexpand(true);
        paned.set_start_child(Some(&scrolled));
        paned.set_end_child(Some(&preview_scroll));
        paned.set_resize_start_child(true);
        paned.set_resize_end_child(true);
        paned.set_shrink_start_child(true);
        paned.set_shrink_end_child(true);
        paned.set_wide_handle(true);
        paned.add_css_class("gtk-content");

        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.add_css_class("gtk-content");
        page.append(&paned);

        // Tab labels ellipsize inside the expanded tab slot (tab-expand on the
        // notebook page). Keep a small width_chars floor so empty titles don't
        // collapse; avoid size-requests that can grow the window.
        let label = gtk::Label::builder()
            .label("Untitled")
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .width_chars(4)
            .hexpand(true)
            .halign(gtk::Align::Fill)
            .single_line_mode(true)
            .xalign(0.0)
            .build();
        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.add_css_class("flat");
        close.add_css_class("small-button");
        close.set_focusable(false);
        close.set_hexpand(false);
        close.set_valign(gtk::Align::Center);

        let tab_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        tab_box.set_hexpand(true);
        tab_box.set_halign(gtk::Align::Fill);
        tab_box.set_valign(gtk::Align::Fill);
        tab_box.append(&label);
        tab_box.append(&close);

        let tab = Rc::new(Self {
            page,
            paned,
            view,
            scrolled,
            preview_scroll,
            preview_buffer,
            document,
            label,
            tab_box,
            close_btn: close,
            preview_positioned: Rc::new(Cell::new(false)),
        });

        {
            let tab2 = Rc::clone(&tab);
            tab.document.buffer.connect_modified_changed(move |_| {
                tab2.refresh_title();
            });
        }
        {
            let tab2 = Rc::clone(&tab);
            tab.document.buffer.connect_changed(move |_| {
                let tab2 = Rc::clone(&tab2);
                glib::idle_add_local_once(move || {
                    tab2.refresh_markdown_preview();
                });
            });
        }

        tab
    }

    pub fn refresh_title(&self) {
        let modified = self.document.is_modified();
        let title = crate::document::short_title(self.document.path().as_deref(), modified);
        self.label.set_text(&title);
    }

    pub fn apply_config(&self, cfg: &EditorConfig) {
        apply_view_config(&self.view, cfg);
        self.document.apply_editor_config(cfg);
    }

    /// Show or hide the bottom Markdown preview based on path / language.
    pub fn sync_markdown_preview(&self) {
        let show = markdown_preview::is_markdown_document(&self.document);
        let was_visible = self.preview_scroll.is_visible();
        self.preview_scroll.set_visible(show);
        if !show {
            self.preview_positioned.set(false);
            return;
        }
        self.refresh_markdown_preview();
        if !was_visible || !self.preview_positioned.get() {
            self.position_preview_split();
        }
    }

    pub fn refresh_markdown_preview(&self) {
        if !self.preview_scroll.is_visible() {
            return;
        }
        let md = self.document.text();
        markdown_preview::render_markdown_to_buffer(&self.preview_buffer, &md);
    }

    fn position_preview_split(&self) {
        let paned = self.paned.clone();
        let positioned = Rc::clone(&self.preview_positioned);
        glib::idle_add_local_once(move || {
            let h = paned.height();
            if h > 120 {
                paned.set_position((h as f64 * 0.55) as i32);
                positioned.set(true);
            } else {
                // Try again after realize / resize.
                let paned2 = paned.clone();
                let positioned2 = Rc::clone(&positioned);
                glib::timeout_add_local_once(std::time::Duration::from_millis(80), move || {
                    let h = paned2.height();
                    if h > 120 {
                        paned2.set_position((h as f64 * 0.55) as i32);
                        positioned2.set(true);
                    }
                });
            }
        });
    }
}

pub fn apply_view_config(view: &sourceview5::View, cfg: &EditorConfig) {
    view.set_show_line_numbers(cfg.display_line_numbers);
    view.set_highlight_current_line(cfg.highlight_current_line);
    view.set_show_right_margin(cfg.display_right_margin);
    view.set_right_margin_position(cfg.right_margin_position);
    view.set_tab_width(cfg.tabs_size);
    view.set_insert_spaces_instead_of_tabs(cfg.insert_spaces);
    view.set_auto_indent(cfg.auto_indent);
    view.set_wrap_mode(wrap_mode_from_str(&cfg.wrap_mode));

    let smart = match cfg.smart_home_end.as_str() {
        "disabled" => sourceview5::SmartHomeEndType::Disabled,
        "before" => sourceview5::SmartHomeEndType::Before,
        "always" => sourceview5::SmartHomeEndType::Always,
        _ => sourceview5::SmartHomeEndType::After,
    };
    view.set_smart_home_end(smart);

    view.add_css_class("gtk-edit-view");
    let font = if cfg.use_default_font {
        pango::FontDescription::from_string("Monospace 12")
    } else {
        pango::FontDescription::from_string(&cfg.editor_font)
    };
    let family = font
        .family()
        .unwrap_or_else(|| "Monospace".into())
        .replace('\'', "")
        .replace('"', "");
    let size_pt = {
        let sz = font.size();
        if sz > 0 {
            sz as f64 / pango::SCALE as f64
        } else {
            12.0
        }
    };
    let css = format!(
        "textview.gtk-edit-view {{ font-family: \"{family}\"; font-size: {size_pt}pt; }}"
    );
    let provider = gtk::CssProvider::new();
    provider.load_from_data(&css);
    // Replace previous provider on this view only (avoid stacking CSS).
    #[allow(deprecated)]
    {
        let ctx = view.style_context();
        unsafe {
            if let Some(old) = view.data::<gtk::CssProvider>("gtk-edit-font-provider") {
                ctx.remove_provider(old.as_ref());
            }
        }
        ctx.add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
    unsafe {
        view.set_data("gtk-edit-font-provider", provider);
    }
}

/// Notebook of editor tabs for one tab group.
pub struct TabNotebook {
    pub notebook: gtk::Notebook,
    pub tabs: RefCell<Vec<Rc<EditorTab>>>,
    /// Dummy page whose tab label is the compact "+" after the last document.
    plus_page: gtk::Box,
    /// Invoked when the trailing "+" control is clicked (set by the window).
    on_plus: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
}

impl TabNotebook {
    pub fn new() -> Rc<Self> {
        let notebook = gtk::Notebook::new();
        // tab-expand shares leftover bar space; scrollable keeps overflow on-screen.
        notebook.set_scrollable(true);
        notebook.set_vexpand(true);
        notebook.set_hexpand(true);
        notebook.add_css_class("gtk-edit-tabs");

        // Button (not a plain label): when "+" is already the current page
        // (e.g. empty window), switch-page never fires — the click must create
        // a document itself. Keep it as a real notebook tab so it sits beside
        // the last document rather than as an action-end widget.
        // Compact fixed-width "+": does not expand. Document tabs share the
        // leftover bar via tab-expand (gedit-style).
        let plus_btn = gtk::Button::builder()
            .label("+")
            .has_frame(false)
            .focus_on_click(false)
            .can_focus(false)
            .hexpand(false)
            .vexpand(true)
            .halign(gtk::Align::Fill)
            .valign(gtk::Align::Fill)
            .build();
        plus_btn.add_css_class("flat");
        plus_btn.add_css_class("gtk-edit-new-tab-label");
        plus_btn.set_tooltip_text(Some("New document"));

        let plus_tab = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        plus_tab.set_hexpand(false);
        plus_tab.set_vexpand(true);
        plus_tab.set_halign(gtk::Align::Fill);
        plus_tab.set_valign(gtk::Align::Fill);
        plus_tab.add_css_class("gtk-edit-new-tab");
        plus_tab.append(&plus_btn);

        let on_plus: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
        {
            let on_plus = Rc::clone(&on_plus);
            plus_btn.connect_clicked(move |_| {
                if let Some(cb) = on_plus.borrow().clone() {
                    cb();
                }
            });
        }

        let plus_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        plus_page.add_css_class("gtk-content");
        notebook.append_page(&plus_page, Some(&plus_tab));
        notebook.set_tab_reorderable(&plus_page, false);
        notebook.set_tab_label(&plus_page, Some(&plus_tab));
        {
            let page = notebook.page(&plus_page);
            page.set_tab_expand(false);
            page.set_tab_fill(true);
        }

        {
            let plus = plus_page.clone();
            notebook.connect_page_reordered(move |nb, _, _| {
                let Some(want) = nb.n_pages().checked_sub(1) else {
                    return;
                };
                if nb.page_num(&plus) != Some(want) {
                    nb.reorder_child(&plus, None);
                }
            });
        }

        Rc::new(Self {
            notebook,
            tabs: RefCell::new(Vec::new()),
            plus_page,
            on_plus,
        })
    }

    pub fn set_on_plus<F: Fn() + 'static>(&self, f: F) {
        *self.on_plus.borrow_mut() = Some(Rc::new(f));
    }

    /// True when `page` is the trailing "+" tab (not a document).
    pub fn is_plus_page(&self, page: u32) -> bool {
        self.notebook.page_num(&self.plus_page) == Some(page)
    }

    pub fn add_tab(&self, tab: Rc<EditorTab>) -> u32 {
        tab.refresh_title();
        // Keep "+" as the last tab: insert documents just before it.
        let plus_idx = self
            .notebook
            .page_num(&self.plus_page)
            .unwrap_or_else(|| self.notebook.n_pages());
        let idx = self
            .notebook
            .insert_page(&tab.page, Some(&tab.tab_box), Some(plus_idx));
        self.notebook.set_tab_reorderable(&tab.page, true);
        // Ensure GTK keeps our custom label widget (not a truncated fallback).
        self.notebook.set_tab_label(&tab.page, Some(&tab.tab_box));
        {
            let page = self.notebook.page(&tab.page);
            page.set_tab_expand(true);
            page.set_tab_fill(true);
        }
        self.tabs.borrow_mut().insert(plus_idx as usize, Rc::clone(&tab));
        self.notebook.set_current_page(Some(idx));
        self.ensure_plus_last();
        idx
    }

    fn ensure_plus_last(&self) {
        let Some(want) = self.notebook.n_pages().checked_sub(1) else {
            return;
        };
        if self.notebook.page_num(&self.plus_page) != Some(want) {
            self.notebook.reorder_child(&self.plus_page, None);
        }
    }

    pub fn current(&self) -> Option<Rc<EditorTab>> {
        let page = self.notebook.current_page()?;
        self.tab_at(page)
    }

    pub fn tab_at(&self, page: u32) -> Option<Rc<EditorTab>> {
        // try_borrow: switch-page handlers may run while tabs are being mutated.
        self.tabs
            .try_borrow()
            .ok()
            .and_then(|tabs| tabs.get(page as usize).cloned())
    }

    pub fn remove_tab(&self, tab: &EditorTab) {
        // Find index without holding a borrow across the GTK call — remove_page
        // emits switch-page, and handlers borrow `tabs` again (RefCell panic).
        let pos = self.tabs.borrow().iter().position(|t| {
            std::ptr::eq(t.as_ref(), tab) || t.page == tab.page
        });
        let Some(pos) = pos else {
            return;
        };
        // Drop our Rc first so the page can be destroyed cleanly, then ask GTK
        // to remove the notebook page (no RefCell borrow held here).
        let _removed = self.tabs.borrow_mut().remove(pos);
        self.notebook.remove_page(Some(pos as u32));
    }

    pub fn len(&self) -> usize {
        self.tabs.borrow().len()
    }

    pub fn update_tabs_visibility(&self, mode: &str) {
        let show = match mode {
            "never" => false,
            "auto" => self.len() > 1,
            _ => true,
        };
        self.notebook.set_show_tabs(show);
    }
}
