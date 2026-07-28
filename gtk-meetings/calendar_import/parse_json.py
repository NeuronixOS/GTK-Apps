"""Parse pasted JSON into structured calendar events."""

from __future__ import annotations

import json
from dataclasses import dataclass
from datetime import datetime
from typing import Any

from .day_label import format_today_label, parse_day_label
from .parse_event_text import ParsedEvent, parse_event_text


@dataclass
class ParsedImport:
    events: list[ParsedEvent]
    day_labels: list[str]


def _normalize_entry(entry: Any) -> str:
    if isinstance(entry, str):
        return entry
    if isinstance(entry, list):
        if not entry:
            raise ValueError("Event entry array is empty")
        if len(entry) == 1 and isinstance(entry[0], str):
            return entry[0]
        if len(entry) == 2 and all(isinstance(part, str) for part in entry):
            time_text, summary = entry
            time_text = time_text.strip()
            summary = summary.strip()
            if not summary:
                raise ValueError("Event summary is empty")
            if time_text:
                return f"{time_text}: {summary}"
            return summary
        raise ValueError(f"Unsupported event entry shape: {entry!r}")
    raise ValueError(f"Unsupported event entry type: {type(entry).__name__}")


def _extract_dated_entries(data: Any) -> list[tuple[str, list[Any], datetime | None]]:
    if isinstance(data, list):
        return [(format_today_label(), data, None)]

    if isinstance(data, dict):
        if not data:
            raise ValueError("JSON object has no day keys")

        today_label = format_today_label()
        if today_label in data:
            entries = data[today_label]
            if not isinstance(entries, list):
                raise ValueError(
                    f"Expected a list for {today_label!r}, got {type(entries).__name__}"
                )
            return [(today_label, entries, None)]

        dated: list[tuple[str, list[Any], datetime | None]] = []
        for day_label, entries in data.items():
            if not isinstance(entries, list):
                raise ValueError(
                    f"Expected a list for {day_label!r}, got {type(entries).__name__}"
                )
            dated.append((day_label, entries, parse_day_label(day_label)))

        dated.sort(
            key=lambda item: item[2] if item[2] is not None else parse_day_label(item[0])
        )
        return dated

    raise ValueError(f"Expected JSON object or array, got {type(data).__name__}")


def parse_json_text(json_text: str) -> ParsedImport:
    json_text = json_text.strip()
    if not json_text:
        raise ValueError("JSON input is empty")

    try:
        data = json.loads(json_text)
    except json.JSONDecodeError as exc:
        raise ValueError(f"Invalid JSON: {exc}") from exc

    dated_entries = _extract_dated_entries(data)
    events: list[ParsedEvent] = []
    day_labels: list[str] = []

    for day_label, entries, on_date in dated_entries:
        if not entries:
            continue
        day_labels.append(day_label)
        for entry in entries:
            events.append(parse_event_text(_normalize_entry(entry), on_date=on_date))

    if not events:
        raise ValueError("No events found in JSON")

    return ParsedImport(events=events, day_labels=day_labels)
