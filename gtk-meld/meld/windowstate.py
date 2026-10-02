# Copyright (C) 2016 Kai Willadsen <kai.willadsen@gmail.com>
#
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 2 of the License, or (at
# your option) any later version.
#
# This program is distributed in the hope that it will be useful, but
# WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
# General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <http://www.gnu.org/licenses/>.


from gi.repository import Gio, GLib, GObject

from meld.settings import load_settings_schema

WINDOW_STATE_SCHEMA = "org.gnome.meld.WindowState"


class SavedWindowState(GObject.GObject):
    """Utility class for saving and restoring GtkWindow state"""

    __gtype_name__ = "SavedWindowState"

    width = GObject.Property(type=int, nick="Current window width", default=-1)
    height = GObject.Property(type=int, nick="Current window height", default=-1)
    is_maximized = GObject.Property(
        type=bool, nick="Is window maximized", default=False
    )
    is_fullscreen = GObject.Property(
        type=bool, nick="Is window fullscreen", default=False
    )

    def bind(self, window):
        window.connect("notify::default-width", self.on_size_allocate)
        window.connect("notify::default-height", self.on_size_allocate)
        window.connect("notify::maximized", self.on_window_state_event)
        window.connect("notify::fullscreen", self.on_window_state_event)

        # Don't re-read from gsettings after initialisation; we've seen
        # what looked like issues with buggy debounce here.
        bind_flags = (
            Gio.SettingsBindFlags.DEFAULT | Gio.SettingsBindFlags.GET_NO_CHANGES
        )
        self.settings = load_settings_schema(WINDOW_STATE_SCHEMA)
        self.settings.bind("width", self, "width", bind_flags)
        self.settings.bind("height", self, "height", bind_flags)
        self.settings.bind("is-maximized", self, "is-maximized", bind_flags)

        # Saved size and maximize must not win. Every launch is fullscreen.
        self._fullscreen_hold = True
        self._enter_fullscreen(window)
        window.connect("map", self._on_map_fullscreen)
        for delay in (80, 250, 600, 1200):
            GLib.timeout_add(delay, self._enter_fullscreen, window)
        GLib.timeout_add(1600, self._release_fullscreen_hold)

    def _release_fullscreen_hold(self):
        self._fullscreen_hold = False
        return False

    def _on_map_fullscreen(self, window, *_args):
        GLib.idle_add(self._enter_fullscreen, window)

    def _enter_fullscreen(self, window):
        if not getattr(self, "_fullscreen_hold", False):
            return False
        try:
            if not window.is_fullscreen():
                window.fullscreen()
        except Exception:
            pass
        action = window.lookup_action("fullscreen")
        if action is not None and window.is_fullscreen():
            action.set_state(GLib.Variant.new_boolean(True))
        return False

    def on_size_allocate(self, window, property):
        if not (self.props.is_maximized or self.props.is_fullscreen):
            width, height = window.get_width(), window.get_height()
            # ignore inital size of (0, 0)
            if width != 0 and width != self.props.width:
                self.props.width = width
            if height != 0 and height != self.props.height:
                self.props.height = height

    def on_window_state_event(self, window, event):
        if event.name == "maximized":
            is_maximized = window.is_maximized()
            if is_maximized != self.props.is_maximized:
                self.props.is_maximized = is_maximized

        if event.name == "fullscreen":
            is_fullscreen = window.is_fullscreen
            if is_fullscreen != self.props.is_fullscreen:
                self.props.is_fullscreen = is_fullscreen
