"""Raw text editor page."""

from __future__ import annotations

from typing import Callable

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gtk

try:
    gi.require_version("GtkSource", "5")
    from gi.repository import GtkSource

    HAS_SOURCE = True
except (ValueError, ImportError):
    HAS_SOURCE = False


class RawPage(Gtk.Box):
    def __init__(self, store, on_change: Callable[[], None] | None = None):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        self.store = store
        self.on_change = on_change
        self._rel: str | None = None
        self._updating = False

        bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.path_label = Gtk.Label(xalign=0)
        self.path_label.set_hexpand(True)
        self.path_label.add_css_class("dim-label")
        bar.append(self.path_label)

        reload_btn = Gtk.Button(label="Revert file")
        reload_btn.connect("clicked", self._on_revert)
        bar.append(reload_btn)
        self.append(bar)

        scrolled = Gtk.ScrolledWindow()
        scrolled.set_vexpand(True)
        scrolled.set_hexpand(True)
        scrolled.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)

        if HAS_SOURCE:
            self.buffer = GtkSource.Buffer()
            self.view = GtkSource.View.new_with_buffer(self.buffer)
            self.view.set_show_line_numbers(True)
            self.view.set_monospace(True)
            lang = GtkSource.LanguageManager.get_default().get_language("ini")
            if lang:
                self.buffer.set_language(lang)
        else:
            self.buffer = Gtk.TextBuffer()
            self.view = Gtk.TextView.new_with_buffer(self.buffer)
            self.view.set_monospace(True)

        self.view.set_wrap_mode(Gtk.WrapMode.NONE)
        self.buffer.connect("changed", self._on_buffer)
        scrolled.set_child(self.view)
        self.append(scrolled)

        empty = Gtk.Label(label="Select a file from a section or the Files list.")
        empty.add_css_class("dim-label")
        self._empty = empty
        self.append(empty)

    def open_file(self, rel: str) -> None:
        self._rel = rel
        self.path_label.set_text(rel)
        self._updating = True
        text = self.store.get_text(rel)
        self.buffer.set_text(text)
        self._updating = False
        self.view.set_visible(True)
        self._empty.set_visible(False)
        parent = self.view.get_parent()
        if parent:
            parent.set_visible(True)

    def _on_buffer(self, *_a) -> None:
        if self._updating or not self._rel:
            return
        start = self.buffer.get_start_iter()
        end = self.buffer.get_end_iter()
        self.store.set_text(self._rel, self.buffer.get_text(start, end, True))
        if self.on_change:
            self.on_change()

    def _on_revert(self, *_a) -> None:
        if not self._rel:
            return
        self.store.discard([self._rel])
        self.open_file(self._rel)
        if self.on_change:
            self.on_change()

    def refresh_if_current(self) -> None:
        if self._rel:
            self.open_file(self._rel)
