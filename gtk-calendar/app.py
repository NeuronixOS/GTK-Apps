#!/usr/bin/env python3
"""gtk-calendar — GTK4 suite calendar (Google + .ics)."""

from __future__ import annotations

import sys
from pathlib import Path

APP_DIR = Path(__file__).resolve().parent
if str(APP_DIR) not in sys.path:
    sys.path.insert(0, str(APP_DIR))

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gio, Gtk

from src.ics import normalize_ics_url
from src.window import CalendarWindow

APP_ID = "org.neuronix.GtkCalendar"


class CalendarApp(Gtk.Application):
    def __init__(self):
        super().__init__(
            application_id=APP_ID,
            flags=Gio.ApplicationFlags.HANDLES_OPEN,
        )
        self._pending: list[tuple[str, str]] = []

    def do_activate(self):
        win = self.props.active_window
        if win is None:
            win = CalendarWindow(self)
        win.present()
        for kind, value in self._pending:
            self._ingest(win, kind, value)
        self._pending.clear()

    def do_open(self, files, _n_files, _hint):
        self.activate()
        win = self.props.active_window
        for gfile in files:
            path = gfile.get_path()
            uri = gfile.get_uri() or ""
            if path:
                self._ingest(win, "path", path)
            elif uri:
                self._ingest(win, "url", uri)

    def _ingest(self, win, kind: str, value: str) -> None:
        if win is None:
            self._pending.append((kind, value))
            return
        if kind == "path":
            win.add_ics_path(value)
        else:
            win.add_ics_url(normalize_ics_url(value))


def main(argv: list[str] | None = None) -> int:
    return CalendarApp().run(argv if argv is not None else sys.argv)


if __name__ == "__main__":
    raise SystemExit(main())
