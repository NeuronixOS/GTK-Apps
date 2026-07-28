#!/usr/bin/env python3
"""GTK app to paste JSON events and import them into Google Calendar."""

from __future__ import annotations

import sys
import threading
from datetime import datetime, timedelta
from pathlib import Path

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import GLib, Gtk

_THEME_PY = Path(__file__).resolve().parent.parent / "gtk-theme" / "python"
if str(_THEME_PY) not in sys.path:
    sys.path.insert(0, str(_THEME_PY))
import gtk_theme  # noqa: E402

from calendar_import.day_label import format_day_label, format_today_label
from calendar_import.google_client import (
    authenticate,
    has_credentials_file,
    insert_events,
    is_authenticated,
)
from calendar_import.parse_event_text import ParsedEvent, TZ
from calendar_import.parse_json import parse_json_text

HOUR_LABELS = [
    "12 AM",
    "1 AM",
    "2 AM",
    "3 AM",
    "4 AM",
    "5 AM",
    "6 AM",
    "7 AM",
    "8 AM",
    "9 AM",
    "10 AM",
    "11 AM",
    "12 PM",
    "1 PM",
    "2 PM",
    "3 PM",
    "4 PM",
    "5 PM",
    "6 PM",
    "7 PM",
    "8 PM",
    "9 PM",
    "10 PM",
    "11 PM",
]
MINUTE_LABELS = ["00", "15", "30", "45"]
# (label, minutes) — default is 1 hour at index 3
LENGTH_OPTIONS: list[tuple[str, int]] = [
    ("15 min", 15),
    ("30 min", 30),
    ("45 min", 45),
    ("1 hour", 60),
    ("1.5 hours", 90),
    ("2 hours", 120),
    ("3 hours", 180),
]
DEFAULT_LENGTH_INDEX = 3


def _format_event(event: ParsedEvent) -> str:
    day = format_day_label(event.start)
    if event.all_day:
        return f"{day} — All day: {event.summary}"
    start = event.start.strftime("%I:%M %p").lstrip("0")
    end = event.end.strftime("%I:%M %p").lstrip("0")
    return f"{day} — {start} - {end}: {event.summary}"


def _next_quarter_hour(now: datetime | None = None) -> tuple[int, int]:
    """Return (hour_index 0-23, minute_index 0-3) for the next :00/:15/:30/:45."""
    current = now if now is not None else datetime.now(TZ)
    if current.tzinfo is None:
        current = current.replace(tzinfo=TZ)
    else:
        current = current.astimezone(TZ)

    rounded = current.replace(second=0, microsecond=0)
    remainder = rounded.minute % 15
    if remainder or rounded.second or current.microsecond:
        rounded += timedelta(minutes=(15 - remainder) % 15 or 15)
    if rounded <= current:
        rounded += timedelta(minutes=15)

    return rounded.hour, rounded.minute // 15


def _dropdown(labels: list[str], selected: int = 0) -> Gtk.DropDown:
    dropdown = Gtk.DropDown.new_from_strings(labels)
    dropdown.set_selected(max(0, min(selected, len(labels) - 1)))
    return dropdown


