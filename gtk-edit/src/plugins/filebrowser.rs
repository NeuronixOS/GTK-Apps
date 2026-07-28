use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4 as gtk;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use glib::prelude::ToValue;

use crate::plugin::activatable::{Plugin, PluginInfo, WindowActivatable, WindowContext};
use crate::plugins::{info, make_factory};

/// Internal clipboard: paths + whether Cut (move on paste).
type Clip = Option<(Vec<PathBuf>, bool)>;

struct FileBrowserPlugin {
    info: PluginInfo,
    root: RefCell<Option<gtk::Box>>,
    current_dir: RefCell<PathBuf>,
    /// Window whose `filebrowser_sync` hook we registered, so we can clear it.
    window: RefCell<Option<gtk::ApplicationWindow>>,
}

impl Plugin for FileBrowserPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn as_window(&mut self) -> Option<&mut dyn WindowActivatable> {
        Some(self)
    }
}

impl WindowActivatable for FileBrowserPlugin {
    fn activate(&mut self, ctx: &WindowContext) {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        // Start in the focused document's folder (like the terminal cwd) so the
        // browser reflects whatever tab is active when the panel opens.
        let start_dir = crate::window::current_from_window(&ctx.window)
            .map(|ew| ew.focused_document_dir())
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| home.clone());
        *self.current_dir.borrow_mut() = start_dir.clone();
        *self.window.borrow_mut() = Some(ctx.window.clone());

        let outer = gtk::Box::new(gtk::Orientation::Vertical, 4);
        outer.add_css_class("side-panel");
        outer.add_css_class("gtk-content");
        let path_label = gtk::Label::new(Some(&start_dir.display().to_string()));
        path_label.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        path_label.set_halign(gtk::Align::Start);
        path_label.set_margin_start(6);
        path_label.set_margin_top(4);

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.add_css_class("side-panel");
        let scroll = gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .hexpand(true)
            .build();
        scroll.add_css_class("side-panel");
        scroll.add_css_class("gtk-content");

        let up_btn = gtk::Button::from_icon_name("go-up-symbolic");
        up_btn.set_tooltip_text(Some("Parent directory"));
        let open_btn = gtk::Button::from_icon_name("document-open-symbolic");
        open_btn.set_tooltip_text(Some("Open folder…"));
        let follow_btn = gtk::ToggleButton::new();
        follow_btn.set_icon_name("go-jump-symbolic");
        follow_btn.set_active(true);
        follow_btn.set_tooltip_text(Some("Follow the active document's folder"));
        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        toolbar.set_margin_start(4);
        toolbar.add_css_class("side-panel");
        toolbar.append(&up_btn);
        toolbar.append(&open_btn);
        toolbar.append(&follow_btn);

        outer.append(&toolbar);
        outer.append(&path_label);
        outer.append(&scroll);

        let follow = Rc::new(Cell::new(true));
        let dir_cell = Rc::new(RefCell::new(start_dir));
        let clip = Rc::new(RefCell::new(None::<(Vec<PathBuf>, bool)>));
        let win = ctx.window.clone();
        let refresh_slot: Rc<RefCell<Rc<dyn Fn()>>> = Rc::new(RefCell::new(Rc::new(|| {})));

