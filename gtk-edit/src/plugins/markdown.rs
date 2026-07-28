//! Markdown Preview — side-panel live preview for `.md` / `.markdown` files
//! (ported from the gedit-md plugin; renders via pulldown-cmark → plain text).
//! The editor tab also shows a top/bottom split preview when a Markdown file is open.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use crate::markdown_preview::{is_markdown_document, render_markdown_to_buffer};
use crate::plugin::activatable::{Plugin, PluginInfo, WindowActivatable, WindowContext};
use crate::plugins::{info, make_factory};

struct MarkdownPlugin {
    info: PluginInfo,
    root: RefCell<Option<gtk::Box>>,
    preview: RefCell<Option<gtk::TextBuffer>>,
    window: RefCell<Option<gtk::ApplicationWindow>>,
    closed_by_user: Rc<Cell<bool>>,
    float_win: Rc<RefCell<Option<gtk::Window>>>,
}

impl Plugin for MarkdownPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn as_window(&mut self) -> Option<&mut dyn WindowActivatable> {
        Some(self)
    }
}

impl WindowActivatable for MarkdownPlugin {
    fn activate(&mut self, ctx: &WindowContext) {
        *self.window.borrow_mut() = Some(ctx.window.clone());

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header.set_margin_start(6);
        header.set_margin_end(6);
        header.set_margin_top(4);
        header.set_margin_bottom(2);
        let title = gtk::Label::new(Some("Markdown Preview"));
        title.add_css_class("heading");
        title.set_hexpand(true);
        title.set_xalign(0.0);
        let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
        refresh.set_tooltip_text(Some("Refresh preview"));
        let pop_out = gtk::Button::from_icon_name("window-new-symbolic");
        pop_out.set_tooltip_text(Some("Open preview window"));
        header.append(&title);
        header.append(&refresh);
        header.append(&pop_out);

        let buffer = gtk::TextBuffer::new(None::<&gtk::TextTagTable>);
        let view = gtk::TextView::with_buffer(&buffer);
        view.set_editable(false);
        view.set_cursor_visible(false);
        view.set_wrap_mode(gtk::WrapMode::Word);
        view.set_left_margin(10);
        view.set_right_margin(10);
        view.set_top_margin(8);
        view.set_bottom_margin(8);
        view.add_css_class("gtk-content");

        let scroll = gtk::ScrolledWindow::builder()
            .child(&view)
            .vexpand(true)
            .hexpand(true)
            .build();
        scroll.add_css_class("gtk-content");

        let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        outer.set_vexpand(true);
        outer.append(&header);
        outer.append(&scroll);

        if let Some(nb) = find_side_notebook(&ctx.side_panel) {
            nb.append_page(&outer, Some(&gtk::Label::new(Some("Markdown"))));
        } else {
            ctx.side_panel.append(&outer);
        }
        ctx.side_panel.set_visible(true);

        *self.root.borrow_mut() = Some(outer);
        *self.preview.borrow_mut() = Some(buffer.clone());

        {
            let win = ctx.window.clone();
            let buffer = buffer.clone();
            refresh.connect_clicked(move |_| {
                refresh_preview(&win, &buffer);
            });
        }
        {
            let float_win = Rc::clone(&self.float_win);
            let closed = Rc::clone(&self.closed_by_user);
            let win = ctx.window.clone();
            let buffer = buffer.clone();
            pop_out.connect_clicked(move |_| {
                closed.set(false);
                show_float_preview(&win, &buffer, &float_win, &closed);
            });
        }

        let action = gio::SimpleAction::new("markdown-preview", None);
        let win = ctx.window.clone();
        let buffer_a = buffer.clone();
        let float_win = Rc::clone(&self.float_win);
        let closed = Rc::clone(&self.closed_by_user);
        action.connect_activate(move |_, _| {
            closed.set(false);
            refresh_preview(&win, &buffer_a);
            show_float_preview(&win, &buffer_a, &float_win, &closed);
        });
        ctx.window.add_action(&action);
        ctx.menu_icons.borrow_mut().append(
            &ctx.tools_menu,
            "Markdown Preview",
            "win.markdown-preview",
            "text-x-generic-symbolic",
        );

        if let Some(tab) = crate::window::current_tab_from_window(&ctx.window) {
            let win = ctx.window.clone();
            let buffer = buffer.clone();
            tab.document.buffer.connect_changed(move |_| {
                let win = win.clone();
                let buffer = buffer.clone();
                glib::idle_add_local_once(move || {
                    refresh_preview(&win, &buffer);
                });
            });
        }

        refresh_preview(&ctx.window, &buffer);
    }

