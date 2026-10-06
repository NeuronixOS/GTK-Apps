"""Persist gtk-calendar sources and Google client settings."""

from __future__ import annotations

import json
import os
import uuid
from pathlib import Path
from typing import Any

from .models import CalendarSource
from .paths import config_file, ics_store_dir


DEFAULT_COLORS = (
    "#458588",
    "#689d6a",
    "#d79921",
    "#cc241d",
    "#b16286",
    "#d65d0e",
    "#8ec07c",
    "#83a598",
)


def _chmod_private(path: Path) -> None:
    try:
        os.chmod(path, 0o600)
    except OSError:
        pass


class Store:
    def __init__(self) -> None:
        self.week_start = "sunday"
        self.view_mode = "month"
        self.event_font_px = 20
        self.google_client_id = ""
        self.google_client_secret = ""
        self.google_account = ""
        self.sources: list[CalendarSource] = []
        self.load()

    def load(self) -> None:
        path = config_file()
        if not path.is_file():
            return
        try:
            raw = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return
        if not isinstance(raw, dict):
            return
        week = str(raw.get("week_start") or "sunday").lower()
        self.week_start = "sunday" if week != "monday" else "monday"
        mode = str(raw.get("view_mode") or "month").lower()
        self.view_mode = "week" if mode == "week" else "month"
        try:
            size = int(raw.get("event_font_px") or 20)
        except (TypeError, ValueError):
            size = 20
        self.event_font_px = max(10, min(36, size))
        google = raw.get("google") if isinstance(raw.get("google"), dict) else {}
        self.google_client_id = str(google.get("client_id") or "").strip()
        self.google_client_secret = str(google.get("client_secret") or "").strip()
        self.google_account = str(google.get("account") or "").strip()
        sources: list[CalendarSource] = []
        for item in raw.get("sources") or []:
            src = CalendarSource.from_json(item)
            if src is not None:
                sources.append(src)
        self.sources = sources

    def save(self) -> None:
        path = config_file()
        payload: dict[str, Any] = {
            "week_start": self.week_start,
            "view_mode": self.view_mode,
            "event_font_px": self.event_font_px,
            "google": {
                "client_id": self.google_client_id,
                "client_secret": self.google_client_secret,
                "account": self.google_account,
            },
            "sources": [src.to_json() for src in self.sources],
        }
        path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
        _chmod_private(path)

    def next_color(self) -> str:
        used = {src.color.lower() for src in self.sources}
        for color in DEFAULT_COLORS:
            if color.lower() not in used:
                return color
        return DEFAULT_COLORS[len(self.sources) % len(DEFAULT_COLORS)]

    def source_by_id(self, ident: str) -> CalendarSource | None:
        for src in self.sources:
            if src.id == ident:
                return src
        return None

    def upsert_source(self, source: CalendarSource) -> CalendarSource:
        for idx, existing in enumerate(self.sources):
            if existing.id == source.id:
                self.sources[idx] = source
                self.save()
                return source
            if source.kind == "ics" and source.path and existing.path == source.path:
                existing.name = source.name or existing.name
                existing.url = source.url or existing.url
                self.save()
                return existing
            if source.kind == "ics" and source.url and existing.url == source.url:
                existing.name = source.name or existing.name
                existing.path = source.path or existing.path
                self.save()
                return existing
            if source.kind == "google" and source.google_id and existing.google_id == source.google_id:
                existing.name = source.name or existing.name
                existing.color = existing.color or source.color
                self.save()
                return existing
        self.sources.append(source)
        self.save()
        return source

    def remove_source(self, ident: str) -> None:
        self.sources = [src for src in self.sources if src.id != ident]
        self.save()

    def replace_google_calendars(self, calendars: list[CalendarSource]) -> None:
        kept = [src for src in self.sources if src.kind != "google"]
        known = {src.google_id: src for src in self.sources if src.kind == "google"}
        merged: list[CalendarSource] = []
        for cal in calendars:
            prev = known.get(cal.google_id)
            if prev is not None:
                cal.id = prev.id
                cal.enabled = prev.enabled
                cal.color = prev.color or cal.color
            merged.append(cal)
        self.sources = kept + merged
        self.save()

    def new_ics_id(self) -> str:
        return f"ics:{uuid.uuid4().hex[:12]}"

    def copy_ics_file(self, src_path: Path, ident: str) -> Path:
        dest = ics_store_dir() / f"{ident.split(':', 1)[-1]}.ics"
        dest.write_bytes(src_path.read_bytes())
        _chmod_private(dest)
        return dest
