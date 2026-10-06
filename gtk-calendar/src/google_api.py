"""Google Calendar OAuth and event fetch."""

from __future__ import annotations

import json
from datetime import datetime
from pathlib import Path
from typing import Optional

from .models import CalendarEvent, CalendarSource, as_aware
from .paths import google_token_file
from .store import DEFAULT_COLORS, Store, _chmod_private

SCOPES = ("https://www.googleapis.com/auth/calendar.readonly",)


class GoogleUnavailable(RuntimeError):
    pass


def google_libs_available() -> bool:
    try:
        import google.auth  # noqa: F401
        import google_auth_oauthlib.flow  # noqa: F401
        import googleapiclient.discovery  # noqa: F401
        return True
    except ImportError:
        return False


def _require_google():
    if not google_libs_available():
        raise GoogleUnavailable(
            "Google Calendar libraries are missing. Install "
            "python3-googleapi python3-google-auth python3-google-auth-oauthlib "
            "python3-google-auth-httplib2."
        )


def client_config(store: Store) -> dict:
    client_id = store.google_client_id.strip()
    client_secret = store.google_client_secret.strip()
    if client_id.startswith("{"):
        try:
            parsed = json.loads(client_id)
            if isinstance(parsed, dict) and ("installed" in parsed or "web" in parsed):
                return parsed
        except json.JSONDecodeError:
            pass
    if not client_id or not client_secret:
        raise ValueError(
            "Add a Google Cloud OAuth Desktop client ID and secret under Calendars."
        )
    return {
        "installed": {
            "client_id": client_id,
            "client_secret": client_secret,
            "auth_uri": "https://accounts.google.com/o/oauth2/auth",
            "token_uri": "https://oauth2.googleapis.com/token",
            "redirect_uris": ["http://localhost"],
        }
    }


def load_credentials():
    _require_google()
    from google.oauth2.credentials import Credentials

    path = google_token_file()
    if not path.is_file():
        return None
    try:
        creds = Credentials.from_authorized_user_file(str(path), list(SCOPES))
    except Exception:
        return None
    return creds


def save_credentials(creds) -> None:
    path = google_token_file()
    path.write_text(creds.to_json(), encoding="utf-8")
    _chmod_private(path)


def clear_credentials() -> None:
    path = google_token_file()
    try:
        path.unlink(missing_ok=True)
    except TypeError:
        if path.exists():
            path.unlink()
    except OSError:
        pass


def signed_in() -> bool:
    creds = load_credentials()
    return bool(creds and (creds.valid or creds.refresh_token))


def credentials_ready(store: Store | None = None):
    """Return valid credentials without opening a browser, or None."""
    _require_google()
    from google.auth.transport.requests import Request

    creds = load_credentials()
    if creds and creds.valid:
        return creds
    if creds and creds.expired and creds.refresh_token:
        creds.refresh(Request())
        save_credentials(creds)
        return creds
    return None


def ensure_credentials(store: Store):
    """Interactive sign-in if needed (opens a local browser)."""
    creds = credentials_ready(store)
    if creds is not None:
        return creds
    from google_auth_oauthlib.flow import InstalledAppFlow

    flow = InstalledAppFlow.from_client_config(client_config(store), list(SCOPES))
    creds = flow.run_local_server(port=0, open_browser=True, prompt="consent")
    save_credentials(creds)
    return creds


def _service(store: Store, interactive: bool = False):
    from googleapiclient.discovery import build

    creds = ensure_credentials(store) if interactive else credentials_ready(store)
    if creds is None:
        raise GoogleUnavailable("Not signed in to Google.")
    return build("calendar", "v3", credentials=creds, cache_discovery=False)


def account_email(store: Store) -> str:
    service = _service(store, interactive=True)
    setting = service.settings().get(setting="timezone").execute()
    # Calendar API does not always expose email; try calendarList primary.
    calendars = service.calendarList().list(minAccessRole="reader").execute()
    for item in calendars.get("items") or []:
        if item.get("primary"):
            return str(item.get("id") or "")
    return str(setting.get("value") or "")


def list_google_calendars(store: Store) -> list[CalendarSource]:
    service = _service(store, interactive=True)
    result = service.calendarList().list(minAccessRole="reader").execute()
    sources: list[CalendarSource] = []
    for idx, item in enumerate(result.get("items") or []):
        cal_id = str(item.get("id") or "").strip()
        if not cal_id:
            continue
        name = str(item.get("summaryOverride") or item.get("summary") or cal_id)
        color = str(item.get("backgroundColor") or DEFAULT_COLORS[idx % len(DEFAULT_COLORS)])
        sources.append(
            CalendarSource(
                id=f"google:{cal_id}",
                kind="google",
                name=name,
                color=color,
                enabled=True,
                google_id=cal_id,
            )
        )
    return sources


def fetch_google_events(
    store: Store,
    source: CalendarSource,
    window_start: datetime,
    window_end: datetime,
) -> list[CalendarEvent]:
    if not source.google_id:
        return []
    service = _service(store, interactive=False)
    events: list[CalendarEvent] = []
    page_token: Optional[str] = None
    while True:
        result = (
            service.events()
            .list(
                calendarId=source.google_id,
                timeMin=window_start.isoformat(),
                timeMax=window_end.isoformat(),
                singleEvents=True,
                orderBy="startTime",
                pageToken=page_token,
                maxResults=2500,
            )
            .execute()
        )
        for item in result.get("items") or []:
            parsed = _event_from_google(item, source)
            if parsed is not None:
                events.append(parsed)
        page_token = result.get("nextPageToken")
        if not page_token:
            break
    return events


def _event_from_google(item: dict, source: CalendarSource) -> Optional[CalendarEvent]:
    if str(item.get("status") or "").lower() == "cancelled":
        return None
    start_info = item.get("start") or {}
    end_info = item.get("end") or {}
    all_day = "date" in start_info and "dateTime" not in start_info
    start = _google_dt(start_info)
    end = _google_dt(end_info)
    if start is None or end is None:
        return None
    title = str(item.get("summary") or "Untitled").strip() or "Untitled"
    return CalendarEvent(
        uid=str(item.get("id") or f"{source.id}:{start.isoformat()}"),
        title=title,
        start=start,
        end=end,
        all_day=all_day,
        source_id=source.id,
        source_name=source.name,
        color=source.color,
        location=str(item.get("location") or "").strip(),
        description=str(item.get("description") or "").strip(),
        url=str(item.get("htmlLink") or "").strip(),
    )


def _google_dt(info: dict) -> Optional[datetime]:
    if not isinstance(info, dict):
        return None
    if info.get("dateTime"):
        raw = str(info["dateTime"]).replace("Z", "+00:00")
        try:
            return as_aware(datetime.fromisoformat(raw))
        except ValueError:
            return None
    if info.get("date"):
        try:
            day = datetime.fromisoformat(str(info["date"])).date()
        except ValueError:
            return None
        from .models import LOCAL_TZ

        return datetime(day.year, day.month, day.day, tzinfo=LOCAL_TZ)
    return None


def load_client_secrets_file(path: Path, store: Store) -> None:
    raw = json.loads(path.read_text(encoding="utf-8"))
    blob = raw.get("installed") or raw.get("web") or raw
    store.google_client_id = str(blob.get("client_id") or "").strip()
    store.google_client_secret = str(blob.get("client_secret") or "").strip()
    if not store.google_client_id:
        raise ValueError("That JSON file has no client_id.")
    store.save()
