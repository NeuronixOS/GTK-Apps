"""Generate website link thumbnails from images on the Photos drive."""

from __future__ import annotations

import os
import random
import re
import subprocess
from pathlib import Path
from urllib.parse import urlparse

IMAGE_EXTS = {".jpg", ".jpeg", ".png", ".gif", ".webp"}
THUMB_SIZE = "200x200"


def url_slug(href: str) -> str:
    parsed = urlparse(href)
    netloc = (parsed.netloc or "").replace("www.", "").split(".")[0]
    path = (parsed.path or "").strip("/")
    parts = path.split("/")
    handle = parts[-1] if parts else netloc
    handle = handle.replace("@", "")
    if "?" in handle:
        handle = handle.split("?")[0]
    s = re.sub(r"[^\w\-.]", "_", handle).strip("_").lower()
    s = re.sub(r"_+", "_", s) or "unknown"
    return f"{netloc}_{s}"


def link_label(href: str) -> str:
    parsed = urlparse(href)
    path = (parsed.path or "").strip("/")
    parts = path.split("/")
    handle = parts[-1] if parts else (parsed.netloc or "").replace("www.", "")
    if "?" in handle:
        handle = handle.split("?")[0]
    return handle.replace("@", "") or "link"


def find_images(folder: Path) -> list[Path]:
    out = []
    for f in folder.rglob("*"):
        if not f.is_file() or f.suffix.lower() not in IMAGE_EXTS:
            continue
        if f.name.startswith("._") or f.name == ".DS_Store":
            continue
        out.append(f)
    return out


def build_folder_map(source_root: Path) -> dict[str, tuple[Path, list[Path]]]:
    out: dict[str, tuple[Path, list[Path]]] = {}
    if not source_root.is_dir():
        return out
    root = source_root.resolve()
    for dirpath, dirnames, _filenames in os.walk(root, topdown=True, followlinks=False):
        dirnames[:] = [d for d in dirnames if not d.startswith(".")]
        p = Path(dirpath)
        if p == root:
            continue
        rel_parts = p.relative_to(root).parts
        if any(part.startswith(".") for part in rel_parts):
            continue
        images = find_images(p)
        if not images:
            continue
        rel_key = p.relative_to(root).as_posix().lower()
        out[rel_key] = (p, images)
        legacy = re.sub(r"[\s_\-.]", "", _folder_slug(p.name)).lower()
        if legacy and legacy not in out:
            out[legacy] = (p, images)
    return out


def _folder_slug(folder_name: str) -> str:
    s = re.sub(r"[^\w\s-]", "", folder_name)
    s = re.sub(r"[-\s]+", "_", s).strip().lower()
    return s or "unknown"


def resolve_under_source(source_root: Path, rel: str) -> Path | None:
    root = source_root.resolve()
    s = str(rel).strip().replace("\\", "/").strip("/")
    if not s:
        return None
    parts = [x for x in s.split("/") if x and x != "."]
    if ".." in parts:
        return None
    cand = (root.joinpath(*parts)).resolve()
    try:
        cand.relative_to(root)
    except ValueError:
        return None
    return cand


def normalize_slug_for_match(s: str) -> str:
    return re.sub(r"[\s_\-.]", "", s).lower()


def link_handle_key(href: str) -> str:
    """Normalized handle from URL path (or site name if path is empty)."""
    parsed = urlparse(href)
    path = (parsed.path or "").strip("/")
    handle = path.split("/")[-1].replace("@", "").split("?")[0]
    if not handle:
        handle = (parsed.netloc or "").replace("www.", "").split(".")[0]
    return normalize_slug_for_match(handle)


