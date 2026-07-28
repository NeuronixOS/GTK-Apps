"""Folders tab: one sample image or video per Photos folder with folder name beneath."""

from __future__ import annotations

import os
import threading

from gi.repository import GLib, Gtk

from .favorites import generate_favorite_title
from .folder_cache import get_folder_previews_cached, get_or_create_thumbnail_path
from .thumbnailer import create_placeholder_thumbnail

# Compact grid: many folders visible at once (other tabs stay at 200px).
FOLDER_THUMBNAIL_SIZE = 72
FOLDER_ITEMS_PER_PAGE = 600
# Limit parallel ffmpeg/decode work; cached JPEG loads are cheap.
MAX_THUMBNAIL_WORKERS = 4


class FolderPreviewWidget(Gtk.FlowBoxChild):
    """Thumbnail for one Photos folder: sample media with folder title below."""

    def __init__(
        self,
        folder_path: str,
        sample_path: str,
        is_video: bool,
        label: str,
        parent_window,
        thumbnail_size: int = FOLDER_THUMBNAIL_SIZE,
    ):
        super().__init__()
        self.folder_path = folder_path
        self.sample_path = sample_path
        self.is_video = is_video
        self._parent_window = parent_window
        self._thumbnail_size = thumbnail_size

        vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        vbox.set_margin_top(2)
        vbox.set_margin_bottom(2)
        vbox.set_margin_start(2)
        vbox.set_margin_end(2)

        self.image = Gtk.Picture()
        self.image.set_size_request(thumbnail_size, thumbnail_size)
        self.image.set_content_fit(Gtk.ContentFit.COVER)
        self.image.set_can_shrink(True)
        vbox.append(self.image)

        name_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=2)
        name_box.set_halign(Gtk.Align.CENTER)
        try:
            icon_name = "video-x-generic" if is_video else "image-x-generic"
            icon = Gtk.Image.new_from_icon_name(icon_name)
            if icon.get_gicon() is None:
                icon = Gtk.Image.new_from_icon_name("video" if is_video else "image")
            icon.set_pixel_size(10)
            name_box.append(icon)
        except Exception:
            pass

        folder_label = Gtk.Label(label=label)
        folder_label.set_max_width_chars(14)
        folder_label.set_ellipsize(3)
        folder_label.set_wrap(True)
        folder_label.set_justify(Gtk.Justification.CENTER)
        name_box.append(folder_label)
        vbox.append(name_box)

        self.set_child(vbox)
        self.set_css_classes(["thumbnail-item"])
        self.set_tooltip_text(folder_path)

        gesture = Gtk.GestureClick()
        gesture.set_button(1)
        gesture.connect("pressed", self._on_pressed)
        self.add_controller(gesture)

    def set_thumbnail_file(self, path: str | None):
        """Load from cached JPEG on disk (GTK loads lazily — lower RAM)."""
        if path and os.path.isfile(path):
            self.image.set_filename(path)
        else:
            self.image.set_pixbuf(
                create_placeholder_thumbnail(self._thumbnail_size)
            )

    def _on_pressed(self, gesture, n_press, x, y):
        if n_press != 1:
            return
        self.add_css_class("selected")

        click_mouse = None
        try:
            from .window import _mouse_screen_position
            click_mouse = _mouse_screen_position()
        except Exception:
            pass

        def open_file(cx=x, cy=y, mouse=click_mouse):
            from .window import open_file_with_chromium, open_image_with_constraints

            if self.is_video:
                open_file_with_chromium(
                    self.sample_path,
                    self._parent_window,
                    click_widget=self,
                    click_x=cx,
                    click_y=cy,
                    click_mouse=mouse,
                )
            else:
                open_image_with_constraints(
                    self.sample_path, self._parent_window
                )
            GLib.timeout_add(300, lambda: self.remove_css_class("selected"))

        GLib.timeout_add(100, open_file)


