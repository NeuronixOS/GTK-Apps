"""Typed field widgets for gtk-configs."""

from __future__ import annotations

from typing import Callable

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
from gi.repository import Gdk, Gtk

from lib import colors_fmt
from lib.schema import FieldDef, get_field_value, set_field_value


def _rgba_to_gdk(c: colors_fmt.RGBA) -> Gdk.RGBA:
    r = Gdk.RGBA()
    r.red, r.green, r.blue, r.alpha = c.r, c.g, c.b, c.a
    return r


def _gdk_to_rgba(g: Gdk.RGBA) -> colors_fmt.RGBA:
    return colors_fmt.RGBA(g.red, g.green, g.blue, g.alpha)


class FieldRow(Gtk.Box):
    def __init__(
        self,
        store,
        field: FieldDef,
        on_change: Callable[[], None] | None = None,
    ):
        super().__init__(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        self.store = store
        self.field = field
        self.on_change = on_change
        self._updating = False

        label = Gtk.Label(label=field.label, xalign=0)
        label.set_size_request(200, -1)
        label.set_hexpand(False)
        self.append(label)

        self._widget_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        self._widget_box.set_hexpand(True)
        self.append(self._widget_box)

        self._build()
        self.refresh()

    def _notify(self) -> None:
        if self.on_change and not self._updating:
            self.on_change()

    def _build(self) -> None:
        kind = self.field.kind
        if kind == "color":
            self.color_btn = Gtk.ColorDialogButton()
            dialog = Gtk.ColorDialog()
            dialog.set_with_alpha(True)
            self.color_btn.set_dialog(dialog)
            self.color_btn.connect("notify::rgba", self._on_color)
            self.hex_entry = Gtk.Entry()
            self.hex_entry.set_placeholder_text("#rrggbb")
            self.hex_entry.set_width_chars(12)
            self.hex_entry.connect("activate", self._on_hex_activate)
            focus = Gtk.EventControllerFocus()
            focus.connect("leave", lambda *_: self._on_hex_activate(self.hex_entry))
            self.hex_entry.add_controller(focus)
            self._widget_box.append(self.color_btn)
            self._widget_box.append(self.hex_entry)
        elif kind == "int":
            adj = Gtk.Adjustment(
                value=0,
                lower=self.field.min_v if self.field.min_v is not None else -1e6,
                upper=self.field.max_v if self.field.max_v is not None else 1e6,
                step_increment=1,
                page_increment=10,
            )
            self.spin = Gtk.SpinButton()
            self.spin.set_adjustment(adj)
            self.spin.set_digits(0)
            self.spin.connect("value-changed", self._on_spin)
            self._widget_box.append(self.spin)
        elif kind == "float":
            adj = Gtk.Adjustment(
                value=0,
                lower=self.field.min_v if self.field.min_v is not None else -1e6,
                upper=self.field.max_v if self.field.max_v is not None else 1e6,
                step_increment=self.field.step or 0.01,
                page_increment=0.1,
            )
            self.spin = Gtk.SpinButton()
            self.spin.set_adjustment(adj)
            self.spin.set_digits(2)
            self.spin.connect("value-changed", self._on_spin)
            self._widget_box.append(self.spin)
        elif kind == "bool":
            self.switch = Gtk.Switch()
            self.switch.connect("notify::active", self._on_switch)
            self._widget_box.append(self.switch)
        elif kind == "choice":
            self.dropdown = Gtk.DropDown.new_from_strings(self.field.choices or [""])
            self.dropdown.connect("notify::selected", self._on_choice)
            self._widget_box.append(self.dropdown)
        elif kind == "font":
            self.entry = Gtk.Entry()
            self.entry.set_hexpand(True)
            self.entry.connect("activate", self._on_entry)
            focus = Gtk.EventControllerFocus()
            focus.connect("leave", lambda *_: self._on_entry(self.entry))
            self.entry.add_controller(focus)
            self._widget_box.append(self.entry)
        else:
            self.entry = Gtk.Entry()
            self.entry.set_hexpand(True)
            self.entry.connect("activate", self._on_entry)
            focus = Gtk.EventControllerFocus()
            focus.connect("leave", lambda *_: self._on_entry(self.entry))
            self.entry.add_controller(focus)
            self._widget_box.append(self.entry)

    def refresh(self) -> None:
        self._updating = True
        val = get_field_value(self.store, self.field)
        kind = self.field.kind
        if kind == "color":
            raw = (val or "").strip()
            rgba = colors_fmt.parse_color(raw, self.field.color_kind) if raw else None  # type: ignore[arg-type]
            if rgba is None and raw:
                rgba = colors_fmt.parse_color(raw, "auto")
            if rgba is None:
                rgba = colors_fmt.RGBA(0, 0, 0, 1)
            self.color_btn.set_rgba(_rgba_to_gdk(rgba))
            if raw:
                self.hex_entry.set_text(raw)
            elif self.field.color_kind == "fuzzel":
                self.hex_entry.set_text(colors_fmt.format_color(rgba, "fuzzel"))
            elif self.field.color_kind == "hypr":
                self.hex_entry.set_text(colors_fmt.format_color(rgba, "hypr"))
            else:
                self.hex_entry.set_text(rgba.hex6())
        elif kind in ("int", "float"):
            try:
                self.spin.set_value(float(val) if val is not None else 0)
            except (TypeError, ValueError):
                self.spin.set_value(0)
        elif kind == "bool":
            self.switch.set_active(str(val).lower() in ("1", "true", "yes", "on"))
        elif kind == "choice":
            choices = self.field.choices or []
            idx = choices.index(val) if val in choices else 0
            self.dropdown.set_selected(idx)
        else:
            self.entry.set_text(val or "")
        self._updating = False

    def _on_color(self, *_a) -> None:
        if self._updating:
            return
        rgba = _gdk_to_rgba(self.color_btn.get_rgba())
        old = get_field_value(self.store, self.field) or ""
        if old.strip():
            token = colors_fmt.rewrite_token(old.strip(), rgba)
        else:
            kind = self.field.color_kind if self.field.color_kind != "auto" else "hex"
            token = colors_fmt.format_color(rgba, kind)  # type: ignore[arg-type]
        self._updating = True
        self.hex_entry.set_text(token)
        self._updating = False
        set_field_value(self.store, self.field, token)
        self._notify()

    def _on_hex_activate(self, entry: Gtk.Entry) -> None:
        if self._updating:
            return
        text = entry.get_text().strip()
        rgba = colors_fmt.parse_color(text, self.field.color_kind)  # type: ignore[arg-type]
        if rgba is None:
            rgba = colors_fmt.parse_color(text, "auto")
        if rgba is None:
            return
        self._updating = True
        self.color_btn.set_rgba(_rgba_to_gdk(rgba))
        self._updating = False
        set_field_value(self.store, self.field, text)
        # normalize display to stored
        self.refresh()
        self._notify()

    def _on_spin(self, *_a) -> None:
        if self._updating:
            return
        if self.field.kind == "int":
            set_field_value(self.store, self.field, str(int(self.spin.get_value())))
        else:
            set_field_value(self.store, self.field, f"{self.spin.get_value():.2f}")
        self._notify()

    def _on_switch(self, *_a) -> None:
        if self._updating:
            return
        set_field_value(
            self.store, self.field, "true" if self.switch.get_active() else "false"
        )
        self._notify()

    def _on_choice(self, *_a) -> None:
        if self._updating:
            return
        idx = self.dropdown.get_selected()
        choices = self.field.choices or []
        if 0 <= idx < len(choices):
            set_field_value(self.store, self.field, choices[idx])
            self._notify()

    def _on_entry(self, entry: Gtk.Entry) -> None:
        if self._updating:
            return
        set_field_value(self.store, self.field, entry.get_text())
        self._notify()

    def matches_filter(self, q: str) -> bool:
        if not q:
            return True
        blob = f"{self.field.label} {self.field.id} {self.field.key} {self.field.search_tags} {self.field.file}".lower()
        return q.lower() in blob
