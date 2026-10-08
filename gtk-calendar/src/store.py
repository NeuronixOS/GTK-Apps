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
        self.hidden_events: list[dict[str, str]] = []
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
        hidden: list[dict[str, str]] = []
        for item in raw.get("hidden_events") or []:
            if isinstance(item, dict):
                uid = str(item.get("uid") or "").strip()
                if not uid:
                    continue
                    hidden.append(
                    {
                        "key": str(item.get("key") or "").strip(),
                        "uid": uid,
                        "title": str(item.get("title") or "Event").strip() or "Event",
                        "source_id": str(item.get("source_id") or "").strip(),
                        "when": str(item.get("when") or "").strip(),
                    }
                )
            elif isinstance(item, str) and item.strip():
                hidden.append({"uid": item.strip(), "title": "Event", "source_id": ""})
        self.hidden_events = hidden

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
            "hidden_events": self.hidden_events,
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

    def hidden_keys(self) -> set[str]:
        return {item["key"] for item in self.hidden_events if item.get("key")}

    def is_hidden(
        self,
        key: str,
        uid: str = "",
        source_id: str = "",
        title: str = "",
        start: str = "",
    ) -> bool:
        title = title.strip()
        start = start.strip()
        if key and key in self.hidden_keys():
            return True
        for item in self.hidden_events:
            if (item.get("title") or "").strip() != title:
                continue
            if (item.get("uid") or "") != uid:
                continue
            stored_src = item.get("source_id") or ""
            if stored_src and stored_src != source_id:
                continue
            stored_start = (item.get("start") or "").strip()
            if stored_start and stored_start != start:
                continue
            return True
        return False

    def hide_event(
        self,
        key: str,
        uid: str,
        title: str,
        source_id: str = "",
        when: str = "",
        start: str = "",
    ) -> None:
        key = key.strip()
        uid = uid.strip()
        if not key or key in self.hidden_keys():
            return
        self.hidden_events.append(
            {
                "key": key,
                "uid": uid,
                "title": title.strip() or "Event",
                "source_id": source_id.strip(),
                "when": when.strip(),
                "start": start.strip(),
            }
        )
        self.save()

    def unhide_event(self, key: str) -> None:
        self.hidden_events = [
            item
            for item in self.hidden_events
            if item.get("key") != key and item.get("uid") != key
        ]
        self.save()
