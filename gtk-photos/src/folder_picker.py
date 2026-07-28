"""Searchable destination-folder dialog (same UI as New Images / New Videos move)."""

from __future__ import annotations

import os
import threading
from collections.abc import Callable
from pathlib import Path
from typing import Optional

from gi.repository import GLib, Gio, Gtk

from .favorites import generate_favorite_title, get_folders_in_directory


def _on_list_item_setup(_factory, list_item):
    label = Gtk.Label()
    label.set_halign(Gtk.Align.START)
    label.set_margin_start(8)
    label.set_margin_end(8)
    label.set_margin_top(4)
    label.set_margin_bottom(4)
    label.add_css_class("favorite-dialog-item")
    list_item.set_child(label)


def _on_list_item_bind(_factory, list_item):
    label = list_item.get_child()
    string_object = list_item.get_item()
    if string_object:
        label.set_text(string_object.get_string())


def _path_contains_video(folder_path: str) -> bool:
    return "VIDEO" in folder_path.upper()


def _is_under_photo_drive(folder_path: str, photo_drive: str) -> bool:
    try:
        Path(folder_path).resolve().relative_to(Path(photo_drive).resolve())
        return True
    except ValueError:
        return False


def _sanitize_folder_name(name: str) -> str | None:
    name = name.strip()
    if not name or name in (".", ".."):
        return None
    if os.sep in name or "/" in name or "\\" in name:
        return None
    return name


def _create_folder_with_video_subfolder(
    parent_path: str, folder_name: str
) -> tuple[str, str] | None:
    """Create parent/name and parent/name/Video. Returns (folder, video_folder) or None."""
    clean_name = _sanitize_folder_name(folder_name)
    if not clean_name:
        return None
    parent_path = os.path.normpath(parent_path)
    new_folder = os.path.join(parent_path, clean_name)
    video_folder = os.path.join(new_folder, "Video")
    if os.path.exists(new_folder):
        return None
    os.makedirs(video_folder, exist_ok=True)
    return new_folder, video_folder


