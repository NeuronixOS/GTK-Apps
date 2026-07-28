#!/usr/bin/env python3
import json
import subprocess
import sys
from pathlib import Path

import gi

gi.require_version("Gdk", "4.0")
gi.require_version("Gtk", "4.0")
from gi.repository import Gdk, Gio, Gtk

_THEME_PY = Path(__file__).resolve().parent.parent / "gtk-theme" / "python"
if str(_THEME_PY) not in sys.path:
    sys.path.insert(0, str(_THEME_PY))
import gtk_theme  # noqa: E402

APP_ID = "org.neuronix.GtkWorkspaces"
APP_DIR = Path(__file__).resolve().parent
APP_NAME = "gtk-workspaces"


def config_dir() -> Path:
    """``~/.config/gtk-apps/gtk-workspaces`` (created if missing)."""
    import os

    base = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    path = base / "gtk-apps" / APP_NAME
    path.mkdir(parents=True, exist_ok=True)
    return path


def config_path() -> Path:
    """Prefer XDG config; one-shot migrate from the old in-tree / flat paths."""
    dest = config_dir() / "workspaces.json"
    if not dest.exists():
        for legacy in (
            APP_DIR / "workspaces.json",
            Path.home() / ".config" / APP_NAME / "workspaces.json",
        ):
            if legacy.is_file():
                try:
                    dest.write_text(legacy.read_text(encoding="utf-8"), encoding="utf-8")
                    break
                except OSError:
                    continue
    return dest


CONFIG_PATH = config_path()  # resolved at import for existing call sites


class WorkspaceRow(Gtk.ListBoxRow):
    def __init__(self, app, workspace_name: str, on_commands_clicked):
        super().__init__()
        self.app = app
        self.workspace_name = workspace_name
        self.on_commands_clicked_cb = on_commands_clicked

        row_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        row_box.set_margin_top(6)
        row_box.set_margin_bottom(6)
        row_box.set_margin_start(6)
        row_box.set_margin_end(6)

        name_button = Gtk.Button(label=workspace_name)
        name_button.set_halign(Gtk.Align.FILL)
        name_button.set_hexpand(True)
        name_button.connect("clicked", self.on_launch_clicked)

        row_box.append(name_button)
        self.set_child(row_box)

        self.context_popover = Gtk.Popover()
        self.context_popover.set_parent(name_button)

        popover_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        commands_button = Gtk.Button(label="Commands")
        commands_button.connect("clicked", self.on_commands_clicked)
        popover_box.append(commands_button)
        delete_button = Gtk.Button(label="Delete Workspace")
        delete_button.connect("clicked", self.on_delete_clicked)
        popover_box.append(delete_button)
        self.context_popover.set_child(popover_box)

        right_click = Gtk.GestureClick()
        right_click.set_button(3)
        right_click.connect("pressed", self.on_right_click_pressed)
        name_button.add_controller(right_click)

    def on_launch_clicked(self, _button):
        self.app.launch_workspace(self.workspace_name)

    def on_commands_clicked(self, _button):
        self.context_popover.popdown()
        self.on_commands_clicked_cb(self.workspace_name)

    def on_delete_clicked(self, _button):
        self.context_popover.popdown()
        self.app.delete_workspace(self.workspace_name)

    def on_right_click_pressed(self, _gesture, _n_press, x, y):
        self.context_popover.set_pointing_to(Gdk.Rectangle(int(x), int(y), 1, 1))
        self.context_popover.popup()


