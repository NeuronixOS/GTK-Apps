#!/usr/bin/env python3
"""WorkTimeZones - GTK4 app for tracking coworker local times."""

from __future__ import annotations

import json
import sys
from dataclasses import dataclass, field
from datetime import datetime, timezone as dt_timezone
from pathlib import Path
from zoneinfo import ZoneInfo, available_timezones

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, Gio, GLib, Gtk

_THEME_PY = Path(__file__).resolve().parent.parent / "gtk-theme" / "python"
if str(_THEME_PY) not in sys.path:
    sys.path.insert(0, str(_THEME_PY))
import gtk_theme  # noqa: E402

APP_ID = "org.neuronix.GtkWorktimezone"
APP_DIR = Path(__file__).resolve().parent
APP_NAME = "gtk-worktimezone"


def config_dir() -> Path:
    """``~/.config/gtk-apps/gtk-worktimezone`` (created if missing)."""
    import os

    base = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    path = base / "gtk-apps" / APP_NAME
    path.mkdir(parents=True, exist_ok=True)
    return path


def datastore_path() -> Path:
    """Canonical data file under XDG config, with legacy fallbacks on first run."""
    dest = config_dir() / "work_time_zones_data.json"
    if dest.exists():
        return dest
    for legacy in (
        APP_DIR / "work_time_zones_data.json",
        Path.home() / ".config" / APP_NAME / "work_time_zones_data.json",
        Path.home() / ".config" / "work_time_zones" / "config.json",
    ):
        if legacy.is_file():
            try:
                dest.write_text(legacy.read_text(encoding="utf-8"), encoding="utf-8")
                return dest
            except OSError:
                continue
    return dest


DATASTORE_PATH = datastore_path()


@dataclass
class AppState:
    timezone_names: dict[str, list[str]] = field(default_factory=dict)
    selected_timezones: list[str] = field(default_factory=list)
    default_timezone: str | None = None

    def ensure_timezone(self, timezone: str) -> None:
        self.timezone_names.setdefault(timezone, [])
        if timezone not in self.selected_timezones:
            self.selected_timezones.append(timezone)


class TimezoneColumn(Gtk.Frame):
    def __init__(
        self,
        timezone: str,
        app_state: AppState,
        on_remove_timezone,
        on_state_changed,
        on_default_changed,
        removable: bool,
    ):
        super().__init__()
        self.timezone = timezone
        self.app_state = app_state
        self.on_remove_timezone = on_remove_timezone
        self.on_state_changed = on_state_changed
        self.on_default_changed = on_default_changed
        self.name_rows: list[tuple[Gtk.Box, Gtk.Entry]] = []

        self.set_margin_start(6)
        self.set_margin_end(6)
        self.set_margin_top(6)
        self.set_margin_bottom(6)

        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        outer.set_margin_start(10)
        outer.set_margin_end(10)
        outer.set_margin_top(10)
        outer.set_margin_bottom(10)
        self.set_child(outer)

        title_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        outer.append(title_row)

        zone_label = Gtk.Label(label=timezone)
        zone_label.set_xalign(0.0)
        zone_label.add_css_class("heading")
        zone_label.set_hexpand(True)
        title_row.append(zone_label)

        self.default_check = Gtk.CheckButton(label="Default")
        if app_state.default_timezone == timezone:
            self.default_check.set_active(True)
        self.default_check.connect("toggled", self._on_default_toggled)
        title_row.append(self.default_check)

        if removable:
            remove_btn = Gtk.Button(label="Remove")
            remove_btn.add_css_class("destructive-action")
            remove_btn.connect("clicked", self._on_remove_timezone_clicked)
            title_row.append(remove_btn)

        self.time_label = Gtk.Label(label="")
        self.time_label.set_xalign(1.0)
        self.time_label.set_halign(Gtk.Align.FILL)
        self.time_label.set_hexpand(True)
        self.time_label.add_css_class("title-3")
        outer.append(self.time_label)

        names_header = Gtk.Label(label="Coworkers")
        names_header.set_xalign(0.0)
        names_header.add_css_class("caption")
        outer.append(names_header)

        self.names_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        outer.append(self.names_box)

        add_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        outer.append(add_row)

        self.new_name_entry = Gtk.Entry()
        self.new_name_entry.set_placeholder_text("Add name")
        self.new_name_entry.connect("activate", self._on_add_name_clicked)
        add_row.append(self.new_name_entry)

        add_name_btn = Gtk.Button(label="Add")
        add_name_btn.connect("clicked", self._on_add_name_clicked)
        add_row.append(add_name_btn)

        for name in self.app_state.timezone_names.get(self.timezone, []):
            self._append_name_row(name)

        self.update_time()

    def set_default_checked(self, active: bool) -> None:
        self.default_check.set_active(active)

    def _on_default_toggled(self, button: Gtk.CheckButton) -> None:
        self.on_default_changed(self.timezone, button.get_active())

    def update_time(self) -> None:
        now = datetime.now(ZoneInfo(self.timezone))
        self.time_label.set_text(now.strftime("%a %I:%M %p"))

    def _on_remove_timezone_clicked(self, _button: Gtk.Button) -> None:
        self.on_remove_timezone(self.timezone)

    def _on_add_name_clicked(self, _widget) -> None:
        name = self.new_name_entry.get_text().strip()
        if not name:
            return
        self.new_name_entry.set_text("")
        self.app_state.timezone_names.setdefault(self.timezone, []).append(name)
        self._append_name_row(name)
        self.on_state_changed()

    def _append_name_row(self, name: str) -> None:
        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)

        entry = Gtk.Entry()
        entry.set_text(name)
        entry.set_hexpand(True)
        entry.connect("changed", self._on_name_edited)
        row.append(entry)

        remove_btn = Gtk.Button(label="X")
        remove_btn.connect("clicked", self._on_remove_name_clicked, row, entry)
        row.append(remove_btn)

        self.names_box.append(row)
        self.name_rows.append((row, entry))

    def _on_name_edited(self, _entry: Gtk.Entry) -> None:
        self._sync_names_from_rows()
        self.on_state_changed()

    def _on_remove_name_clicked(
        self,
        _button: Gtk.Button,
        row: Gtk.Box,
        _entry: Gtk.Entry,
    ) -> None:
        self.names_box.remove(row)
        self.name_rows = [pair for pair in self.name_rows if pair[0] != row]
        self._sync_names_from_rows()
        self.on_state_changed()

    def _sync_names_from_rows(self) -> None:
        names: list[str] = []
        for _, entry in self.name_rows:
            value = entry.get_text().strip()
            if value:
                names.append(value)
        self.app_state.timezone_names[self.timezone] = names