def _show_create_folder_dialog(
    parent: Gtk.Window,
    photo_drive: str,
    on_created: Callable[[str, str], None],
) -> None:
    """Pick a parent path on the Photos drive, name a folder, create it plus Video/."""
    state = {"parent": photo_drive}

    dialog = Gtk.Dialog(
        title="Create New Folder",
        transient_for=parent,
        modal=True,
    )
    dialog.add_button("Cancel", Gtk.ResponseType.CANCEL)
    dialog.add_button("Create", Gtk.ResponseType.OK)

    content = dialog.get_content_area()
    content.set_spacing(12)
    content.set_margin_start(12)
    content.set_margin_end(12)
    content.set_margin_top(12)
    content.set_margin_bottom(12)

    intro = Gtk.Label(
        label=(
            "Choose where on the Photos drive to create the folder, enter a name, "
            'then click Create. A "Video" subfolder will be created inside it.'
        )
    )
    intro.set_halign(Gtk.Align.START)
    intro.set_wrap(True)
    content.append(intro)

    parent_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
    parent_label = Gtk.Label(label="Parent location:")
    parent_label.set_halign(Gtk.Align.START)
    parent_path_label = Gtk.Label(label=state["parent"])
    parent_path_label.set_halign(Gtk.Align.START)
    parent_path_label.set_wrap(True)
    parent_path_label.set_hexpand(True)
    parent_path_label.set_selectable(True)
    browse_btn = Gtk.Button(label="Browse…")
    parent_row.append(parent_label)
    parent_row.append(parent_path_label)
    parent_row.append(browse_btn)
    content.append(parent_row)

    name_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
    name_label = Gtk.Label(label="New folder name:")
    name_entry = Gtk.Entry()
    name_entry.set_hexpand(True)
    name_entry.set_placeholder_text("e.g. LovelyLilith")
    name_row.append(name_label)
    name_row.append(name_entry)
    content.append(name_row)

    preview_label = Gtk.Label(label="")
    preview_label.set_halign(Gtk.Align.START)
    preview_label.set_wrap(True)
    preview_label.add_css_class("dim-label")
    content.append(preview_label)

    def update_preview(*_args):
        name = _sanitize_folder_name(name_entry.get_text())
        if name:
            preview_label.set_label(
                f"Will create:\n{os.path.join(state['parent'], name)}\n"
                f"{os.path.join(state['parent'], name, 'Video')}"
            )
        else:
            preview_label.set_label("Enter a folder name (no slashes).")

    name_entry.connect("changed", update_preview)
    update_preview()

    def on_browse(_btn):
        file_dialog = Gtk.FileDialog()
        file_dialog.set_title("Choose parent location on Photos drive")
        file_dialog.set_initial_folder(Gio.File.new_for_path(state["parent"]))

        def on_pick_finished(_fd, result):
            try:
                gfile = file_dialog.select_folder_finish(result)
            except GLib.Error:
                return
            if gfile is None:
                return
            path = gfile.get_path()
            if not path or not _is_under_photo_drive(path, photo_drive):
                err = Gtk.MessageDialog(
                    transient_for=dialog,
                    message_type=Gtk.MessageType.ERROR,
                    buttons=Gtk.ButtonsType.OK,
                    text="Parent folder must be inside the Photos drive.",
                )
                err.connect("response", lambda d, _r: d.destroy())
                err.present()
                return

            def apply():
                state["parent"] = os.path.normpath(path)
                parent_path_label.set_label(state["parent"])
                update_preview()
                return False

            GLib.idle_add(apply)

        file_dialog.select_folder(dialog, None, on_pick_finished)

    browse_btn.connect("clicked", on_browse)

    def on_response(_dlg, response_id):
        if response_id != Gtk.ResponseType.OK:
            dialog.destroy()
            return
        parent_path = state["parent"]
        if not _is_under_photo_drive(parent_path, photo_drive):
            err = Gtk.MessageDialog(
                transient_for=dialog,
                message_type=Gtk.MessageType.ERROR,
                buttons=Gtk.ButtonsType.OK,
                text="Parent folder must be inside the Photos drive.",
            )
            err.connect("response", lambda d, _r: d.destroy())
            err.present()
            return
        try:
            created = _create_folder_with_video_subfolder(
                parent_path, name_entry.get_text()
            )
        except OSError as e:
            err = Gtk.MessageDialog(
                transient_for=dialog,
                message_type=Gtk.MessageType.ERROR,
                buttons=Gtk.ButtonsType.OK,
                text=f"Could not create folder:\n{e}",
            )
            err.connect("response", lambda d, _r: d.destroy())
            err.present()
            return
        if created is None:
            name = name_entry.get_text().strip()
            if os.path.exists(os.path.join(parent_path, name)):
                msg = f"A folder named “{name}” already exists at that location."
            else:
                msg = "Enter a valid folder name (no slashes)."
            err = Gtk.MessageDialog(
                transient_for=dialog,
                message_type=Gtk.MessageType.ERROR,
                buttons=Gtk.ButtonsType.OK,
                text=msg,
            )
            err.connect("response", lambda d, _r: d.destroy())
            err.present()
            return
        new_folder, video_folder = created
        dialog.destroy()
        on_created(new_folder, video_folder)

    dialog.connect("response", on_response)
    dialog.present()
    name_entry.grab_focus()