class CalendarImportApp(Gtk.Application):
    def __init__(self) -> None:
        super().__init__(application_id="org.neuronix.GtkMeetings")
        self._import_thread: threading.Thread | None = None
        self._auth_thread: threading.Thread | None = None
        self._quick_thread: threading.Thread | None = None

    def do_activate(self) -> None:
        window = Gtk.ApplicationWindow(application=self, title="Calendar Import")
        window.set_default_size(720, 720)

        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        root.set_margin_top(12)
        root.set_margin_bottom(12)
        root.set_margin_start(12)
        root.set_margin_end(12)
        window.set_child(root)

        today_label = format_today_label()
        header = Gtk.Label()
        header.set_markup(
            f"<b>Import events for {today_label}</b>\n"
            "<span size='small'>Target: authenticated Google primary calendar</span>"
        )
        header.set_halign(Gtk.Align.START)
        root.append(header)

        self._build_quick_add(root)

        paste_label = Gtk.Label(label="Or paste JSON events:", halign=Gtk.Align.START)
        root.append(paste_label)

        paste_scroll = Gtk.ScrolledWindow()
        paste_scroll.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)
        paste_scroll.set_vexpand(True)
        self._paste_view = Gtk.TextView()
        self._paste_view.set_monospace(True)
        self._paste_view.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
        paste_scroll.set_child(self._paste_view)
        root.append(paste_scroll)

        button_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self._auth_button = Gtk.Button(label="Authenticate")
        self._preview_button = Gtk.Button(label="Preview")
        self._import_button = Gtk.Button(label="Import")
        self._import_button.add_css_class("suggested-action")
        button_box.append(self._auth_button)
        button_box.append(self._preview_button)
        button_box.append(self._import_button)
        root.append(button_box)

        log_label = Gtk.Label(label="Status:", halign=Gtk.Align.START)
        root.append(log_label)

        log_scroll = Gtk.ScrolledWindow()
        log_scroll.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)
        log_scroll.set_min_content_height(180)
        self._log_view = Gtk.TextView()
        self._log_view.set_editable(False)
        self._log_view.set_monospace(True)
        self._log_view.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
        log_scroll.set_child(self._log_view)
        root.append(log_scroll)

        self._auth_button.connect("clicked", self._on_authenticate)
        self._preview_button.connect("clicked", self._on_preview)
        self._import_button.connect("clicked", self._on_import)
        self._quick_add_button.connect("clicked", self._on_quick_add)
        self._quick_name.connect("activate", self._on_quick_add)

        self._refresh_auth_state()
        self._log("Ready. Quick-add an event, or paste JSON then Preview / Import.")
        header = Gtk.HeaderBar()
        header.set_show_title_buttons(True)
        header.set_title_widget(Gtk.Label(label="Calendar Import"))
        window.set_titlebar(header)
        gtk_theme.attach_profile_menu(
            window,
            header,
            about_name="GTK Meetings",
            about_comments="Paste JSON events and import them into Google Calendar.",
        )
        window.present()

    def _build_quick_add(self, root: Gtk.Box) -> None:
        hour_idx, minute_idx = _next_quarter_hour()

        section = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        label = Gtk.Label(label="Quick add today:", halign=Gtk.Align.START)
        section.append(label)

        row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        row.set_hexpand(True)

        self._quick_name = Gtk.Entry()
        self._quick_name.set_placeholder_text("Meeting name")
        self._quick_name.set_hexpand(True)
        row.append(self._quick_name)

        self._quick_hour = _dropdown(HOUR_LABELS, hour_idx)
        self._quick_hour.set_tooltip_text("Start hour")
        row.append(self._quick_hour)

        colon = Gtk.Label(label=":")
        row.append(colon)

        self._quick_minute = _dropdown(MINUTE_LABELS, minute_idx)
        self._quick_minute.set_tooltip_text("Start minute")
        row.append(self._quick_minute)

        self._quick_length = _dropdown(
            [label for label, _ in LENGTH_OPTIONS],
            DEFAULT_LENGTH_INDEX,
        )
        self._quick_length.set_tooltip_text("Meeting length")
        row.append(self._quick_length)

        self._quick_add_button = Gtk.Button(label="Add")
        self._quick_add_button.add_css_class("suggested-action")
        row.append(self._quick_add_button)

        section.append(row)
        root.append(section)

    def _set_buttons_sensitive(self, enabled: bool) -> None:
        self._auth_button.set_sensitive(enabled)
        self._preview_button.set_sensitive(enabled)
        self._import_button.set_sensitive(enabled)
        self._quick_add_button.set_sensitive(enabled)
        self._quick_name.set_sensitive(enabled)
        self._quick_hour.set_sensitive(enabled)
        self._quick_minute.set_sensitive(enabled)
        self._quick_length.set_sensitive(enabled)

    def _get_pasted_text(self) -> str:
        buffer = self._paste_view.get_buffer()
        start, end = buffer.get_bounds()
        return buffer.get_text(start, end, False)

    def _log(self, message: str) -> None:
        buffer = self._log_view.get_buffer()
        timestamp = datetime.now().strftime("%H:%M:%S")
        buffer.insert(buffer.get_end_iter(), f"[{timestamp}] {message}\n")

    def _clear_log(self) -> None:
        self._log_view.get_buffer().set_text("")

    def _refresh_auth_state(self) -> None:
        if not has_credentials_file():
            self._log("credentials.json not found in ~/.config/gtk-apps/gtk-meetings/.")
            self._auth_button.set_sensitive(True)
            return

        if is_authenticated():
            self._log("Google Calendar authentication is active.")
            self._auth_button.set_label("Re-authenticate")
        else:
            self._log("Not authenticated yet. Click Authenticate.")
            self._auth_button.set_label("Authenticate")

    def _on_authenticate(self, *_args) -> None:
        if self._auth_thread and self._auth_thread.is_alive():
            return

        self._set_buttons_sensitive(False)
        self._log("Starting OAuth flow...")

        def worker() -> None:
            try:
                authenticate()
                GLib.idle_add(self._log, "Authentication successful.")
            except Exception as exc:
                GLib.idle_add(self._log, f"Authentication failed: {exc}")
            finally:
                GLib.idle_add(self._set_buttons_sensitive, True)
                GLib.idle_add(self._refresh_auth_state)

        self._auth_thread = threading.Thread(target=worker, daemon=True)
        self._auth_thread.start()

    def _on_preview(self, *_args) -> None:
        try:
            parsed = parse_json_text(self._get_pasted_text())
        except Exception as exc:
            self._log(f"Preview failed: {exc}")
            return

        days = ", ".join(parsed.day_labels)
        self._log(f"Preview: {len(parsed.events)} event(s) for {days}")
        for index, event in enumerate(parsed.events, start=1):
            self._log(f"  {index}. {_format_event(event)}")

    def _on_import(self, *_args) -> None:
        if self._import_thread and self._import_thread.is_alive():
            return

        try:
            parsed = parse_json_text(self._get_pasted_text())
        except Exception as exc:
            self._log(f"Import failed: {exc}")
            return

        if not is_authenticated():
            self._log("Import failed: not authenticated.")
            return

        self._set_buttons_sensitive(False)
        days = ", ".join(parsed.day_labels)
        self._log(f"Importing {len(parsed.events)} event(s) for {days}...")

        def worker() -> None:
            results = insert_events(parsed.events)
            success_count = 0
            for event, result in results:
                if isinstance(result, Exception):
                    GLib.idle_add(
                        self._log,
                        f"Failed: {_format_event(event)} ({result})",
                    )
                else:
                    success_count += 1
                    GLib.idle_add(
                        self._log,
                        f"Created: {_format_event(event)}",
                    )
                    GLib.idle_add(self._log, f"  Link: {result}")

            GLib.idle_add(
                self._log,
                f"Import complete: {success_count}/{len(results)} succeeded.",
            )
            GLib.idle_add(self._set_buttons_sensitive, True)

        self._import_thread = threading.Thread(target=worker, daemon=True)
        self._import_thread.start()

    def _quick_event_from_ui(self) -> ParsedEvent:
        summary = self._quick_name.get_text().strip()
        if not summary:
            raise ValueError("Enter a meeting name.")

        hour = int(self._quick_hour.get_selected())
        minute = int(MINUTE_LABELS[self._quick_minute.get_selected()])
        length_idx = int(self._quick_length.get_selected())
        duration_minutes = LENGTH_OPTIONS[length_idx][1]

        now = datetime.now(TZ)
        start = now.replace(hour=hour, minute=minute, second=0, microsecond=0)
        end = start + timedelta(minutes=duration_minutes)
        return ParsedEvent(summary=summary, start=start, end=end, all_day=False)

    def _on_quick_add(self, *_args) -> None:
        if self._quick_thread and self._quick_thread.is_alive():
            return

        try:
            event = self._quick_event_from_ui()
        except Exception as exc:
            self._log(f"Quick add failed: {exc}")
            return

        if not is_authenticated():
            self._log("Quick add failed: not authenticated.")
            return

        self._set_buttons_sensitive(False)
        self._log(f"Adding: {_format_event(event)}...")

        def worker() -> None:
            results = insert_events([event])
            _, result = results[0]
            if isinstance(result, Exception):
                GLib.idle_add(self._log, f"Failed: {_format_event(event)} ({result})")
            else:
                GLib.idle_add(self._log, f"Created: {_format_event(event)}")
                GLib.idle_add(self._log, f"  Link: {result}")
                GLib.idle_add(self._quick_name.set_text, "")
            GLib.idle_add(self._set_buttons_sensitive, True)

        self._quick_thread = threading.Thread(target=worker, daemon=True)
        self._quick_thread.start()


def main() -> int:
    app = CalendarImportApp()
    return app.run(None)


if __name__ == "__main__":
    raise SystemExit(main())
