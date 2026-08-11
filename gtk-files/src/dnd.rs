//! Drag-and-drop of files (export + import into folders).
//!
//! Cursor / action convention (this app):
//! - Default drop → **COPY** (plus cursor): add a copy with a unique name
//! - Hold **Ctrl** → **MOVE** (arrow): relocate; unique name if a clash exists
//! - Hold **Shift** → **MOVE + replace** (arrow): relocate; ask before clobbering
//!   existing names (Replace / Skip / apply to the rest)
//!
//! On Wayland, `DropTarget::current_event_state()` usually has only the mouse
//! button during a drag — keyboard modifiers must be read from the seat
//! keyboard (and from a Capture key controller while a drag is active).

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use glib::prelude::ToValue;

use crate::file_ops;

// Ctrl/Shift held while a file drag is in progress (Wayland-safe).
thread_local! {
    static ACTIVE_DRAG_MODS: Cell<gdk::ModifierType> = const {
        Cell::new(gdk::ModifierType::empty())
    };
    static DRAG_KEY_TRACKER: RefCell<Option<(gtk::Widget, gtk::EventControllerKey)>> =
        const { RefCell::new(None) };
}

const INTERESTING_MODS: gdk::ModifierType = gdk::ModifierType::SHIFT_MASK
    .union(gdk::ModifierType::CONTROL_MASK)
    .union(gdk::ModifierType::ALT_MASK)
    .union(gdk::ModifierType::SUPER_MASK);

fn interesting(mods: gdk::ModifierType) -> gdk::ModifierType {
    mods.intersection(INTERESTING_MODS)
}

/// Seat keyboard/pointer modifier bits (keyboard is what matters on Wayland DnD).
fn seat_key_modifiers() -> gdk::ModifierType {
    let mut mods = gdk::ModifierType::empty();
    let Some(display) = gdk::Display::default() else {
        return mods;
    };
    let Some(seat) = display.default_seat() else {
        return mods;
    };
    if let Some(keyboard) = seat.keyboard() {
        mods |= interesting(keyboard.modifier_state());
    }
    if let Some(pointer) = seat.pointer() {
        mods |= interesting(pointer.modifier_state());
    }
    mods
}

fn refresh_active_drag_mods() -> gdk::ModifierType {
    let mods = seat_key_modifiers();
    ACTIVE_DRAG_MODS.with(|c| c.set(mods));
    mods
}

fn active_drag_mods() -> gdk::ModifierType {
    ACTIVE_DRAG_MODS.with(|c| c.get()) | seat_key_modifiers()
}

fn attach_drag_key_tracker(host: &gtk::Widget) {
    detach_drag_key_tracker();
    refresh_active_drag_mods();

    let Some(root) = host.root() else {
        return;
    };
    let root = root.upcast::<gtk::Widget>();

    let key = gtk::EventControllerKey::new();
    key.set_propagation_phase(gtk::PropagationPhase::Capture);
    key.connect_key_pressed(move |_, _keyval, _keycode, _state| {
        refresh_active_drag_mods();
        glib::Propagation::Proceed
    });
    key.connect_key_released(move |_, _keyval, _keycode, _state| {
        refresh_active_drag_mods();
    });
    // Also catch modifier-only changes that some backends report via modifiers.
    key.connect_modifiers(move |_, state| {
        let mods = interesting(state) | seat_key_modifiers();
        ACTIVE_DRAG_MODS.with(|c| c.set(mods));
        glib::Propagation::Proceed
    });

    root.add_controller(key.clone());
    DRAG_KEY_TRACKER.with(|slot| {
        *slot.borrow_mut() = Some((root, key));
    });
}