        {
            let list = list.clone();
            let path_label = path_label.clone();
            let dir_cell = Rc::clone(&dir_cell);
            let clip = Rc::clone(&clip);
            let win = win.clone();
            let refresh_slot_for_assign = Rc::clone(&refresh_slot);
            let refresh_slot = Rc::clone(&refresh_slot);
            let refresh_fn: Rc<dyn Fn()> = Rc::new(move || {
                // Only remove list rows — context-menu Popovers are also children of
                // the ListBox (via set_parent) and destroying them mid-dialog crashes.
                clear_list_rows(&list);
                let dir = dir_cell.borrow().clone();
                path_label.set_text(&dir.display().to_string());
                let mut entries: Vec<PathBuf> = fs::read_dir(&dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .collect();
                entries.sort_by(|a, b| {
                    let ad = a.is_dir();
                    let bd = b.is_dir();
                    bd.cmp(&ad).then(
                        a.file_name()
                            .unwrap_or_default()
                            .cmp(b.file_name().unwrap_or_default()),
                    )
                });
                for path in entries {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if name.starts_with('.') {
                        continue;
                    }
                    let row = gtk::ListBoxRow::new();
                    let icon = if path.is_dir() {
                        "folder-symbolic"
                    } else {
                        "text-x-generic-symbolic"
                    };
                    let h = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                    h.set_margin_start(6);
                    h.set_margin_top(2);
                    h.set_margin_bottom(2);
                    h.append(&gtk::Image::from_icon_name(icon));
                    let label = gtk::Label::new(Some(&name));
                    label.set_halign(gtk::Align::Start);
                    h.append(&label);
                    row.set_child(Some(&h));
                    unsafe {
                        row.set_data("path", path.clone());
                    }

                    let do_refresh = {
                        let slot = Rc::clone(&refresh_slot);
                        Rc::new(move || slot.borrow()()) as Rc<dyn Fn()>
                    };
                    wire_row(
                        &row,
                        &path,
                        &dir_cell,
                        &clip,
                        &do_refresh,
                        &win,
                        &list,
                    );
                    list.append(&row);
                }
            });
            *refresh_slot_for_assign.borrow_mut() = Rc::clone(&refresh_fn);
            refresh_fn();
        }

        let do_refresh = {
            let slot = Rc::clone(&refresh_slot);
            Rc::new(move || slot.borrow()()) as Rc<dyn Fn()>
        };

        {
            let dir_cell = Rc::clone(&dir_cell);
            let refresh = Rc::clone(&do_refresh);
            up_btn.connect_clicked(move |_| {
                let parent = dir_cell
                    .borrow()
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("/"));
                *dir_cell.borrow_mut() = parent;
                refresh();
            });
        }
        {
            let dir_cell = Rc::clone(&dir_cell);
            let refresh = Rc::clone(&do_refresh);
            let win2 = win.clone();
            open_btn.connect_clicked(move |_| {
                let dir_cell = Rc::clone(&dir_cell);
                let refresh = Rc::clone(&refresh);
                let start = dir_cell.borrow().clone();
                gtk_theme::present_file_chooser_at(
                    Some(&win2),
                    "Open Folder",
                    gtk::FileChooserAction::SelectFolder,
                    "Open",
                    None,
                    None,
                    Some(&gio::File::for_path(&start)),
                    move |file| {
                        if let Some(path) = file.and_then(|f| f.path()) {
                            *dir_cell.borrow_mut() = path;
                            refresh();
                        }
                    },
                );
            });
        }
        {
            let follow = Rc::clone(&follow);
            let dir_cell = Rc::clone(&dir_cell);
            let refresh = Rc::clone(&do_refresh);
            let win2 = win.clone();
            follow_btn.connect_toggled(move |b| {
                follow.set(b.is_active());
                // Re-enabling should snap straight to the focused doc's folder.
                if b.is_active() {
                    if let Some(ew) = crate::window::current_from_window(&win2) {
                        let dir = ew.focused_document_dir();
                        if dir.is_dir() && dir_cell.borrow().as_path() != dir.as_path() {
                            *dir_cell.borrow_mut() = dir;
                            refresh();
                        }
                    }
                }
            });
        }
        // Follow the focused document's folder (tab switch / open / view focus),
        // driven by the same window hook that syncs the terminal cwd.
        {
            let follow = Rc::clone(&follow);
            let dir_cell = Rc::clone(&dir_cell);
            let refresh = Rc::clone(&do_refresh);
            if let Some(ew) = crate::window::current_from_window(&ctx.window) {
                *ew.filebrowser_sync.borrow_mut() = Some(Rc::new(move |path: &Path| {
                    if !follow.get() || !path.is_dir() {
                        return;
                    }
                    if dir_cell.borrow().as_path() == path {
                        return;
                    }
                    *dir_cell.borrow_mut() = path.to_path_buf();
                    refresh();
                }) as Rc<dyn Fn(&Path)>);
            }
        }
        {
            let dir_cell = Rc::clone(&dir_cell);
            let refresh = Rc::clone(&do_refresh);
            let win2 = win.clone();
            list.connect_row_activated(move |_, row| {
                let path = row_path(row);
                let Some(path) = path else { return };
                if path.is_dir() {
                    *dir_cell.borrow_mut() = path;
                    refresh();
                } else {
                    crate::window::open_path_in_window(&win2, &path);
                }
            });
        }

        attach_drop_target(&list, Rc::clone(&dir_cell), Rc::clone(&do_refresh));
        attach_drop_target(&scroll, Rc::clone(&dir_cell), Rc::clone(&do_refresh));

