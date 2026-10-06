"""XDG config / cache paths for gtk-calendar."""

from __future__ import annotations

import os
from pathlib import Path

APP_NAME = "gtk-calendar"


def project_dir() -> Path:
    return Path(__file__).resolve().parent.parent


def config_dir() -> Path:
    base = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    path = base / "gtk-apps" / APP_NAME
    path.mkdir(parents=True, exist_ok=True)
    return path


def cache_dir() -> Path:
    base = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
    path = base / "gtk-apps" / APP_NAME
    path.mkdir(parents=True, exist_ok=True)
    return path


def config_file() -> Path:
    return config_dir() / "config.json"


def google_token_file() -> Path:
    return config_dir() / "google-token.json"


def ics_store_dir() -> Path:
    path = config_dir() / "ics"
    path.mkdir(parents=True, exist_ok=True)
    return path


def ics_cache_file(source_id: str) -> Path:
    safe = "".join(ch if ch.isalnum() or ch in "-._" else "_" for ch in source_id)
    return cache_dir() / f"{safe}.ics"