fn detach_drag_key_tracker() {
    DRAG_KEY_TRACKER.with(|slot| {
        if let Some((root, key)) = slot.borrow_mut().take() {
            root.remove_controller(&key);
        }
    });
    ACTIVE_DRAG_MODS.with(|c| c.set(gdk::ModifierType::empty()));
}

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
        let paths: Vec<PathBuf> = paths_for_drag()
            .into_iter()
            .filter(|p| !p.as_os_str().is_empty() && p.exists())
            .collect();
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
        if let Some(host) = source.widget() {
            attach_drag_key_tracker(&host);
        } else {
            refresh_active_drag_mods();
        }
    });
    drag.connect_drag_end(|_, _, _| {
        detach_drag_key_tracker();
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

    let mods = Rc::new(Cell::new(gdk::ModifierType::empty()));
    wire_action_cursors(&target, Rc::clone(&mods));

    let host = widget.clone().upcast::<gtk::Widget>();
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

        let state = drop_modifier_state(drop_target, &mods);
        let (move_files, overwrite) = drop_intent(state);
        let parent = host.root().and_downcast::<gtk::Window>();
        let on_done = Rc::clone(&on_done);
        file_ops::drop_into(
            parent.as_ref(),
            &dest,
            &paths,
            move_files,
            overwrite,
            move || {
                // Defer refresh so we don't destroy list/grid rows while GTK is
                // still finishing the drop sequence (that crashes the app).
                defer_done(on_done);
            },
        );
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

    let mods = Rc::new(Cell::new(gdk::ModifierType::empty()));

    let accept_target = Rc::clone(&row_target);
    drop.connect_accept(move |_, _| {
        accept_target
            .borrow()
            .as_ref()
            .map(|(_, is_dir, _)| *is_dir)
            .unwrap_or(false)
    });

    // Only negotiate a cursor action when this row is a directory.
    let enter_target = Rc::clone(&row_target);
    let enter_mods = Rc::clone(&mods);
    drop.connect_enter(move |dt, _, _| {
        enter_mods.set(snapshot_modifiers(dt));
        if enter_target
            .borrow()
            .as_ref()
            .map(|(_, is_dir, _)| *is_dir)
            .unwrap_or(false)
        {
            action_for_state(enter_mods.get())
        } else {
            gdk::DragAction::empty()
        }
    });
    let motion_target = Rc::clone(&row_target);
    let motion_mods = Rc::clone(&mods);
    drop.connect_motion(move |dt, _, _| {
        motion_mods.set(snapshot_modifiers(dt));
        if motion_target
            .borrow()
            .as_ref()
            .map(|(_, is_dir, _)| *is_dir)
            .unwrap_or(false)
        {
            action_for_state(motion_mods.get())
        } else {
            gdk::DragAction::empty()
        }
    });

    let host = widget.clone().upcast::<gtk::Widget>();
    let on_done = Rc::new(on_done);
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

        let state = drop_modifier_state(drop_target, &mods);
        let (move_files, overwrite) = drop_intent(state);
        let parent = host.root().and_downcast::<gtk::Window>();
        let on_done = Rc::clone(&on_done);
        file_ops::drop_into(
            parent.as_ref(),
            &dest,
            &paths,
            move_files,
            overwrite,
            move || {
                defer_done(on_done);
            },
        );
        true
    });

    widget.add_controller(drop);
}

fn wire_action_cursors(target: &gtk::DropTarget, mods: Rc<Cell<gdk::ModifierType>>) {
    // Returning a *single* action from enter/motion sets the drag cursor
    // (+ for COPY, arrow for MOVE). Without this, GTK leaves both offered and
    // the drop path cannot tell which the user chose.
    let mods_enter = Rc::clone(&mods);
    target.connect_enter(move |dt, _, _| {
        mods_enter.set(snapshot_modifiers(dt));
        action_for_state(mods_enter.get())
    });
    let mods_motion = Rc::clone(&mods);
    target.connect_motion(move |dt, _, _| {
        mods_motion.set(snapshot_modifiers(dt));
        action_for_state(mods_motion.get())
    });
}

/// Merge DropTarget event bits with seat keyboard + in-drag key tracker.
fn snapshot_modifiers(drop_target: &gtk::DropTarget) -> gdk::ModifierType {
    let mut mods = interesting(drop_target.current_event_state());
    if let Some(event) = drop_target.current_event() {
        mods |= interesting(event.modifier_state());
    }
    mods |= active_drag_mods();
    // Keep the tracker fresh while hovering a drop zone.
    ACTIVE_DRAG_MODS.with(|c| c.set(mods | seat_key_modifiers()));
    mods | seat_key_modifiers()
}

/// Prefer a fresh keyboard read; keep the enter/motion snapshot as backup.
fn drop_modifier_state(
    drop_target: &gtk::DropTarget,
    snapshot: &Cell<gdk::ModifierType>,
) -> gdk::ModifierType {
    // Always re-poll — drop-time `current_event_state` is often button-only.
    let mut mods = snapshot_modifiers(drop_target);
    mods |= interesting(snapshot.get());
    mods |= active_drag_mods();
    interesting(mods)
}

/// Map modifiers → (move_files, overwrite/clobber).
///
/// - Shift → move + replace existing names (after confirm)
/// - Ctrl (without Shift) → move, uniquify on clash
/// - neither → copy, uniquify on clash
fn drop_intent(state: gdk::ModifierType) -> (bool, bool) {
    let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
    let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
    if shift {
        (true, true)
    } else if ctrl {
        (true, false)
    } else {
        (false, false)
    }
}

/// Ctrl or Shift → MOVE cursor (arrow); otherwise COPY (+).
fn action_for_state(state: gdk::ModifierType) -> gdk::DragAction {
    if state.contains(gdk::ModifierType::CONTROL_MASK)
        || state.contains(gdk::ModifierType::SHIFT_MASK)
    {
        gdk::DragAction::MOVE
    } else {
        gdk::DragAction::COPY
    }
}

fn defer_done<F: Fn() + 'static>(on_done: Rc<F>) {
    // Idle alone can still run while GTK is finishing the drag sequence and
    // destroying the drag-source row — that crashes. Wait a beat first.
    glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
        on_done();
    });
}

fn paths_from_drop_value(value: &glib::Value) -> Vec<PathBuf> {
    if let Ok(list) = value.get::<gdk::FileList>() {
        return list
            .files()
            .into_iter()
            .filter_map(|f| f.path())
            .filter(|p| !p.as_os_str().is_empty())
            .collect();
    }
    if let Ok(file) = value.get::<gio::File>() {
        if let Some(p) = file.path() {
            if !p.as_os_str().is_empty() {
                return vec![p];
            }
        }
    }
    Vec::new()
}
