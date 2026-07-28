"""Websites tab: URL grid with manually assigned thumbnails (GTK, opens links in default browser)."""

from __future__ import annotations

import uuid
import webbrowser
from pathlib import Path

from gi.repository import GdkPixbuf, GLib, Gio, Gtk

from .folder_picker import show_downloads_thumbnail_dialog
from .thumbnailer import THUMBNAIL_SIZE
from . import websites_store as store
from . import websites_thumbnails as wt


def _open_url_in_browser(url: str) -> None:
    try:
        Gio.AppInfo.launch_default_for_uri(url, None)
    except GLib.Error:
        webbrowser.open(url)


class WebsiteLinkWidget(Gtk.FlowBoxChild):
    def __init__(
        self,
        link: dict,
        thumb_path: Path | None,
        missing_icon: Path,
        on_open,
        on_pick_file,
        on_delete,
    ):
        super().__init__()
        self.link_id = link["id"]
        self.url = link["url"]
        self._on_open = on_open
        self._on_pick_file = on_pick_file
        self._on_delete = on_delete

        vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        vbox.set_margin_top(8)
        vbox.set_margin_bottom(8)
        vbox.set_margin_start(8)
        vbox.set_margin_end(8)

        self.picture = Gtk.Picture()
        self.picture.set_size_request(THUMBNAIL_SIZE, THUMBNAIL_SIZE)
        self.picture.set_content_fit(Gtk.ContentFit.COVER)
        self.picture.set_can_shrink(False)
        self._set_thumbnail(thumb_path, missing_icon)

        pick_btn = Gtk.Button(icon_name="go-up-symbolic")
        pick_btn.set_tooltip_text("Select image file for thumbnail")
        pick_btn.set_halign(Gtk.Align.END)
        pick_btn.set_valign(Gtk.Align.END)
        pick_btn.add_css_class("flat")
        pick_btn.connect("clicked", self._on_pick_clicked)

        delete_btn = Gtk.Button(icon_name="user-trash-symbolic")
        delete_btn.set_tooltip_text("Delete this link")
        delete_btn.set_halign(Gtk.Align.START)
        delete_btn.set_valign(Gtk.Align.START)
        delete_btn.add_css_class("flat")
        delete_btn.add_css_class("destructive-action")
        delete_btn.connect("clicked", self._on_delete_clicked)

        overlay = Gtk.Overlay()
        overlay.set_child(self.picture)
        overlay.add_overlay(pick_btn)
        overlay.add_overlay(delete_btn)
        vbox.append(overlay)

        label = Gtk.Label(label=wt.link_label(self.url))
        label.set_max_width_chars(24)
        label.set_ellipsize(3)  # Pango.EllipsizeMode.END
        label.set_wrap(True)
        label.set_justify(Gtk.Justification.CENTER)
        vbox.append(label)

        self.set_child(vbox)
        self.set_tooltip_text(self.url)

        gesture = Gtk.GestureClick()
        gesture.set_button(1)
        gesture.connect("pressed", self._on_thumb_pressed)
        self.picture.add_controller(gesture)

    def _set_thumbnail(self, thumb_path: Path | None, missing_icon: Path):
        if thumb_path and thumb_path.is_file():
            try:
                pixbuf = GdkPixbuf.Pixbuf.new_from_file_at_scale(
                    str(thumb_path),
                    THUMBNAIL_SIZE,
                    THUMBNAIL_SIZE,
                    True,
                )
                self.picture.set_pixbuf(pixbuf)
                return
            except GLib.Error:
                pass
        if missing_icon.is_file():
            try:
                pixbuf = GdkPixbuf.Pixbuf.new_from_file_at_scale(
                    str(missing_icon),
                    THUMBNAIL_SIZE,
                    THUMBNAIL_SIZE,
                    True,
                )
                self.picture.set_pixbuf(pixbuf)
                return
            except GLib.Error:
                pass
        self.picture.set_pixbuf(None)

    def update_thumbnail(self, thumb_path: Path | None, missing_icon: Path):
        self._set_thumbnail(thumb_path, missing_icon)

    def _on_thumb_pressed(self, _gesture, _n_press, _x, _y):
        self._on_open(self.url)

    def _on_pick_clicked(self, _btn):
        self._on_pick_file(self.link_id)

    def _on_delete_clicked(self, _btn):
        self._on_delete(self.link_id, self.url)