class WorkTimeZonesWindow(Gtk.ApplicationWindow):
    def __init__(self, app: "WorkTimeZonesApp"):
        super().__init__(application=app, title="🕐 Work Time Zones")
        self.app = app
        self.set_default_size(1200, 320)
        self.columns_by_timezone: dict[str, TimezoneColumn] = {}
        self.view_rows_by_timezone: dict[str, tuple[Gtk.Frame, Gtk.Label, Gtk.Label, Gtk.Label]] = {}
        self._updating_default_checks = False
        self.all_timezones = sorted(available_timezones())

        header = Gtk.HeaderBar()
        title = Gtk.Label(label="🕐 Work Time Zones")
        title.add_css_class("title")
        header.set_title_widget(title)
        header.set_show_title_buttons(True)
        self.set_titlebar(header)
        gtk_theme.attach_profile_menu(
            self,
            header,
            about_name="GTK Work Time Zones",
            about_comments="Coworker timezone tracker for the Neuronix GTK-Apps suite.",
        )

        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        root.set_margin_start(10)
        root.set_margin_end(10)
        root.set_margin_top(10)
        root.set_margin_bottom(10)
        self.set_child(root)

        self.stack = Gtk.Stack()
        self.stack.set_hexpand(True)
        self.stack.set_vexpand(True)

        root.append(self.stack)

        bottom_bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
        bottom_bar.set_halign(Gtk.Align.START)
        root.append(bottom_bar)

        self.edit_toggle = Gtk.ToggleButton(label="Edit")
        self.edit_toggle.add_css_class("flat")
        self.edit_toggle.set_size_request(52, 24)
        self.edit_toggle.connect("toggled", self._on_toggle_panel, "edit")
        bottom_bar.append(self.edit_toggle)

        self.view_toggle = Gtk.ToggleButton(label="View")
        self.view_toggle.add_css_class("flat")
        self.view_toggle.set_size_request(52, 24)
        self.view_toggle.connect("toggled", self._on_toggle_panel, "view")
        bottom_bar.append(self.view_toggle)

        self._build_edit_panel()
        self._build_view_panel()

        for timezone in self.app.state.selected_timezones:
            self._add_timezone_widgets(timezone)

        self._refresh_view_list()
        self._start_clock_updates()
        self._refresh_timezone_matches()
        self.stack.connect("notify::visible-child-name", self._sync_toggle_state)
        self.edit_toggle.set_active(True)
        self._install_view_panel_css()

    def _install_view_panel_css(self) -> None:
        display = Gdk.Display.get_default()
        if display is None:
            return
        css = Gtk.CssProvider()
        css.load_from_data(
            b"""
            frame.view-default-highlight {
              background-color: color-mix(in srgb, @theme_bg_color 88%, white 12%);
              border-radius: 0.75rem;
            }
            label.view-offset-large {
              font-size: 1.65rem;
              font-weight: 800;
            }
            """
        )
        Gtk.StyleContext.add_provider_for_display(
            display,
            css,
            Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION,
        )

    def _build_edit_panel(self) -> None:
        panel = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        panel.set_margin_top(8)

        controls = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        panel.append(controls)

        self.timezone_search_entry = Gtk.Entry()
        self.timezone_search_entry.set_placeholder_text("Search timezone (e.g. New_York)")
        self.timezone_search_entry.set_hexpand(True)
        self.timezone_search_entry.connect("changed", self._on_timezone_search_changed)
        controls.append(self.timezone_search_entry)

        self.matches_model = Gtk.StringList.new([])
        self.timezone_dropdown = Gtk.DropDown(model=self.matches_model)
        self.timezone_dropdown.set_hexpand(True)
        controls.append(self.timezone_dropdown)

        add_timezone_btn = Gtk.Button(label="Add Timezone")
        add_timezone_btn.connect("clicked", self._on_add_timezone_clicked)
        controls.append(add_timezone_btn)

        scroller = Gtk.ScrolledWindow()
        scroller.set_hexpand(True)
        scroller.set_vexpand(True)
        panel.append(scroller)

        self.columns_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.columns_row.set_margin_top(4)
        self.columns_row.set_margin_bottom(4)
        scroller.set_child(self.columns_row)

        self.stack.add_titled(panel, "edit", "Edit")

    def _build_view_panel(self) -> None:
        panel = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        panel.set_margin_top(8)

        scroller = Gtk.ScrolledWindow()
        scroller.set_hexpand(True)
        scroller.set_vexpand(True)
        scroller.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.NEVER)
        panel.append(scroller)

        self.view_list = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.view_list.set_margin_top(6)
        self.view_list.set_margin_bottom(6)
        self.view_list.set_margin_start(4)
        self.view_list.set_margin_end(4)
        scroller.set_child(self.view_list)

        self.stack.add_titled(panel, "view", "View")

    def _on_timezone_search_changed(self, _entry: Gtk.Entry) -> None:
        self._refresh_timezone_matches()

    def _on_toggle_panel(self, button: Gtk.ToggleButton, panel_name: str) -> None:
        if not button.get_active():
            return
        self.stack.set_visible_child_name(panel_name)
        other = self.view_toggle if panel_name == "edit" else self.edit_toggle
        if other.get_active():
            other.set_active(False)

    def _sync_toggle_state(self, _stack, _param) -> None:
        current = self.stack.get_visible_child_name()
        if current == "view":
            if not self.view_toggle.get_active():
                self.view_toggle.set_active(True)
        else:
            if not self.edit_toggle.get_active():
                self.edit_toggle.set_active(True)

    def _refresh_timezone_matches(self) -> None:
        term = self.timezone_search_entry.get_text().strip().lower()
        self.matches_model.splice(0, self.matches_model.get_n_items(), [])

        matches = [tz for tz in self.all_timezones if term in tz.lower()]
        for timezone in matches[:300]:
            self.matches_model.append(timezone)

        if self.matches_model.get_n_items() > 0:
            self.timezone_dropdown.set_selected(0)
        else:
            self.timezone_dropdown.set_selected(Gtk.INVALID_LIST_POSITION)

    def _on_add_timezone_clicked(self, _button: Gtk.Button) -> None:
        idx = self.timezone_dropdown.get_selected()
        if idx == Gtk.INVALID_LIST_POSITION:
            return
        timezone = self.matches_model.get_string(idx)
        if timezone in self.columns_by_timezone:
            return
        self.app.state.ensure_timezone(timezone)
        self._add_timezone_widgets(timezone)
        self._reorder_edit_columns()
        self.app.save_state()
        self._refresh_view_list()

    def _add_timezone_widgets(self, timezone: str) -> None:
        column = TimezoneColumn(
            timezone=timezone,
            app_state=self.app.state,
            on_remove_timezone=self._remove_timezone,
            on_state_changed=self.app.save_state,
            on_default_changed=self._on_default_timezone_changed,
            removable=True,
        )
        self.columns_by_timezone[timezone] = column
        self.columns_row.append(column)
        self._create_view_row(timezone)

    def _remove_timezone(self, timezone: str) -> None:
        column = self.columns_by_timezone.pop(timezone, None)
        if column is None:
            return
        self.columns_row.remove(column)
        if timezone in self.app.state.selected_timezones:
            self.app.state.selected_timezones.remove(timezone)
        self.app.state.timezone_names.pop(timezone, None)
        if self.app.state.default_timezone == timezone:
            self.app.state.default_timezone = None
        self._remove_view_row(timezone)
        self._reorder_edit_columns()
        self.app.save_state()
        self._refresh_view_list()

    def _create_view_row(self, timezone: str) -> None:
        frame = Gtk.Frame()
        frame.set_size_request(260, -1)
        content = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=3)
        content.set_margin_top(8)
        content.set_margin_bottom(8)
        content.set_margin_start(8)
        content.set_margin_end(8)
        frame.set_child(content)

        timezone_label = Gtk.Label(label=timezone)
        timezone_label.set_xalign(0.0)
        timezone_label.set_hexpand(True)
        timezone_label.add_css_class("caption")
        content.append(timezone_label)

        time_label = Gtk.Label(label="")
        time_label.set_xalign(1.0)
        time_label.set_halign(Gtk.Align.FILL)
        time_label.set_hexpand(True)
        time_label.add_css_class("title-1")
        time_label.set_margin_bottom(8)
        content.append(time_label)

        names_label = Gtk.Label(label="")
        names_label.set_xalign(0.0)
        names_label.set_wrap(True)
        names_label.set_use_markup(True)
        content.append(names_label)

        bottom_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=0)
        bottom_row.set_margin_top(2)
        spacer = Gtk.Box()
        spacer.set_hexpand(True)
        bottom_row.append(spacer)

        offset_label = Gtk.Label(label="")
        offset_label.set_xalign(1.0)
        offset_label.set_halign(Gtk.Align.END)
        offset_label.set_valign(Gtk.Align.END)
        offset_label.add_css_class("view-offset-large")
        bottom_row.append(offset_label)
        content.append(bottom_row)

        self.view_rows_by_timezone[timezone] = (frame, offset_label, time_label, names_label)
        self.view_list.append(frame)

    def _remove_view_row(self, timezone: str) -> None:
        row_parts = self.view_rows_by_timezone.pop(timezone, None)
        if row_parts is None:
            return
        frame = row_parts[0]
        self.view_list.remove(frame)

    def _start_clock_updates(self) -> None:
        self._update_all_times()
        GLib.timeout_add_seconds(1, self._update_all_times)

    def _update_all_times(self) -> bool:
        for column in self.columns_by_timezone.values():
            column.update_time()
        self._refresh_view_list()
        return True

    def _on_default_timezone_changed(self, timezone: str, active: bool) -> None:
        if self._updating_default_checks:
            return
        if active:
            self._updating_default_checks = True
            self.app.state.default_timezone = timezone
            for otz, column in self.columns_by_timezone.items():
                if otz != timezone:
                    column.set_default_checked(False)
            self._updating_default_checks = False
            self.app.save_state()
            self._refresh_view_list()
        elif self.app.state.default_timezone == timezone:
            self.app.state.default_timezone = None
            self.app.save_state()
            self._refresh_view_list()

    @staticmethod
    def _hour_offset_vs_default(default_tz: str, other_tz: str) -> str:
        if not default_tz or default_tz == other_tz:
            return ""
        z_def = ZoneInfo(default_tz)
        z_other = ZoneInfo(other_tz)
        utc_now = datetime.now(dt_timezone.utc)
        off_def = utc_now.astimezone(z_def).utcoffset()
        off_other = utc_now.astimezone(z_other).utcoffset()
        if off_def is None or off_other is None:
            return ""
        diff_hours = int(round((off_other - off_def).total_seconds() / 3600.0))
        if diff_hours == 0:
            return ""
        if diff_hours > 0:
            return f"+{diff_hours}"
        return str(diff_hours)

    def _sorted_selected_timezones(self) -> list[str]:
        def key_func(timezone: str) -> tuple[int, int, str]:
            now = datetime.now(ZoneInfo(timezone))
            return (now.hour, now.minute, timezone)

        return sorted(self.app.state.selected_timezones, key=key_func)

    def _reorder_edit_columns(self) -> None:
        ordered = self._sorted_selected_timezones()
        for timezone in ordered:
            column = self.columns_by_timezone.get(timezone)
            if column is None:
                continue
            self.columns_row.remove(column)
            self.columns_row.append(column)

    def _refresh_view_list(self) -> None:
        ordered = self._sorted_selected_timezones()

        for timezone in ordered:
            if timezone not in self.view_rows_by_timezone:
                self._create_view_row(timezone)

            frame, offset_label, time_label, names_label = self.view_rows_by_timezone[timezone]
            now = datetime.now(ZoneInfo(timezone))
            time_label.set_text(now.strftime("%I:%M %p"))

            default_tz = self.app.state.default_timezone
            if default_tz == timezone:
                frame.add_css_class("view-default-highlight")
            else:
                frame.remove_css_class("view-default-highlight")

            offset_text = self._hour_offset_vs_default(default_tz or "", timezone)
            if offset_text:
                offset_label.set_text(offset_text)
            else:
                offset_label.set_text("")

            names = self.app.state.timezone_names.get(timezone, [])
            if names:
                escaped_names = [GLib.markup_escape_text(name) for name in names]
                names_markup = "\n".join(f"<b>{name}</b>" for name in escaped_names)
            else:
                names_markup = "<b>None</b>"
            names_label.set_markup(names_markup)

        for timezone in list(self.view_rows_by_timezone.keys()):
            if timezone not in self.app.state.selected_timezones:
                self._remove_view_row(timezone)

        for timezone in ordered:
            row_parts = self.view_rows_by_timezone.get(timezone)
            if row_parts is None:
                continue
            frame = row_parts[0]
            self.view_list.remove(frame)
            self.view_list.append(frame)

        return True


