//! Side list of media files. The window decides what to play.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;

pub struct Playlist {
    pub root: gtk::Box,
    list: gtk::ListBox,
    paths: RefCell<Vec<PathBuf>>,
    current: Cell<Option<usize>>,
    on_activate: RefCell<Option<Box<dyn Fn(usize)>>>,
}

impl Playlist {
    pub fn new() -> Rc<Self> {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.add_css_class("playlist");
        list.set_vexpand(true);

        let scroll = gtk::ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_propagate_natural_width(false);
        scroll.set_child(Some(&list));
        scroll.set_vexpand(true);
        scroll.set_hexpand(true);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_vexpand(true);
        root.set_hexpand(false);
        root.append(&scroll);

        let pl = Rc::new(Self {
            root,
            list,
            paths: RefCell::new(Vec::new()),
            current: Cell::new(None),
            on_activate: RefCell::new(None),
        });

        {
            let pl = Rc::clone(&pl);
            let list = pl.list.clone();
            list.connect_row_activated(move |_, row| {
                let index = row.index();
                if index < 0 {
                    return;
                }
                if let Some(cb) = pl.on_activate.borrow().as_ref() {
                    cb(index as usize);
                }
            });
        }
        pl
    }

    pub fn connect_activated<F: Fn(usize) + 'static>(&self, f: F) {
        *self.on_activate.borrow_mut() = Some(Box::new(f));
    }

    pub fn len(&self) -> usize {
        self.paths.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.borrow().is_empty()
    }

    pub fn current(&self) -> Option<usize> {
        self.current.get()
    }

    pub fn path_at(&self, index: usize) -> Option<PathBuf> {
        self.paths.borrow().get(index).cloned()
    }

    pub fn selected(&self) -> Option<usize> {
        self.list.selected_row().map(|row| row.index() as usize)
    }

    pub fn replace(&self, paths: Vec<PathBuf>) {
        *self.paths.borrow_mut() = paths;
        self.current.set(None);
        self.rebuild();
    }

    pub fn extend(&self, paths: Vec<PathBuf>) {
        self.paths.borrow_mut().extend(paths);
        self.rebuild();
    }

    pub fn set_current(&self, index: Option<usize>) {
        let index = index.filter(|i| *i < self.paths.borrow().len());
        self.current.set(index);
        self.mark_current();
    }

    /// Drop `index`. Returns the index that should play next, if any.
    pub fn remove_at(&self, index: usize) -> Option<usize> {
        let mut paths = self.paths.borrow_mut();
        if index >= paths.len() {
            return self.current.get();
        }
        paths.remove(index);
        let len = paths.len();
        drop(paths);

        let current = self.current.get();
        let next = match current {
            Some(c) if c == index => {
                if len == 0 {
                    None
                } else if index < len {
                    Some(index)
                } else {
                    Some(len - 1)
                }
            }
            Some(c) if c > index => Some(c - 1),
            other => other,
        };
        self.current.set(next);
        self.rebuild();
        next
    }

    pub fn clear(&self) {
        self.paths.borrow_mut().clear();
        self.current.set(None);
        self.rebuild();
    }

    fn rebuild(&self) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        for (i, path) in self.paths.borrow().iter().enumerate() {
            let row = gtk::ListBoxRow::new();
            let box_ = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            box_.set_margin_top(2);
            box_.set_margin_bottom(2);
            box_.set_margin_start(4);
            box_.set_margin_end(4);
            let icon = gtk::Image::from_icon_name(icon_for(path));
            icon.set_pixel_size(16);
            let label = gtk::Label::new(Some(&file_name(path)));
            label.set_halign(gtk::Align::Start);
            label.set_hexpand(true);
            label.set_width_chars(10);
            label.set_max_width_chars(10);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_tooltip_text(Some(&path.display().to_string()));
            box_.append(&icon);
            box_.append(&label);
            row.set_child(Some(&box_));
            if self.current.get() == Some(i) {
                row.add_css_class("current");
            }
            self.list.append(&row);
        }
        self.mark_current();
    }

    fn mark_current(&self) {
        let mut i = 0;
        let mut child = self.list.first_child();
        while let Some(widget) = child {
            if let Some(row) = widget.downcast_ref::<gtk::ListBoxRow>() {
                if self.current.get() == Some(i) {
                    row.add_css_class("current");
                    self.list.select_row(Some(row));
                } else {
                    row.remove_css_class("current");
                }
                i += 1;
            }
            child = widget.next_sibling();
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn icon_for(path: &Path) -> &'static str {
    if crate::media::looks_like_audio(path) {
        "audio-x-generic"
    } else {
        "video-x-generic"
    }
}