def show_destination_folder_dialog(
    parent: Gtk.Window,
    photo_drive: str,
    *,
    title: str = "Select Destination Folder",
    prompt: str,
    hint: str | None = None,
    ok_label: str = "OK",
    single_click_activate: bool = True,
    on_selected: Callable[[str], None],
    threads: list | None = None,
    folder_filter: Optional[Callable[[str], bool]] = None,
    recent_destinations: list[str] | None = None,
    allow_create_folder: bool = False,
) -> None:
    """Open searchable folder list; calls on_selected with absolute path after dialog closes."""
    if not photo_drive or not os.path.isdir(photo_drive):
        dialog = Gtk.MessageDialog(
            transient_for=parent,
            message_type=Gtk.MessageType.ERROR,
            buttons=Gtk.ButtonsType.OK,
            text=(
                "Photos drive not configured\n\n"
                "Set photo_drive in ~/.config/gtk-apps/gtk-photos/config.json "
                "to a valid directory."
            ),
        )
        dialog.connect("response", lambda d, _r: d.destroy())
        dialog.present()
        return

    dialog = Gtk.Dialog(
        title=title,
        transient_for=parent,
        modal=True,
    )
    dialog.set_default_size(460, 620)
    dialog.add_button("Cancel", Gtk.ResponseType.CANCEL)
    dialog.add_button(ok_label, Gtk.ResponseType.OK)

    content = dialog.get_content_area()
    content.set_spacing(12)
    content.set_margin_start(12)
    content.set_margin_end(12)
    content.set_margin_top(12)
    content.set_margin_bottom(12)

    label = Gtk.Label(label=prompt)
    label.set_halign(Gtk.Align.START)
    label.set_wrap(True)
    content.append(label)

    if hint:
        hint_label = Gtk.Label(label=hint)
        hint_label.set_halign(Gtk.Align.START)
        hint_label.set_wrap(True)
        hint_label.add_css_class("dim-label")
        content.append(hint_label)

    finished = {"done": False}

    def finish_with_folder(folder: dict):
        if finished["done"]:
            return
        finished["done"] = True
        path = folder["path"]
        dialog.destroy()

        def run_callback():
            on_selected(path)
            return False

        GLib.idle_add(run_callback)

    if recent_destinations:
        recent_sorted = sorted(
            recent_destinations,
            key=lambda p: generate_favorite_title(p, photo_drive).upper(),
        )
        recent_section = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        recent_heading = Gtk.Label(label="Recent destinations (click to move):")
        recent_heading.set_halign(Gtk.Align.START)
        recent_heading.add_css_class("dim-label")
        recent_section.append(recent_heading)

        recent_flow = Gtk.FlowBox()
        recent_flow.set_selection_mode(Gtk.SelectionMode.NONE)
        recent_flow.set_max_children_per_line(3)
        recent_flow.set_column_spacing(6)
        recent_flow.set_row_spacing(6)
        recent_flow.set_hexpand(False)
        for path in recent_sorted:
            title = generate_favorite_title(path, photo_drive)
            btn = Gtk.Button(label=title)
            btn.set_tooltip_text(path)
            btn.add_css_class("recent-destination-btn")
            btn.connect(
                "clicked",
                lambda _b, folder_path=path: finish_with_folder(
                    {
                        "path": folder_path,
                        "title": generate_favorite_title(folder_path, photo_drive),
                    }
                ),
            )
            recent_flow.append(btn)
        recent_section.append(recent_flow)
        content.append(recent_section)

    loading_label = Gtk.Label(label="Scanning folders...")
    loading_label.set_halign(Gtk.Align.START)
    content.append(loading_label)

    search_label_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
    search_label = Gtk.Label(label="Search:")
    search_entry = Gtk.SearchEntry()
    search_entry.set_placeholder_text("Search folders by name or path...")
    search_entry.set_sensitive(False)
    search_label_box.append(search_label)
    search_label_box.append(search_entry)
    content.append(search_label_box)

    folders_store = Gtk.StringList()
    folders_selection = Gtk.SingleSelection(model=folders_store)
    folders_list_view = Gtk.ListView(
        model=folders_selection,
        single_click_activate=single_click_activate,
    )

    factory = Gtk.SignalListItemFactory()
    factory.connect("setup", _on_list_item_setup)
    factory.connect("bind", _on_list_item_bind)
    folders_list_view.set_factory(factory)

    filtered_folder_indices: list[int] = []
    all_folders_data: list[dict] = []

    def filter_folders(search_text: str = ""):
        nonlocal filtered_folder_indices, folders_store
        new_store = Gtk.StringList()
        search_lower = search_text.lower().strip()
        filtered_folder_indices = []
        for i, folder in enumerate(all_folders_data):
            if not search_text:
                new_store.append(folder["title"])
                filtered_folder_indices.append(i)
                continue
            title_lower = folder["title"].lower()
            path_lower = folder["path"].lower()
            matches = search_lower in title_lower or search_lower in path_lower
            if not matches:
                path_components = [c.lower() for c in folder["path"].split(os.sep) if c]
                title_components = [c.lower() for c in folder["title"].split("-") if c]
                for component in path_components + title_components:
                    if search_lower in component or component.startswith(search_lower):
                        matches = True
                        break
            if matches:
                new_store.append(folder["title"])
                filtered_folder_indices.append(i)
        folders_selection.set_model(new_store)
        folders_store = new_store

    def on_search_changed(entry):
        filter_folders(entry.get_text())
        folders_selection.unselect_all()

    search_entry.connect("search-changed", on_search_changed)

    create_btn = None
    if allow_create_folder:
        create_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        create_btn = Gtk.Button(label="New folder…")
        create_btn.set_halign(Gtk.Align.START)
        create_row.append(create_btn)
        content.append(create_row)

    def update_dialog_with_folders(folders_list: list[dict]):
        nonlocal all_folders_data
        all_folders_data = folders_list
        if loading_label.get_parent() is content:
            content.remove(loading_label)
        search_entry.set_sensitive(True)
        search_entry.grab_focus()
        filter_folders("")

    def add_created_folders_to_list(folder_paths: list[str], select_path: str | None = None):
        nonlocal all_folders_data
        existing = {f["path"] for f in all_folders_data}
        added = []
        for path in folder_paths:
            norm = os.path.normpath(path)
            if norm in existing or not os.path.isdir(norm):
                continue
            if folder_filter is not None and not folder_filter(norm):
                continue
            entry = {
                "path": norm,
                "title": generate_favorite_title(norm, photo_drive),
            }
            all_folders_data.append(entry)
            existing.add(norm)
            added.append(entry)
        if not added:
            return
        all_folders_data.sort(key=lambda x: x["title"].upper())
        filter_folders(search_entry.get_text())
        if select_path:
            select_norm = os.path.normpath(select_path)
            for i, folder in enumerate(all_folders_data):
                if folder["path"] == select_norm:
                    try:
                        pos = filtered_folder_indices.index(i)
                        folders_selection.set_selected(pos)
                        folders_list_view.scroll_to(
                            pos, Gtk.ListScrollFlags.FOCUS, None
                        )
                    except ValueError:
                        pass
                    break
        selected_path_label.set_text(
            f"Created {len(added)} folder(s) — select one above or click a recent destination."
        )

    if allow_create_folder:
        def on_create_clicked(_btn):
            def on_created(new_folder: str, video_folder: str):
                if folder_filter is not None and folder_filter(video_folder):
                    select_path = video_folder
                else:
                    select_path = new_folder
                add_created_folders_to_list(
                    [new_folder, video_folder],
                    select_path=select_path,
                )

            _show_create_folder_dialog(dialog, photo_drive, on_created)

        create_btn.connect("clicked", on_create_clicked)

    folders_scrolled = Gtk.ScrolledWindow()
    folders_scrolled.set_child(folders_list_view)
    folders_scrolled.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)
    folders_scrolled.set_size_request(420, 360)
    folders_scrolled.set_hexpand(False)
    content.append(folders_scrolled)

    selected_path_label = Gtk.Label(label="No folder selected")
    selected_path_label.set_halign(Gtk.Align.START)
    selected_path_label.set_wrap(True)
    selected_path_label.set_selectable(True)
    content.append(selected_path_label)

    ok_button = dialog.get_widget_for_response(Gtk.ResponseType.OK)
    if ok_button:
        ok_button.set_sensitive(False)

    def folder_at_position(position) -> dict | None:
        if position == Gtk.INVALID_LIST_POSITION:
            return None
        if position >= len(filtered_folder_indices):
            return None
        return all_folders_data[filtered_folder_indices[position]]

    def on_selection_changed(selection, *_args):
        selected = folder_at_position(selection.get_selected())
        if selected:
            selected_path_label.set_text(f"Selected: {selected['path']}")
            if ok_button:
                ok_button.set_sensitive(True)
        else:
            selected_path_label.set_text("No folder selected")
            if ok_button:
                ok_button.set_sensitive(False)

    folders_selection.connect("selection-changed", on_selection_changed)

    def on_folders_activate(_list_view, position):
        selected = folder_at_position(position)
        if selected:
            finish_with_folder(selected)
        else:
            on_selection_changed(folders_selection)

    folders_list_view.connect("activate", on_folders_activate)

    def on_response(_dialog, response_id):
        if finished["done"]:
            return
        if response_id == Gtk.ResponseType.OK:
            selected = folder_at_position(folders_selection.get_selected())
            if selected:
                finish_with_folder(selected)
                return
        dialog.destroy()

    dialog.connect("response", on_response)

    def scan_folders():
        all_folders = get_folders_in_directory(photo_drive)
        folders_list = []
        for folder_path in all_folders:
            if not os.path.isdir(folder_path):
                continue
            if folder_filter is not None and not folder_filter(folder_path):
                continue
            folders_list.append(
                {
                    "path": folder_path,
                    "title": generate_favorite_title(folder_path, photo_drive),
                }
            )
        folders_list.sort(key=lambda x: x["title"].upper())
        GLib.idle_add(update_dialog_with_folders, folders_list)

    dialog.present()
    GLib.idle_add(search_entry.grab_focus)

    scan_thread = threading.Thread(target=scan_folders, daemon=True)
    if threads is not None:
        threads.append(scan_thread)
    scan_thread.start()