class WebsitesPanel:
    def __init__(self, parent_window: Gtk.Window, source_root: str, project_root: str):
        self.window = parent_window
        self.source_root = Path(source_root)
        self.project_root = Path(project_root)
        store.migrate_from_legacy_if_needed()
        self.paths = store.store_paths()

    def build(self) -> Gtk.Widget:
        container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        container.set_hexpand(True)
        container.set_vexpand(True)

        toolbar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        toolbar.set_margin_start(8)
        toolbar.set_margin_end(8)
        toolbar.set_margin_top(8)

        url_label = Gtk.Label(label="URL:")
        toolbar.append(url_label)
        self.url_entry = Gtk.Entry()
        self.url_entry.set_placeholder_text("https://…")
        self.url_entry.set_hexpand(True)
        self.url_entry.connect("activate", lambda *_: self._on_add_clicked(None))
        toolbar.append(self.url_entry)

        add_btn = Gtk.Button(label="Add link")
        add_btn.connect("clicked", self._on_add_clicked)
        toolbar.append(add_btn)

        container.append(toolbar)

        self.status_label = Gtk.Label(label="")
        self.status_label.set_halign(Gtk.Align.START)
        self.status_label.set_margin_start(8)
        self.status_label.set_margin_end(8)
        container.append(self.status_label)

        self.flowbox = Gtk.FlowBox()
        self.flowbox.set_selection_mode(Gtk.SelectionMode.NONE)
        self.flowbox.set_max_children_per_line(5)
        self.flowbox.set_column_spacing(8)
        self.flowbox.set_row_spacing(8)
        self.flowbox.set_hexpand(True)
        self.flowbox.set_vexpand(True)

        scrolled = Gtk.ScrolledWindow()
        scrolled.set_child(self.flowbox)
        scrolled.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)
        scrolled.set_hexpand(True)
        scrolled.set_vexpand(True)
        container.append(scrolled)

        self._update_status()
        self.refresh_grid()
        return container

    def _update_status(self):
        self.status_label.set_label(
            "Thumbnails are set manually (↑ = pick an image file). Click a thumbnail to open the URL."
        )

    def _load_store(self) -> dict:
        return store.load_store(self.paths["links"])

    def _save_store(self, data: dict) -> None:
        store.save_store(data, self.paths["links"], self.paths["overrides"])

    def _thumb_path(self, url: str) -> Path:
        return self.paths["img"] / f"{wt.url_slug(url)}.jpg"

    def refresh_grid(self):
        while child := self.flowbox.get_first_child():
            self.flowbox.remove(child)

        data = self._load_store()
        missing = self.paths["missing_svg"]
        for item in data["links"]:
            tp = self._thumb_path(item["url"])
            thumb = tp if tp.is_file() else None
            w = WebsiteLinkWidget(
                item,
                thumb,
                missing,
                on_open=_open_url_in_browser,
                on_pick_file=self._pick_file_for_link,
                on_delete=self._confirm_delete_link,
            )
            self.flowbox.append(w)

    def _on_add_clicked(self, _btn):
        url = self.url_entry.get_text().strip()
        if not url.startswith("http://") and not url.startswith("https://"):
            self._show_message("Enter a valid http(s) URL.")
            return
        entry = {
            "id": uuid.uuid4().hex[:12],
            "url": url,
            "folder_override": None,
            "thumbnail_locked": False,
        }
        data = self._load_store()
        data["links"].append(entry)
        self._save_store(data)
        self.url_entry.set_text("")
        self.refresh_grid()

    def _find_link(self, link_id: str) -> dict | None:
        for item in self._load_store()["links"]:
            if item["id"] == link_id:
                return item
        return None

    def _pick_file_for_link(self, link_id: str):
        show_downloads_thumbnail_dialog(
            self.window,
            on_selected=lambda path: self._apply_thumbnail_file(link_id, path),
        )

    def _apply_thumbnail_file(self, link_id: str, file_path: str):
        path = Path(file_path)
        if not path.is_file() or path.suffix.lower() not in wt.IMAGE_EXTS:
            self._show_message("Choose a JPG, PNG, GIF, or WebP image.")
            return

        link = self._find_link(link_id)
        if not link:
            return

        thumb_path = self._thumb_path(link["url"])
        thumb_path.parent.mkdir(parents=True, exist_ok=True)
        if not wt.write_thumbnail(path, thumb_path):
            self._show_message(
                "Could not save thumbnail.\n\n"
                "Check that ImageMagick (convert) is installed and the file is readable."
            )
            return

        data = self._load_store()
        for item in data["links"]:
            if item["id"] == link_id:
                item["thumbnail_locked"] = True
                break
        self._save_store(data)
        self.refresh_grid()

    def _confirm_delete_link(self, link_id: str, url: str):
        label = wt.link_label(url)
        dialog = Gtk.MessageDialog(
            transient_for=self.window,
            message_type=Gtk.MessageType.WARNING,
            buttons=Gtk.ButtonsType.NONE,
            text=f"Delete this website link?\n\n{label}",
            secondary_text=url,
        )
        dialog.add_button("Cancel", Gtk.ResponseType.CANCEL)
        dialog.add_button("Delete", Gtk.ResponseType.OK)
        dialog.set_default_response(Gtk.ResponseType.CANCEL)

        def on_response(d, response):
            d.destroy()
            if response == Gtk.ResponseType.OK:
                self._delete_link(link_id)

        dialog.connect("response", on_response)
        dialog.present()

    def _delete_link(self, link_id: str):
        data = self._load_store()
        removed_url = None
        new_links = []
        for item in data["links"]:
            if item["id"] == link_id:
                removed_url = item["url"]
            else:
                new_links.append(item)
        if removed_url is None:
            return
        data["links"] = new_links
        self._save_store(data)

        thumb_path = self._thumb_path(removed_url)
        if thumb_path.is_file():
            try:
                thumb_path.unlink()
            except OSError:
                pass

        self.refresh_grid()

    def _show_message(self, text: str):
        dialog = Gtk.MessageDialog(
            transient_for=self.window,
            message_type=Gtk.MessageType.INFO,
            buttons=Gtk.ButtonsType.OK,
            text=text,
        )
        dialog.connect("response", lambda d, _r: d.destroy())
        dialog.present()
