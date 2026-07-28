//! Drag-and-drop of files (export + import into folders).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use glib::prelude::ToValue;

use crate::file_ops;

/// Build a content provider that other apps (and our drop target) understand.
pub fn content_for_paths(paths: &[PathBuf]) -> Option<gdk::ContentProvider> {
    if paths.is_empty() {
        return None;
    }
    let files: Vec<gio::File> = paths.iter().map(gio::File::for_path).collect();

    let list = gdk::FileList::from_array(&files);
    let typed = gdk::ContentProvider::for_value(&list.to_value());

    let uri_text = files
        .iter()
        .map(|f| f.uri().to_string())
        .collect::<Vec<_>>()
        .join("\r\n");
    let uris = gdk::ContentProvider::for_bytes(
        "text/uri-list",
        &glib::Bytes::from(uri_text.as_bytes()),
    );

    Some(gdk::ContentProvider::new_union(&[typed, uris]))
}

/// Make `widget` a drag source for the file(s) produced by `paths_for_drag`.
pub fn attach_drag_source<F>(widget: &impl IsA<gtk::Widget>, paths_for_drag: F)
where
    F: Fn() -> Vec<PathBuf> + 'static,
{
    let drag = gtk::DragSource::new();
    drag.set_actions(gdk::DragAction::COPY | gdk::DragAction::MOVE);
    // Win the pointer sequence over click gestures once the drag threshold is hit.
    drag.set_exclusive(true);
    drag.connect_prepare(move |_, _, _| {
        let paths = paths_for_drag();
        if paths.is_empty() {
            return None;
        }
        content_for_paths(&paths)
    });
    drag.connect_drag_begin(|source, _| {
        if let Some(display) = gdk::Display::default() {
            let theme = gtk::IconTheme::for_display(&display);
            let icon = theme.lookup_icon(
                "text-x-generic",
                &[],
                48,
                1,
                gtk::TextDirection::Ltr,
                gtk::IconLookupFlags::empty(),
            );
            source.set_icon(Some(&icon), 24, 24);
        }
    });
    widget.add_controller(drag);
}

/// Accept file drops onto `widget`, placing them into `dest_dir()`.
pub fn attach_drop_target<D, Done>(widget: &impl IsA<gtk::Widget>, dest_dir: D, on_done: Done)
where
    D: Fn() -> Option<PathBuf> + 'static,
    Done: Fn() + 'static,
{
    let target =
        gtk::DropTarget::new(glib::Type::INVALID, gdk::DragAction::COPY | gdk::DragAction::MOVE);
    target.set_types(&[gdk::FileList::static_type(), gio::File::static_type()]);
    target.set_preload(true);

    let on_done = Rc::new(on_done);
    target.connect_drop(move |drop_target, value, _x, _y| {
        let Some(dest) = dest_dir() else {
            return false;
        };
        if !dest.is_dir() {
            return false;
        }

        let paths = paths_from_drop_value(value);
        if paths.is_empty() {
            return false;
        }

        let move_files = prefer_move(drop_target);
        file_ops::drop_into(None::<&gtk::Window>, &dest, &paths, move_files, {
            let on_done = Rc::clone(&on_done);
            move || on_done()
        });
        true
    });

    widget.add_controller(target);
}

/// Drop onto a folder row when the bound item is a directory.
pub fn attach_folder_drop_target<Done>(
    widget: &impl IsA<gtk::Widget>,
    row_target: Rc<RefCell<Option<(gio::File, bool, PathBuf)>>>,
    on_done: Done,
) where
    Done: Fn() + 'static,
{
    let drop =
        gtk::DropTarget::new(glib::Type::INVALID, gdk::DragAction::COPY | gdk::DragAction::MOVE);
    drop.set_types(&[gdk::FileList::static_type(), gio::File::static_type()]);
    drop.set_preload(true);

    let on_done = Rc::new(on_done);
    let accept_target = Rc::clone(&row_target);
    drop.connect_accept(move |_, _| {
        accept_target
            .borrow()
            .as_ref()
            .map(|(_, is_dir, _)| *is_dir)
            .unwrap_or(false)
    });

    drop.connect_drop(move |drop_target, value, _x, _y| {
        let Some((_, is_dir, dest)) = row_target.borrow().clone() else {
            return false;
        };
        if !is_dir {
            return false;
        }
        let paths: Vec<PathBuf> = paths_from_drop_value(value)
            .into_iter()
            .filter(|p| p != &dest && !dest.starts_with(p))
            .collect();
        if paths.is_empty() {
            return false;
        }

        let move_files = prefer_move(drop_target);
        file_ops::drop_into(None::<&gtk::Window>, &dest, &paths, move_files, {
            let on_done = Rc::clone(&on_done);
            move || on_done()
        });
        true
    });

    widget.add_controller(drop);
}

fn prefer_move(drop_target: &gtk::DropTarget) -> bool {
    // Shift during drop → move (Nautilus / desktop convention).
    if drop_target
        .current_event_state()
        .contains(gdk::ModifierType::SHIFT_MASK)
    {
        return true;
    }
    // When only MOVE is offered, or the drop negotiated MOVE alone.
    drop_target
        .current_drop()
        .map(|d| {
            let a = d.actions();
            a == gdk::DragAction::MOVE
                || (a.contains(gdk::DragAction::MOVE) && !a.contains(gdk::DragAction::COPY))
        })
        .unwrap_or(false)
}

fn paths_from_drop_value(value: &glib::Value) -> Vec<PathBuf> {
    if let Ok(list) = value.get::<gdk::FileList>() {
        return list.files().into_iter().filter_map(|f| f.path()).collect();
    }
    if let Ok(file) = value.get::<gio::File>() {
        if let Some(p) = file.path() {
            return vec![p];
        }
    }
    Vec::new()
}