def _image_from_override(
    override: str,
    folder_map: dict[str, tuple[Path, list[Path]]],
    source_root: Path,
) -> Path | None:
    s = override.strip()
    if not s:
        return None
    path = Path(s)
    if path.is_absolute():
        if path.is_file() and path.suffix.lower() in IMAGE_EXTS:
            return path
        if path.is_dir():
            imgs = find_images(path)
            if imgs:
                return random.choice(imgs)
    under = resolve_under_source(source_root, s)
    if under is not None:
        if under.is_dir():
            imgs = find_images(under)
            if imgs:
                return random.choice(imgs)
        if under.is_file() and under.suffix.lower() in IMAGE_EXTS:
            return under
    rel_key = s.replace("\\", "/").strip().strip("/").lower()
    if rel_key in folder_map:
        _, images = folder_map[rel_key]
        return random.choice(images) if images else None
    folder_key = normalize_slug_for_match(s)
    if folder_key and folder_key in folder_map:
        _, images = folder_map[folder_key]
        return random.choice(images) if images else None
    return None


def find_exact_folder_for_handle(
    handle_key: str,
    folder_map: dict[str, tuple[Path, list[Path]]],
    source_root: Path,
) -> tuple[Path, list[Path]] | None:
    """Match only when a folder's leaf name equals the link handle (normalized)."""
    if not handle_key:
        return None
    root = source_root.resolve()
    seen: set[Path] = set()
    for folder_key, (folder_path, images) in folder_map.items():
        if folder_path in seen or not images:
            continue
        try:
            rel_key = folder_path.resolve().relative_to(root).as_posix().lower()
        except ValueError:
            continue
        if folder_key != rel_key:
            continue
        seen.add(folder_path)
        if normalize_slug_for_match(folder_path.name) == handle_key:
            return folder_path, images
    return None


def choose_image(
    slug_name: str,
    folder_map: dict[str, tuple[Path, list[Path]]],
    handle_key: str,
    overrides: dict,
    source_root: Path,
) -> Path | None:
    override = overrides.get(slug_name)
    if override is not None and str(override).strip():
        return _image_from_override(str(override), folder_map, source_root)

    match = find_exact_folder_for_handle(handle_key, folder_map, source_root)
    if match:
        _folder_path, images = match
        return random.choice(images)
    return None


def write_thumbnail(chosen: Path, thumb_path: Path) -> bool:
    try:
        subprocess.run(
            [
                "convert",
                str(chosen),
                "-resize",
                f"{THUMB_SIZE}^",
                "-gravity",
                "center",
                "-extent",
                THUMB_SIZE,
                str(thumb_path),
            ],
            check=True,
            capture_output=True,
        )
        return True
    except (subprocess.CalledProcessError, FileNotFoundError, OSError):
        return False


def regenerate_one(
    href: str,
    folder_map: dict[str, tuple[Path, list[Path]]],
    base_overrides: dict,
    img_dir: Path,
    source_root: Path,
    per_slug_folder_override: str | None = None,
) -> tuple[bool, str]:
    slug_name = url_slug(href)
    overrides = dict(base_overrides)
    if per_slug_folder_override is not None and str(per_slug_folder_override).strip():
        overrides[slug_name] = per_slug_folder_override.strip()
    handle_key = link_handle_key(href)
    chosen = choose_image(slug_name, folder_map, handle_key, overrides, source_root)
    thumb_path = img_dir / f"{slug_name}.jpg"
    img_dir.mkdir(parents=True, exist_ok=True)
    if chosen is None:
        if thumb_path.is_file():
            try:
                thumb_path.unlink()
            except OSError:
                pass
        return False, slug_name
    ok = write_thumbnail(chosen, thumb_path)
    return ok, slug_name


def folder_choices(source_root: Path) -> list[dict]:
    if not source_root.is_dir():
        return []
    fm = build_folder_map(source_root)
    root = source_root.resolve()
    seen: set[Path] = set()
    rows: list[dict] = []
    for key, (folder_path, _) in fm.items():
        if folder_path in seen:
            continue
        rel_key = folder_path.relative_to(root).as_posix().lower()
        if key != rel_key:
            continue
        seen.add(folder_path)
        rows.append({"key": rel_key, "name": folder_path.relative_to(root).as_posix()})
    rows.sort(key=lambda r: r["name"].lower())
    return rows


def rel_folder_key(source_root: Path, folder: Path) -> str | None:
    try:
        return folder.resolve().relative_to(source_root.resolve()).as_posix().lower()
    except ValueError:
        return None