    fn update_state(&mut self) {
        let Some(win) = self.window.borrow().clone() else {
            return;
        };
        let Some(buffer) = self.preview.borrow().clone() else {
            return;
        };
        refresh_preview(&win, &buffer);

        if is_markdown_active(&win) && !self.closed_by_user.get() {
            if let Some(fw) = self.float_win.borrow().clone() {
                fw.present();
            }
        } else if let Some(fw) = self.float_win.borrow().clone() {
            fw.set_visible(false);
        }

        if let Some(tab) = crate::window::current_tab_from_window(&win) {
            let win2 = win.clone();
            let buffer2 = buffer.clone();
            tab.document.buffer.connect_changed(move |_| {
                let win = win2.clone();
                let buffer = buffer2.clone();
                glib::idle_add_local_once(move || {
                    refresh_preview(&win, &buffer);
                });
            });
        }
    }

    fn deactivate(&mut self) {
        if let Some(fw) = self.float_win.borrow_mut().take() {
            fw.close();
        }
        if let Some(root) = self.root.borrow_mut().take() {
            if let Some(parent) = root.parent() {
                if let Some(nb) = parent.downcast_ref::<gtk::Notebook>() {
                    nb.detach_tab(&root);
                }
            }
        }
        *self.preview.borrow_mut() = None;
        *self.window.borrow_mut() = None;
    }
}

fn show_float_preview(
    parent: &gtk::ApplicationWindow,
    buffer: &gtk::TextBuffer,
    float_slot: &Rc<RefCell<Option<gtk::Window>>>,
    closed: &Rc<Cell<bool>>,
) {
    refresh_preview(parent, buffer);
    if let Some(existing) = float_slot.borrow().clone() {
        existing.present();
        return;
    }

    let win = gtk::Window::builder()
        .title("Markdown Preview")
        .transient_for(parent)
        .default_width(parent.default_size().0.max(480))
        .default_height(parent.default_size().1.max(400))
        .build();
    gtk_theme::style_dialog(&win);

    let view = gtk::TextView::with_buffer(buffer);
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_wrap_mode(gtk::WrapMode::Word);
    view.set_left_margin(12);
    view.set_right_margin(12);
    view.set_top_margin(10);
    view.set_bottom_margin(10);
    view.add_css_class("gtk-content");
    let scroll = gtk::ScrolledWindow::builder()
        .child(&view)
        .vexpand(true)
        .hexpand(true)
        .build();
    scroll.add_css_class("gtk-content");
    win.set_child(Some(&scroll));

    {
        let slot = Rc::clone(float_slot);
        let closed = Rc::clone(closed);
        win.connect_close_request(move |_| {
            *slot.borrow_mut() = None;
            closed.set(true);
            glib::Propagation::Proceed
        });
    }

    *float_slot.borrow_mut() = Some(win.clone());
    win.present();
}

fn is_markdown_active(win: &gtk::ApplicationWindow) -> bool {
    let Some(tab) = crate::window::current_tab_from_window(win) else {
        return false;
    };
    is_markdown_document(&tab.document)
}

fn refresh_preview(win: &gtk::ApplicationWindow, buffer: &gtk::TextBuffer) {
    let Some(tab) = crate::window::current_tab_from_window(win) else {
        buffer.set_text("No active document.");
        return;
    };
    if !is_markdown_document(&tab.document) {
        buffer.set_text("Open a Markdown file (.md) to see a live preview.");
        return;
    }
    render_markdown_to_buffer(buffer, &tab.document.text());
}

fn find_side_notebook(side: &gtk::Box) -> Option<gtk::Notebook> {
    let mut child = side.first_child();
    while let Some(c) = child {
        let next = c.next_sibling();
        if let Ok(nb) = c.clone().downcast::<gtk::Notebook>() {
            return Some(nb);
        }
        if let Some(box_) = c.downcast_ref::<gtk::Box>() {
            if let Some(nb) = find_side_notebook(box_) {
                return Some(nb);
            }
        }
        child = next;
    }
    None
}

pub fn factory() -> (PluginInfo, crate::plugin::activatable::PluginFactory) {
    let i = info(
        "markdown",
        "Markdown Preview",
        "Live Markdown preview in the side panel (and optional floating window).",
    );
    make_factory(i.clone(), move || {
        Box::new(MarkdownPlugin {
            info: i.clone(),
            root: RefCell::new(None),
            preview: RefCell::new(None),
            window: RefCell::new(None),
            closed_by_user: Rc::new(Cell::new(false)),
            float_win: Rc::new(RefCell::new(None)),
        })
    })
}
