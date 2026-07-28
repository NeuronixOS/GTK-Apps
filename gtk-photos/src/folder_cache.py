"""Disk cache for Folders tab thumbnails and folder index."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
from typing import List, Tuple

from gi.repository import GdkPixbuf, GLib

from .config_paths import cache_dir
from .media_types import newest_media_in_folder
from .thumbnailer import (
    create_placeholder_thumbnail,
    generate_image_thumbnail,
    generate_video_thumbnail,
)

PreviewEntry = Tuple[str, str, bool, float]  # folder, sample, is_video, newest_mtime

INDEX_VERSION = 1
THUMB_QUALITY = 82


def get_cache_root() -> Path:
    """``~/.cache/gtk-apps/gtk-photos/folders`` (created if missing)."""
    root = cache_dir() / "folders"
    root.mkdir(parents=True, exist_ok=True)
    (root / "thumbnails").mkdir(parents=True, exist_ok=True)
    return root


def _folder_hash(folder_path: str) -> str:
    normalized = os.path.normpath(folder_path)
    return hashlib.sha256(normalized.encode("utf-8")).hexdigest()[:32]


def _thumb_path(cache_root: Path, folder_path: str) -> Path:
    return cache_root / "thumbnails" / f"{_folder_hash(folder_path)}.jpg"


def _meta_path(cache_root: Path, folder_path: str) -> Path:
    return cache_root / "thumbnails" / f"{_folder_hash(folder_path)}.meta.json"


def _load_meta(meta_path: Path) -> dict | None:
    try:
        with open(meta_path, encoding="utf-8") as f:
            return json.load(f)
    except (OSError, json.JSONDecodeError):
        return None


def _save_meta(meta_path: Path, data: dict) -> None:
    tmp = meta_path.with_suffix(".meta.json.tmp")
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=0)
    tmp.replace(meta_path)


def thumbnail_cache_valid(
    cache_root: Path,
    folder_path: str,
    sample_path: str,
    sample_mtime: float,
    size: int,
) -> Path | None:
    """Return cached JPEG path if metadata and file match the current sample."""
    thumb = _thumb_path(cache_root, folder_path)
    meta = _meta_path(cache_root, folder_path)
    if not thumb.is_file() or not meta.is_file():
        return None
    data = _load_meta(meta)
    if not data:
        return None
    if data.get("sample_path") != sample_path:
        return None
    if data.get("size") != size:
        return None
    if abs(float(data.get("sample_mtime", -1)) - sample_mtime) > 0.001:
        return None
    return thumb


def save_thumbnail_cache(
    cache_root: Path,
    folder_path: str,
    sample_path: str,
    sample_mtime: float,
    newest_mtime: float,
    is_video: bool,
    size: int,
    pixbuf: GdkPixbuf.Pixbuf,
) -> Path | None:
    thumb = _thumb_path(cache_root, folder_path)
    meta = _meta_path(cache_root, folder_path)
    try:
        pixbuf.savev(str(thumb), "jpeg", ["quality"], [str(THUMB_QUALITY)])
        _save_meta(
            meta,
            {
                "folder_path": folder_path,
                "sample_path": sample_path,
                "sample_mtime": sample_mtime,
                "newest_mtime": newest_mtime,
                "is_video": is_video,
                "size": size,
            },
        )
        return thumb
    except (GLib.Error, OSError, TypeError, ValueError) as e:
        print(f"Error saving folder thumbnail cache for {folder_path}: {e}")
        return None


def generate_thumbnail_pixbuf(sample_path: str, is_video: bool, size: int) -> GdkPixbuf.Pixbuf | None:
    if is_video:
        return generate_video_thumbnail(sample_path, size=size)
    return generate_image_thumbnail(sample_path, size=size)


def get_or_create_thumbnail_path(
    folder_path: str,
    sample_path: str,
    sample_mtime: float,
    newest_mtime: float,
    is_video: bool,
    size: int,
) -> str | None:
    """
    Return path to a cached JPEG thumbnail, generating and saving if needed.
    """
    cache_root = get_cache_root()
    cached = thumbnail_cache_valid(
        cache_root, folder_path, sample_path, sample_mtime, size
    )
    if cached:
        return str(cached)

    pixbuf = generate_thumbnail_pixbuf(sample_path, is_video, size)
    if not pixbuf:
        return None

    saved = save_thumbnail_cache(
        cache_root,
        folder_path,
        sample_path,
        sample_mtime,
        newest_mtime,
        is_video,
        size,
        pixbuf,
    )
    return str(saved) if saved else None


def _index_path(cache_root: Path) -> Path:
    return cache_root / "folders_index.json"


def _load_index(cache_root: Path, photo_drive: str) -> dict | None:
    path = _index_path(cache_root)
    if not path.is_file():
        return None
    try:
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
        if data.get("photo_drive") != photo_drive:
            return None
        if data.get("version") != INDEX_VERSION:
            return None
        return data
    except (OSError, json.JSONDecodeError):
        return None


def _save_index(cache_root: Path, photo_drive: str, entries: List[PreviewEntry]) -> None:
    path = _index_path(cache_root)
    payload = {
        "version": INDEX_VERSION,
        "photo_drive": photo_drive,
        "entries": [
            {
                "folder": folder,
                "sample": sample,
                "is_video": is_video,
                "newest_mtime": newest_mtime,
                "sample_mtime": _safe_mtime(sample),
            }
            for folder, sample, is_video, newest_mtime in entries
        ],
    }
    tmp = path.with_suffix(".json.tmp")
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(payload, f)
    tmp.replace(path)


def _safe_mtime(path: str) -> float:
    try:
        return os.path.getmtime(path)
    except OSError:
        return 0.0


def get_folder_previews_cached(photo_drive: str) -> List[PreviewEntry]:
    """
    Folder list with per-folder sample media, using a saved index when possible.
    Only re-scans folders that are new or have changed media.
    """
    from .favorites import get_folders_in_directory

    if not photo_drive or not os.path.isdir(photo_drive):
        return []

    cache_root = get_cache_root()
    all_folders = set(get_folders_in_directory(photo_drive))
    index = _load_index(cache_root, photo_drive)
    by_folder: dict[str, dict] = {}
    if index:
        for entry in index.get("entries", []):
            folder = entry.get("folder")
            if folder:
                by_folder[folder] = entry

    results: List[PreviewEntry] = []
    to_scan: list[str] = []

    for folder in all_folders:
        prev = by_folder.get(folder)
        if prev is None:
            to_scan.append(folder)
            continue
        sample = prev.get("sample")
        if not sample or not os.path.isfile(sample):
            to_scan.append(folder)
            continue
        try:
            sample_mtime = os.path.getmtime(sample)
        except OSError:
            to_scan.append(folder)
            continue
        if abs(sample_mtime - float(prev.get("sample_mtime", -1))) > 0.001:
            to_scan.append(folder)
            continue
        newest_mtime = float(prev.get("newest_mtime", 0))
        results.append((folder, sample, bool(prev.get("is_video")), newest_mtime))

    for folder in to_scan:
        sample = newest_media_in_folder(folder)
        if sample:
            filepath, is_video, mtime = sample
            results.append((folder, filepath, is_video, mtime))

    results.sort(key=lambda x: x[3], reverse=True)
    _save_index(cache_root, photo_drive, results)
    return results
