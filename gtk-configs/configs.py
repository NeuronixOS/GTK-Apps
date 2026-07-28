#!/usr/bin/env python3
"""gtk-configs — GTK4 editor for Neuronix ~/configs (or --root)."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
from gi.repository import Gtk

APP_DIR = Path(__file__).resolve().parent
if str(APP_DIR) not in sys.path:
    sys.path.insert(0, str(APP_DIR))

_THEME_CANDIDATES = [
    APP_DIR.parent / "gtk-theme" / "python",
    Path("/usr/share/neuronix/gtk-theme/python"),
]
for cand in _THEME_CANDIDATES:
    if cand.is_dir() and str(cand) not in sys.path:
        sys.path.insert(0, str(cand))
        break

from lib.root import add_root_arg, resolve_root  # noqa: E402
from lib.store import ConfigStore  # noqa: E402
from ui.window import ConfigsWindow  # noqa: E402

APP_ID = "org.neuronix.GtkConfigs"


class ConfigsApp(Gtk.Application):
    def __init__(self, root: Path):
        super().__init__(application_id=APP_ID)
        self.root = root
        self.store = ConfigStore(root)

    def do_activate(self):
        if not self.root.is_dir():
            dialog = Gtk.AlertDialog()
            dialog.set_message("Configs root not found")
            dialog.set_detail(
                f"{self.root}\n\nCreate ~/configs or pass --root PATH "
                "(e.g. Neuronix/Build/default/configs)."
            )
            dialog.set_modal(True)
            dialog.set_buttons(["OK"])
            dialog.choose(None, None, lambda *_: self.quit())
            return
        self.store.load_all()
        win = ConfigsWindow(self, self.store)
        win.present()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Neuronix configs editor (GTK4)")
    add_root_arg(parser)
    args, gtk_args = parser.parse_known_args(argv)
    root = resolve_root(args.root)
    # Gtk.Application also parses argv — pass remaining
    app = ConfigsApp(root)
    return app.run([sys.argv[0], *gtk_args])


if __name__ == "__main__":
    raise SystemExit(main())
