"""Main window class for the media organizer application."""

import os
import shutil
import sys
import threading
import subprocess
import tempfile
import html
import hashlib
import random
import json
from pathlib import Path
import gi
gi.require_version('Graphene', '1.0')
from gi.repository import Gtk, Gdk, Gio, GLib, GdkPixbuf, Graphene
from .media_types import get_media_files, get_all_media_files_sorted_by_date

_THEME_PY = Path(__file__).resolve().parents[2] / "gtk-theme" / "python"
if str(_THEME_PY) not in sys.path:
    sys.path.insert(0, str(_THEME_PY))
import gtk_theme  # noqa: E402
from .thumbnailer import (
    generate_image_thumbnail,
    generate_video_thumbnail,
    create_placeholder_thumbnail,
    rotate_image_file,
    THUMBNAIL_SIZE
)
from .websites_tab import WebsitesPanel
from .folders_tab import FoldersPanel

# Global registry for tracking subprocesses across all functions
_global_subprocesses = []

# Temp Chromium profile dirs / HTML player files to remove when viewers quit or the app exits
_global_chromium_temps = []
_chromium_temps_lock = threading.Lock()

# When opening images or videos, the longer side of the viewer window is at most this many pixels.
VIEWER_MAX_LARGER_SIDE = 1000


def _register_chromium_temps(*paths: str) -> None:
    with _chromium_temps_lock:
        for path in paths:
            if path and path not in _global_chromium_temps:
                _global_chromium_temps.append(path)


def _remove_path_quiet(path: str) -> None:
    if not path or not os.path.exists(path):
        return
    try:
        if os.path.isdir(path):
            shutil.rmtree(path, ignore_errors=True)
        else:
            os.unlink(path)
    except OSError:
        pass


def _chromium_still_using_user_data(user_data_dir: str) -> bool:
    """True if any Chromium process still references this --user-data-dir."""
    try:
        result = subprocess.run(
            ['pgrep', '-af', user_data_dir],
            capture_output=True,
            text=True,
            timeout=2,
        )
    except Exception:
        return False
    for line in result.stdout.splitlines():
        lower = line.lower()
        if 'chromium' in lower or 'chrome' in lower:
            if 'pgrep' not in lower:
                return True
    return False


def _watch_and_cleanup_chromium(user_data_dir: str, *extra_paths: str) -> None:
    """After Chromium leaves this profile dir, delete it and any related temp files."""
    paths = [user_data_dir, *extra_paths]
    _register_chromium_temps(*paths)

    def _worker():
        import time
        # Launcher may exit while children keep the profile; wait until none remain.
        while _chromium_still_using_user_data(user_data_dir):
            time.sleep(2)
        for path in paths:
            _remove_path_quiet(path)
        with _chromium_temps_lock:
            for path in paths:
                if path in _global_chromium_temps:
                    _global_chromium_temps.remove(path)

    threading.Thread(target=_worker, daemon=True).start()


def _cleanup_all_chromium_temps(*, force: bool = False) -> None:
    """Remove tracked temps and sweep leftover viewer junk under /tmp.

    If force is False, skip Chromium profile dirs still referenced by a process.
    """
    with _chromium_temps_lock:
        paths = list(_global_chromium_temps)
        _global_chromium_temps.clear()
    for path in paths:
        if (
            not force
            and os.path.isdir(path)
            and os.path.basename(path).startswith('chromium-')
            and _chromium_still_using_user_data(path)
        ):
            _register_chromium_temps(path)
            continue
        _remove_path_quiet(path)

    tmp = tempfile.gettempdir()
    players_dir = os.path.join(tmp, 'gtk-photos-players')
    if os.path.isdir(players_dir):
        try:
            for name in os.listdir(players_dir):
                if name.endswith('.html') and name.startswith('.gtk-photos-player-'):
                    _remove_path_quiet(os.path.join(players_dir, name))
            # Remove empty cache dir if nothing else is left
            if not os.listdir(players_dir):
                _remove_path_quiet(players_dir)
        except OSError:
            pass

    try:
        for name in os.listdir(tmp):
            if not name.startswith(('chromium-video-', 'chromium-image-', 'chromium-html-')):
                continue
            path = os.path.join(tmp, name)
            if not force and _chromium_still_using_user_data(path):
                continue
            _remove_path_quiet(path)
    except OSError:
        pass


def viewer_window_size_from_media_dimensions(
    width: int, height: int, max_larger_side: int = VIEWER_MAX_LARGER_SIDE
):
    """Return (w, h) preserving aspect ratio with max(w, h) <= max_larger_side."""
    w, h = max(1, int(width)), max(1, int(height))
    larger = max(w, h)
    if larger <= max_larger_side:
        return w, h
    scale = max_larger_side / larger
    return max(1, int(round(w * scale))), max(1, int(round(h * scale)))


def bind_gtk_window_aspect_ratio(window: Gtk.Window):
    """Keep a GTK window's resize handles locked to its media aspect ratio."""
    window._aspect_ratio = None  # type: ignore[attr-defined]
    window._aspect_enforcing = False  # type: ignore[attr-defined]

    def _enforce(_window, pspec):
        if window._aspect_enforcing or not window._aspect_ratio:  # type: ignore[attr-defined]
            return
        w = window.get_default_width()
        h = window.get_default_height()
        if w <= 0 or h <= 0:
            return
        target_h = max(1, round(w / window._aspect_ratio))  # type: ignore[attr-defined]
        target_w = max(1, round(h * window._aspect_ratio))  # type: ignore[attr-defined]
        if pspec.name == 'default-width' and abs(h - target_h) > 1:
            window._aspect_enforcing = True  # type: ignore[attr-defined]
            window.set_default_size(w, target_h)
            window._aspect_enforcing = False  # type: ignore[attr-defined]
        elif pspec.name == 'default-height' and abs(w - target_w) > 1:
            window._aspect_enforcing = True  # type: ignore[attr-defined]
            window.set_default_size(target_w, h)
            window._aspect_enforcing = False  # type: ignore[attr-defined]

    window.connect('notify::default-width', _enforce)
    window.connect('notify::default-height', _enforce)


def set_gtk_window_aspect_ratio(window: Gtk.Window, width: int, height: int):
    """Set the aspect ratio enforced during window resize."""
    if width > 0 and height > 0:
        window._aspect_ratio = width / height  # type: ignore[attr-defined]
    else:
        window._aspect_ratio = None  # type: ignore[attr-defined]


class ImageViewerWindow(Gtk.Window):
    """GTK window for viewing images with keyboard navigation."""
    
    def __init__(self, filepath: str, image_list: list = None, current_index: int = 0, parent_window=None):
        super().__init__(title=os.path.basename(filepath))
        # Don't set transient_for - allows images to be behind main window
        self.set_modal(False)
        
        # If image_list is not provided, get images from the same folder
        if image_list is None:
            image_list = self._get_images_in_folder(filepath)
            try:
                current_index = image_list.index(filepath)
            except ValueError:
                current_index = 0
        
        self.image_list = image_list
        self.current_index = current_index
        
        # Create main container
        main_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.set_child(main_box)
        
        # Create picture widget; CONTAIN preserves aspect ratio when the window is resized
        self.picture = Gtk.Picture()
        self.picture.set_content_fit(Gtk.ContentFit.CONTAIN)
        self.picture.set_hexpand(True)
        self.picture.set_vexpand(True)
        main_box.append(self.picture)

        bind_gtk_window_aspect_ratio(self)
        
        # When pointer enters this window, focus it so this viewer receives keys (multi-window use).
        motion_ctrl = Gtk.EventControllerMotion.new()
        motion_ctrl.connect('enter', self._on_pointer_enter)
        self.add_controller(motion_ctrl)
        
        click_ctrl = Gtk.GestureClick.new()
        click_ctrl.set_propagation_phase(Gtk.PropagationPhase.CAPTURE)
        click_ctrl.connect('pressed', self._on_viewer_click)
        main_box.add_controller(click_ctrl)
        
        # Load initial image
        self.load_image(filepath)
        
        # Connect keyboard events - add to window and make sure it can receive focus
        controller = Gtk.EventControllerKey()
        controller.set_propagation_phase(Gtk.PropagationPhase.CAPTURE)
        controller.connect('key-pressed', self.on_key_pressed)
        self.add_controller(controller)
        
        # Also add controller to the main box to catch events
        box_controller = Gtk.EventControllerKey()
        box_controller.connect('key-pressed', self.on_key_pressed)
        main_box.add_controller(box_controller)
        
        # Make window focusable and ensure it can receive keyboard events
        self.set_focus_on_click(True)
        self.set_focusable(True)
        
        # Focus when mapped; pointer-enter / click also focus for multi-window use.
        self.connect('map', self._on_map_focus)
    
    def _on_map_focus(self, window):
        GLib.idle_add(self._focus_this_viewer)
    
    def _focus_this_viewer(self):
        self.present()
        self.grab_focus()
        self.picture.grab_focus()
        return False
    
    def _on_pointer_enter(self, controller, x, y):
        self._focus_this_viewer()
    
    def _on_viewer_click(self, gesture, n_press, x, y):
        self._focus_this_viewer()
    
    def _get_images_in_folder(self, filepath: str):
        """Get all image files in the same folder as the given filepath."""
        from .media_types import is_image_file
        
        folder_path = os.path.dirname(filepath)
        if not os.path.isdir(folder_path):
            return []
        
        image_files = []
        try:
            for item in os.listdir(folder_path):
                item_path = os.path.join(folder_path, item)
                if os.path.isfile(item_path) and is_image_file(item_path):
                    image_files.append(item_path)
            # Sort alphabetically
            image_files.sort()
        except (OSError, PermissionError):
            pass
        
        return image_files
    
    def load_image(self, filepath: str):
        """Load and display the image; longer side is at most VIEWER_MAX_LARGER_SIDE."""
        try:
            pixbuf = GdkPixbuf.Pixbuf.new_from_file(filepath)
            if not pixbuf:
                return
            w = max(1, pixbuf.get_width())
            h = max(1, pixbuf.get_height())
            tw, th = viewer_window_size_from_media_dimensions(w, h)
            if tw != w or th != h:
                pixbuf = pixbuf.scale_simple(tw, th, GdkPixbuf.InterpType.BILINEAR)
            self.picture.set_pixbuf(pixbuf)
            self.set_default_size(tw, th)
            set_gtk_window_aspect_ratio(self, tw, th)
            self.set_title(os.path.basename(filepath))
        except Exception as e:
            print(f"Error loading image {filepath}: {e}")
    
    def on_key_pressed(self, controller, keyval, keycode, state):
        """Handle keyboard events."""
        # Only handle navigation if we have an image list
        if not self.image_list:
            return False
        
        if keyval == Gdk.KEY_Left:
            # Navigate to previous image
            if self.current_index > 0:
                self.current_index -= 1
                next_filepath = self.image_list[self.current_index]
                self.load_image(next_filepath)
            return True
        elif keyval == Gdk.KEY_Right:
            # Navigate to next image
            if self.current_index < len(self.image_list) - 1:
                self.current_index += 1
                next_filepath = self.image_list[self.current_index]
                self.load_image(next_filepath)
            return True
        elif keyval == Gdk.KEY_Escape:
            # Close window
            self.close()
            return True
        
        return False
    


