"""Load events from .ics files and HTTP/webcal URLs."""

from __future__ import annotations

import calendar as pycal
import re
import ssl
import urllib.request
from datetime import date, datetime, timedelta, timezone, tzinfo
from pathlib import Path
from typing import Iterable
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from .models import LOCAL_TZ, CalendarEvent, CalendarSource, as_aware
from .paths import ics_cache_file

_UNFOLD = re.compile(r"\r?\n[ \t]")
_DOW = {"MO": 0, "TU": 1, "WE": 2, "TH": 3, "FR": 4, "SA": 5, "SU": 6}

# Outlook / Windows TZID → IANA (the Windows names include DST).
_WIN_TZ = {
    "utc": "UTC",
    "gmt standard time": "Europe/London",
    "gmt": "Europe/London",
    "eastern standard time": "America/New_York",
    "us eastern standard time": "America/Indianapolis",
    "central standard time": "America/Chicago",
    "central standard time (mexico)": "America/Mexico_City",
    "mountain standard time": "America/Denver",
    "us mountain standard time": "America/Phoenix",
    "pacific standard time": "America/Los_Angeles",
    "pacific standard time (mexico)": "America/Tijuana",
    "alaskan standard time": "America/Anchorage",
    "hawaiian standard time": "Pacific/Honolulu",
    "atlantic standard time": "America/Halifax",
    "sa pacific standard time": "America/Bogota",
    "sa western standard time": "America/La_Paz",
    "sa eastern standard time": "America/Cayenne",
    "argentina standard time": "America/Argentina/Buenos_Aires",
    "e. south america standard time": "America/Sao_Paulo",
    "romance standard time": "Europe/Paris",
    "w. europe standard time": "Europe/Berlin",
    "central europe standard time": "Europe/Budapest",
    "central european standard time": "Europe/Warsaw",
    "greenwich standard time": "Atlantic/Reykjavik",
    "gtb standard time": "Europe/Bucharest",
    "fli standard time": "Europe/Helsinki",
    "russian standard time": "Europe/Moscow",
    "india standard time": "Asia/Kolkata",
    "singapore standard time": "Asia/Singapore",
    "china standard time": "Asia/Shanghai",
    "tokyo standard time": "Asia/Tokyo",
    "korea standard time": "Asia/Seoul",
    "aus eastern standard time": "Australia/Sydney",
    "e. australia standard time": "Australia/Brisbane",
    "aus central standard time": "Australia/Darwin",
    "w. australia standard time": "Australia/Perth",
    "new zealand standard time": "Pacific/Auckland",
    "israel standard time": "Asia/Jerusalem",
    "egypt standard time": "Africa/Cairo",
    "south africa standard time": "Africa/Johannesburg",
}


def normalize_ics_url(url: str) -> str:
    value = (url or "").strip()
    if value.lower().startswith("webcal://"):
        return "https://" + value[9:]
    if value.lower().startswith("webcals://"):
        return "https://" + value[10:]
    return value


def fetch_ics_text(url: str, timeout: int = 45) -> str:
    target = normalize_ics_url(url)
    if not target:
        raise ValueError("Empty calendar URL")
    req = urllib.request.Request(
        target,
        headers={
            "User-Agent": (
                "Mozilla/5.0 gtk-calendar/1.0 (Neuronix GTK-Apps) "
                "AppleWebKit/537.36 (KHTML, like Gecko)"
            ),
            "Accept": "text/calendar, text/plain, */*",
        },
    )
    context = ssl.create_default_context()
    with urllib.request.urlopen(req, timeout=timeout, context=context) as resp:
        raw = resp.read()
        charset = "utf-8"
        ctype = resp.headers.get("Content-Type", "")
        if "charset=" in ctype.lower():
            charset = ctype.split("charset=", 1)[-1].split(";")[0].strip() or charset
    text = raw.decode(charset, errors="replace")
    if "BEGIN:VCALENDAR" not in text:
        raise ValueError("That URL did not return an iCalendar (.ics) feed.")
    return text


def load_ics_source(
    source: CalendarSource,
    window_start: datetime,
    window_end: datetime,
    *,
    network: bool = True,
) -> tuple[list[CalendarEvent], str]:
    """Return (events, calendar_name_hint)."""
    cache = ics_cache_file(source.id)
    text = ""
    error: Exception | None = None
    if source.path:
        path = Path(source.path).expanduser()
        if path.is_file():
            text = path.read_text(encoding="utf-8", errors="replace")
        elif source.url and network:
            try:
                text = fetch_ics_text(source.url)
            except Exception as exc:
                error = exc
        elif not path.is_file():
            raise FileNotFoundError(f"ICS file not found: {path}")
    elif source.url:
        if network:
            try:
                text = fetch_ics_text(source.url)
                try:
                    cache.write_text(text, encoding="utf-8")
                except OSError:
                    pass
            except Exception as exc:
                error = exc
        if not text and cache.is_file():
            text = cache.read_text(encoding="utf-8", errors="replace")
    else:
        raise ValueError("ICS source has no file or URL")
    if not text and cache.is_file():
        text = cache.read_text(encoding="utf-8", errors="replace")
    if not text:
        raise error or ValueError("ICS source has no file or URL")
    events, hint = parse_ics(text, source, window_start, window_end)
    if error and not events:
        raise error
    return events, hint