class CommandsWindow(Gtk.ApplicationWindow):
    def __init__(self, app, main_window, workspace_name: str):
        super().__init__(application=app)
        self._workspaces_app = app
        self.main_window = main_window
        self.workspace_name = workspace_name

        self.set_transient_for(main_window)
        self.set_resizable(True)
        self.set_default_size(520, 420)
        self.set_destroy_with_parent(True)

        self._build_ui()
        self._load_buffer()
        self._update_title()
        self.connect("close-request", self._on_close_request)
        self.connect("destroy", self._on_destroy)

    def _update_title(self):
        self.set_title(f"Commands — {self.workspace_name}")

    def _build_ui(self):
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        box.set_margin_top(12)
        box.set_margin_bottom(12)
        box.set_margin_start(12)
        box.set_margin_end(12)
        self.set_child(box)

        self.hint = Gtk.Label(
            label="One command per line. Save before closing if you made changes.",
            xalign=0,
        )
        self.hint.add_css_class("dim-label")
        box.append(self.hint)

        self.commands_text = Gtk.TextView()
        self.commands_text.set_monospace(True)
        self.commands_text.set_vexpand(True)

        scroll = Gtk.ScrolledWindow()
        scroll.set_vexpand(True)
        scroll.set_child(self.commands_text)
        box.append(scroll)

        actions = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        save_button = Gtk.Button(label="Save")
        save_button.connect("clicked", self._on_save_clicked)
        launch_button = Gtk.Button(label="Launch")
        launch_button.connect("clicked", self._on_launch_clicked)
        close_button = Gtk.Button(label="Close")
        close_button.connect("clicked", self._on_close_clicked)
        actions.append(save_button)
        actions.append(launch_button)
        actions.append(close_button)
        box.append(actions)

        self.status_label = Gtk.Label(label="", xalign=0)
        self.status_label.set_wrap(True)
        box.append(self.status_label)

    def _load_buffer(self):
        buf = self.commands_text.get_buffer()
        commands = self._workspaces_app.workspaces.get(self.workspace_name, [])
        buf.set_text("\n".join(commands))

    def set_workspace(self, name: str):
        self.workspace_name = name
        self._update_title()
        self._load_buffer()
        self.status_label.set_text("")

    def _on_save_clicked(self, _b):
        buf = self.commands_text.get_buffer()
        start_iter = buf.get_start_iter()
        end_iter = buf.get_end_iter()
        raw_text = buf.get_text(start_iter, end_iter, False)
        commands = [line.strip() for line in raw_text.splitlines() if line.strip()]
        self._workspaces_app.workspaces[self.workspace_name] = commands
        self._workspaces_app.save_data()
        self.status_label.set_text(
            f"Saved {len(commands)} command(s) for '{self.workspace_name}'."
        )

    def _on_launch_clicked(self, _b):
        self._workspaces_app.launch_workspace(self.workspace_name, status_setter=self._set_status)

    def _on_close_clicked(self, _b):
        self.close()

    def _on_close_request(self, _w):
        if self.main_window and self.main_window.commands_window is self:
            self.main_window.commands_window = None
        return False

    def _set_status(self, text: str):
        self.status_label.set_text(text)

    def _on_destroy(self, _w):
        if self.main_window and self.main_window.commands_window is self:
            self.main_window.commands_window = None


