//! Folder search bar + recursive filename search under the current directory.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use crate::util::FILE_ATTRIBUTES;

/// Cap so huge trees stay responsive.
pub const MAX_NAME_HITS: usize = 2_000;
const MAX_DEPTH: usize = 40;

pub struct SearchBar {
    pub root: gtk::Revealer,
    entry: gtk::Entry,
    status: gtk::Label,
    on_changed: RefCell<Option<Rc<dyn Fn(String)>>>,
}

impl SearchBar {
    pub fn new() -> Rc<Self> {
        let entry = gtk::Entry::builder()
            .hexpand(true)
            .placeholder_text("Search files under this folder…")
            .primary_icon_name("edit-find-symbolic")
            .build();

        let status = gtk::Label::new(None);
        status.add_css_class("dim-label");
        status.set_margin_end(4);
        status.set_visible(false);

        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.set_tooltip_text(Some("Close search"));
        close.add_css_class("flat");

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.set_margin_start(6);
        row.set_margin_end(6);
        row.set_margin_top(4);
        row.set_margin_bottom(4);
        row.add_css_class("search-bar");
        row.append(&entry);
        row.append(&status);
        row.append(&close);

        let root = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .reveal_child(false)
            .child(&row)
            .build();

        let bar = Rc::new(Self {
            root: root.clone(),
            entry: entry.clone(),
            status: status.clone(),
            on_changed: RefCell::new(None),
        });

        {
            let bar2 = Rc::clone(&bar);
            entry.connect_changed(move |e| {
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
            let key = gtk::EventControllerKey::new();
            key.connect_key_pressed(move |_, keyval, _, _| {
                if keyval == gdk::Key::Escape {
                    bar2.hide();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
            entry.add_controller(key);
        }

        bar
    }

    pub fn set_on_changed<F: Fn(String) + 'static>(&self, f: F) {
        *self.on_changed.borrow_mut() = Some(Rc::new(f));
    }

    pub fn set_status(&self, text: Option<&str>) {
        match text {
            Some(t) if !t.is_empty() => {
                self.status.set_text(t);
                self.status.set_visible(true);
            }
            _ => {
                self.status.set_text("");
                self.status.set_visible(false);
            }
        }
    }

    pub fn show(&self) {
        self.root.set_reveal_child(true);
        self.root.set_visible(true);
        let entry = self.entry.clone();
        glib::idle_add_local_once(move || {
            entry.grab_focus();
            entry.select_region(0, -1);
        });
    }

    pub fn hide(&self) {
        self.root.set_reveal_child(false);
        self.clear_query();
    }

    /// Clear the query without closing the bar (e.g. after folder navigation).
    pub fn clear_query(&self) {
        if !self.entry.text().is_empty() {
            self.entry.set_text("");
        } else if let Some(cb) = self.on_changed.borrow().as_ref() {
            cb(String::new());
        }
        self.set_status(None);
    }

    pub fn toggle(&self) {
        if self.root.is_child_revealed() {
            self.hide();
        } else {
            self.show();
        }
    }

    #[allow(dead_code)]
    pub fn is_visible(&self) -> bool {
        self.root.is_child_revealed()
    }
}

/// Background recursive filename search. Calls `on_done` on the GTK main loop.
pub fn search_names_async(
    root: PathBuf,
    query: String,
    show_hidden: bool,
    generation: u64,
    latest: Arc<AtomicU64>,
    on_done: impl FnOnce(u64, Vec<PathBuf>) + 'static,
) {
    let (tx, rx) = mpsc::channel::<(u64, Vec<PathBuf>)>();
    thread::spawn(move || {
        let hits = if query.trim().is_empty() {
            Vec::new()
        } else {
            find_names(&root, query.trim(), show_hidden)
        };
        let _ = tx.send((generation, hits));
    });

    let on_done = RefCell::new(Some(on_done));
    glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
        match rx.try_recv() {
            Ok((gen, hits)) => {
                if latest.load(Ordering::Relaxed) == gen {
                    if let Some(cb) = on_done.borrow_mut().take() {
                        cb(gen, hits);
                    }
                }
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
}

fn find_names(root: &Path, query: &str, show_hidden: bool) -> Vec<PathBuf> {
    let needle = query.to_lowercase();
    let mut out = Vec::new();
    walk(root, root, &needle, show_hidden, 0, &mut out);
    out
}

fn walk(
    root: &Path,
    dir: &Path,
    needle: &str,
    show_hidden: bool,
    depth: usize,
    out: &mut Vec<PathBuf>,
) {
    if out.len() >= MAX_NAME_HITS || depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_NAME_HITS {
            return;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !show_hidden && name_str.starts_with('.') {
            continue;
        }
        // Skip huge / irrelevant trees.
        if name_str == "node_modules"
            || name_str == ".git"
            || name_str == "target"
            || name_str == "__pycache__"
            || name_str == ".venv"
            || name_str == "venv"
        {
            continue;
        }
        let path = entry.path();
        if name_str.to_lowercase().contains(needle) {
            out.push(path.clone());
        }
        let is_dir = entry
            .file_type()
            .map(|t| t.is_dir())
            .unwrap_or_else(|_| path.is_dir());
        if is_dir && !entry.path().is_symlink() {
            walk(root, &path, needle, show_hidden, depth + 1, out);
        }
    }
}

/// Build a FileInfo the list/grid can open (includes `standard::file`).
pub fn file_info_for_search_hit(root: &Path, path: &Path) -> Option<gio::FileInfo> {
    let file = gio::File::for_path(path);
    let info = file
        .query_info(
            FILE_ATTRIBUTES,
            gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
            gio::Cancellable::NONE,
        )
        .ok()?;
    info.set_attribute_object("standard::file", &file);
    if let Ok(rel) = path.strip_prefix(root) {
        let rel = rel.to_string_lossy();
        if !rel.is_empty() {
            info.set_display_name(&rel);
        }
    }
    Some(info)
}