class FoldersPanel:
    """One sample thumbnail per folder on the Photos drive."""

    def __init__(
        self,
        parent_window: Gtk.Window,
        photo_drive: str,
        project_root: str,
        threads: list,
    ):
        self._parent_window = parent_window
        self.photo_drive = photo_drive
        self.project_root = project_root
        self._threads = threads
        self.flowbox: Gtk.FlowBox | None = None
        self._status_label: Gtk.Label | None = None
        self._all_previews: list = []
        self._filter_text = ""
        self._filtered_previews: list = []
        self._current_page = 1
        self._items_per_page = FOLDER_ITEMS_PER_PAGE
        self._scanning = False
        self._pending_thumbnails = 0
        self._spinner: Gtk.Spinner | None = None
        self._thumb_semaphore = threading.Semaphore(MAX_THUMBNAIL_WORKERS)

    def build(self) -> Gtk.Widget:
        container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        container.set_hexpand(True)
        container.set_vexpand(True)

        header = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        header.set_margin_start(8)
        header.set_margin_end(8)
        header.set_margin_top(8)

        title = Gtk.Label(
            label="One preview per folder on the Photos drive (folder name below each thumbnail)"
        )
        title.set_halign(Gtk.Align.START)
        title.set_wrap(True)
        title.add_css_class("title-4")
        header.append(title)

        filter_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        filter_box.set_margin_top(4)
        filter_label = Gtk.Label(label="Filter folders:")
        filter_label.set_halign(Gtk.Align.START)
        self._search_entry = Gtk.SearchEntry()
        self._search_entry.set_placeholder_text("Search by folder name or path…")
        self._search_entry.set_hexpand(True)
        self._search_entry.connect("search-changed", self._on_search_changed)
        filter_box.append(filter_label)
        filter_box.append(self._search_entry)
        header.append(filter_box)

        pagination_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self._prev_btn = Gtk.Button(label="Previous")
        self._prev_btn.connect("clicked", self._on_prev_clicked)
        self._prev_btn.set_sensitive(False)
        pagination_box.append(self._prev_btn)
        self._page_label = Gtk.Label(label="Page 1 of 1")
        pagination_box.append(self._page_label)
        self._next_btn = Gtk.Button(label="Next")
        self._next_btn.connect("clicked", self._on_next_clicked)
        self._next_btn.set_sensitive(False)
        pagination_box.append(self._next_btn)
        header.append(pagination_box)

        status_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        status_box.set_margin_top(4)
        self._spinner = Gtk.Spinner()
        self._spinner.start()
        status_box.append(self._spinner)
        self._status_label = Gtk.Label(label="Waiting: scanning folders on Photos drive…")
        self._status_label.set_halign(Gtk.Align.START)
        self._status_label.set_wrap(True)
        self._status_label.set_hexpand(True)
        status_box.append(self._status_label)
        header.append(status_box)
        container.append(header)

        self.flowbox = Gtk.FlowBox()
        self.flowbox.set_selection_mode(Gtk.SelectionMode.NONE)
        # Large value = effectively unlimited per row (GTK4 rejects 0)
        self.flowbox.set_max_children_per_line(9999)
        self.flowbox.set_column_spacing(4)
        self.flowbox.set_row_spacing(4)
        self.flowbox.set_homogeneous(False)
        self.flowbox.set_hexpand(True)
        self.flowbox.set_vexpand(True)

        scrolled = Gtk.ScrolledWindow()
        scrolled.set_child(self.flowbox)
        scrolled.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)
        scrolled.set_hexpand(True)
        scrolled.set_vexpand(True)
        container.append(scrolled)

        self._set_loading_ui(True, "Waiting: scanning folders on Photos drive…")
        return container

    def _set_loading_ui(self, loading: bool, message: str | None = None):
        self._scanning = loading
        if message is not None and self._status_label:
            self._status_label.set_text(message)
        if self._spinner:
            if loading:
                self._spinner.start()
                self._spinner.set_visible(True)
            else:
                self._spinner.stop()
                self._spinner.set_visible(False)
        sensitive = not loading
        self._search_entry.set_sensitive(sensitive)
        self._prev_btn.set_sensitive(sensitive and self._current_page > 1)
        self._next_btn.set_sensitive(sensitive)

    def load(self):
        self._set_loading_ui(True, "Waiting: scanning folders on Photos drive…")
        thread = threading.Thread(target=self._scan_folders)
        thread.daemon = True
        self._threads.append(thread)
        thread.start()

    def _scan_folders(self):
        previews = get_folder_previews_cached(self.photo_drive)
        GLib.idle_add(self._populate, previews)

    def _populate(self, previews: list):
        self._all_previews = previews
        n = len(previews)
        self._set_loading_ui(
            True,
            f"Waiting: preparing {n} folder{'s' if n != 1 else ''} (newest first)…",
        )
        self._apply_filters(reset_page=True)
        return False

    def _apply_filters(self, reset_page: bool = True):
        if self.flowbox is None:
            return

        filtered = list(self._all_previews)
        if self._filter_text:
            needle = self._filter_text.lower()
            filtered = [
                item
                for item in filtered
                if needle in item[0].lower()
                or needle in generate_favorite_title(item[0], self.photo_drive).lower()
            ]
        self._filtered_previews = filtered

        if reset_page:
            self._current_page = 1

        total_items = len(filtered)
        total_pages = (
            (total_items + self._items_per_page - 1) // self._items_per_page
            if total_items > 0
            else 1
        )
        if self._current_page > total_pages:
            self._current_page = total_pages
        if self._current_page < 1:
            self._current_page = 1

        start = (self._current_page - 1) * self._items_per_page
        end = min(start + self._items_per_page, total_items)
        page_items = filtered[start:end]

        for child in list(self.flowbox):
            self.flowbox.remove(child)

        self._pending_thumbnails = len(page_items)
        if page_items:
            self._set_loading_ui(
                True,
                f"Waiting: loading thumbnails for page {self._current_page} "
                f"({len(page_items)} folders)…",
            )
        else:
            self._finish_loading()

        for folder_path, sample_path, is_video, newest_mtime in page_items:
            label = generate_favorite_title(folder_path, self.photo_drive)
            widget = FolderPreviewWidget(
                folder_path,
                sample_path,
                is_video,
                label,
                self._parent_window,
            )
            self.flowbox.append(widget)
            try:
                sample_mtime = os.path.getmtime(sample_path)
            except OSError:
                sample_mtime = 0.0
            thread = threading.Thread(
                target=self._generate_thumbnail,
                args=(
                    widget,
                    folder_path,
                    sample_path,
                    sample_mtime,
                    newest_mtime,
                    is_video,
                ),
            )
            thread.daemon = True
            self._threads.append(thread)
            thread.start()

        self._page_label.set_text(
            f"Page {self._current_page} of {total_pages} ({total_items} folders)"
        )
        if not self._scanning:
            self._prev_btn.set_sensitive(self._current_page > 1)
            self._next_btn.set_sensitive(self._current_page < total_pages)

    def _finish_loading(self):
        self._set_loading_ui(False)
        if self._status_label:
            n = len(self._all_previews)
            drive = self.photo_drive or "(not configured)"
            self._status_label.set_text(
                f"{n} folder{'s' if n != 1 else ''} with media under {drive} "
                "(sorted by newest file; thumbnails under ~/.cache/gtk-apps/gtk-photos/)"
            )
        total_items = len(self._filtered_previews)
        total_pages = (
            (total_items + self._items_per_page - 1) // self._items_per_page
            if total_items > 0
            else 1
        )
        self._prev_btn.set_sensitive(self._current_page > 1)
        self._next_btn.set_sensitive(self._current_page < total_pages)

    def _on_thumbnail_ready(self):
        self._pending_thumbnails -= 1
        if self._pending_thumbnails <= 0:
            self._finish_loading()
        elif self._status_label:
            self._status_label.set_text(
                f"Waiting: loading thumbnails… "
                f"({self._pending_thumbnails} remaining on this page)"
            )
        return False

    def _on_search_changed(self, entry):
        self._filter_text = entry.get_text()
        self._apply_filters(reset_page=True)

    def _on_prev_clicked(self, _button):
        if self._current_page > 1:
            self._current_page -= 1
            self._apply_filters(reset_page=False)

    def _on_next_clicked(self, _button):
        total_items = len(self._filtered_previews)
        total_pages = (
            (total_items + self._items_per_page - 1) // self._items_per_page
            if total_items > 0
            else 1
        )
        if self._current_page < total_pages:
            self._current_page += 1
            self._apply_filters(reset_page=False)

    def _generate_thumbnail(
        self,
        widget: FolderPreviewWidget,
        folder_path: str,
        sample_path: str,
        sample_mtime: float,
        newest_mtime: float,
        is_video: bool,
    ):
        thumb_path = None
        with self._thumb_semaphore:
            thumb_path = get_or_create_thumbnail_path(
                folder_path,
                sample_path,
                sample_mtime,
                newest_mtime,
                is_video,
                FOLDER_THUMBNAIL_SIZE,
            )

        def apply():
            widget.set_thumbnail_file(thumb_path)
            self._on_thumbnail_ready()

        GLib.idle_add(apply)
