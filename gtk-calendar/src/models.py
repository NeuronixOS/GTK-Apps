"""Calendar events and sources."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import date, datetime, timedelta
from typing import Optional
from zoneinfo import ZoneInfo

LOCAL_TZ = datetime.now().astimezone().tzinfo or ZoneInfo("UTC")


def as_aware(value: datetime) -> datetime:
    if value.tzinfo is None:
        return value.replace(tzinfo=LOCAL_TZ)
    return value


def as_local(value: datetime) -> datetime:
    return as_aware(value).astimezone(LOCAL_TZ)


def day_key(value: date | datetime) -> date:
    if isinstance(value, datetime):
        return as_local(value).date()
    return value


@dataclass
class CalendarSource:
    id: str
    kind: str  # "google" | "ics"
    name: str
    color: str
    enabled: bool = True
    google_id: str = ""
    url: str = ""
    path: str = ""
    error: str = ""

    def to_json(self) -> dict:
        return {
            "id": self.id,
            "kind": self.kind,
            "name": self.name,
            "color": self.color,
            "enabled": self.enabled,
            "google_id": self.google_id,
            "url": self.url,
            "path": self.path,
        }

    @classmethod
    def from_json(cls, raw: object) -> Optional["CalendarSource"]:
        if not isinstance(raw, dict):
            return None
        ident = str(raw.get("id") or "").strip()
        kind = str(raw.get("kind") or "").strip()
        name = str(raw.get("name") or "").strip() or "Calendar"
        color = str(raw.get("color") or "#458588").strip() or "#458588"
        if not ident or kind not in ("google", "ics"):
            return None
        return cls(
            id=ident,
            kind=kind,
            name=name,
            color=color,
            enabled=bool(raw.get("enabled", True)),
            google_id=str(raw.get("google_id") or "").strip(),
            url=str(raw.get("url") or "").strip(),
            path=str(raw.get("path") or "").strip(),
        )


@dataclass
class CalendarEvent:
    uid: str
    title: str
    start: datetime
    end: datetime
    all_day: bool
    source_id: str
    source_name: str
    color: str
    location: str = ""
    description: str = ""
    url: str = ""
    days: list[date] = field(default_factory=list)

    def __post_init__(self) -> None:
        self.start = as_aware(self.start)
        self.end = as_aware(self.end)
        if not self.days:
            self.days = list(self.iter_days())

    def iter_days(self):
        start_d = day_key(self.start)
        end_dt = as_local(self.end)
        if self.all_day:
            # ICS DTEND for all-day is exclusive.
            end_d = end_dt.date() - timedelta(days=1) if end_dt.time() == datetime.min.time() else end_dt.date()
            if end_d < start_d:
                end_d = start_d
        else:
            end_d = day_key(self.end)
            if self.end.astimezone(LOCAL_TZ).time() == datetime.min.time() and self.end > self.start:
                end_d = end_d - timedelta(days=1)
            if end_d < start_d:
                end_d = start_d
        cur = start_d
        while cur <= end_d:
            yield cur
            cur += timedelta(days=1)

    def time_label(self) -> str:
        if self.all_day:
            days = list(self.iter_days())
            if len(days) > 1:
                return f"All day · {days[0].strftime('%b')} {days[0].day}–{days[-1].strftime('%b')} {days[-1].day}"
            return "All day"
        start = as_local(self.start)
        end = as_local(self.end)
        return f"{_clock(start)} – {_clock(end)}"

    def start_clock(self) -> str:
        if self.all_day:
            return "All day"
        return _clock(as_local(self.start))

    def hide_key(self) -> str:
        return "|".join(
            (
                self.source_id,
                self.uid,
                as_local(self.start).isoformat(),
                as_local(self.end).isoformat(),
                self.title.strip(),
            )
        )


def _clock(value: datetime) -> str:
    hour12 = value.hour % 12 or 12
    return f"{hour12}:{value.strftime('%M')}"