def open_image_with_constraints(filepath: str, parent_window=None, image_list: list = None, current_index: int = 0):
    """Open an image with optional keyboard navigation.
    
    Args:
        filepath: Path to the image file
        parent_window: Parent window (optional)
        image_list: List of image filepaths for navigation (optional, for Explore tab)
        current_index: Current index in image_list (optional)
    """
    # Ensure filepath is absolute
    if not os.path.isabs(filepath):
        filepath = os.path.abspath(filepath)
    
    # Use GTK viewer if image_list is provided OR if parent_window is set (Explore tab)
    # The ImageViewerWindow will automatically get images from folder if image_list is None
    use_gtk_viewer = image_list is not None or parent_window is not None
    
    if use_gtk_viewer:
        viewer = ImageViewerWindow(filepath, image_list, current_index, parent_window)
        # Track viewer window if parent_window is MainWindow
        if parent_window and hasattr(parent_window, '_image_viewer_windows'):
            parent_window._image_viewer_windows.append(viewer)
            # Remove from list when window is closed
            viewer.connect('close-request', lambda w: parent_window._image_viewer_windows.remove(w) if w in parent_window._image_viewer_windows else None)
        viewer.present()
        return
    
    # Otherwise, use Chromium (existing behavior for other tabs)
    
    # Get image dimensions using GdkPixbuf
    try:
        pixbuf = GdkPixbuf.Pixbuf.new_from_file(filepath)
        if pixbuf:
            orig_width = pixbuf.get_width()
            orig_height = pixbuf.get_height()
        else:
            orig_width = None
            orig_height = None
    except Exception:
        orig_width = None
        orig_height = None
    
    # Window size: longer side at most VIEWER_MAX_LARGER_SIDE
    window_width = None
    window_height = None
    
    if orig_width and orig_height:
        window_width, window_height = viewer_window_size_from_media_dimensions(
            orig_width, orig_height
        )
    else:
        # If we can't get dimensions, use a default size
        window_width = 800
        window_height = 600
    
    # Try to find Chromium/Chrome
    browser_paths = ['chromium', 'chromium-browser', 'google-chrome', '/usr/bin/chromium', '/usr/bin/chromium-browser']
    browser_path = None
    
    for path in browser_paths:
        if path.startswith('/'):
            if os.path.exists(path) and os.access(path, os.X_OK):
                browser_path = path
                break
        else:
            try:
                result = subprocess.run(['which', path], capture_output=True, text=True, timeout=1)
                if result.returncode == 0:
                    browser_path = result.stdout.strip()
                    break
            except Exception:
                continue
    
    if not browser_path:
        # Browser not found, fall back to default handler
        print("Error: Chromium/Chrome not found. Please install chromium: sudo apt install chromium")
        file_uri = GLib.filename_to_uri(filepath)
        try:
            Gio.AppInfo.launch_default_for_uri(file_uri, None)
        except Exception as e:
            print(f"Error opening file {filepath}: {e}")
        return
    
    try:
        # Convert file path to file:// URL
        file_uri = GLib.filename_to_uri(filepath)
        
        # Create a temporary HTML file that displays the image
        html_temp_dir = tempfile.mkdtemp(prefix='chromium-html-')
        html_file = os.path.join(html_temp_dir, 'image_viewer.html')
        
        # Create HTML file with image viewer
        html_content = f"""<!DOCTYPE html>
<html>
<head>
    <title>{html.escape(os.path.basename(filepath))}</title>
    <style>
        body {{
            margin: 0;
            padding: 0;
            background: black;
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            overflow: hidden;
        }}
        img {{
            max-width: 100%;
            max-height: 100vh;
            width: auto;
            height: auto;
            object-fit: contain;
        }}
    </style>
</head>
<body>
    <img src="{html.escape(file_uri)}" alt="{html.escape(os.path.basename(filepath))}">
</body>
</html>"""
        
        # Write HTML to temporary file
        with open(html_file, 'w', encoding='utf-8') as f:
            f.write(html_content)
        
        # Ensure file exists and is readable
        if not os.path.exists(html_file):
            print(f"Error: HTML file was not created: {html_file}")
            return
        
        # Convert to absolute path and then to URI
        html_file_abs = os.path.abspath(html_file)
        html_uri = GLib.filename_to_uri(html_file_abs)
        
        # Use a unique user data directory to force separate instances
        # Include timestamp and random component to ensure absolute uniqueness
        import time
        import random
        unique_id = f'{int(time.time()*1000000)}_{random.randint(1000, 9999)}'
        unique_user_data = tempfile.mkdtemp(prefix=f'chromium-image-{unique_id}-')
        
        # Build Chromium command
        # The key is to set --user-data-dir FIRST (before --app) to prevent singleton detection
        # Chromium's singleton process manager checks for --user-data-dir early
        chromium_cmd = [
            browser_path,
            '--user-data-dir=' + unique_user_data,  # MUST be first flag to prevent singleton detection
            '--app=' + html_uri,  # Open as app (minimal UI, opens only this URL)
            '--new-window',  # Force new window for each image
            '--new-instance',  # Force a new Chromium instance
            '--disable-session-crashed-bubble',  # Prevent session reuse prompts
            '--disable-infobars',  # Disable info bars
        ]
        
        # Add window size and center on the monitor showing the main app window
        if window_width and window_height:
            pos_x, pos_y = centered_window_position(
                window_width, window_height, parent_window
            )
            chromium_cmd.append(f'--window-size={window_width},{window_height}')
            chromium_cmd.append(f'--window-position={pos_x},{pos_y}')
        
        # Launch Chromium - each instance opens in its own window
        # The unique user-data-dir should force a new instance, but Chromium's singleton
        # process manager may still intercept. We'll handle this by ensuring the command
        # is launched in a completely isolated way.
        process = subprocess.Popen(
            chromium_cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
            env={**os.environ, 'CHROMIUM_FLAGS': ''}  # Clear any Chromium flags from environment
        )
        # Track process for cleanup
        _global_subprocesses.append(process)
        
        # Check if process started successfully
        import time
        time.sleep(0.3)
        if process.poll() is not None:
            # Process exited immediately, likely an error
            stdout_output = process.stdout.read().decode('utf-8', errors='ignore') if process.stdout else ""
            stderr_output = process.stderr.read().decode('utf-8', errors='ignore') if process.stderr else ""
            print(f"Chromium exited immediately. Return code: {process.returncode}")
            print(f"Command: {' '.join(chromium_cmd)}")
            if stdout_output:
                print(f"Stdout: {stdout_output}")
            if stderr_output:
                print(f"Stderr: {stderr_output}")
            _remove_path_quiet(unique_user_data)
            _remove_path_quiet(html_temp_dir)
            return

        # Remove Chromium profile + HTML wrapper after the viewer closes
        _watch_and_cleanup_chromium(unique_user_data, html_temp_dir)
            
    except Exception as e:
        print(f"Error launching Chromium: {e}")
        return


def get_video_dimensions(filepath: str):
    """Get video dimensions using ffprobe."""
    try:
        cmd = [
            'ffprobe',
            '-v', 'error',
            '-select_streams', 'v:0',
            '-show_entries', 'stream=width,height',
            '-of', 'csv=s=x:p=0',
            filepath
        ]
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=5)
        if result.returncode == 0 and result.stdout.strip():
            dimensions = result.stdout.strip().split('x')
            if len(dimensions) == 2:
                return int(dimensions[0]), int(dimensions[1])
    except Exception as e:
        print(f"Error getting video dimensions: {e}")
    return None, None


def default_portrait_9_16_window_size(max_larger_side: int = VIEWER_MAX_LARGER_SIDE):
    """Portrait 9:16 (width:height), height (longer side) = max_larger_side."""
    h = max(1, max_larger_side)
    w = max(1, int(round(9 * h / 16)))
    return w, h


def _find_chromium() -> str | None:
    """Find Chromium or Chrome executable."""
    browser_paths = [
        'chromium', 'chromium-browser', 'google-chrome',
        '/usr/bin/chromium', '/usr/bin/chromium-browser', '/usr/bin/google-chrome',
    ]
    for path in browser_paths:
        if path.startswith('/'):
            if os.path.exists(path) and os.access(path, os.X_OK):
                return path
        else:
            try:
                result = subprocess.run(
                    ['which', path], capture_output=True, text=True, timeout=1
                )
                if result.returncode == 0:
                    return result.stdout.strip()
            except Exception:
                continue
    return None


def _get_reference_monitor(reference_window: Gtk.Window | None = None) -> Gdk.Monitor | None:
    display = Gdk.Display.get_default()
    if display is None:
        return None
    if reference_window is not None:
        try:
            surface = reference_window.get_surface()
            if surface is not None:
                monitor = display.get_monitor_at_surface(surface)
                if monitor is not None:
                    return monitor
        except Exception:
            pass
    monitors = display.get_monitors()
    if monitors.get_n_items() > 0:
        return monitors.get_item(0)
    return None


def _mouse_screen_position() -> tuple[int, int] | None:
    """Best-effort global mouse position (works with XWayland on GNOME Wayland)."""
    try:
        result = subprocess.run(
            ['xdotool', 'getmouselocation', '--shell'],
            capture_output=True,
            text=True,
            timeout=0.5,
        )
        if result.returncode != 0:
            return None
        coords = {}
        for line in result.stdout.splitlines():
            if '=' in line:
                key, value = line.split('=', 1)
                coords[key] = value
        if 'X' in coords and 'Y' in coords:
            return int(coords['X']), int(coords['Y'])
    except (OSError, subprocess.TimeoutExpired, ValueError):
        pass
    return None


def _click_position_in_window(
    reference_window: Gtk.Window,
    click_widget: Gtk.Widget,
    click_x: float,
    click_y: float,
) -> tuple[float, float] | None:
    point = Graphene.Point()
    point.x = click_x
    point.y = click_y
    ok, out = click_widget.compute_point(reference_window, point)
    if not ok:
        return None
    return out.x, out.y


def _clamp_position_to_monitor(
    x: int,
    y: int,
    window_width: int,
    window_height: int,
    reference_window: Gtk.Window | None,
) -> tuple[int, int]:
    monitor = _get_reference_monitor(reference_window)
    if monitor is None:
        return x, y
    geom = monitor.get_geometry()
    x = max(geom.x, min(x, geom.x + max(0, geom.width - window_width)))
    y = max(geom.y, min(y, geom.y + max(0, geom.height - window_height)))
    return x, y


