"""Main gtk-configs window."""

from __future__ import annotations

import subprocess
from pathlib import Path
from typing import Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
from gi.repository import Gtk

try:
    from gi.repository import Pango
except ImportError:
    Pango = None  # type: ignore

from lib import apply as applymod
from lib import root as rootmod
from lib.schema import SectionDef, build_schema
from lib.store import ConfigStore
from ui.colors_page import ColorsPage
from ui.raw_page import RawPage
from ui.widgets import FieldRow

try:
    import gtk_theme
except ImportError:
    gtk_theme = None  # type: ignore


class ConfigsWindow(Gtk.ApplicationWindow):
    def __init__(self, app: Gtk.Application, store: ConfigStore):
        super().__init__(application=app, title="Configs")
        self.set_default_size(980, 720)
        self.store = store
        self.sections = build_schema()
        self._field_rows: list[FieldRow] = []
        self._section_pages: dict[str, Gtk.Widget] = {}

        header = Gtk.HeaderBar()
        header.set_show_title_buttons(True)
        self._title = Gtk.Label(label="Configs")
        header.set_title_widget(self._title)
        self.set_titlebar(header)
        if gtk_theme is not None:
            try:
                gtk_theme.attach_profile_menu(
                    self,
                    header,
                    about_name="GTK Configs",
                    about_comments="Neuronix config tree editor for Hyprland, Waybar, Fuzzel, Mako, and more.",
                )
            except Exception:
                pass

        self.root_label = Gtk.Label(xalign=0)
        self.root_label.add_css_class("dim-label")
        if Pango is not None:
            self.root_label.set_ellipsize(Pango.EllipsizeMode.MIDDLE)
        self.root_label.set_max_width_chars(48)
        header.pack_start(self.root_label)

        open_btn = Gtk.Button(label="Open folder")
        open_btn.connect("clicked", self._open_folder)
        header.pack_end(open_btn)

        reload_btn = Gtk.Button(label="Reload")
        reload_btn.set_tooltip_text("Reload all files from disk (discards unsaved)")
        reload_btn.connect("clicked", self._reload_disk)
        header.pack_end(reload_btn)

        apply_btn = Gtk.Button(label="Apply")
        apply_btn.set_tooltip_text("Save all + reload Hypr/Waybar/Mako if possible")
        apply_btn.add_css_class("suggested-action")
        apply_btn.connect("clicked", self._save_apply)
        header.pack_end(apply_btn)

        save_btn = Gtk.Button(label="Save")
        save_btn.connect("clicked", self._save)
        header.pack_end(save_btn)

        # Body: sidebar + stack
        paned = Gtk.Paned(orientation=Gtk.Orientation.HORIZONTAL)
        paned.set_shrink_start_child(False)
        paned.set_resize_start_child(False)
        self.set_child(paned)

        side = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        side.set_size_request(220, -1)
        side.set_margin_start(8)
        side.set_margin_end(4)
        side.set_margin_top(8)
        side.set_margin_bottom(8)

        self.search = Gtk.SearchEntry()
        self.search.set_placeholder_text("Search…")
        self.search.connect("search-changed", self._on_search)
        side.append(self.search)

        side_scroll = Gtk.ScrolledWindow()
        side_scroll.set_vexpand(True)
        side_scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        self.nav = Gtk.ListBox()
        self.nav.set_selection_mode(Gtk.SelectionMode.SINGLE)
        self.nav.connect("row-selected", self._on_nav)
        side_scroll.set_child(self.nav)
        side.append(side_scroll)

        self.status = Gtk.Label(xalign=0, wrap=True)
        self.status.add_css_class("dim-label")
        side.append(self.status)

        paned.set_start_child(side)

        self.stack = Gtk.Stack()
        self.stack.set_hexpand(True)
        self.stack.set_vexpand(True)
        paned.set_end_child(self.stack)

        # Colors page first
        self.colors_page = ColorsPage(
            store, on_change=self._mark_dirty, on_jump=self.open_raw
        )
        self.stack.add_titled(self._wrap_scroll(self.colors_page), "colors", "Colors")
        self._add_nav_row("colors", "Colors")

        for sec in self.sections:
            page = self._build_section_page(sec)
            self._section_pages[sec.id] = page
            self.stack.add_titled(page, sec.id, sec.title)
            self._add_nav_row(sec.id, sec.title)

        self.raw_page = RawPage(store, on_change=self._mark_dirty)
        self.stack.add_titled(self.raw_page, "raw", "Raw")
        self._add_nav_row("raw", "Raw")

        self._update_root_label()
        self.colors_page.rebuild()
        self._update_files_section()
        self._mark_dirty()
        # select Colors
        self.nav.select_row(self.nav.get_row_at_index(0))

    def _wrap_scroll(self, child: Gtk.Widget) -> Gtk.ScrolledWindow:
        sc = Gtk.ScrolledWindow()
        sc.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        sc.set_child(child)
        return sc

    def _add_nav_row(self, page_id: str, title: str) -> None:
        row = Gtk.ListBoxRow()
        row.set_name(page_id)
        lab = Gtk.Label(label=title, xalign=0)
        lab.set_margin_start(10)
        lab.set_margin_end(10)
        lab.set_margin_top(8)
        lab.set_margin_bottom(8)
        row.set_child(lab)
        row._page_id = page_id  # type: ignore[attr-defined]
        self.nav.append(row)

    def _build_section_page(self, sec: SectionDef) -> Gtk.Widget:
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        outer.set_margin_start(16)
        outer.set_margin_end(16)
        outer.set_margin_top(12)
        outer.set_margin_bottom(16)

        title = Gtk.Label(label=sec.title, xalign=0)
        title.add_css_class("title-2")
        outer.append(title)
        blurb = Gtk.Label(label=sec.blurb, wrap=True, xalign=0)
        blurb.add_css_class("dim-label")
        outer.append(blurb)

        fields_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        fields_box.set_name(f"fields-{sec.id}")
        for fdef in sec.fields:
            row = FieldRow(self.store, fdef, on_change=self._mark_dirty)
            fields_box.append(row)
            self._field_rows.append(row)
        outer.append(fields_box)

        if sec.raw_files or sec.id == "files":
            raw_frame = Gtk.Frame(label="Raw files")
            raw_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
            raw_box.set_margin_start(8)
            raw_box.set_margin_end(8)
            raw_box.set_margin_top(8)
            raw_box.set_margin_bottom(8)
            raw_frame.set_child(raw_box)
            outer.append(raw_frame)
            # store ref for files section refresh
            setattr(outer, "_raw_box", raw_box)
            setattr(outer, "_sec_id", sec.id)
            for rel in sec.raw_files:
                self._add_raw_button(raw_box, rel)

        return self._wrap_scroll(outer)

    def _add_raw_button(self, box: Gtk.Box, rel: str) -> None:
        exists = self.store.abs(rel).is_file() or rel in getattr(self.store, "_texts", {})
        btn = Gtk.Button(label=rel + ("" if exists else " (missing)"))
        btn.set_halign(Gtk.Align.START)
        if not exists and not self.store.abs(rel).exists():
            btn.set_sensitive(False)
        btn.connect("clicked", lambda *_a, r=rel: self.open_raw(r))
        box.append(btn)

    def _update_files_section(self) -> None:
        # Find files section page and refill
        for child in self.stack:
            # Gtk.Stack children are scrolled windows
            pass
        page = self._section_pages.get("files")
        if not page:
            return
        # unwrap scroll
        inner = page.get_child() if hasattr(page, "get_child") else None
        if inner is None:
            return
        raw_box = getattr(inner, "_raw_box", None)
        if raw_box is None:
            return
        while True:
            ch = raw_box.get_first_child()
            if ch is None:
                break
            raw_box.remove(ch)
        for path in rootmod.list_text_files(self.store.root):
            rel = self.store.rel(path)
            self._add_raw_button(raw_box, rel)
        # secrets note
        sec_dir = self.store.root / "secrets"
        if sec_dir.is_dir():
            note = Gtk.Label(
                label="secrets/ is present — open via file manager; tokens are not edited here.",
                wrap=True,
                xalign=0,
            )
            note.add_css_class("dim-label")
            raw_box.append(note)

    def open_raw(self, rel: str) -> None:
        self.raw_page.open_file(rel)
        self.stack.set_visible_child_name("raw")
        for i in range(100):
            row = self.nav.get_row_at_index(i)
            if row is None:
                break
            if getattr(row, "_page_id", None) == "raw":
                self.nav.select_row(row)
                break

    def _on_nav(self, _lb, row) -> None:
        if row is None:
            return
        pid = getattr(row, "_page_id", None)
        if pid:
            self.stack.set_visible_child_name(pid)
            if pid == "colors":
                self.colors_page.rebuild()

    def _on_search(self, *_a) -> None:
        q = self.search.get_text().strip()
        for row in self._field_rows:
            row.set_visible(row.matches_filter(q))
        if q:
            self.colors_page.set_filter(q)
            self.colors_page.rebuild()

    def _update_root_label(self) -> None:
        self.root_label.set_text(str(self.store.root))
        dirty = " • modified" if self.store.is_dirty() else ""
        self._title.set_text(f"Configs{dirty}")

    def _mark_dirty(self) -> None:
        n = len(self.store.dirty_files())
        if n:
            self.status.set_text(f"{n} file(s) modified: " + ", ".join(self.store.dirty_files()[:5]))
        else:
            self.status.set_text("No unsaved changes")
        self._update_root_label()

    def _save(self, *_a) -> None:
        saved = self.store.save()
        self._mark_dirty()
        self.status.set_text("Saved: " + (", ".join(saved) if saved else "(nothing)"))
        self.colors_page.rebuild()

    def _save_apply(self, *_a) -> None:
        saved = self.store.save()
        notes = applymod.apply_for_files(saved)
        self._mark_dirty()
        self.status.set_text(" | ".join(["Saved " + ",".join(saved or ["—"])] + notes))
        self.colors_page.rebuild()

    def _reload_disk(self, *_a) -> None:
        self.store.reload_from_disk()
        for row in self._field_rows:
            row.refresh()
        self.colors_page.rebuild()
        self.raw_page.refresh_if_current()
        self._update_files_section()
        self._mark_dirty()
        self.status.set_text("Reloaded from disk")

    def _open_folder(self, *_a) -> None:
        path = str(self.store.root)
        for cmd in (["xdg-open", path], ["gio", "open", path]):
            try:
                subprocess.Popen(cmd, start_new_session=True)
                return
            except OSError:
                continue
