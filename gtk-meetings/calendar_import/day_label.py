"""Day label formatting matching api/calendar/events.py."""

from __future__ import annotations

import re
from datetime import datetime
from zoneinfo import ZoneInfo

TZ = ZoneInfo("America/New_York")


def ordinal(day: int) -> str:
    if 10 <= day % 100 <= 20:
        suffix = "th"
    else:
        suffix = {1: "st", 2: "nd", 3: "rd"}.get(day % 10, "th")
    return f"{day}{suffix}"


def format_day_label(dt: datetime) -> str:
    return dt.strftime("%a, %B ") + ordinal(dt.day) + dt.strftime(", %Y")


def format_today_label() -> str:
    now = datetime.now(TZ)
    return format_day_label(now)


def parse_day_label(label: str) -> datetime:
    clean = re.sub(r"(\d+)(st|nd|rd|th)", r"\1", label.strip())
    parsed = datetime.strptime(clean, "%a, %B %d, %Y")
    return parsed.replace(tzinfo=TZ)