        {
            let dir_cell = Rc::clone(&dir_cell);
            let clip = Rc::clone(&clip);
            let refresh = Rc::clone(&do_refresh);
            let win2 = win.clone().upcast::<gtk::Window>();
            let list2 = list.clone();
            let gesture = gtk::GestureClick::new();
            gesture.set_button(gdk::BUTTON_SECONDARY);
            gesture.connect_pressed(move |g, _, x, y| {
                if list2.row_at_y(y as i32).is_some() {
                    return;
                }
                let dest = dir_cell.borrow().clone();
                present_context_menu(
                    &list2,
                    gdk::Rectangle::new(x as i32, y as i32, 1, 1),
                    None,
                    dest,
                    Rc::clone(&clip),
                    Rc::clone(&refresh),
                    win2.clone(),
                    Some(Rc::clone(&dir_cell)),
                );
                g.set_state(gtk::EventSequenceState::Claimed);
            });
            list.add_controller(gesture);
        }

        let notebook = find_side_notebook(&ctx.side_panel);
        if let Some(nb) = notebook {
            let label = gtk::Label::new(Some("File Browser"));
            nb.append_page(&outer, Some(&label));
        } else {
            ctx.side_panel.append(&outer);
        }
        *self.root.borrow_mut() = Some(outer);
        ctx.side_panel.set_visible(true);
    }

    fn deactivate(&mut self) {
        if let Some(win) = self.window.borrow_mut().take() {
            if let Some(ew) = crate::window::current_from_window(&win) {
                *ew.filebrowser_sync.borrow_mut() = None;
            }
        }
        if let Some(root) = self.root.borrow_mut().take() {
            if let Some(parent) = root.parent() {
                if let Some(nb) = parent.downcast_ref::<gtk::Notebook>() {
                    nb.detach_tab(&root);
                }
            }
        }
    }
}

fn row_path(row: &gtk::ListBoxRow) -> Option<PathBuf> {
    unsafe { row.data::<PathBuf>("path").map(|p| p.as_ref().clone()) }
}

/// Remove only [`gtk::ListBoxRow`] children. Popovers parented to the list must stay.
fn clear_list_rows(list: &gtk::ListBox) {
    let mut rows = Vec::new();
    let mut child = list.first_child();
    while let Some(c) = child {
        let next = c.next_sibling();
        if let Ok(row) = c.downcast::<gtk::ListBoxRow>() {
            rows.push(row);
        }
        child = next;
    }
    for row in rows {
        list.remove(&row);
    }
}

fn wire_row(
    row: &gtk::ListBoxRow,
    path: &Path,
    dir_cell: &Rc<RefCell<PathBuf>>,
    clip: &Rc<RefCell<Clip>>,
    refresh: &Rc<dyn Fn()>,
    win: &impl IsA<gtk::Window>,
    list: &gtk::ListBox,
) {
    attach_drag_source(row, path.to_path_buf());

    if path.is_dir() {
        attach_folder_drop(row, path.to_path_buf(), Rc::clone(refresh));
    }

    let path = path.to_path_buf();
    let dir_cell = Rc::clone(dir_cell);
    let clip = Rc::clone(clip);
    let refresh = Rc::clone(refresh);
    let win = win.clone().upcast::<gtk::Window>();
    let list = list.clone();
    let row_weak = row.downgrade();
    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_SECONDARY);
    gesture.connect_pressed(move |g, _, x, y| {
        let Some(row) = row_weak.upgrade() else {
            return;
        };
        list.select_row(Some(&row));
        let dest = dir_cell.borrow().clone();
        let alloc = row.allocation();
        present_context_menu(
            &list,
            gdk::Rectangle::new(alloc.x() + x as i32, alloc.y() + y as i32, 1, 1),
            Some(path.clone()),
            dest,
            Rc::clone(&clip),
            Rc::clone(&refresh),
            win.clone(),
            Some(Rc::clone(&dir_cell)),
        );
        g.set_state(gtk::EventSequenceState::Claimed);
    });
    row.add_controller(gesture);
}

