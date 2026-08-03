//! Cut / copy / paste clipboard for files.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use glib::prelude::ToValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipOp {
    Copy,
    Cut,
}

#[derive(Debug, Clone, Default)]
pub struct ClipboardState {
    pub paths: Vec<PathBuf>,
    pub op: Option<ClipOp>,
}

impl ClipboardState {
    pub fn clear(&mut self) {
        self.paths.clear();
        self.op = None;
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty() || self.op.is_none()
    }
}

pub type SharedClipboard = Rc<RefCell<ClipboardState>>;

thread_local! {
    static ACTIVE: RefCell<Option<SharedClipboard>> = const { RefCell::new(None) };
}

/// Register the app-wide clipboard so list/grid binds can style cut items.
pub fn set_active(clip: SharedClipboard) {
    ACTIVE.with(|a| *a.borrow_mut() = Some(clip));
}

pub fn new_shared() -> SharedClipboard {
    let clip = Rc::new(RefCell::new(ClipboardState::default()));
    set_active(Rc::clone(&clip));
    clip
}

pub fn is_empty(clip: &SharedClipboard) -> bool {
    clip.borrow().is_empty()
}

/// True when `path` is in the current Cut set (semi-transparent until paste).
pub fn is_path_cut(path: &Path) -> bool {
    ACTIVE.with(|a| {
        let active = a.borrow();
        let Some(clip) = active.as_ref() else {
            return false;
        };
        let st = clip.borrow();
        if st.op != Some(ClipOp::Cut) {
            return false;
        }
        st.paths.iter().any(|p| paths_match(p, path))
    })
}

fn paths_match(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    // DirectoryList / selection can disagree on trailing separators or
    // relative vs absolute forms — compare canonical paths when possible.
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

pub fn set_files(
    clip: &SharedClipboard,
    paths: Vec<PathBuf>,
    op: ClipOp,
    widget: &impl IsA<gtk::Widget>,
) {
    set_active(Rc::clone(clip));
    {
        let mut st = clip.borrow_mut();
        st.paths = paths.clone();
        st.op = Some(op);
    }

    // Publish to the system clipboard for other apps (Nautilus, etc.).
    let files: Vec<gio::File> = paths.iter().map(gio::File::for_path).collect();
    let file_list = gdk::FileList::from_array(&files);
    let typed = gdk::ContentProvider::for_value(&file_list.to_value());

    let uris: Vec<String> = files.iter().map(|f| f.uri().to_string()).collect();
    let uri_text = uris.join("\r\n");
    let uri_list =
        gdk::ContentProvider::for_bytes("text/uri-list", &glib::Bytes::from(uri_text.as_bytes()));

    // GNOME / Nautilus copied-files format.
    let gnome_op = match op {
        ClipOp::Copy => "copy",
        ClipOp::Cut => "cut",
    };
    let gnome_body = format!("{gnome_op}\n{}", uris.join("\n"));
    let gnome = gdk::ContentProvider::for_bytes(
        "x-special/gnome-copied-files",
        &glib::Bytes::from(gnome_body.as_bytes()),
    );

    let provider = gdk::ContentProvider::new_union(&[typed, uri_list, gnome]);
    let _ = widget.clipboard().set_content(Some(&provider));
}

pub fn take_for_paste(clip: &SharedClipboard) -> Option<(Vec<PathBuf>, ClipOp)> {
    let st = clip.borrow();
    if st.is_empty() {
        return None;
    }
    Some((st.paths.clone(), st.op.unwrap()))
}

pub fn clear_after_cut_paste(clip: &SharedClipboard) {
    let mut st = clip.borrow_mut();
    if st.op == Some(ClipOp::Cut) {
        st.clear();
    }
}

/// Read file paths from the system clipboard (`text/uri-list`).
pub fn read_paths_from_gdk(
    widget: &impl IsA<gtk::Widget>,
    on_paths: impl FnOnce(Vec<PathBuf>) + 'static,
) {
    let clipboard = widget.clipboard();
    clipboard.read_async(
        &["text/uri-list"],
        glib::Priority::DEFAULT,
        None::<&gio::Cancellable>,
        move |result| {
            let Ok((stream, _)) = result else {
                on_paths(Vec::new());
                return;
            };
            let mem = gio::MemoryOutputStream::new_resizable();
            if mem
                .splice(
                    &stream,
                    gio::OutputStreamSpliceFlags::CLOSE_SOURCE
                        | gio::OutputStreamSpliceFlags::CLOSE_TARGET,
                    None::<&gio::Cancellable>,
                )
                .is_err()
            {
                on_paths(Vec::new());
                return;
            }
            let bytes = mem.steal_as_bytes();
            let text = String::from_utf8_lossy(bytes.as_ref());
            let paths: Vec<PathBuf> = text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .filter_map(|uri| gio::File::for_uri(uri).path())
                .collect();
            on_paths(paths);
        },
    );
}