class WorkspacesWindow(Gtk.ApplicationWindow):
    def __init__(self, app):
        super().__init__(application=app)
        self.app = app
        self.set_title("💼️ Workspaces")
        self.set_default_size(420, 400)

        self.commands_window = None

        header = Gtk.HeaderBar()
        header.set_show_title_buttons(True)
        header.set_title_widget(Gtk.Label(label="Workspaces"))
        self.set_titlebar(header)
        gtk_theme.attach_profile_menu(
            self,
            header,
            about_name="GTK Workspaces",
            about_comments="Named workspace launcher for the Neuronix GTK-Apps suite.",
        )

        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        root.set_margin_top(12)
        root.set_margin_bottom(12)
        root.set_margin_start(12)
        root.set_margin_end(12)
        self.set_child(root)

        add_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.workspace_name_entry = Gtk.Entry()
        self.workspace_name_entry.set_placeholder_text("New workspace name")
        self.workspace_name_entry.connect("activate", self.on_add_workspace_clicked)
        add_button = Gtk.Button(label="Add Workspace")
        add_button.connect("clicked", self.on_add_workspace_clicked)
        add_row.append(self.workspace_name_entry)
        add_row.append(add_button)
        root.append(add_row)

        self.workspace_list = Gtk.ListBox()
        self.workspace_list.set_selection_mode(Gtk.SelectionMode.NONE)

        left_scroll = Gtk.ScrolledWindow()
        left_scroll.set_vexpand(True)
        left_scroll.set_child(self.workspace_list)
        root.append(left_scroll)

        self.status_label = Gtk.Label(label="", xalign=0)
        self.status_label.set_wrap(True)
        root.append(self.status_label)
        self.status_label.set_text("Click a workspace name to launch. Right-click it for Commands.")

        self.refresh_workspace_list()

    def open_commands_editor(self, name):
        if name not in self.app.workspaces:
            return
        if self.commands_window is not None:
            # Reuse: switch workspace in existing window
            self.commands_window.set_workspace(name)
            self.commands_window.present()
            return
        self.commands_window = CommandsWindow(self.app, self, name)
        self.commands_window.present()

    def on_add_workspace_clicked(self, _widget):
        name = self.workspace_name_entry.get_text().strip()
        if not name:
            self.status_label.set_text("Enter a workspace name first.")
            return
        if name in self.app.workspaces:
            self.status_label.set_text(f"Workspace '{name}' already exists.")
            return

        self.app.workspaces[name] = []
        self.app.save_data()
        self.workspace_name_entry.set_text("")
        self.refresh_workspace_list()
        self.open_commands_editor(name)
        self.status_label.set_text(f"Added '{name}'. Commands open in the other window.")

    def refresh_workspace_list(self):
        while True:
            row = self.workspace_list.get_row_at_index(0)
            if row is None:
                break
            self.workspace_list.remove(row)

        for workspace_name in sorted(self.app.workspaces.keys(), key=lambda x: x.lower()):
            row = WorkspaceRow(self.app, workspace_name, self.open_commands_editor)
            self.workspace_list.append(row)


class WorkspacesApp(Gtk.Application):
    def __init__(self):
        super().__init__(application_id=APP_ID, flags=Gio.ApplicationFlags.FLAGS_NONE)
        self.workspaces = self.load_data()
        self.window = None

    def do_activate(self):
        if self.window is None:
            self.window = WorkspacesWindow(self)
        self.window.present()

    def load_data(self):
        if not CONFIG_PATH.exists():
            return {}
        try:
            data = json.loads(CONFIG_PATH.read_text(encoding="utf-8"))
            if isinstance(data, dict):
                normalized = {}
                for key, value in data.items():
                    if isinstance(key, str) and isinstance(value, list):
                        normalized[key] = [str(v) for v in value]
                return normalized
        except (json.JSONDecodeError, OSError):
            pass
        return {}

    def save_data(self):
        try:
            CONFIG_PATH.write_text(json.dumps(self.workspaces, indent=2), encoding="utf-8")
        except OSError:
            if self.window:
                self.window.status_label.set_text(f"Failed to save config: {CONFIG_PATH}")

    def delete_workspace(self, name: str):
        if name in self.workspaces:
            del self.workspaces[name]
            self.save_data()
            if self.window:
                self.window.refresh_workspace_list()
                if self.window.commands_window and self.window.commands_window.workspace_name == name:
                    self.window.commands_window.close()
                self.window.status_label.set_text(f"Deleted workspace '{name}'.")

    def _default_status(self, text: str):
        if self.window:
            self.window.status_label.set_text(text)

    def launch_workspace(self, name: str, status_setter=None):
        set_status = status_setter or self._default_status
        commands = self.workspaces.get(name, [])
        if not commands:
            set_status(f"No commands saved for '{name}'.")
            return

        launched = 0
        for command in commands:
            try:
                subprocess.Popen(command, shell=True, start_new_session=True)
                launched += 1
            except OSError as err:
                set_status(f"Failed command '{command}': {err}")
                return

        set_status(f"Launched {launched} command(s) for '{name}'.")


def main():
    app = WorkspacesApp()
    return app.run()


if __name__ == "__main__":
    raise SystemExit(main())