fn present_context_menu(
    anchor: &impl IsA<gtk::Widget>,
    pointing: gdk::Rectangle,
    item: Option<PathBuf>,
    dest_dir: PathBuf,
    clip: Rc<RefCell<Clip>>,
    refresh: Rc<dyn Fn()>,
    win: gtk::Window,
    dir_cell: Option<Rc<RefCell<PathBuf>>>,
) {
    // Parent to the window — never to the ListBox. ListBox refresh walks/removes
    // children; a Popover there caused infinite "Tried to remove non-child" and crashes.
    thread_local! {
        static ACTIVE: RefCell<Option<gtk::PopoverMenu>> = RefCell::new(None);
    }

    let group = gio::SimpleActionGroup::new();
    let menu = gio::Menu::new();
    let mut icons = gtk_theme::IconMenu::new();

    if let Some(ref path) = item {
        {
            let p = path.clone();
            let r = Rc::clone(&refresh);
            let w = win.clone();
            let dir_cell = dir_cell.clone();
            let open = gio::SimpleAction::new("open", None);
            open.connect_activate(move |_, _| {
                if p.is_dir() {
                    if let Some(ref cell) = dir_cell {
                        *cell.borrow_mut() = p.clone();
                        r();
                    }
                } else if let Ok(app_win) = w.clone().downcast::<gtk::ApplicationWindow>() {
                    crate::window::open_path_in_window(&app_win, &p);
                }
            });
            group.add_action(&open);
            icons.append_action(&menu, "Open", "fb.open");
        }

        let edit = gio::Menu::new();
        {
            let p = path.clone();
            let c = Rc::clone(&clip);
            let w = win.clone();
            let copy = gio::SimpleAction::new("copy", None);
            copy.connect_activate(move |_, _| {
                set_clip(&c, vec![p.clone()], false);
                set_gdk_clipboard(w.clone().upcast(), &[p.clone()]);
            });
            group.add_action(&copy);
            icons.append_action(&edit, "Copy", "fb.copy");
        }
        {
            let p = path.clone();
            let c = Rc::clone(&clip);
            let w = win.clone();
            let cut = gio::SimpleAction::new("cut", None);
            cut.connect_activate(move |_, _| {
                set_clip(&c, vec![p.clone()], true);
                set_gdk_clipboard(w.clone().upcast(), &[p.clone()]);
            });
            group.add_action(&cut);
            icons.append_action(&edit, "Cut", "fb.cut");
        }
        menu.append_section(None, &edit);
    }

    let can_paste = clip
        .borrow()
        .as_ref()
        .map(|c| !c.0.is_empty())
        .unwrap_or(false);
    let paste_dest = match &item {
        Some(path) if path.is_dir() => path.clone(),
        _ => dest_dir.clone(),
    };
    {
        let c = Rc::clone(&clip);
        let r = Rc::clone(&refresh);
        let w = win.clone();
        let paste = gio::SimpleAction::new("paste", None);
        paste.set_enabled(can_paste);
        paste.connect_activate(move |_, _| {
            paste_clip(&c, &paste_dest, &w, &r);
        });
        group.add_action(&paste);
        icons.append_action(&menu, "Paste", "fb.paste");
    }

    if let Some(path) = item {
        let danger = gio::Menu::new();
        {
            let p = path.clone();
            let r = Rc::clone(&refresh);
            let w = win.clone();
            let trash = gio::SimpleAction::new("trash", None);
            trash.connect_activate(move |_, _| {
                // Defer past popover teardown before showing a modal dialog.
                let p = p.clone();
                let r = Rc::clone(&r);
                let w = w.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
                    trash_path(&w, &p, r);
                });
            });
            group.add_action(&trash);
            icons.append_action(&danger, "Move to Trash", "fb.trash");
        }
        {
            let r = Rc::clone(&refresh);
            let w = win.clone();
            let delete = gio::SimpleAction::new("delete", None);
            delete.connect_activate(move |_, _| {
                let path = path.clone();
                let r = Rc::clone(&r);
                let w = w.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
                    delete_path(&w, &path, r);
                });
            });
            group.add_action(&delete);
            icons.append(
                &danger,
                "Delete Permanently",
                "fb.delete",
                "edit-delete-symbolic",
            );
        }
        menu.append_section(None, &danger);
    }

    ACTIVE.with(|slot| {
        if let Some(old) = slot.borrow_mut().take() {
            old.popdown();
            old.unparent();
        }

        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        icons.bind_popover(&popover);
        popover.set_has_arrow(false);
        popover.set_autohide(true);
        popover.insert_action_group("fb", Some(&group));

        let rect = if let Some((x, y)) =
            anchor.translate_coordinates(&win, pointing.x() as f64, pointing.y() as f64)
        {
            gdk::Rectangle::new(x as i32, y as i32, pointing.width(), pointing.height())
        } else {
            pointing
        };

        popover.set_parent(&win);
        popover.set_pointing_to(Some(&rect));
        popover.popup();
        *slot.borrow_mut() = Some(popover);
    });
}

