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
    
    def on_activate(self, app):
        """Handle application activation."""
        # Check if media directory exists
        if not os.path.isdir(MEDIA_DIRECTORY):
            dialog = Gtk.MessageDialog(
                transient_for=None,
                message_type=Gtk.MessageType.ERROR,
                buttons=Gtk.ButtonsType.OK,
                text=f"Directory not found: {MEDIA_DIRECTORY}"
            )
            def on_response(d, r):
                d.destroy()
                sys.exit(1)
            dialog.connect('response', on_response)
            dialog.present()
            return
        
        # Create and show main window
        win = MainWindow(
            app,
            MEDIA_DIRECTORY,
            EXPLORE_DIRECTORY,
            EXPLORE_DIRECTORY_FEATURES,
            PHOTO_DRIVE,
        )
        win.present()


def main():
    """Main function to start the application."""
    # Clear leftover Chromium viewer profiles / player HTML from prior runs
    _cleanup_all_chromium_temps()
    app = PhotosApp()
    return app.run(sys.argv)


if __name__ == '__main__':
    sys.exit(main())