def parse_ics(
    text: str,
    source: CalendarSource,
    window_start: datetime,
    window_end: datetime,
) -> tuple[list[CalendarEvent], str]:
    unfolded = _unfold(text)
    hint = _header_name(unfolded)
    try:
        from icalendar import Calendar  # type: ignore
    except ImportError:
        return _parse_ics_fallback(unfolded, source, window_start, window_end), hint

    cal = Calendar.from_ical(text.encode("utf-8") if isinstance(text, str) else text)
    hint = str(cal.get("X-WR-CALNAME") or cal.get("NAME") or hint or "").strip()
    events: list[CalendarEvent] = []
    for component in cal.walk("VEVENT"):
        status = str(component.get("STATUS") or "").upper()
        if status == "CANCELLED":
            continue
        events.extend(_expand_ical_event(component, source, window_start, window_end))
    events.sort(key=lambda ev: (ev.start, ev.title.lower()))
    return events, hint


def _unfold(text: str) -> str:
    return _UNFOLD.sub("", text.replace("\r\n", "\n").replace("\r", "\n"))


def _header_name(text: str) -> str:
    for key in ("X-WR-CALNAME", "NAME"):
        match = re.search(rf"^{key}[^:]*:(.*)$", text, re.I | re.M)
        if match:
            return _unescape(match.group(1).strip())
    return ""