fn set_clip(clip: &Rc<RefCell<Clip>>, paths: Vec<PathBuf>, cut: bool) {
    *clip.borrow_mut() = Some((paths, cut));
}

fn set_gdk_clipboard(widget: gtk::Widget, paths: &[PathBuf]) {
    if let Some(provider) = content_for_paths(paths) {
        let _ = widget.clipboard().set_content(Some(&provider));
    }
}

fn content_for_paths(paths: &[PathBuf]) -> Option<gdk::ContentProvider> {
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

fn attach_drag_source(widget: &impl IsA<gtk::Widget>, path: PathBuf) {
    let drag = gtk::DragSource::new();
    drag.set_actions(gdk::DragAction::COPY | gdk::DragAction::MOVE);
    drag.set_exclusive(true);
    drag.connect_prepare(move |_, _, _| content_for_paths(&[path.clone()]));
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

fn attach_drop_target(
    widget: &impl IsA<gtk::Widget>,
    dest_dir: Rc<RefCell<PathBuf>>,
    on_done: Rc<dyn Fn()>,
) {
    let target =
        gtk::DropTarget::new(glib::Type::INVALID, gdk::DragAction::COPY | gdk::DragAction::MOVE);
    target.set_types(&[gdk::FileList::static_type(), gio::File::static_type()]);
    target.set_preload(true);
    target.connect_drop(move |drop_target, value, _x, _y| {
        let dest = dest_dir.borrow().clone();
        if !dest.is_dir() {
            return false;
        }
        let paths = paths_from_drop_value(value);
        if paths.is_empty() {
            return false;
        }
        let move_files = prefer_move(drop_target);
        drop_into(&dest, &paths, move_files);
        on_done();
        true
    });
    widget.add_controller(target);
}

fn attach_folder_drop(widget: &impl IsA<gtk::Widget>, dest: PathBuf, on_done: Rc<dyn Fn()>) {
    let drop =
        gtk::DropTarget::new(glib::Type::INVALID, gdk::DragAction::COPY | gdk::DragAction::MOVE);
    drop.set_types(&[gdk::FileList::static_type(), gio::File::static_type()]);
    drop.set_preload(true);
    drop.connect_drop(move |drop_target, value, _x, _y| {
        if !dest.is_dir() {
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
        drop_into(&dest, &paths, move_files);
        on_done();
        true
    });
    widget.add_controller(drop);
}

fn prefer_move(drop_target: &gtk::DropTarget) -> bool {
    drop_target
        .current_drop()
        .map(|d| {
            let a = d.actions();
            a.contains(gdk::DragAction::MOVE) && !a.contains(gdk::DragAction::COPY)
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

fn drop_into(dest_dir: &Path, paths: &[PathBuf], move_files: bool) {
    for src in paths {
        let Some(name) = src.file_name() else {
            continue;
        };
        let dest = uniquify_path(dest_dir, &name.to_string_lossy());
        if move_files {
            if fs::rename(src, &dest).is_err() {
                if let Err(e) = copy_path(src, &dest) {
                    eprintln!("gtk-edit filebrowser: copy failed: {e}");
                    continue;
                }
                let _ = remove_path(src);
            }
        } else if let Err(e) = copy_path(src, &dest) {
            eprintln!("gtk-edit filebrowser: copy failed: {e}");
        }
    }
}

fn paste_clip(clip: &Rc<RefCell<Clip>>, dest_dir: &Path, win: &gtk::Window, refresh: &Rc<dyn Fn()>) {
    let Some((paths, cut)) = clip.borrow_mut().take() else {
        return;
    };
    if !dest_dir.is_dir() {
        show_error(win, "Paste failed", "Destination is not a folder.");
        *clip.borrow_mut() = Some((paths, cut));
        return;
    }
    drop_into(dest_dir, &paths, cut);
    if !cut {
        // Keep copy on clipboard for repeated paste.
        *clip.borrow_mut() = Some((paths, false));
    }
    refresh();
}

fn trash_path(win: &gtk::Window, path: &Path, refresh: Rc<dyn Fn()>) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());
    let path = path.to_path_buf();
    let win = win.clone();
    let parent = win.clone();
    confirm_dialog(
        Some(&parent),
        "Move to Trash",
        &format!("Move “{name}” to the Trash?"),
        "Move to Trash",
        move |ok| {
            if !ok {
                return;
            }
            let file = gio::File::for_path(&path);
            if let Err(e) = file.trash(None::<&gio::Cancellable>) {
                show_error(&win, "Could not move to Trash", &format!("{}: {e}", path.display()));
            }
            refresh();
        },
    );
}

fn delete_path(win: &gtk::Window, path: &Path, refresh: Rc<dyn Fn()>) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());
    let path = path.to_path_buf();
    let win = win.clone();
    let parent = win.clone();
    confirm_dialog(
        Some(&parent),
        "Delete Permanently",
        &format!("Permanently delete “{name}”? This cannot be undone."),
        "Delete",
        move |ok| {
            if !ok {
                return;
            }
            if let Err(e) = remove_path(&path) {
                show_error(&win, "Could not delete", &format!("{}: {e}", path.display()));
            }
            refresh();
        },
    );
}

fn remove_path(path: &Path) -> Result<(), String> {
    // Prefer GIO so Dropbox / FUSE mounts behave; fall back to std::fs.
    let file = gio::File::for_path(path);
    match file.delete(None::<&gio::Cancellable>) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Directories need recursive delete.
            if path.is_dir() {
                fs::remove_dir_all(path).map_err(|err| err.to_string())
            } else {
                Err(format!("{e}"))
            }
        }
    }
}