def show_thumbnail_folder_dialog(
    parent: Gtk.Window,
    photo_drive: str,
    on_selected: Callable[[str], None],
) -> None:
    """Folder picker for website thumbnails (select, then Set Thumbnail or double-click)."""
    show_destination_folder_dialog(
        parent,
        photo_drive,
        title="Pick Thumbnail Folder",
        prompt="Choose a folder on the Photos drive for this link's thumbnail.",
        hint=(
            "Select a folder, then click Set Thumbnail — or double-click a folder to apply. "
            "A random image from that folder will be used."
        ),
        ok_label="Set Thumbnail",
        single_click_activate=False,
        on_selected=on_selected,
    )


def _image_file_filter() -> Gtk.FileFilter:
    filt = Gtk.FileFilter()
    filt.set_name("Images")
    for mime in ("image/jpeg", "image/png", "image/gif", "image/webp"):
        filt.add_mime_type(mime)
    for pattern in ("*.jpg", "*.jpeg", "*.png", "*.gif", "*.webp"):
        filt.add_pattern(pattern)
    return filt


def show_downloads_thumbnail_dialog(
    parent: Gtk.Window,
    on_selected: Callable[[str], None],
) -> None:
    """Pick an image file from ~/Downloads to use as a website link thumbnail."""
    downloads = Path.home() / "Downloads"
    if not downloads.is_dir():
        dialog = Gtk.MessageDialog(
            transient_for=parent,
            message_type=Gtk.MessageType.ERROR,
            buttons=Gtk.ButtonsType.OK,
            text=f"Downloads folder not found:\n{downloads}",
        )
        dialog.connect("response", lambda d, _r: d.destroy())
        dialog.present()
        return

    file_dialog = Gtk.FileDialog()
    file_dialog.set_title("Select Thumbnail Image")
    file_dialog.set_initial_folder(Gio.File.new_for_path(str(downloads)))
    image_filter = _image_file_filter()
    filters = Gio.ListStore.new(Gtk.FileFilter)
    filters.append(image_filter)
    file_dialog.set_filters(filters)
    file_dialog.set_default_filter(image_filter)

    def on_open_finished(_dialog, result):
        try:
            gfile = file_dialog.open_finish(result)
        except GLib.Error:
            return
        if gfile is None:
            return
        path = gfile.get_path()
        if not path:
            return

        def run_callback():
            on_selected(path)
            return False

        GLib.idle_add(run_callback)

    file_dialog.open(parent, None, on_open_finished)