def _unescape(value: str) -> str:
    return (
        value.replace("\\n", "\n")
        .replace("\\N", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
    )


def _tz_from_name(name: str) -> tzinfo:
    raw = (name or "").strip().strip('"')
    if not raw:
        return LOCAL_TZ
    mapped = _WIN_TZ.get(raw.lower()) or raw
    try:
        return ZoneInfo(mapped)
    except (ZoneInfoNotFoundError, Exception):
        return LOCAL_TZ


def _parse_content_line(line: str) -> tuple[str, dict[str, str], str] | None:
    if ":" not in line:
        return None
    head, value = line.split(":", 1)
    parts = head.split(";")
    name = parts[0].strip().upper()
    if not name:
        return None
    params: dict[str, str] = {}
    for part in parts[1:]:
        if "=" in part:
            key, val = part.split("=", 1)
            params[key.strip().upper()] = val.strip().strip('"')
        elif part.strip():
            params[part.strip().upper()] = "TRUE"
    return name, params, value


def _parse_dt(value: str, params: dict[str, str] | None = None) -> tuple[datetime | None, bool]:
    params = params or {}
    raw = (value or "").strip()
    if not raw:
        return None, False
    tz = LOCAL_TZ
    tzid = params.get("TZID") or ""
    match = re.search(r"TZID=([^;:]+)", raw, re.I)
    if match and not tzid:
        tzid = match.group(1).strip().strip('"')
        raw = raw.split(":", 1)[-1].strip()
    if tzid:
        tz = _tz_from_name(tzid)
    if raw.endswith("Z"):
        tz = timezone.utc
        raw = raw[:-1]
    compact = raw.replace("-", "").replace(":", "")
    all_day = params.get("VALUE", "").upper() == "DATE" or ("T" not in compact)
    try:
        if "T" in compact:
            fmt = "%Y%m%dT%H%M%S" if len(compact) >= 15 else "%Y%m%dT%H%M"
            dt = datetime.strptime(compact[:15] if len(compact) >= 15 else compact, fmt)
            return dt.replace(tzinfo=tz), False
        day = datetime.strptime(compact[:8], "%Y%m%d").date()
        return datetime(day.year, day.month, day.day, tzinfo=tz), True
    except ValueError:
        return None, all_day


def _as_datetime(value: object, default_tz=LOCAL_TZ) -> tuple[datetime, bool]:
    if isinstance(value, datetime):
        return as_aware(value if value.tzinfo else value.replace(tzinfo=default_tz)), False
    if isinstance(value, date):
        return datetime(value.year, value.month, value.day, tzinfo=default_tz), True
    raise TypeError(f"Unsupported date value: {type(value)!r}")


def _expand_ical_event(component, source: CalendarSource, window_start: datetime, window_end: datetime) -> Iterable[CalendarEvent]:
    try:
        dtstart_raw = component.decoded("DTSTART")
    except Exception:
        return
    start, all_day = _as_datetime(dtstart_raw)
    if "DTEND" in component:
        try:
            end, end_all_day = _as_datetime(component.decoded("DTEND"))
            all_day = all_day or end_all_day
        except Exception:
            end = start + (timedelta(days=1) if all_day else timedelta(hours=1))
    elif "DURATION" in component:
        try:
            end = start + component.decoded("DURATION")
        except Exception:
            end = start + (timedelta(days=1) if all_day else timedelta(hours=1))
    else:
        end = start + (timedelta(days=1) if all_day else timedelta(hours=1))

    title = str(component.get("SUMMARY") or "Untitled").strip() or "Untitled"
    location = str(component.get("LOCATION") or "").strip()
    description = str(component.get("DESCRIPTION") or "").strip()
    uid = str(component.get("UID") or f"{source.id}:{title}:{start.isoformat()}")
    url = str(component.get("URL") or "").strip()

    rrule = component.get("RRULE")
    rrule_text = ""
    if rrule is not None:
        rrule_text = rrule.to_ical().decode("utf-8") if hasattr(rrule, "to_ical") else str(rrule)
    exdates: list[datetime] = []
    for ex in _as_list(component.get("EXDATE")):
        try:
            decoded = ex.dts if hasattr(ex, "dts") else [ex]
            for item in decoded:
                val = item.dt if hasattr(item, "dt") else item
                if isinstance(val, datetime):
                    exdates.append(as_aware(val))
                elif isinstance(val, date):
                    exdates.append(datetime(val.year, val.month, val.day, tzinfo=start.tzinfo or LOCAL_TZ))
        except Exception:
            continue
    occurrences = _occurrences(start, end, rrule_text, exdates, window_start, window_end)
    for occ_start, occ_end in occurrences:
        yield CalendarEvent(
            uid=f"{uid}:{occ_start.date().isoformat()}",
            title=title,
            start=occ_start,
            end=occ_end,
            all_day=all_day,
            source_id=source.id,
            source_name=source.name,
            color=source.color,
            location=location,
            description=description,
            url=url,
        )


def _as_list(value) -> list:
    if value is None:
        return []
    if isinstance(value, list):
        return value
    return [value]


def _occurrences(
    start: datetime,
    end: datetime,
    rrule_text: str,
    exdates: list[datetime],
    window_start: datetime,
    window_end: datetime,
) -> list[tuple[datetime, datetime]]:
    duration = end - start
    skip = {_exdate_key(dt) for dt in exdates}
    if not rrule_text.strip():
        if end < window_start or start > window_end:
            return []
        return [(start, end)]

    try:
        from dateutil.rrule import rrulestr

        dtstart = start.replace(tzinfo=None)
        rule = rrulestr(rrule_text, dtstart=dtstart)
        window_naive_start = window_start.astimezone(start.tzinfo or LOCAL_TZ).replace(tzinfo=None)
        window_naive_end = window_end.astimezone(start.tzinfo or LOCAL_TZ).replace(tzinfo=None)
        found: list[tuple[datetime, datetime]] = []
        for occ in rule.between(window_naive_start - timedelta(days=1), window_naive_end + timedelta(days=1), inc=True):
            occ_aware = occ.replace(tzinfo=start.tzinfo or LOCAL_TZ) if occ.tzinfo is None else as_aware(occ)
            if _exdate_key(occ_aware) in skip:
                continue
            occ_end = occ_aware + duration
            if occ_end < window_start or occ_aware > window_end:
                continue
            found.append((occ_aware, occ_end))
        return found
    except Exception:
        return _expand_rrule_stdlib(start, duration, rrule_text, skip, window_start, window_end)


def _exdate_key(value: datetime) -> str:
    local = as_aware(value).astimezone(timezone.utc)
    return local.strftime("%Y%m%dT%H%M%S")


def _expand_rrule_stdlib(
    start: datetime,
    duration: timedelta,
    rrule_text: str,
    skip: set[str],
    window_start: datetime,
    window_end: datetime,
) -> list[tuple[datetime, datetime]]:
    params: dict[str, str] = {}
    for part in rrule_text.replace("RRULE:", "").split(";"):
        if "=" in part:
            key, val = part.split("=", 1)
            params[key.strip().upper()] = val.strip()
    freq = (params.get("FREQ") or "DAILY").upper()
    interval = max(1, int(params.get("INTERVAL") or 1))
    until, _ = _parse_dt(params.get("UNTIL") or "")
    count = int(params["COUNT"]) if params.get("COUNT") else None
    bydays = [_DOW[d[-2:]] for d in (params.get("BYDAY") or "").split(",") if d[-2:] in _DOW]
    tz = start.tzinfo or LOCAL_TZ
    cursor = start
    emitted = 0
    out: list[tuple[datetime, datetime]] = []
    guard = 0
    limit = count if count is not None else 800

    def accept(occ: datetime) -> bool:
        nonlocal emitted
        if occ < start - timedelta(minutes=1):
            return True
        if until is not None and occ > until:
            return False
        if _exdate_key(occ) in skip:
            return True
        occ_end = occ + duration
        if not (occ_end < window_start or occ > window_end):
            out.append((occ, occ_end))
        emitted += 1
        return count is None or emitted < count

    if freq == "WEEKLY" and bydays:
        week0 = start.date() - timedelta(days=start.weekday())
        day = window_start.date() - timedelta(days=14)
        last = (until.date() if until else window_end.date()) + timedelta(days=1)
        while day <= last and emitted < limit and guard < 4000:
            guard += 1
            if day.weekday() in bydays:
                weeks = (day - week0).days // 7
                if weeks >= 0 and weeks % interval == 0:
                    occ = datetime(day.year, day.month, day.day, start.hour, start.minute, start.second, tzinfo=tz)
                    if occ + duration >= start:
                        if not accept(occ):
                            break
            day += timedelta(days=1)
        return out

    step = {
        "DAILY": timedelta(days=interval),
        "WEEKLY": timedelta(weeks=interval),
        "YEARLY": None,
        "MONTHLY": None,
    }.get(freq, timedelta(days=interval))

    while emitted < limit and guard < 4000:
        guard += 1
        if not accept(cursor):
            break
        if freq == "MONTHLY":
            month = cursor.month - 1 + interval
            year = cursor.year + month // 12
            month = month % 12 + 1
            last = pycal.monthrange(year, month)[1]
            cursor = cursor.replace(year=year, month=month, day=min(cursor.day, last))
        elif freq == "YEARLY":
            try:
                cursor = cursor.replace(year=cursor.year + interval)
            except ValueError:
                cursor = cursor.replace(year=cursor.year + interval, day=28)
        elif step is not None:
            cursor = cursor + step
        else:
            break
        if until is not None and cursor > until + timedelta(days=1):
            break
        if cursor > window_end + timedelta(days=370) and count is None:
            break
    return out


def _parse_ics_fallback(
    text: str,
    source: CalendarSource,
    window_start: datetime,
    window_end: datetime,
) -> list[CalendarEvent]:
    events: list[CalendarEvent] = []
    blocks = re.split(r"BEGIN:VEVENT\n", text, flags=re.I)
    for block in blocks[1:]:
        body = block.split("END:VEVENT", 1)[0]
        props: dict[str, tuple[dict[str, str], str]] = {}
        extras: dict[str, list[tuple[dict[str, str], str]]] = {}
        for line in body.split("\n"):
            parsed = _parse_content_line(line)
            if parsed is None:
                continue
            name, params, value = parsed
            if name in ("EXDATE", "RDATE"):
                extras.setdefault(name, []).append((params, value))
            else:
                props[name] = (params, value)
        status = (props.get("STATUS", ({}, ""))[1] or "").upper()
        if status == "CANCELLED":
            continue
        start_params, start_raw = props.get("DTSTART", ({}, ""))
        start, all_day = _parse_dt(start_raw, start_params)
        if start is None:
            continue
        if "DTEND" in props:
            end_params, end_raw = props["DTEND"]
            end, end_all = _parse_dt(end_raw, end_params)
            all_day = all_day or end_all
            if end is None:
                end = start + (timedelta(days=1) if all_day else timedelta(hours=1))
        else:
            end = start + (timedelta(days=1) if all_day else timedelta(hours=1))
        title = _unescape((props.get("SUMMARY", ({}, "Untitled"))[1] or "Untitled")).strip() or "Untitled"
        uid = (props.get("UID", ({}, f"{source.id}:{title}:{start.isoformat()}"))[1] or "").strip()
        rrule_text = props.get("RRULE", ({}, ""))[1]
        exdates: list[datetime] = []
        for params, value in extras.get("EXDATE", []):
            for chunk in value.split(","):
                dt, _ = _parse_dt(chunk, params)
                if dt is not None:
                    exdates.append(dt)
        for occ_start, occ_end in _occurrences(start, end, rrule_text, exdates, window_start, window_end):
            events.append(
                CalendarEvent(
                    uid=f"{uid}:{occ_start.date().isoformat()}",
                    title=title,
                    start=occ_start,
                    end=occ_end,
                    all_day=all_day,
                    source_id=source.id,
                    source_name=source.name,
                    color=source.color,
                    location=_unescape((props.get("LOCATION", ({}, ""))[1] or "")).strip(),
                    description=_unescape((props.get("DESCRIPTION", ({}, ""))[1] or "")).strip(),
                    url=(props.get("URL", ({}, ""))[1] or "").strip(),
                )
            )
    events.sort(key=lambda ev: (ev.start, ev.title.lower()))
    return events