fn copy_path(src: &Path, dest: &Path) -> Result<(), String> {
    if src.is_dir() {
        fs::create_dir_all(dest).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            copy_path(&entry.path(), &dest.join(entry.file_name()))?;
        }
    } else {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(src, dest).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn uniquify_path(dest_dir: &Path, name: &str) -> PathBuf {
    let candidate = dest_dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for i in 1..10_000 {
        let candidate = dest_dir.join(format!("{stem} ({i}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dest_dir.join(format!("{stem}-copy{ext}"))
}

fn confirm_dialog(
    parent: Option<&impl IsA<gtk::Window>>,
    title: &str,
    detail: &str,
    confirm_label: &str,
    cb: impl FnOnce(bool) + 'static,
) {
    let dialog = gtk::AlertDialog::builder()
        .modal(true)
        .message(title)
        .detail(detail)
        .buttons(["Cancel", confirm_label])
        .default_button(1)
        .cancel_button(0)
        .build();
    dialog.choose(
        parent.map(|w| w.upcast_ref()),
        None::<&gio::Cancellable>,
        move |res| {
            cb(matches!(res, Ok(1)));
        },
    );
}

fn show_error(parent: &impl IsA<gtk::Window>, title: &str, detail: &str) {
    let dialog = gtk::AlertDialog::builder()
        .modal(true)
        .message(title)
        .detail(detail)
        .buttons(["OK"])
        .build();
    dialog.show(Some(parent.upcast_ref()));
}

fn find_side_notebook(side: &gtk::Box) -> Option<gtk::Notebook> {
    let mut child = side.first_child();
    while let Some(c) = child {
        if let Ok(nb) = c.clone().downcast::<gtk::Notebook>() {
            return Some(nb);
        }
        if let Some(box_) = c.downcast_ref::<gtk::Box>() {
            if let Some(nb) = find_side_notebook(box_) {
                return Some(nb);
            }
        }
        child = c.next_sibling();
    }
    child = side.first_child();
    while let Some(c) = child {
        let next = c.next_sibling();
        if let Ok(nb) = c.downcast::<gtk::Notebook>() {
            return Some(nb);
        }
        child = next;
    }
    None
}

pub fn factory() -> (PluginInfo, crate::plugin::activatable::PluginFactory) {
    let i = info(
        "filebrowser",
        "File Browser",
        "A simple file browser pane for opening files.",
    );
    make_factory(i.clone(), move || {
        Box::new(FileBrowserPlugin {
            info: i.clone(),
            root: RefCell::new(None),
            current_dir: RefCell::new(PathBuf::from("/")),
            window: RefCell::new(None),
        })
    })
}
