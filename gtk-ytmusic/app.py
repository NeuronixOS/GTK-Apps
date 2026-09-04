#!/usr/bin/env python3
"""gtk-ytmusic — dedicated YouTube Music player window.

Default mode launches Chromium/Chrome as an app window with an isolated
profile (no browser extensions) and flags that reduce background audio
throttling — the usual cause of skips/gaps in a normal browser tab.

Optional ``--webkit`` embeds music.youtube.com in WebKitGTK (GTK3) when
you want a native Neuronix window instead of Chromium.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
from pathlib import Path

APP_ID = "org.neuronix.GtkYtMusic"
HOME_URL = "https://music.youtube.com/"
CONFIG_DIR = Path.home() / ".config" / "gtk-ytmusic"
CHROME_PROFILE = CONFIG_DIR / "chromium-profile"
WEBKIT_DATA = CONFIG_DIR / "webkit-data"
WEBKIT_CACHE = CONFIG_DIR / "webkit-cache"

# Modern Chrome UA — YT Music rejects some WebKit default strings as "deprecated".
CHROME_UA = (
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) "
    "Chrome/131.0.0.0 Safari/537.36"
)

# ContentSetting values: 1=allow, 2=block
_BLOCK = 2


def find_browser() -> str | None:
    env = os.environ.get("GTK_YTMUSIC_BROWSER", "").strip()
    if env and shutil.which(env):
        return shutil.which(env)
    for name in (
        "google-chrome-stable",
        "google-chrome",
        "chromium",
        "chromium-browser",
        "brave-browser",
        "microsoft-edge",
    ):
        path = shutil.which(name)
        if path:
            return path
    return None


def _deep_merge(base: dict, overlay: dict) -> dict:
    out = dict(base)
    for key, value in overlay.items():
        if isinstance(value, dict) and isinstance(out.get(key), dict):
            out[key] = _deep_merge(out[key], value)
        else:
            out[key] = value
    return out


def seed_chrome_preferences() -> None:
    """Block camera/mic by default — YT Music may probe devices for voice search."""
    CHROME_PROFILE.mkdir(parents=True, exist_ok=True)
    prefs_path = CHROME_PROFILE / "Default" / "Preferences"
    prefs_path.parent.mkdir(parents=True, exist_ok=True)

    overlay = {
        "profile": {
            "default_content_setting_values": {
                "media_stream_camera": _BLOCK,
                "media_stream_mic": _BLOCK,
            },
            "content_settings": {
                "exceptions": {
                    "media_stream_camera": {
                        "https://music.youtube.com:443,*": {
                            "setting": _BLOCK,
                        },
                        "https://www.youtube.com:443,*": {
                            "setting": _BLOCK,
                        },
                    },
                    "media_stream_mic": {
                        "https://music.youtube.com:443,*": {
                            "setting": _BLOCK,
                        },
                    },
                }
            },
        }
    }

    data: dict = {}
    if prefs_path.is_file():
        try:
            data = json.loads(prefs_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            data = {}
    data = _deep_merge(data, overlay)
    prefs_path.write_text(json.dumps(data, separators=(",", ":")), encoding="utf-8")


def chrome_app_argv(browser: str, url: str) -> list[str]:
    seed_chrome_preferences()
    return [
        browser,
        f"--app={url}",
        f"--user-data-dir={CHROME_PROFILE}",
        f"--class={APP_ID}",
        "--name=gtk-ytmusic",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-sync",
        "--disable-features=TranslateUI,MediaRouter",
        # Keep media timers running when the window is occluded / unfocused.
        "--disable-background-timer-throttling",
        "--disable-backgrounding-occluded-windows",
        "--disable-renderer-backgrounding",
        "--autoplay-policy=no-user-gesture-required",
        # Fresh profile = no extensions (AdBlock etc. often cause audio gaps).
        "--disable-extensions",
    ]


def run_chrome(url: str) -> int:
    browser = find_browser()
    if not browser:
        print(
            "gtk-ytmusic: no Chromium/Chrome found.\n"
            "Install chromium or google-chrome, or set GTK_YTMUSIC_BROWSER.",
            file=sys.stderr,
        )
        return 1
    argv = chrome_app_argv(browser, url)
    print(f"gtk-ytmusic: launching {browser} (isolated profile)")
    print(f"  profile: {CHROME_PROFILE}")
    # Replace this process so the desktop session tracks the player window.
    os.execv(browser, argv)
    return 0  # unreachable


def run_webkit(url: str) -> int:
    try:
        import gi

        gi.require_version("Gtk", "3.0")
        gi.require_version("WebKit2", "4.1")
        from gi.repository import Gtk, WebKit2
    except (ImportError, ValueError) as e:
        print(f"gtk-ytmusic: WebKit2 unavailable ({e})", file=sys.stderr)
        print("Falling back to Chromium app mode.", file=sys.stderr)
        return run_chrome(url)

    WEBKIT_DATA.mkdir(parents=True, exist_ok=True)
    WEBKIT_CACHE.mkdir(parents=True, exist_ok=True)

    data = WebKit2.WebsiteDataManager(
        base_data_directory=str(WEBKIT_DATA),
        base_cache_directory=str(WEBKIT_CACHE),
    )
    context = WebKit2.WebContext.new_with_website_data_manager(data)
    view = WebKit2.WebView.new_with_context(context)

    settings = view.get_settings()
    settings.set_user_agent(CHROME_UA)
    settings.set_enable_media(True)
    settings.set_enable_webaudio(True)
    settings.set_enable_mediasource(True)
    settings.set_enable_javascript(True)
    settings.set_enable_smooth_scrolling(True)
    try:
        settings.set_hardware_acceleration_policy(
            WebKit2.HardwareAccelerationPolicy.ALWAYS
        )
    except AttributeError:
        pass

    win = Gtk.Window(title="YouTube Music")
    win.set_default_size(1280, 800)
    win.set_wmclass("gtk-ytmusic", APP_ID)
    win.connect("destroy", Gtk.main_quit)
    win.add(view)
    win.show_all()
    view.load_uri(url)
    Gtk.main()
    return 0


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description="Dedicated YouTube Music player (Chromium app window by default)."
    )
    p.add_argument(
        "url",
        nargs="?",
        default=HOME_URL,
        help=f"Start URL (default: {HOME_URL})",
    )
    p.add_argument(
        "--webkit",
        action="store_true",
        help="Embed in WebKitGTK instead of Chromium (experimental)",
    )
    p.add_argument(
        "--chrome",
        action="store_true",
        help="Force Chromium/Chrome app mode (default)",
    )
    return p.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    url = args.url.strip() or HOME_URL
    if not url.startswith(("http://", "https://")):
        url = HOME_URL

    CONFIG_DIR.mkdir(parents=True, exist_ok=True)

    if args.webkit and not args.chrome:
        return run_webkit(url)
    return run_chrome(url)


if __name__ == "__main__":
    # Allow `python3 -m` or direct exec without leaving a zombie parent when
    # execv is used — only webkit returns.
    raise SystemExit(main())