def _position_on_monitor(
    monitor: Gdk.Monitor,
    window_width: int,
    window_height: int,
) -> tuple[int, int]:
    geom = monitor.get_geometry()
    x = geom.x + max(0, (geom.width - window_width) // 2)
    y = geom.y + max(0, (geom.height - window_height) // 2)
    return x, y


def _position_visible_on_monitor(
    x: int,
    y: int,
    window_width: int,
    window_height: int,
    monitor: Gdk.Monitor,
) -> bool:
    geom = monitor.get_geometry()
    return (
        x >= geom.x - window_width // 2
        and y >= geom.y - window_height // 2
        and x <= geom.x + geom.width
        and y <= geom.y + geom.height
    )


def centered_window_position(
    window_width: int,
    window_height: int,
    reference_window: Gtk.Window | None = None,
    click_widget: Gtk.Widget | None = None,
    click_x: float | None = None,
    click_y: float | None = None,
    click_mouse: tuple[int, int] | None = None,
) -> tuple[int, int]:
    """Return (x, y) to center a viewer on the reference window, or its monitor."""
    monitor = _get_reference_monitor(reference_window)
    fallback = (
        _position_on_monitor(monitor, window_width, window_height)
        if monitor is not None
        else (100, 100)
    )

    if (
        reference_window is not None
        and click_widget is not None
        and click_x is not None
        and click_y is not None
    ):
        local = _click_position_in_window(reference_window, click_widget, click_x, click_y)
        mouse = click_mouse or _mouse_screen_position()
        if local is not None and mouse is not None:
            origin_x = mouse[0] - local[0]
            origin_y = mouse[1] - local[1]
            ref_w = max(1, reference_window.get_width())
            ref_h = max(1, reference_window.get_height())
            center_x = int(origin_x + ref_w / 2)
            center_y = int(origin_y + ref_h / 2)
            x = center_x - window_width // 2
            y = center_y - window_height // 2
            x, y = _clamp_position_to_monitor(
                x, y, window_width, window_height, reference_window
            )
            if monitor is None or _position_visible_on_monitor(
                x, y, window_width, window_height, monitor
            ):
                return x, y

    return fallback


def open_file_with_chromium(
    filepath: str,
    parent_window: Gtk.Window | None = None,
    click_widget: Gtk.Widget | None = None,
    click_x: float | None = None,
    click_y: float | None = None,
    click_mouse: tuple[int, int] | None = None,
):
    """Open a video file in Chromium; no sound; longer side at most VIEWER_MAX_LARGER_SIDE."""
    # Ensure filepath is absolute
    if not os.path.isabs(filepath):
        filepath = os.path.abspath(filepath)

    video_width, video_height = get_video_dimensions(filepath)
    if video_width and video_height:
        window_width, window_height = viewer_window_size_from_media_dimensions(
            video_width, video_height
        )
    else:
        window_width, window_height = default_portrait_9_16_window_size()

    def _log_opening(player: str):
        print(
            f"Opening video {os.path.basename(filepath)} with {player} "
            f"at {window_width}x{window_height} "
            f"(aspect ratio: {window_width / window_height:.2f})"
        )

    browser_path = _find_chromium()

    if not browser_path:
        # Browser not found, fall back to default handler
        print("Error: Chromium/Chrome not found. Please install chromium: sudo apt install chromium")
        file_uri = GLib.filename_to_uri(filepath)
        try:
            Gio.AppInfo.launch_default_for_uri(file_uri, None)
        except Exception as e:
            print(f"Error opening file {filepath}: {e}")
        return
    
    try:
        video_name = os.path.basename(filepath)
        file_uri = GLib.filename_to_uri(filepath)
        video_ext = os.path.splitext(filepath)[1].lower()
        mime_types = {
            '.mp4': 'video/mp4',
            '.avi': 'video/x-msvideo',
            '.mkv': 'video/x-matroska',
            '.webm': 'video/webm',
            '.mov': 'video/quicktime',
            '.flv': 'video/x-flv',
        }
        video_mime = mime_types.get(video_ext, 'video/mp4')

        player_tmp_dir = os.path.join(tempfile.gettempdir(), 'gtk-photos-players')
        os.makedirs(player_tmp_dir, exist_ok=True)
        html_file = os.path.join(
            player_tmp_dir,
            f'.gtk-photos-player-{hashlib.md5(filepath.encode()).hexdigest()[:8]}.html',
        )
        html_content = f"""<!DOCTYPE html>
<html>
<head>
    <title>{html.escape(video_name)}</title>
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        html, body {{
            width: 100%; height: 100%; overflow: hidden; background: black;
        }}
        #wrap {{
            width: 100%; height: 100%;
        }}
        video {{ width: 100%; height: 100%; object-fit: contain; }}
    </style>
</head>
<body>
    <div id="wrap">
        <video id="player" autoplay muted loop playsinline>
            <source src="{html.escape(file_uri)}" type="{video_mime}">
        </video>
    </div>
    <script>
        const wrap = document.getElementById('wrap');
        const player = document.getElementById('player');
        wrap.addEventListener('mouseenter', () => {{ player.controls = true; }});
        wrap.addEventListener('mouseleave', () => {{ player.controls = false; }});
    </script>
</body>
</html>"""
        with open(html_file, 'w', encoding='utf-8') as f:
            f.write(html_content)

        html_uri = GLib.filename_to_uri(os.path.abspath(html_file))

        import time
        import random
        unique_id = f'{int(time.time()*1000000)}_{random.randint(1000, 9999)}'
        unique_user_data = tempfile.mkdtemp(prefix=f'chromium-video-{unique_id}-')

        chromium_cmd = [
            browser_path,
            '--user-data-dir=' + unique_user_data,
            '--app=' + html_uri,
            '--new-window',
            '--allow-file-access-from-files',
            '--autoplay-policy=no-user-gesture-required',
            '--mute-audio',
        ]

        _log_opening('Chromium')
        chromium_cmd.append(f'--window-size={window_width},{window_height}')
        pos_x, pos_y = centered_window_position(
            window_width,
            window_height,
            parent_window,
            click_widget,
            click_x,
            click_y,
            click_mouse,
        )
        chromium_cmd.append(f'--window-position={pos_x},{pos_y}')

        print(f"Chromium command: {' '.join(chromium_cmd)}")

        process = subprocess.Popen(
            chromium_cmd,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
        _global_subprocesses.append(process)

        chromium_procs = []
        for _ in range(6):
            time.sleep(0.5)
            verify = subprocess.run(
                ['pgrep', '-af', unique_user_data],
                capture_output=True,
                text=True,
                timeout=1,
            )
            chromium_procs = [
                line for line in verify.stdout.splitlines()
                if 'chromium' in line.lower() and 'pgrep' not in line
            ]
            if chromium_procs:
                break

        if not chromium_procs:
            rc = process.poll()
            print(
                f"Error: Chromium did not stay running "
                f"(launcher exit code: {rc})."
            )
            print(f"Command: {' '.join(chromium_cmd)}")
            _remove_path_quiet(unique_user_data)
            _remove_path_quiet(html_file)
            return

        print(f"Chromium started ({len(chromium_procs)} process(es)) at {pos_x},{pos_y}")
        # Remove Chromium profile + player HTML after the viewer closes
        _watch_and_cleanup_chromium(unique_user_data, html_file)

    except Exception as e:
        print(f"Error launching Chromium: {e}")
        import traceback
        traceback.print_exc()
        return


class ThumbnailWidget(Gtk.FlowBoxChild):
    """Widget for displaying a single thumbnail with filename."""
    
    def __init__(self, filepath: str, is_video: bool = False, move_callback=None, single_click_open: bool = False, show_star: bool = False, best_callback=None, is_best_tab: bool = False, image_list: list = None, current_index: int = 0):
        super().__init__()
        self.filepath = filepath
        self.is_video = is_video
        self.move_callback = move_callback
        self.single_click_open = single_click_open
        self.best_callback = best_callback
        self.show_star = show_star
        self.is_best_tab = is_best_tab
        self.image_list = image_list  # List of image filepaths for navigation (Explore tab only)
        self.current_index = current_index  # Current index in image_list
        
        # Create vertical box for thumbnail and label
        vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        vbox.set_margin_top(8)
        vbox.set_margin_bottom(8)
        vbox.set_margin_start(8)
        vbox.set_margin_end(8)
        
        # Create image widget for thumbnail
        self.image = Gtk.Picture()
        self.image.set_size_request(THUMBNAIL_SIZE, THUMBNAIL_SIZE)
        self.image.set_content_fit(Gtk.ContentFit.COVER)
        self.image.set_can_shrink(False)
        
        vbox.append(self.image)
        
        # Create horizontal box for icon and filename
        name_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=4)
        name_box.set_halign(Gtk.Align.CENTER)
        
        # Create icon to indicate file type
        try:
            icon_name = "video-x-generic" if is_video else "image-x-generic"
            icon = Gtk.Image.new_from_icon_name(icon_name)
            if icon.get_gicon() is None:
                # Icon not found, try alternative names
                icon_name = "video" if is_video else "image"
                icon = Gtk.Image.new_from_icon_name(icon_name)
            
            icon.set_pixel_size(16)  # Smaller icon size for next to text
            name_box.append(icon)
        except Exception as e:
            # If icon creation fails, just continue without icon
            print(f"Warning: Could not create icon: {e}")
        
        # Create label for filename with smart truncation
        filename = os.path.basename(filepath)
        truncated_filename = self._truncate_filename(filename, max_length=20)
        self.label = Gtk.Label(label=truncated_filename)
        self.label.set_max_width_chars(20)
        self.label.set_ellipsize(3)  # Ellipsize at end
        self.label.set_wrap(True)
        self.label.set_justify(Gtk.Justification.CENTER)
        name_box.append(self.label)
        
        # Add star icon if file is in best.json
        self.star_icon = None
        if show_star:
            try:
                self.star_icon = Gtk.Image.new_from_icon_name("starred-symbolic")
                if self.star_icon.get_gicon() is None:
                    # Fallback to non-symbolic icon
                    self.star_icon = Gtk.Image.new_from_icon_name("starred")
                self.star_icon.set_pixel_size(16)
                name_box.append(self.star_icon)
            except Exception:
                self.star_icon = None
        
        vbox.append(name_box)
        
        self.set_child(vbox)
        
        # Make clickable
        self.set_css_classes(['thumbnail-item'])
        
        # Connect gesture for Ctrl+Click and Ctrl+Shift+Click selection
        gesture = Gtk.GestureClick()
        gesture.set_button(1)  # Left mouse button
        gesture.connect('pressed', self.on_gesture_pressed)
        self.add_controller(gesture)
        
        # Connect right-click gesture for context menu
        right_click_gesture = Gtk.GestureClick()
        right_click_gesture.set_button(3)  # Right mouse button
        right_click_gesture.connect('pressed', self.on_right_click)
        self.add_controller(right_click_gesture)
    
    def _truncate_filename(self, filename: str, max_length: int = 20) -> str:
        """Truncate filename to show beginning and extension with ... in between.
        
        Example: '349782897234789324.png' -> '349782...24.png'
        """
        if len(filename) <= max_length:
            return filename
        
        # Split filename and extension
        name, ext = os.path.splitext(filename)
        
        # Calculate how much space we have
        # max_length - len(ext) - 3 (for "...") = space for name parts
        available_chars = max_length - len(ext) - 3
        
        if available_chars < 2:
            # If extension is very long, just truncate normally
            return filename[:max_length-3] + "..."
        
        # Favor start more - use 2/3 for start, 1/3 for end
        start_chars = (available_chars * 2) // 3
        end_chars = available_chars - start_chars
        
        # Ensure we have at least 2 chars for end part
        if end_chars < 2:
            end_chars = 2
            start_chars = available_chars - end_chars
        
        # Get start and end of name
        start_part = name[:start_chars]
        end_part = name[-end_chars:] if len(name) > start_chars else ""
        
        # Combine: start...end.ext
        return f"{start_part}...{end_part}{ext}"
    
    def set_thumbnail(self, pixbuf):
        """Set the thumbnail pixbuf."""
        if pixbuf:
            self.image.set_pixbuf(pixbuf)
        else:
            # Use placeholder
            placeholder = create_placeholder_thumbnail()
            self.image.set_pixbuf(placeholder)
    
    def update_star_icon(self, show: bool):
        """Update star icon visibility."""
        if show and not self.show_star:
            # Add star icon
            if self.star_icon is None:
                self.star_icon = Gtk.Image.new_from_icon_name("starred-symbolic")
                if self.star_icon.get_gicon() is None:
                    self.star_icon = Gtk.Image.new_from_icon_name("starred")
                self.star_icon.set_pixel_size(16)
                # Find the name_box and add star after label
                name_box = self.label.get_parent()
                if name_box:
                    name_box.append(self.star_icon)
            self.show_star = True
        elif not show and self.show_star:
            # Remove star icon
            if self.star_icon:
                name_box = self.star_icon.get_parent()
                if name_box:
                    name_box.remove(self.star_icon)
                self.star_icon = None
            self.show_star = False
    
    def _get_parent_window(self) -> Gtk.Window | None:
        widget = self
        while widget:
            widget = widget.get_parent()
            if isinstance(widget, Gtk.Window):
                return widget
        return None

    def _open_video(self, click_x: float, click_y: float, click_mouse=None):
        parent = self._get_parent_window()
        if click_mouse is None:
            click_mouse = _mouse_screen_position()
        open_file_with_chromium(
            self.filepath,
            parent,
            click_widget=self,
            click_x=click_x,
            click_y=click_y,
            click_mouse=click_mouse,
        )

    def on_gesture_pressed(self, gesture, n_press, x, y):
        """Handle click gesture for selection."""
        # Get the FlowBox parent
        flowbox = self.get_parent()
        if not isinstance(flowbox, Gtk.FlowBox):
            return
        
        # Single click open (for explore/best tabs) - skip all selection logic
        if self.single_click_open and n_press == 1:
            # Add blue highlight CSS class
            self.add_css_class('selected')
            click_mouse = _mouse_screen_position()
            
            # Open file after a brief delay to show highlight
            def open_file(cx=x, cy=y, mouse=click_mouse):
                if self.is_video:
                    self._open_video(cx, cy, mouse)
                else:
                    parent = self._get_parent_window()
                    # If image_list is provided (Explore tab), use it for keyboard navigation
                    open_image_with_constraints(self.filepath, parent, self.image_list, self.current_index)
                # Remove highlight after opening
                GLib.timeout_add(300, lambda: self.remove_css_class('selected'))
            
            GLib.timeout_add(100, open_file)
            return
        
        # For non-explore tabs, handle selection logic
        # Get modifier keys from the gesture
        ctrl_pressed = False
        shift_pressed = False
        try:
            state = gesture.get_current_event_state()
            if state:
                ctrl_pressed = bool(state & Gdk.ModifierType.CONTROL_MASK)
                shift_pressed = bool(state & Gdk.ModifierType.SHIFT_MASK)
        except (AttributeError, TypeError, ValueError):
            # Fallback if state cannot be retrieved
            pass
        
        is_selected = self in flowbox.get_selected_children()
        
        if n_press == 2:
            # Double click: open file
            if self.is_video:
                self._open_video(x, y, _mouse_screen_position())
            else:
                parent = self._get_parent_window()
                open_image_with_constraints(self.filepath, parent)
            return
        
        if ctrl_pressed and shift_pressed:
            # Ctrl+Shift+Click: range selection (select from last selected to this)
            selected = flowbox.get_selected_children()
            if selected:
                # Find the range and select all items in between
                all_children = list(flowbox)
                try:
                    last_selected_idx = all_children.index(selected[-1])
                    current_idx = all_children.index(self)
                    start_idx = min(last_selected_idx, current_idx)
                    end_idx = max(last_selected_idx, current_idx)
                    for i in range(start_idx, end_idx + 1):
                        flowbox.select_child(all_children[i])
                except ValueError:
                    flowbox.select_child(self)
            else:
                flowbox.select_child(self)
        elif ctrl_pressed:
            # Ctrl+Click: toggle selection
            if is_selected:
                flowbox.unselect_child(self)
            else:
                flowbox.select_child(self)
        else:
            # Regular click: toggle if already selected, otherwise single selection
            if is_selected:
                # Click on selected item: unselect it
                flowbox.unselect_child(self)
            else:
                # Click on unselected item: clear others and select this one
                flowbox.unselect_all()
                flowbox.select_child(self)
    
    def on_right_click(self, gesture, n_press, x, y):
        """Handle right-click to open move dialog or best.json menu."""
        if n_press == 1:  # Single right-click
            # Get the FlowBox parent
            flowbox = self.get_parent()
            if not isinstance(flowbox, Gtk.FlowBox):
                return
            
            # If best_callback is available (Explore or Best tab), show best.json menu
            if self.best_callback:
                # Determine if file is in best.json
                # In Best tab: all files are in best.json, so always show "Remove from Best"
                # In Explore tab: use show_star value to determine add/remove
                if getattr(self, 'is_best_tab', False):
                    # Best tab - all files are in best.json
                    is_in_best = True
                else:
                    # Explore tab - use show_star value which reflects actual status
                    is_in_best = getattr(self, 'show_star', False)
                self._show_best_menu(gesture, x, y, is_in_best)
            # Otherwise, show move dialog if callback is available
            elif self.move_callback:
                # Select this item if not already selected
                if self not in flowbox.get_selected_children():
                    flowbox.unselect_all()
                    flowbox.select_child(self)
                self.move_callback()
    
    def _show_best_menu(self, gesture, x, y, is_in_best: bool = None):
        """Show context menu for adding/removing from best.json."""
        # Create a simple popover menu
        popover = Gtk.Popover()
        popover.set_parent(self)
        popover.set_position(Gtk.PositionType.BOTTOM)
        
        # Create menu box
        menu_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        menu_box.set_margin_top(8)
        menu_box.set_margin_bottom(8)
        menu_box.set_margin_start(8)
        menu_box.set_margin_end(8)
        
        # Check if file is in best.json (use provided value or check show_star)
        if is_in_best is None:
            is_in_best = getattr(self, 'show_star', False)
        
        if is_in_best:
            menu_item = Gtk.Button(label="Remove from Best")
            menu_item.connect('clicked', lambda b: self._on_best_menu_action(False, popover))
        else:
            menu_item = Gtk.Button(label="Add to Best")
            menu_item.connect('clicked', lambda b: self._on_best_menu_action(True, popover))
        
        menu_box.append(menu_item)
        popover.set_child(menu_box)
        popover.popup()
    
    def _on_best_menu_action(self, add: bool, popover: Gtk.Popover):
        """Handle best menu action."""
        popover.popdown()
        if self.best_callback:
            self.best_callback(self.filepath, add)


class MainWindow(Gtk.ApplicationWindow):
    """Main application window."""
    
    def __init__(
        self,
        app,
        media_directory: str,
        explore_directory: str = None,
        explore_directory_features: str = None,
        photo_drive: str = None,
    ):
        super().__init__(application=app, title="Photo Organizer")
        self.media_directory = media_directory
        self.explore_directory = explore_directory or media_directory
        self.explore_directory_features = explore_directory_features
        self.photo_drive = photo_drive or self.explore_directory or media_directory
        self.set_default_size(1200, 1600)
        # Initialize best.json path (under ~/.config/gtk-apps/gtk-photos/)
        from .config_paths import best_file

        project_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        self.best_json_path = str(best_file())
        # Cache for best files list
        self._best_files_cache = None
        
        # Track all subprocesses and threads for clean shutdown
        self._subprocesses = []  # List of subprocess.Popen instances
        self._threads = []  # List of threading.Thread instances
        self._image_viewer_windows = []  # List of ImageViewerWindow instances
        self._shutting_down = False  # Flag to prevent new operations during shutdown

        # Add CSS for selection highlighting
        css = """
        .thumbnail-item.selected {
            background-color: rgba(78, 154, 6, 0.3);
            border: 2px solid rgb(78, 154, 6);
        }
        .favorite-dialog-item {
            padding: 8px;
        }
        .recent-destination-btn {
            padding: 4px 10px;
        }
        """
        css_provider = Gtk.CssProvider()
        css_provider.load_from_data(css.encode())
        Gtk.StyleContext.add_provider_for_display(
            self.get_display(),
            css_provider,
            Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
        )
        
        header = Gtk.HeaderBar()
        header.set_show_title_buttons(True)
        header.set_title_widget(Gtk.Label(label="Photo Organizer"))
        self.set_titlebar(header)
        gtk_theme.attach_profile_menu(
            self,
            header,
            about_name="GTK Photos",
            about_comments="Photo organizer for browsing drives, favorites, folders, and website thumbnails.",
        )

        # Create main container
        main_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        main_box.set_hexpand(True)
        main_box.set_vexpand(True)
        self.set_child(main_box)
        
        # Create notebook for tabs
        self.notebook = Gtk.Notebook()
        self.notebook.set_tab_pos(Gtk.PositionType.TOP)
        self.notebook.set_hexpand(True)
        self.notebook.set_vexpand(True)
        main_box.append(self.notebook)
        
        # Create tabs for images and videos
        self.images_flowbox = Gtk.FlowBox()
        self.images_flowbox.set_selection_mode(Gtk.SelectionMode.MULTIPLE)
        self.images_flowbox.set_max_children_per_line(5)
        self.images_flowbox.set_column_spacing(8)
        self.images_flowbox.set_row_spacing(8)
        self.images_flowbox.set_hexpand(True)
        self.images_flowbox.set_vexpand(True)
        self.images_flowbox.set_halign(Gtk.Align.FILL)
        self.images_flowbox.set_valign(Gtk.Align.FILL)
        self.images_flowbox.connect('selected-children-changed', self._on_images_selection_changed)
        
        self.videos_flowbox = Gtk.FlowBox()
        self.videos_flowbox.set_selection_mode(Gtk.SelectionMode.MULTIPLE)
        self.videos_flowbox.set_max_children_per_line(5)
        self.videos_flowbox.set_column_spacing(8)
        self.videos_flowbox.set_row_spacing(8)
        self.videos_flowbox.set_hexpand(True)
        self.videos_flowbox.set_vexpand(True)
        self.videos_flowbox.set_halign(Gtk.Align.FILL)
        self.videos_flowbox.set_valign(Gtk.Align.FILL)
        self.videos_flowbox.connect('selected-children-changed', self._on_videos_selection_changed)
        
        # Create containers for images tab with button
        images_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        images_container.set_hexpand(True)
        images_container.set_vexpand(True)
        
        # Button bar for images
        images_buttons = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        images_buttons.set_margin_start(8)
        images_buttons.set_margin_end(8)
        images_buttons.set_margin_top(8)
        images_buttons.set_margin_bottom(8)
        
        self.images_move_btn = Gtk.Button(label="Move")
        self.images_move_btn.connect('clicked', lambda btn: self._trigger_move_images())
        self.images_move_btn.set_sensitive(False)
        images_buttons.append(self.images_move_btn)
        
        self.images_trash_btn = Gtk.Button(label="Move to Trash")
        self.images_trash_btn.connect('clicked', lambda btn: self._trigger_trash_images())
        self.images_trash_btn.set_sensitive(False)
        images_buttons.append(self.images_trash_btn)

        self.images_rotate_left_btn = Gtk.Button(label="Rotate left")
        self.images_rotate_left_btn.connect(
            'clicked', lambda btn: self._rotate_images(clockwise=False)
        )
        self.images_rotate_left_btn.set_sensitive(False)
        images_buttons.append(self.images_rotate_left_btn)

        self.images_rotate_right_btn = Gtk.Button(label="Rotate right")
        self.images_rotate_right_btn.connect(
            'clicked', lambda btn: self._rotate_images(clockwise=True)
        )
        self.images_rotate_right_btn.set_sensitive(False)
        images_buttons.append(self.images_rotate_right_btn)
        
        images_container.append(images_buttons)
        
        images_scrolled = Gtk.ScrolledWindow()
        images_scrolled.set_child(self.images_flowbox)
        images_scrolled.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)
        images_scrolled.set_hexpand(True)
        images_scrolled.set_vexpand(True)
        images_scrolled.set_halign(Gtk.Align.FILL)
        images_scrolled.set_valign(Gtk.Align.FILL)
        images_container.append(images_scrolled)
        
        # Create containers for videos tab with button
        videos_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        videos_container.set_hexpand(True)
        videos_container.set_vexpand(True)
        
        # Button bar for videos
        videos_buttons = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        videos_buttons.set_margin_start(8)
        videos_buttons.set_margin_end(8)
        videos_buttons.set_margin_top(8)
        videos_buttons.set_margin_bottom(8)
        
        self.videos_move_btn = Gtk.Button(label="Move")
        self.videos_move_btn.connect('clicked', lambda btn: self._trigger_move_videos())
        self.videos_move_btn.set_sensitive(False)
        videos_buttons.append(self.videos_move_btn)
        
        self.videos_trash_btn = Gtk.Button(label="Move to Trash")
        self.videos_trash_btn.connect('clicked', lambda btn: self._trigger_trash_videos())
        self.videos_trash_btn.set_sensitive(False)
        videos_buttons.append(self.videos_trash_btn)
        
        videos_container.append(videos_buttons)
        
        videos_scrolled = Gtk.ScrolledWindow()
        videos_scrolled.set_child(self.videos_flowbox)
        videos_scrolled.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)
        videos_scrolled.set_hexpand(True)
        videos_scrolled.set_vexpand(True)
        videos_scrolled.set_halign(Gtk.Align.FILL)
        videos_scrolled.set_valign(Gtk.Align.FILL)
        videos_container.append(videos_scrolled)
        
        # Create Explore tab (before Images)
        explore_container = self._create_explore_tab()
        explore_label = Gtk.Label(label="Explore")
        self.notebook.insert_page(explore_container, explore_label, 0)
        
        # Create Best tab (after Explore)
        best_container = self._create_best_tab()
        best_label = Gtk.Label(label="Best")
        self.notebook.insert_page(best_container, best_label, 1)

        self.folders_panel = FoldersPanel(
            self, self.photo_drive, project_root, self._threads
        )
        folders_container = self.folders_panel.build()
        folders_label = Gtk.Label(label="Folders")
        self.notebook.insert_page(folders_container, folders_label, 2)

        self.websites_panel = WebsitesPanel(self, self.photo_drive, project_root)
        websites_container = self.websites_panel.build()
        websites_label = Gtk.Label(label="Websites")
        self.notebook.insert_page(websites_container, websites_label, 3)
        
        # Add tabs
        images_label = Gtk.Label(label="New Images")
        videos_label = Gtk.Label(label="New Videos")
        
        self.notebook.append_page(images_container, images_label)
        self.notebook.append_page(videos_container, videos_label)
        
        # Load media files
        self.load_media_files()
        
        # Load explore tab data
        self.load_explore_files()
        
        # Load best tab data
        self.load_best_files()

        # Load folders tab (one sample per Photos folder)
        self.folders_panel.load()
        
        # Add shutdown button in bottom right corner
        shutdown_container = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL)
        shutdown_container.set_hexpand(True)
        shutdown_container.set_halign(Gtk.Align.END)
        shutdown_container.set_margin_start(8)
        shutdown_container.set_margin_end(8)
        shutdown_container.set_margin_top(8)
        shutdown_container.set_margin_bottom(8)
        
        shutdown_btn = Gtk.Button(label="Close all")
        shutdown_btn.set_css_classes(['destructive-action'])
        shutdown_btn.connect('clicked', lambda btn: self._shutdown())
        shutdown_container.append(shutdown_btn)
        
        main_box.append(shutdown_container)
        
        # Handle window close event
        self.connect('close-request', self._on_close_request)
    
    def _on_close_request(self, window):
        """Handle window close event - trigger clean shutdown."""
        self._shutdown()
        return True  # Prevent default close behavior
    
    def load_media_files(self):
        """Load and display media files from the directory."""
        # Run file scanning in a separate thread to avoid blocking UI
        thread = threading.Thread(target=self._scan_and_load_files)
        thread.daemon = True
        self._threads.append(thread)
        thread.start()
    
    def _scan_and_load_files(self):
        """Scan directory and load thumbnails (runs in background thread)."""
        image_files, video_files = get_media_files(self.media_directory)
        
        # Load image thumbnails
        for filepath in image_files:
            GLib.idle_add(self._add_image_thumbnail, filepath)
        
        # Load video thumbnails
        for filepath in video_files:
            GLib.idle_add(self._add_video_thumbnail, filepath)
    
    def _add_image_thumbnail(self, filepath: str):
        """Add an image thumbnail to the UI (called from main thread)."""
        widget = ThumbnailWidget(filepath, is_video=False, move_callback=self._trigger_move_images)
        self.images_flowbox.append(widget)
        
        # Generate thumbnail in background
        thread = threading.Thread(target=self._generate_and_set_thumbnail, args=(widget, filepath, False))
        thread.daemon = True
        self._threads.append(thread)
        thread.start()
    
    def _add_video_thumbnail(self, filepath: str):
        """Add a video thumbnail to the UI (called from main thread)."""
        widget = ThumbnailWidget(filepath, is_video=True, move_callback=self._trigger_move_videos)
        self.videos_flowbox.append(widget)
        
        # Generate thumbnail in background
        thread = threading.Thread(target=self._generate_and_set_thumbnail, args=(widget, filepath, True))
        thread.daemon = True
        self._threads.append(thread)
        thread.start()
    
    def _generate_and_set_thumbnail(self, widget: ThumbnailWidget, filepath: str, is_video: bool):
        """Generate thumbnail and update widget (runs in background thread)."""
        if is_video:
            pixbuf = generate_video_thumbnail(filepath)
        else:
            pixbuf = generate_image_thumbnail(filepath)
        
        # Update UI from main thread
        GLib.idle_add(widget.set_thumbnail, pixbuf)
    
    def _on_images_selection_changed(self, flowbox):
        """Handle selection change in images FlowBox."""
        selected = flowbox.get_selected_children()
        
        # Update button sensitivity
        has_selection = len(selected) > 0
        if hasattr(self, 'images_move_btn'):
            self.images_move_btn.set_sensitive(has_selection)
        if hasattr(self, 'images_trash_btn'):
            self.images_trash_btn.set_sensitive(has_selection)
        if hasattr(self, 'images_rotate_left_btn'):
            self.images_rotate_left_btn.set_sensitive(has_selection)
        if hasattr(self, 'images_rotate_right_btn'):
            self.images_rotate_right_btn.set_sensitive(has_selection)
        
        # Update highlighting
        for child in flowbox:
            if child in selected:
                child.add_css_class('selected')
            else:
                child.remove_css_class('selected')
    
    def _on_videos_selection_changed(self, flowbox):
        """Handle selection change in videos FlowBox."""
        selected = flowbox.get_selected_children()
        
        # Update button sensitivity
        has_selection = len(selected) > 0
        if hasattr(self, 'videos_move_btn'):
            self.videos_move_btn.set_sensitive(has_selection)
        if hasattr(self, 'videos_trash_btn'):
            self.videos_trash_btn.set_sensitive(has_selection)
        
        # Update highlighting
        for child in flowbox:
            if child in selected:
                child.add_css_class('selected')
            else:
                child.remove_css_class('selected')
    
    def _trigger_move_images(self):
        """Trigger move dialog for images (called from right-click or button)."""
        selected = self.images_flowbox.get_selected_children()
        if selected:
            files = [child.filepath for child in selected]
            self._show_move_dialog(files, 'images', self.images_flowbox, selected)
    
    def _trigger_move_videos(self):
        """Trigger move dialog for videos (called from right-click or button)."""
        selected = self.videos_flowbox.get_selected_children()
        if selected:
            files = [child.filepath for child in selected]
            self._show_move_dialog(files, 'videos', self.videos_flowbox, selected)
    
    def _trigger_trash_images(self):
        """Move selected images to trash."""
        selected = self.images_flowbox.get_selected_children()
        if selected:
            files = [child.filepath for child in selected]
            self._move_to_trash(files, 'images', self.images_flowbox, selected)

    def _rotate_images(self, clockwise: bool):
        """Rotate selected images 90° left or right and refresh thumbnails."""
        selected = self.images_flowbox.get_selected_children()
        if not selected:
            return

        def work():
            errors = []
            for widget in selected:
                if rotate_image_file(widget.filepath, clockwise):
                    self._generate_and_set_thumbnail(widget, widget.filepath, False)
                else:
                    errors.append(os.path.basename(widget.filepath))
            if errors:
                GLib.idle_add(self._show_rotate_errors, errors)

        thread = threading.Thread(target=work, daemon=True)
        self._threads.append(thread)
        thread.start()

    def _show_rotate_errors(self, filenames: list):
        msg = "Could not rotate:\n" + "\n".join(filenames[:8])
        if len(filenames) > 8:
            msg += f"\n… and {len(filenames) - 8} more"
        dialog = Gtk.MessageDialog(
            transient_for=self,
            message_type=Gtk.MessageType.WARNING,
            buttons=Gtk.ButtonsType.OK,
            text="Some images could not be rotated",
            secondary_text=msg,
        )
        dialog.connect('response', lambda d, _r: d.destroy())
        dialog.present()
        return False
    
    def _trigger_trash_videos(self):
        """Move selected videos to trash."""
        selected = self.videos_flowbox.get_selected_children()
        if selected:
            files = [child.filepath for child in selected]
            self._move_to_trash(files, 'videos', self.videos_flowbox, selected)
    
    def _move_to_trash(self, files: list, file_type: str, flowbox: Gtk.FlowBox, selected_widgets: list):
        """Move files to trash and remove from UI."""
        if not files:
            return
        
        # Confirm deletion
        dialog = Gtk.MessageDialog(
            transient_for=self,
            message_type=Gtk.MessageType.WARNING,
            buttons=Gtk.ButtonsType.YES_NO,
            text=f"Move {len(files)} {file_type} to trash?",
            secondary_text="This will move the files to your system trash. They can be recovered from the trash if needed."
        )
        
        def on_response(dialog, response_id):
            if response_id == Gtk.ResponseType.YES:
                # Move files to trash
                for filepath in files:
                    try:
                        # Use Gio.File to move to trash
                        file = Gio.File.new_for_path(filepath)
                        file.trash(None)  # None means use default cancellable
                    except Exception as e:
                        print(f"Error moving {filepath} to trash: {e}")
                        # Fallback: try using send2trash if available
                        try:
                            import send2trash
                            send2trash.send2trash(filepath)
                        except ImportError:
                            # Last resort: delete permanently (not ideal)
                            try:
                                os.remove(filepath)
                            except Exception as e2:
                                print(f"Error deleting {filepath}: {e2}")
                
                # Remove widgets from UI
                for widget in selected_widgets:
                    flowbox.remove(widget)
            
            dialog.destroy()
        
        dialog.connect('response', on_response)
        dialog.present()
    
    
    def _remember_move_destination(self, folder_path: str) -> None:
        from .favorites import add_recent_folder

        add_recent_folder(os.path.normpath(folder_path))

    def _recent_destinations_for_filter(self, folder_filter) -> list[str]:
        from .favorites import generate_favorite_title, load_recent_folders

        recent = []
        for item in load_recent_folders(max_items=20):
            path = item.get("path")
            if not path or not os.path.isdir(path):
                continue
            if folder_filter is not None and not folder_filter(path):
                continue
            recent.append(path)
        recent.sort(
            key=lambda p: generate_favorite_title(p, self.photo_drive).upper()
        )
        return recent

    def _show_move_dialog(self, files: list, file_type: str, flowbox: Gtk.FlowBox, selected_widgets: list):
        """Show dialog to select destination folder for moving files."""
        from .favorites import load_config
        from .folder_picker import (
            _path_contains_video,
            show_destination_folder_dialog,
        )

        config = load_config()
        photo_drive = config.get("photo_drive", "")
        if file_type == "images":
            folder_filter = lambda path: not _path_contains_video(path)
            hint = "Showing folders without VIDEO in the path."
        elif file_type == "videos":
            folder_filter = lambda path: _path_contains_video(path)
            hint = "Showing folders with VIDEO in the path."
        else:
            folder_filter = None
            hint = None

        def on_dest_selected(dest: str):
            self._remember_move_destination(dest)
            self._generate_move_list(files, dest, flowbox, selected_widgets)

        show_destination_folder_dialog(
            self,
            photo_drive,
            prompt=f"Select a destination folder to move {len(files)} {file_type}:",
            hint=hint,
            ok_label="Move Files",
            on_selected=on_dest_selected,
            threads=self._threads,
            folder_filter=folder_filter,
            recent_destinations=self._recent_destinations_for_filter(folder_filter),
            allow_create_folder=True,
        )
    
    def _generate_move_list(self, files: list, dest_folder: str, flowbox: Gtk.FlowBox, selected_widgets: list):
        """Move files to destination folder and remove items from list."""
        # Ensure destination folder exists
        if not os.path.exists(dest_folder):
            try:
                os.makedirs(dest_folder, exist_ok=True)
            except Exception as e:
                dialog = Gtk.MessageDialog(
                    transient_for=self,
                    message_type=Gtk.MessageType.ERROR,
                    buttons=Gtk.ButtonsType.OK,
                    text=f"Error creating destination folder\n\nCould not create destination folder: {str(e)}"
                )
                dialog.connect('response', lambda d, r: d.destroy())
                dialog.present()
                return
        
        # Move files in a background thread to avoid blocking UI
        def move_files():
            moved_count = 0
            errors = []
            
            for file_path in files:
                try:
                    if not os.path.exists(file_path):
                        errors.append(f"Source file not found: {os.path.basename(file_path)}")
                        continue
                    
                    filename = os.path.basename(file_path)
                    dest_path = os.path.join(dest_folder, filename)
                    
                    # Handle file name conflicts
                    if os.path.exists(dest_path):
                        base, ext = os.path.splitext(filename)
                        counter = 1
                        while os.path.exists(dest_path):
                            new_filename = f"{base}_{counter}{ext}"
                            dest_path = os.path.join(dest_folder, new_filename)
                            counter += 1
                    
                    shutil.move(file_path, dest_path)
                    moved_count += 1
                except Exception as e:
                    errors.append(f"{os.path.basename(file_path)}: {str(e)}")
            
            # Update UI from main thread
            GLib.idle_add(self._on_files_moved, moved_count, len(files), errors, flowbox, selected_widgets)
        
        # Show progress dialog
        progress_dialog = Gtk.MessageDialog(
            transient_for=self,
            message_type=Gtk.MessageType.INFO,
            buttons=Gtk.ButtonsType.NONE,
            text=f"Moving files...\n\nMoving {len(files)} file(s) to {os.path.basename(dest_folder)}..."
        )
        progress_dialog.present()
        
        # Start moving files in background
        thread = threading.Thread(target=move_files)
        thread.daemon = True
        self._threads.append(thread)
        thread.start()
        
        # Store progress dialog reference
        self._progress_dialog = progress_dialog
    
    def _on_files_moved(self, moved_count: int, total_count: int, errors: list, flowbox: Gtk.FlowBox, selected_widgets: list):
        """Handle completion of file moving operation."""
        # Close progress dialog
        if hasattr(self, '_progress_dialog'):
            self._progress_dialog.destroy()
            delattr(self, '_progress_dialog')
        
        # Remove selected items from the FlowBox
        for widget in selected_widgets:
            flowbox.remove(widget)
        
        # Show result message only if there were errors
        if moved_count != total_count:
            error_msg = f"Moved {moved_count} of {total_count} file(s).\n\nErrors:\n" + "\n".join(errors[:5])
            if len(errors) > 5:
                error_msg += f"\n... and {len(errors) - 5} more errors"
            dialog = Gtk.MessageDialog(
                transient_for=self,
                message_type=Gtk.MessageType.WARNING,
                buttons=Gtk.ButtonsType.OK,
                text=f"Files moved with errors\n\n{error_msg}"
            )
            dialog.connect('response', lambda d, r: d.destroy())
            dialog.present()
    
    def _create_explore_tab(self):
        """Create the Explore tab with paginated recent media files."""
        # Main container
        main_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        main_container.set_hexpand(True)
        main_container.set_vexpand(True)
        
        # Header with title and pagination
        header_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        header_box.set_margin_start(8)
        header_box.set_margin_end(8)
        header_box.set_margin_top(8)
        
        # Title label showing it's sorted by most recent
        title_label = Gtk.Label(label="Most Recent Media Files (sorted by modification date)")
        title_label.set_halign(Gtk.Align.START)
        title_label.add_css_class('title-4')
        header_box.append(title_label)
        
        # Filter controls
        filter_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        filter_box.set_margin_top(4)
        
        # Search filter entry
        search_label = Gtk.Label(label="Filter by path:")
        search_label.set_halign(Gtk.Align.START)
        self.explore_search_entry = Gtk.SearchEntry()
        self.explore_search_entry.set_placeholder_text("Enter text to filter by file path...")
        self.explore_search_entry.set_hexpand(True)
        self.explore_search_entry.connect('search-changed', self._on_explore_search_changed)
        filter_box.append(search_label)
        filter_box.append(self.explore_search_entry)
        
        # Media type toggle buttons
        self.explore_images_toggle = Gtk.ToggleButton(label="Images Only")
        self.explore_images_toggle.set_tooltip_text("Show only image files")
        self.explore_images_toggle.connect('toggled', self._on_explore_images_toggled)
        filter_box.append(self.explore_images_toggle)
        
        self.explore_videos_toggle = Gtk.ToggleButton(label="Videos Only")
        self.explore_videos_toggle.set_tooltip_text("Show only video files")
        self.explore_videos_toggle.connect('toggled', self._on_explore_videos_toggled)
        filter_box.append(self.explore_videos_toggle)
        
        # FEATURES toggle button
        if self.explore_directory_features:
            self.explore_features_toggle = Gtk.ToggleButton(label="Features only")
            self.explore_features_toggle.set_tooltip_text(
                f"Show only files from {self.explore_directory_features}"
            )
            self.explore_features_toggle.connect('toggled', self._on_explore_features_toggled)
            filter_box.append(self.explore_features_toggle)
        else:
            self.explore_features_toggle = None
        
        # Random toggle button
        self.explore_random_toggle = Gtk.ToggleButton(label="Random")
        self.explore_random_toggle.set_tooltip_text("Randomly shuffle the filtered results")
        self.explore_random_toggle.set_visible(True)
        self.explore_random_toggle.connect('toggled', self._on_explore_random_toggled)
        filter_box.append(self.explore_random_toggle)
        
        # Make sure filter_box is added to header
        header_box.append(filter_box)
        
        # Pagination controls
        pagination_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        
        self.explore_prev_btn = Gtk.Button(label="Previous")
        self.explore_prev_btn.connect('clicked', self._on_explore_prev_clicked)
        self.explore_prev_btn.set_sensitive(False)
        pagination_box.append(self.explore_prev_btn)
        
        self.explore_page_label = Gtk.Label(label="Page 1 of 1")
        pagination_box.append(self.explore_page_label)
        
        self.explore_next_btn = Gtk.Button(label="Next")
        self.explore_next_btn.connect('clicked', self._on_explore_next_clicked)
        self.explore_next_btn.set_sensitive(False)
        pagination_box.append(self.explore_next_btn)
        
        header_box.append(pagination_box)
        main_container.append(header_box)
        
        # FlowBox for thumbnails
        self.explore_flowbox = Gtk.FlowBox()
        self.explore_flowbox.set_selection_mode(Gtk.SelectionMode.NONE)  # No selection in Explore tab
        self.explore_flowbox.set_max_children_per_line(5)
        self.explore_flowbox.set_column_spacing(8)
        self.explore_flowbox.set_row_spacing(8)
        self.explore_flowbox.set_hexpand(True)
        self.explore_flowbox.set_vexpand(True)
        self.explore_flowbox.set_halign(Gtk.Align.FILL)
        self.explore_flowbox.set_valign(Gtk.Align.FILL)
        
        
        # Scrolled window for thumbnails
        explore_scrolled = Gtk.ScrolledWindow()
        explore_scrolled.set_child(self.explore_flowbox)
        explore_scrolled.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)
        explore_scrolled.set_hexpand(True)
        explore_scrolled.set_vexpand(True)
        explore_scrolled.set_halign(Gtk.Align.FILL)
        explore_scrolled.set_valign(Gtk.Align.FILL)
        main_container.append(explore_scrolled)
        
        # Store explore data
        self.explore_all_files = []  # List of (filepath, is_video) tuples - all files from directory
        self.explore_filtered_files = []  # List of (filepath, is_video) tuples - after filtering
        self.explore_current_page = 1
        self.explore_items_per_page = 100
        self.explore_filter_text = ""
        self.explore_features_only = False
        self.explore_images_only = False
        self.explore_videos_only = False
        self.explore_random_mode = False
        
        return main_container
    
    def _create_best_tab(self):
        """Create the Best tab showing files from best.json with pagination."""
        # Main container
        main_container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        main_container.set_hexpand(True)
        main_container.set_vexpand(True)
        
        # Header with title and pagination
        header_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        header_box.set_margin_start(8)
        header_box.set_margin_end(8)
        header_box.set_margin_top(8)
        
        # Title label
        title_label = Gtk.Label(label="Best Files")
        title_label.set_halign(Gtk.Align.START)
        title_label.add_css_class('title-4')
        header_box.append(title_label)
        
        # Filter controls (same as Explore)
        best_filter_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        best_filter_box.set_margin_top(4)
        
        best_search_label = Gtk.Label(label="Filter by path:")
        best_search_label.set_halign(Gtk.Align.START)
        self.best_search_entry = Gtk.SearchEntry()
        self.best_search_entry.set_placeholder_text("Enter text to filter by file path...")
        self.best_search_entry.set_hexpand(True)
        self.best_search_entry.connect('search-changed', self._on_best_search_changed)
        best_filter_box.append(best_search_label)
        best_filter_box.append(self.best_search_entry)
        
        self.best_images_toggle = Gtk.ToggleButton(label="Images Only")
        self.best_images_toggle.set_tooltip_text("Show only image files")
        self.best_images_toggle.connect('toggled', self._on_best_images_toggled)
        best_filter_box.append(self.best_images_toggle)
        
        self.best_videos_toggle = Gtk.ToggleButton(label="Videos Only")
        self.best_videos_toggle.set_tooltip_text("Show only video files")
        self.best_videos_toggle.connect('toggled', self._on_best_videos_toggled)
        best_filter_box.append(self.best_videos_toggle)
        
        if self.explore_directory_features:
            self.best_features_toggle = Gtk.ToggleButton(label="Features only")
            self.best_features_toggle.set_tooltip_text(
                f"Show only files under {self.explore_directory_features}"
            )
            self.best_features_toggle.connect('toggled', self._on_best_features_toggled)
            best_filter_box.append(self.best_features_toggle)
        else:
            self.best_features_toggle = None
        
        self.best_random_toggle = Gtk.ToggleButton(label="Random")
        self.best_random_toggle.set_tooltip_text("Randomly shuffle the filtered results")
        self.best_random_toggle.connect('toggled', self._on_best_random_toggled)
        best_filter_box.append(self.best_random_toggle)
        
        header_box.append(best_filter_box)
        
        # Pagination controls
        pagination_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        
        self.best_prev_btn = Gtk.Button(label="Previous")
        self.best_prev_btn.connect('clicked', self._on_best_prev_clicked)
        self.best_prev_btn.set_sensitive(False)
        pagination_box.append(self.best_prev_btn)
        
        self.best_page_label = Gtk.Label(label="Page 1 of 1")
        pagination_box.append(self.best_page_label)
        
        self.best_next_btn = Gtk.Button(label="Next")
        self.best_next_btn.connect('clicked', self._on_best_next_clicked)
        self.best_next_btn.set_sensitive(False)
        pagination_box.append(self.best_next_btn)
        
        header_box.append(pagination_box)
        main_container.append(header_box)
        
        # FlowBox for thumbnails
        self.best_flowbox = Gtk.FlowBox()
        self.best_flowbox.set_selection_mode(Gtk.SelectionMode.NONE)  # No selection in Best tab
        self.best_flowbox.set_max_children_per_line(5)
        self.best_flowbox.set_column_spacing(8)
        self.best_flowbox.set_row_spacing(8)
        self.best_flowbox.set_hexpand(True)
        self.best_flowbox.set_vexpand(True)
        self.best_flowbox.set_halign(Gtk.Align.FILL)
        self.best_flowbox.set_valign(Gtk.Align.FILL)
        
        # Scrolled window for thumbnails
        best_scrolled = Gtk.ScrolledWindow()
        best_scrolled.set_child(self.best_flowbox)
        best_scrolled.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)
        best_scrolled.set_hexpand(True)
        best_scrolled.set_vexpand(True)
        best_scrolled.set_halign(Gtk.Align.FILL)
        best_scrolled.set_valign(Gtk.Align.FILL)
        main_container.append(best_scrolled)
        
        # Unfiltered (from best.json) and after filters; matches Explore pattern
        self.best_all_files = []  # List of (filepath, is_video) tuples
        self.best_filtered_files = []  # After path/type/features/random
        self.best_filter_text = ""
        self.best_features_only = False
        self.best_images_only = False
        self.best_videos_only = False
        self.best_random_mode = False
        self.best_current_page = 1
        self.best_items_per_page = 100
        
        return main_container
    
    def load_best_files(self):
        """Load files from best.json for Best tab."""
        # Run file loading in a separate thread to avoid blocking UI
        thread = threading.Thread(target=self._load_best_files_thread)
        thread.daemon = True
        self._threads.append(thread)
        thread.start()
    
    def _load_best_files_thread(self):
        """Load best.json and populate files (runs in background thread)."""
        best_files = self._load_best_files_list()
        
        # Update UI from main thread
        GLib.idle_add(self._on_best_data_loaded, best_files)
    
    def _on_best_data_loaded(self, best_files: list):
        """Apply filters and show Best tab (main thread, from background load)."""
        self.best_all_files = best_files
        self._apply_best_filters()
        return False
    
    def _load_best_files_list(self):
        """Load best.json and return list of (filepath, is_video) tuples.
        Returns files in reverse order (latest entries first).
        """
        if self._best_files_cache is not None:
            return self._best_files_cache
        
        best_files = []
        
        try:
            if os.path.exists(self.best_json_path):
                with open(self.best_json_path, 'r', encoding='utf-8') as f:
                    file_paths = json.load(f)
                
                # Reverse the list so latest entries (at end of JSON) appear first
                file_paths = list(reversed(file_paths))
                
                # Only include paths we treat as image or video (excludes .mkv and unknown ext)
                from .media_types import is_image_file, is_video_file
                for filepath in file_paths:
                    if not os.path.exists(filepath):
                        continue
                    if not (is_image_file(filepath) or is_video_file(filepath)):
                        continue
                    is_video = is_video_file(filepath)
                    best_files.append((filepath, is_video))
        except Exception as e:
            print(f"Error loading best.json: {e}")
        
        self._best_files_cache = best_files
        return best_files
    
    def _is_file_in_best(self, filepath: str) -> bool:
        """Check if a file is in best.json.
        Note: This loads the original order from JSON, not the reversed cache.
        """
        try:
            if os.path.exists(self.best_json_path):
                with open(self.best_json_path, 'r', encoding='utf-8') as f:
                    file_paths = json.load(f)
                return filepath in file_paths
        except Exception:
            pass
        return False
    
    def _apply_best_filters(self, reset_page: bool = True):
        """Apply path / type / features / random filters to best_all_files and refresh the flowbox."""
        filtered = list(self.best_all_files)
        
        if self.best_images_only and not self.best_videos_only:
            filtered = [(fp, is_vid) for fp, is_vid in filtered if not is_vid]
        elif self.best_videos_only and not self.best_images_only:
            filtered = [(fp, is_vid) for fp, is_vid in filtered if is_vid]
        
        if self.best_features_only and self.explore_directory_features:
            features_path = self.explore_directory_features
            filtered = [(fp, is_vid) for fp, is_vid in filtered if features_path in fp]
        
        if self.best_filter_text:
            filter_lower = self.best_filter_text.lower()
            filtered = [(fp, is_vid) for fp, is_vid in filtered if filter_lower in fp.lower()]
        
        if self.best_random_mode:
            filtered = list(filtered)
            random.shuffle(filtered)
        
        self.best_filtered_files = filtered
        
        if reset_page:
            self.best_current_page = 1
        
        total_items = len(filtered)
        total_pages = (total_items + self.best_items_per_page - 1) // self.best_items_per_page if total_items > 0 else 1
        
        if self.best_current_page > total_pages:
            self.best_current_page = total_pages
        if self.best_current_page < 1:
            self.best_current_page = 1
        
        start_idx = (self.best_current_page - 1) * self.best_items_per_page
        end_idx = min(start_idx + self.best_items_per_page, total_items)
        page_files = filtered[start_idx:end_idx]
        
        for child in list(self.best_flowbox):
            self.best_flowbox.remove(child)
        
        for filepath, is_video in page_files:
            widget = ThumbnailWidget(
                filepath,
                is_video=is_video,
                move_callback=None,
                single_click_open=True,
                best_callback=self._handle_best_action,
                show_star=False,
                is_best_tab=True
            )
            self.best_flowbox.append(widget)
            thread = threading.Thread(target=self._generate_and_set_thumbnail, args=(widget, filepath, is_video))
            thread.daemon = True
            self._threads.append(thread)
            thread.start()
        
        self.best_page_label.set_text(f"Page {self.best_current_page} of {total_pages} ({total_items} total)")
        self.best_prev_btn.set_sensitive(self.best_current_page > 1)
        self.best_next_btn.set_sensitive(self.best_current_page < total_pages)
    
    def _on_best_search_changed(self, entry):
        """Handle path filter in Best tab."""
        self.best_filter_text = entry.get_text()
        self._apply_best_filters()
    
    def _on_best_images_toggled(self, button):
        self.best_images_only = button.get_active()
        if self.best_images_only and self.best_videos_toggle.get_active():
            self.best_videos_toggle.set_active(False)
        self._apply_best_filters()
    
    def _on_best_videos_toggled(self, button):
        self.best_videos_only = button.get_active()
        if self.best_videos_only and self.best_images_toggle.get_active():
            self.best_images_toggle.set_active(False)
        self._apply_best_filters()
    
    def _on_best_features_toggled(self, button):
        self.best_features_only = button.get_active()
        self._apply_best_filters()
    
    def _on_best_random_toggled(self, button):
        self.best_random_mode = button.get_active()
        self._apply_best_filters()
    
    def _on_best_prev_clicked(self, button):
        """Handle Previous button click in Best tab."""
        if self.best_current_page > 1:
            self.best_current_page -= 1
            self._apply_best_filters(reset_page=False)
    
    def _on_best_next_clicked(self, button):
        """Handle Next button click in Best tab."""
        total_items = len(self.best_filtered_files)
        total_pages = (total_items + self.best_items_per_page - 1) // self.best_items_per_page if total_items > 0 else 1
        if self.best_current_page < total_pages:
            self.best_current_page += 1
            self._apply_best_filters(reset_page=False)
    
    def _add_to_best(self, filepath: str):
        """Add a file to best.json."""
        try:
            # Load current best files
            if os.path.exists(self.best_json_path):
                with open(self.best_json_path, 'r', encoding='utf-8') as f:
                    file_paths = json.load(f)
            else:
                file_paths = []
            
            # Add file if not already present
            if filepath not in file_paths:
                file_paths.append(filepath)
                
                # Save back to file
                with open(self.best_json_path, 'w', encoding='utf-8') as f:
                    json.dump(file_paths, f, indent=2, ensure_ascii=False)
                
                # Clear cache
                self._best_files_cache = None
                
                # Reload best tab
                self.load_best_files()
                
                # Refresh explore tab to show star
                self._refresh_explore_tab()
        except Exception as e:
            print(f"Error adding to best.json: {e}")
    
    def _remove_from_best(self, filepath: str):
        """Remove a file from best.json."""
        try:
            # Load current best files
            if os.path.exists(self.best_json_path):
                with open(self.best_json_path, 'r', encoding='utf-8') as f:
                    file_paths = json.load(f)
            else:
                return
            
            # Remove file if present
            if filepath in file_paths:
                file_paths.remove(filepath)
                
                # Save back to file
                with open(self.best_json_path, 'w', encoding='utf-8') as f:
                    json.dump(file_paths, f, indent=2, ensure_ascii=False)
                
                # Clear cache
                self._best_files_cache = None
                
                # Reload best tab
                self.load_best_files()
                
                # Refresh explore tab to remove star
                self._refresh_explore_tab()
        except Exception as e:
            print(f"Error removing from best.json: {e}")
    
    def _handle_best_action(self, filepath: str, add: bool):
        """Handle add/remove action for best.json."""
        if add:
            self._add_to_best(filepath)
        else:
            self._remove_from_best(filepath)
    
    def _refresh_explore_tab(self):
        """Refresh explore tab to update star icons."""
        # Update star icons for all visible widgets
        for widget in self.explore_flowbox:
            if isinstance(widget, ThumbnailWidget):
                is_in_best = self._is_file_in_best(widget.filepath)
                widget.update_star_icon(is_in_best)
    
    def load_explore_files(self):
        """Load all media files sorted by date for Explore tab."""
        # Run file scanning in a separate thread to avoid blocking UI
        thread = threading.Thread(target=self._scan_explore_files)
        thread.daemon = True
        self._threads.append(thread)
        thread.start()
    
    def _scan_explore_files(self):
        """Scan directory and load files sorted by date (runs in background thread)."""
        all_files = get_all_media_files_sorted_by_date(self.explore_directory)
        GLib.idle_add(self._populate_explore_page, all_files)
    
    def _populate_explore_page(self, all_files: list):
        """Populate the explore page with current page's items."""
        self.explore_all_files = all_files
        self._apply_explore_filters()
    
    def _apply_explore_filters(self, reset_page: bool = True):
        """Apply current filters and update the display.
        
        Args:
            reset_page: If True, reset to page 1 when filters change. If False, keep current page.
        """
        # Start with all files
        filtered = list(self.explore_all_files)
        
        # Apply media type filters (Images Only or Videos Only)
        if self.explore_images_only and not self.explore_videos_only:
            # Show only images
            filtered = [(fp, is_vid) for fp, is_vid in filtered if not is_vid]
        elif self.explore_videos_only and not self.explore_images_only:
            # Show only videos
            filtered = [(fp, is_vid) for fp, is_vid in filtered if is_vid]
        # If both are selected or neither, show all (no filtering by type)
        
        # Apply FEATURES filter if enabled
        if self.explore_features_only and self.explore_directory_features:
            features_path = self.explore_directory_features
            filtered = [(fp, is_vid) for fp, is_vid in filtered if features_path in fp]
        
        # Apply text filter if present
        if self.explore_filter_text:
            filter_lower = self.explore_filter_text.lower()
            filtered = [(fp, is_vid) for fp, is_vid in filtered if filter_lower in fp.lower()]
        
        # Apply random shuffle if enabled
        if self.explore_random_mode:
            filtered = list(filtered)  # Make a copy to avoid modifying the original
            random.shuffle(filtered)
        
        # Store filtered files
        self.explore_filtered_files = filtered
        
        # Reset to page 1 when filters change (but not when navigating pages)
        if reset_page:
            self.explore_current_page = 1
        
        # Calculate pagination
        total_items = len(filtered)
        total_pages = (total_items + self.explore_items_per_page - 1) // self.explore_items_per_page if total_items > 0 else 1
        
        # Clamp current page
        if self.explore_current_page > total_pages:
            self.explore_current_page = total_pages
        if self.explore_current_page < 1:
            self.explore_current_page = 1
        
        # Calculate range for current page
        start_idx = (self.explore_current_page - 1) * self.explore_items_per_page
        end_idx = min(start_idx + self.explore_items_per_page, total_items)
        page_files = filtered[start_idx:end_idx]
        
        # Clear existing thumbnails
        for child in list(self.explore_flowbox):
            self.explore_flowbox.remove(child)
        
        # Add thumbnails for current page
        for idx, (filepath, is_video) in enumerate(page_files):
            # Check if file is in best.json
            is_in_best = self._is_file_in_best(filepath)
            
            # Don't pass image_list - ImageViewerWindow will get images from the folder automatically
            widget = ThumbnailWidget(
                filepath, 
                is_video=is_video, 
                move_callback=None, 
                single_click_open=True,
                show_star=is_in_best,
                best_callback=self._handle_best_action,
                image_list=None,  # Let ImageViewerWindow get images from folder
                current_index=0
            )
            self.explore_flowbox.append(widget)
            
            # Generate thumbnail in background
            thread = threading.Thread(target=self._generate_and_set_thumbnail, args=(widget, filepath, is_video))
            thread.daemon = True
            self._threads.append(thread)
            thread.start()
        
        # Update pagination controls
        self.explore_page_label.set_text(f"Page {self.explore_current_page} of {total_pages} ({total_items} total)")
        self.explore_prev_btn.set_sensitive(self.explore_current_page > 1)
        self.explore_next_btn.set_sensitive(self.explore_current_page < total_pages)
    
    def _on_explore_search_changed(self, entry):
        """Handle search filter text change."""
        self.explore_filter_text = entry.get_text()
        self._apply_explore_filters()
    
    def _on_explore_images_toggled(self, button):
        """Handle Images Only toggle button."""
        self.explore_images_only = button.get_active()
        # If Images Only is enabled, disable Videos Only
        if self.explore_images_only and self.explore_videos_toggle.get_active():
            self.explore_videos_toggle.set_active(False)
        self._apply_explore_filters()
    
    def _on_explore_videos_toggled(self, button):
        """Handle Videos Only toggle button."""
        self.explore_videos_only = button.get_active()
        # If Videos Only is enabled, disable Images Only
        if self.explore_videos_only and self.explore_images_toggle.get_active():
            self.explore_images_toggle.set_active(False)
        self._apply_explore_filters()
    
    def _on_explore_features_toggled(self, button):
        """Handle FEATURES toggle button."""
        self.explore_features_only = button.get_active()
        self._apply_explore_filters()
    
    def _on_explore_random_toggled(self, button):
        """Handle Random toggle button."""
        self.explore_random_mode = button.get_active()
        self._apply_explore_filters()
    
    def _on_explore_prev_clicked(self, button):
        """Handle Previous button click in Explore tab."""
        if self.explore_current_page > 1:
            self.explore_current_page -= 1
            self._apply_explore_filters(reset_page=False)
    
    def _on_explore_next_clicked(self, button):
        """Handle Next button click in Explore tab."""
        total_items = len(self.explore_filtered_files)
        total_pages = (total_items + self.explore_items_per_page - 1) // self.explore_items_per_page if total_items > 0 else 1
        if self.explore_current_page < total_pages:
            self.explore_current_page += 1
            self._apply_explore_filters(reset_page=False)
    
    def _shutdown(self):
        """Clean shutdown: kill all processes, stop threads, close windows."""
        if self._shutting_down:
            return
        
        self._shutting_down = True
        print("Initiating clean shutdown...")
        
        # Close all image viewer windows
        for window in self._image_viewer_windows[:]:
            try:
                window.destroy()
            except Exception:
                pass
        self._image_viewer_windows.clear()
        
        # Kill all tracked subprocesses
        for process in _global_subprocesses[:]:
            try:
                if process.poll() is None:  # Process is still running
                    process.terminate()
                    try:
                        process.wait(timeout=2)
                    except subprocess.TimeoutExpired:
                        process.kill()
            except Exception:
                pass
        _global_subprocesses.clear()
        
        # Kill all Chromium processes (including ones we might have missed)
        try:
            subprocess.run(['pkill', '-f', 'chromium.*--app'], 
                         capture_output=True, timeout=2, check=False)
            subprocess.run(['pkill', '-f', 'chromium.*--user-data-dir'], 
                         capture_output=True, timeout=2, check=False)
        except Exception:
            pass
        
        # Kill all mpv processes
        try:
            subprocess.run(['pkill', 'mpv'], capture_output=True, timeout=2, check=False)
        except Exception:
            pass
        
        # Kill all vlc processes
        try:
            subprocess.run(['pkill', 'vlc'], capture_output=True, timeout=2, check=False)
        except Exception:
            pass

        # Give Chromium a moment to exit, then remove viewer temp files/dirs
        import time
        time.sleep(0.5)
        _cleanup_all_chromium_temps(force=True)
        
        # Kill all Python processes running src.main (but not the current process)
        try:
            import psutil
            current_pid = os.getpid()
            for proc in psutil.process_iter(['pid', 'name', 'cmdline']):
                try:
                    if proc.info['name'] == 'python3' or 'python' in proc.info['name'].lower():
                        cmdline = proc.info.get('cmdline', [])
                        if cmdline and 'src.main' in ' '.join(cmdline):
                            pid = proc.info['pid']
                            if pid != current_pid:  # Don't kill ourselves
                                proc.terminate()
                                try:
                                    proc.wait(timeout=2)
                                except psutil.TimeoutExpired:
                                    proc.kill()
                except (psutil.NoSuchProcess, psutil.AccessDenied, psutil.ZombieProcess):
                    pass
        except ImportError:
            # psutil not available, use pkill instead (less precise but works)
            # Note: This might kill other Python processes, but it's a fallback
            try:
                # Get current PID to avoid killing ourselves
                current_pid = os.getpid()
                # Use pgrep to find PIDs first, then kill only those that aren't us
                result = subprocess.run(['pgrep', '-f', 'python.*src.main'], 
                                      capture_output=True, text=True, timeout=2, check=False)
                if result.returncode == 0:
                    for pid_str in result.stdout.strip().split('\n'):
                        if pid_str:
                            try:
                                pid = int(pid_str.strip())
                                if pid != current_pid:
                                    os.kill(pid, 15)  # SIGTERM
                                    time.sleep(0.5)
                                    try:
                                        os.kill(pid, 9)  # SIGKILL if still alive
                                    except ProcessLookupError:
                                        pass  # Already dead
                            except (ValueError, ProcessLookupError, PermissionError):
                                pass
            except Exception:
                pass
        except Exception:
            pass
        
        print("Shutdown complete. Exiting...")
        
        # Close main window and exit
        self.destroy()
        import sys
        sys.exit(0)
        if self.explore_current_page < total_pages:
            self.explore_current_page += 1
            self._apply_explore_filters(reset_page=False)
    
    