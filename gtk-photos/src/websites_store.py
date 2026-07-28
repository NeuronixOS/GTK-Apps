"""Persist website links and thumbnail overrides for the Websites tab."""

from __future__ import annotations

import json
import shutil
import uuid
from pathlib import Path

from . import websites_thumbnails as wt
from .config_paths import config_dir, project_dir


def store_paths() -> dict[str, Path]:
    """Website data lives under ``~/.config/gtk-apps/gtk-photos/``; missing icon stays in the app tree."""
    cfg = config_dir()
    return {
        "links": cfg / "websites.json",
        "img": cfg / "websites_img",
        "overrides": cfg / "websites_thumbnail_overrides.json",
        "missing_svg": project_dir() / "assets" / "websites_missing.svg",
    }


# Back-compat alias used by older call sites.
def project_paths(_project_root: Path | None = None) -> dict[str, Path]:
    return store_paths()


def migrate_from_legacy_if_needed(_project_root: Path | None = None) -> None:
    """Copy websites data into the config dir from an old in-tree location if present."""
    paths = store_paths()
    paths["img"].mkdir(parents=True, exist_ok=True)

    sources: list[Path] = [project_dir()]
    link_names = ("websites.json",)
    override_names = ("websites_thumbnail_overrides.json",)
    img_names = ("websites_img",)

    if not paths["links"].is_file():
        for root in sources:
            for name in link_names:
                src = root / name
                if src.is_file():
                    try:
                        shutil.copy2(src, paths["links"])
                    except OSError:
                        pass
                    break
            if paths["links"].is_file():
                break

    if not paths["overrides"].is_file():
        for root in sources:
            for name in override_names:
                src = root / name
                if src.is_file():
                    try:
                        shutil.copy2(src, paths["overrides"])
                    except OSError:
                        pass
                    break
            if paths["overrides"].is_file():
                break

    # Copy any missing thumbnail JPGs from older img dirs into config.
    for root in sources:
        for name in img_names:
            legacy_img = root / name
            if not legacy_img.is_dir() or legacy_img.resolve() == paths["img"].resolve():
                continue
            for jpg in legacy_img.glob("*.jpg"):
                dest = paths["img"] / jpg.name
                if dest.is_file():
                    continue
                try:
                    shutil.copy2(jpg, dest)
                except OSError:
                    pass


def load_overrides(path: Path) -> dict:
    if not path.is_file():
        return {}
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        return {k: v for k, v in data.items() if not str(k).startswith("_")}
    except (json.JSONDecodeError, OSError):
        return {}


def _sync_overrides_file(store: dict, overrides_path: Path) -> None:
    meta: dict = {}
    rest: dict = {}
    if overrides_path.is_file():
        try:
            raw = json.loads(overrides_path.read_text(encoding="utf-8"))
            for k, v in raw.items():
                if str(k).startswith("_"):
                    meta[k] = v
                elif isinstance(v, str):
                    rest[k] = v
        except (json.JSONDecodeError, OSError):
            pass
    for item in store.get("links", []):
        slug = wt.url_slug(item["url"])
        fo = item.get("folder_override")
        if fo:
            rest[slug] = fo
        else:
            rest.pop(slug, None)
    out: dict = {}
    for k in sorted(meta.keys()):
        out[k] = meta[k]
    for k in sorted(rest.keys()):
        out[k] = rest[k]
    overrides_path.parent.mkdir(parents=True, exist_ok=True)
    overrides_path.write_text(json.dumps(out, indent=2) + "\n", encoding="utf-8")


def load_store(links_path: Path) -> dict:
    if not links_path.is_file():
        return {"links": []}
    try:
        data = json.loads(links_path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return {"links": []}
    links = data.get("links")
    if not isinstance(links, list):
        return {"links": []}
    out = []
    for item in links:
        if not isinstance(item, dict):
            continue
        url = item.get("url")
        if not url or not isinstance(url, str):
            continue
        lid = item.get("id")
        if not lid or not isinstance(lid, str):
            lid = uuid.uuid4().hex[:12]
        fo = item.get("folder_override")
        if fo is not None and not isinstance(fo, str):
            fo = None
        locked = item.get("thumbnail_locked") is True
        out.append(
            {
                "id": lid,
                "url": url.strip(),
                "folder_override": fo,
                "thumbnail_locked": locked,
            }
        )
    return {"links": out}


def clear_thumbnail_files(img_dir: Path) -> int:
    """Remove all generated thumbnails (and partial encodes) under img_dir."""
    removed = 0
    if not img_dir.is_dir():
        return 0
    for pattern in ("*.jpg", "*.jpg.part", "*.mp4.part"):
        for path in img_dir.glob(pattern):
            try:
                path.unlink()
                removed += 1
            except OSError:
                pass
    return removed


def save_store(store: dict, links_path: Path, overrides_path: Path) -> None:
    links_path.parent.mkdir(parents=True, exist_ok=True)
    links_path.write_text(json.dumps(store, indent=2), encoding="utf-8")
    _sync_overrides_file(store, overrides_path)
