"""Aggregated color swatches across config files."""

from __future__ import annotations

from typing import Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
from gi.repository import Gdk, Gtk

from lib import colors_fmt


def _rgba_to_gdk(c: colors_fmt.RGBA) -> Gdk.RGBA:
    r = Gdk.RGBA()
    r.red, r.green, r.blue, r.alpha = c.r, c.g, c.b, c.a
    return r


def _gdk_to_rgba(g: Gdk.RGBA) -> colors_fmt.RGBA:
    return colors_fmt.RGBA(g.red, g.green, g.blue, g.alpha)


class ColorsPage(Gtk.Box):
    def __init__(
        self,
        store,
        on_change: Callable[[], None] | None = None,
        on_jump: Callable[[str], None] | None = None,
    ):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        self.store = store
        self.on_change = on_change
        self.on_jump = on_jump
        self._filter = ""
        self._hits: list[colors_fmt.ColorHit] = []

        hint = Gtk.Label(
            label="Every color token found under the configs root. "
            "Changing a swatch writes back in that file’s native format.",
            wrap=True,
            xalign=0,
        )
        hint.add_css_class("dim-label")
        self.append(hint)

        self.search = Gtk.SearchEntry()
        self.search.set_placeholder_text("Filter colors…")
        self.search.connect("search-changed", self._on_search)
        self.append(self.search)

        scrolled = Gtk.ScrolledWindow()
        scrolled.set_vexpand(True)
        scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        self.list_box = Gtk.ListBox()
        self.list_box.set_selection_mode(Gtk.SelectionMode.NONE)
        scrolled.set_child(self.list_box)
        self.append(scrolled)

    def set_filter(self, q: str) -> None:
        self._filter = q
        self.search.set_text(q)

    def rebuild(self) -> None:
        while True:
            row = self.list_box.get_row_at_index(0)
            if row is None:
                break
            self.list_box.remove(row)

        self._hits = self.store.scan_colors()
        q = (self._filter or self.search.get_text() or "").lower()
        for hit in self._hits:
            blob = f"{hit.path_key} {hit.token} {hit.context}".lower()
            if q and q not in blob:
                continue
            self.list_box.append(self._make_row(hit))

    def _make_row(self, hit: colors_fmt.ColorHit) -> Gtk.ListBoxRow:
        row = Gtk.ListBoxRow()
        box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        box.set_margin_start(8)
        box.set_margin_end(8)
        box.set_margin_top(4)
        box.set_margin_bottom(4)

        rgba = colors_fmt.parse_color(hit.token, hit.kind) or colors_fmt.RGBA(0, 0, 0)
        btn = Gtk.ColorDialogButton()
        dialog = Gtk.ColorDialog()
        dialog.set_with_alpha(True)
        btn.set_dialog(dialog)
        busy = {"v": True}
        btn.set_rgba(_rgba_to_gdk(rgba))
        busy["v"] = False

        def on_color(button, *_a, h=hit):
            if busy["v"]:
                return
            new = _gdk_to_rgba(button.get_rgba())
            self.store.set_color_hit(h, new)
            tok.set_text(colors_fmt.rewrite_token(h.token, new))
            if self.on_change:
                self.on_change()

        btn.connect("notify::rgba", on_color)
        box.append(btn)

        info = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        info.set_hexpand(True)
        path_l = Gtk.Label(label=hit.path_key, xalign=0)
        path_l.add_css_class("caption")
        tok = Gtk.Label(label=hit.token, xalign=0)
        tok.set_selectable(True)
        ctx = Gtk.Label(label=hit.context, xalign=0)
        ctx.add_css_class("dim-label")
        info.append(path_l)
        info.append(tok)
        info.append(ctx)
        box.append(info)

        jump = Gtk.Button(label="Raw")
        jump.connect(
            "clicked",
            lambda *_a, p=hit.path_key: self.on_jump(p) if self.on_jump else None,
        )
        box.append(jump)

        row.set_child(box)
        return row

    def _on_search(self, *_a) -> None:
        self._filter = self.search.get_text()
        self.rebuild()
