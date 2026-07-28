"""XDG config paths for gtk-photos (`~/.config/gtk-apps/gtk-photos/`)."""

from __future__ import annotations

import os
import shutil
from pathlib import Path

APP_NAME = "gtk-photos"


def project_dir() -> Path:
    """Repo / install root (parent of ``src/``)."""
    return Path(__file__).resolve().parent.parent


def config_dir() -> Path:
    """``~/.config/gtk-apps/gtk-photos`` (created if missing)."""
    base = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    path = base / "gtk-apps" / APP_NAME
    path.mkdir(parents=True, exist_ok=True)
    _migrate_project_tree_once(path)
    return path


def cache_dir() -> Path:
    """``~/.cache/gtk-apps/gtk-photos`` (created if missing)."""
    base = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
    path = base / "gtk-apps" / APP_NAME
    path.mkdir(parents=True, exist_ok=True)
    _migrate_project_cache_once(path)
    return path


def _migrate_project_cache_once(dest_dir: Path) -> None:
    """Move leftover in-tree ``.cache/folders`` into the XDG cache dir once."""
    src = project_dir() / ".cache" / "folders"
    dest = dest_dir / "folders"
    if not src.is_dir() or dest.exists():
        return
    try:
        shutil.move(str(src), str(dest))
        legacy = project_dir() / ".cache"
        if legacy.is_dir() and not any(legacy.iterdir()):
            legacy.rmdir()
    except OSError:
        pass


def _migrate_project_tree_once(dest_dir: Path) -> None:
    """Copy JSON into the config dir from leftover in-tree files if dest is missing."""
    root = project_dir()
    for name in (
        "config.json",
        "favorites.json",
        "recent.json",
        "best.json",
        "websites.json",
        "websites_thumbnail_overrides.json",
    ):
        dest = dest_dir / name
        if dest.exists():
            continue
        src = root / name
        if src.is_file():
            try:
                shutil.copy2(src, dest)
            except OSError:
                pass

    img_dest = dest_dir / "websites_img"
    src_img = root / "websites_img"
    if not img_dest.exists() and src_img.is_dir():
        try:
            shutil.copytree(src_img, img_dest)
        except OSError:
            pass
    elif img_dest.is_dir() and src_img.is_dir() and src_img.resolve() != img_dest.resolve():
        for jpg in src_img.glob("*.jpg"):
            dest = img_dest / jpg.name
            if dest.is_file():
                continue
            try:
                shutil.copy2(jpg, dest)
            except OSError:
                pass


def config_file() -> Path:
    return config_dir() / "config.json"


def favorites_file() -> Path:
    return config_dir() / "favorites.json"


def recent_file() -> Path:
    return config_dir() / "recent.json"


def best_file() -> Path:
    return config_dir() / "best.json"
