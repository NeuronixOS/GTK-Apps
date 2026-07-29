#!/usr/bin/env python3
"""Main entry point for the Photo Organizer (gtk-photos) application."""

import sys
import os
import json
import gi
gi.require_version('Gtk', '4.0')
from gi.repository import Gtk, Gio, GLib
from .window import MainWindow, _cleanup_all_chromium_temps


def get_config_file():
    """Get the path to the config.json file (``~/.config/gtk-apps/gtk-photos/``)."""
    from .config_paths import config_file
    return str(config_file())


def load_config():
    """Load configuration from config.json."""
    config_file = get_config_file()
    default_media_directory = ''
    default_explore_directory = ''
    default_explore_directory_features = ''
    default_photo_drive = ''

    if not os.path.exists(config_file):
        # Create default config file
        default_config = {
            'media_directory': default_media_directory,
            'explore_directory': default_explore_directory,
            'explore_directory_features': default_explore_directory_features,
            'photo_drive': default_photo_drive
        }
        try:
            with open(config_file, 'w') as f:
                json.dump(default_config, f, indent=2)
            return default_config
        except IOError:
            return {
                'media_directory': default_media_directory,
                'explore_directory': default_explore_directory,
                'explore_directory_features': default_explore_directory_features,
                'photo_drive': default_photo_drive
            }

    try:
        with open(config_file, 'r') as f:
            config = json.load(f)
            # Ensure required directories are set
            if 'media_directory' not in config:
                config['media_directory'] = default_media_directory
            if 'explore_directory' not in config:
                config['explore_directory'] = default_explore_directory
            if 'explore_directory_features' not in config:
                config['explore_directory_features'] = default_explore_directory_features
            if 'photo_drive' not in config:
                config['photo_drive'] = default_photo_drive
            return config
    except (json.JSONDecodeError, IOError):
        return {
            'media_directory': default_media_directory,
            'explore_directory': default_explore_directory,
            'explore_directory_features': default_explore_directory_features,
            'photo_drive': default_photo_drive
        }


def save_config(config: dict) -> None:
    """Persist config.json (best-effort)."""
    try:
        with open(get_config_file(), 'w') as f:
            json.dump(config, f, indent=2)
            f.write('\n')
    except IOError:
        pass


def _existing_dir(path: str) -> str:
    """Return path if it is an existing directory, else empty string."""
    if path and os.path.isdir(path):
        return path
    return ''


config = load_config()
MEDIA_DIRECTORY = config.get('media_directory', '')
EXPLORE_DIRECTORY = config.get('explore_directory', '')
EXPLORE_DIRECTORY_FEATURES = config.get('explore_directory_features', '')
PHOTO_DRIVE = config.get('photo_drive', '') or EXPLORE_DIRECTORY


class PhotosApp(Gtk.Application):
    """Main application class."""

    def __init__(self):
        super().__init__(
            application_id='org.neuronix.GtkPhotos',
            flags=Gio.ApplicationFlags.FLAGS_NONE
        )
        self.connect('activate', self.on_activate)

    def _open_main(self, media_directory: str):
        """Open MainWindow with validated paths (missing optional dirs fall back)."""
        explore = _existing_dir(EXPLORE_DIRECTORY) or media_directory
        features = _existing_dir(EXPLORE_DIRECTORY_FEATURES)
        photo_drive = (
            _existing_dir(PHOTO_DRIVE)
            or _existing_dir(EXPLORE_DIRECTORY)
            or media_directory
        )
        win = MainWindow(
            self,
            media_directory,
            explore,
            features,
            photo_drive,
        )
        win.present()

    def _pick_media_directory(self, missing_path: str):
        """Ask the user to choose a media folder when config points nowhere."""
        dialog = Gtk.AlertDialog()
        dialog.set_message("Photos folder not found")
        dialog.set_detail(
            f"Configured media_directory is missing:\n{missing_path or '(empty)'}\n\n"
            "Choose a folder that holds images/videos to organize, "
            "or Cancel to quit."
        )
        dialog.set_buttons(["Cancel", "Choose Folder…"])
        dialog.set_cancel_button(0)
        dialog.set_default_button(1)

        def on_alert(_d, result):
            try:
                choice = dialog.choose_finish(result)
            except GLib.Error:
                self.quit()
                return
            if choice != 1:
                self.quit()
                return

            file_dialog = Gtk.FileDialog()
            file_dialog.set_title("Choose Photos media folder")
            home = Gio.File.new_for_path(os.path.expanduser("~"))
            file_dialog.set_initial_folder(home)

            def on_folder(_fd, folder_result):
                try:
                    gfile = file_dialog.select_folder_finish(folder_result)
                except GLib.Error:
                    self.quit()
                    return
                if gfile is None:
                    self.quit()
                    return
                path = gfile.get_path()
                if not path or not os.path.isdir(path):
                    self.quit()
                    return

                global MEDIA_DIRECTORY, EXPLORE_DIRECTORY, PHOTO_DRIVE, config
                MEDIA_DIRECTORY = path
                # Keep explore/photo_drive when they exist; otherwise pin to media.
                if not _existing_dir(EXPLORE_DIRECTORY):
                    EXPLORE_DIRECTORY = path
                if not _existing_dir(PHOTO_DRIVE):
                    PHOTO_DRIVE = path
                config = dict(config)
                config["media_directory"] = MEDIA_DIRECTORY
                config["explore_directory"] = EXPLORE_DIRECTORY
                config["photo_drive"] = PHOTO_DRIVE
                save_config(config)
                self._open_main(MEDIA_DIRECTORY)

            file_dialog.select_folder(None, None, on_folder)

        dialog.choose(None, None, on_alert)

    def on_activate(self, app):
        """Handle application activation."""
        media = _existing_dir(MEDIA_DIRECTORY)
        if not media:
            self._pick_media_directory(MEDIA_DIRECTORY)
            return
        self._open_main(media)


def main():
    """Main function to start the application."""
    # Clear leftover Chromium viewer profiles / player HTML from prior runs
    _cleanup_all_chromium_temps()
    app = PhotosApp()
    return app.run(sys.argv)


if __name__ == '__main__':
    sys.exit(main())