class WorkTimeZonesApp(Adw.Application):
    def __init__(self):
        super().__init__(application_id=APP_ID, flags=Gio.ApplicationFlags.NON_UNIQUE)
        self.state = self.load_state()
        self.save_state()

        quit_action = Gio.SimpleAction.new("quit", None)
        quit_action.connect("activate", self._on_quit_action)
        self.add_action(quit_action)
        self.set_accels_for_action("app.quit", ["<Control>q"])

    def _on_quit_action(self, _action, _param) -> None:
        self.quit()

    def do_activate(self) -> None:
        if Gdk.Display.get_default() is None:
            print(
                "WorkTimeZones: no display. Set WAYLAND_DISPLAY or DISPLAY.",
                file=sys.stderr,
            )
            self.quit()
            return
        window = WorkTimeZonesWindow(self)
        window.present()

    def load_state(self) -> AppState:
        default = AppState()
        source_path = DATASTORE_PATH
        if not source_path.exists():
            return default
        try:
            data = json.loads(source_path.read_text(encoding="utf-8"))
            timezone_names = data.get("timezone_names", {})
            selected_timezones = data.get("selected_timezones", [])

            valid_timezones = set(available_timezones())
            clean_selected = [tz for tz in selected_timezones if tz in valid_timezones]
            clean_names = {
                tz: [str(name) for name in names if str(name).strip()]
                for tz, names in timezone_names.items()
                if tz in valid_timezones and isinstance(names, list)
            }
            raw_default = data.get("default_timezone")
            default_tz: str | None = None
            if isinstance(raw_default, str) and raw_default in valid_timezones:
                default_tz = raw_default
            state = AppState(
                timezone_names=clean_names,
                selected_timezones=clean_selected,
                default_timezone=default_tz,
            )
            for tz in clean_selected:
                state.timezone_names.setdefault(tz, [])
            if state.default_timezone and state.default_timezone not in clean_selected:
                state.default_timezone = None
            return state
        except Exception:
            return default

    def save_state(self) -> None:
        payload = {
            "timezone_names": self.state.timezone_names,
            "selected_timezones": self.state.selected_timezones,
            "default_timezone": self.state.default_timezone,
        }
        DATASTORE_PATH.parent.mkdir(parents=True, exist_ok=True)
        DATASTORE_PATH.write_text(json.dumps(payload, indent=2), encoding="utf-8")


def main() -> int:
    Adw.init()
    app = WorkTimeZonesApp()
    return app.run(sys.argv)


if __name__ == "__main__":
    sys.exit(main())
