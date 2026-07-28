"""Resolve the configs root directory."""

from __future__ import annotations

import argparse
from pathlib import Path


def default_root() -> Path:
    return Path.home() / "configs"


def resolve_root(cli_root: str | None = None) -> Path:
    if cli_root:
        return Path(cli_root).expanduser().resolve()
    return default_root().resolve()


def add_root_arg(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--root",
        metavar="PATH",
        help="Configs tree to edit (default: ~/configs)",
    )


def list_text_files(root: Path) -> list[Path]:
    """Text-ish configs under root, excluding secrets tokens and binaries."""
    skip_dirs = {"secrets", "__pycache__", ".git"}
    skip_suffixes = {".odt", ".odg", ".odp", ".ods", ".xcf", ".png", ".jpg", ".jpeg", ".gif", ".webp"}
    skip_names = {".gitkeep"}
    out: list[Path] = []
    if not root.is_dir():
        return out
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        rel = path.relative_to(root)
        if any(p in skip_dirs for p in rel.parts):
            continue
        if path.name in skip_names:
            continue
        if path.suffix.lower() in skip_suffixes:
            continue
        out.append(path)
    return out
