"""Reverse-parse event text strings produced by api/calendar/events.py."""

from __future__ import annotations

import re
from dataclasses import dataclass
from datetime import datetime, timedelta
from zoneinfo import ZoneInfo

TZ = ZoneInfo("America/New_York")

TIMED_RANGE_RE = re.compile(
    r"^(\d{1,2}:\d{2} [AP]M) - (\d{1,2}:\d{2} [AP]M): (.+)$",
    re.IGNORECASE,
)
TIMED_START_RE = re.compile(
    r"^(\d{1,2}:\d{2} [AP]M): (.+)$",
    re.IGNORECASE,
)
DEFAULT_DURATION = timedelta(hours=1)


@dataclass
class ParsedEvent:
    summary: str
    start: datetime
    end: datetime
    all_day: bool


def _today_midnight() -> datetime:
    now = datetime.now(TZ)
    return now.replace(hour=0, minute=0, second=0, microsecond=0)


def _midnight_on(date: datetime) -> datetime:
    return date.replace(hour=0, minute=0, second=0, microsecond=0)


def _parse_time_on_date(time_text: str, date: datetime) -> datetime:
    parsed = datetime.strptime(time_text.upper(), "%I:%M %p")
    base = _midnight_on(date)
    return base.replace(hour=parsed.hour, minute=parsed.minute)


def parse_event_text(text: str, *, on_date: datetime | None = None) -> ParsedEvent:
    text = text.strip()
    if not text:
        raise ValueError("Event text is empty")

    base_date = _today_midnight() if on_date is None else _midnight_on(on_date)

    match = TIMED_RANGE_RE.match(text)
    if match:
        start = _parse_time_on_date(match.group(1), base_date)
        end = _parse_time_on_date(match.group(2), base_date)
        if end <= start:
            raise ValueError(f"End time must be after start time: {text}")
        return ParsedEvent(summary=match.group(3).strip(), start=start, end=end, all_day=False)

    match = TIMED_START_RE.match(text)
    if match:
        start = _parse_time_on_date(match.group(1), base_date)
        end = start + DEFAULT_DURATION
        return ParsedEvent(summary=match.group(2).strip(), start=start, end=end, all_day=False)

    start = base_date
    end = start + timedelta(days=1)
    return ParsedEvent(summary=text, start=start, end=end, all_day=True)
