"""Google Calendar OAuth and event insertion."""

from __future__ import annotations

from pathlib import Path

from google.auth.transport.requests import Request
from google.oauth2.credentials import Credentials
from google_auth_oauthlib.flow import InstalledAppFlow
from googleapiclient.discovery import build

from .parse_event_text import ParsedEvent

SCOPES = ["https://www.googleapis.com/auth/calendar.events"]
TIMEZONE = "America/New_York"
CALENDAR_ID = "primary"
APP_NAME = "gtk-meetings"


def _app_dir() -> Path:
    return Path(__file__).resolve().parent.parent


def config_dir() -> Path:
    """``~/.config/gtk-apps/gtk-meetings`` (created if missing)."""
    import os

    base = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    path = base / "gtk-apps" / APP_NAME
    path.mkdir(parents=True, exist_ok=True)
    return path


def _migrate_secret(name: str) -> Path:
    dest = config_dir() / name
    if dest.exists():
        return dest
    for legacy in (
        _app_dir() / name,
        Path.home() / ".config" / APP_NAME / name,
    ):
        if legacy.is_file():
            try:
                dest.write_text(legacy.read_text(encoding="utf-8"), encoding="utf-8")
                break
            except OSError:
                continue
    return dest


def credentials_path() -> Path:
    return _migrate_secret("credentials.json")


def token_path() -> Path:
    return _migrate_secret("token.json")


def has_credentials_file() -> bool:
    return credentials_path().is_file()


def is_authenticated() -> bool:
    creds = _load_credentials(allow_oauth=False)
    return creds is not None and creds.valid


def _validate_credentials_file(creds_file: Path) -> None:
    import json

    try:
        data = json.loads(creds_file.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise ValueError(f"{creds_file.name} is not valid JSON: {exc}") from exc

    section = data.get("installed") or data.get("web")
    if section is None:
        raise ValueError(
            f"{creds_file.name} must contain an 'installed' (Desktop) section. "
            "Re-download as a Desktop OAuth client, not Web."
        )

    secret = section.get("client_secret", "")
    if len(secret) < 20:
        raise ValueError(
            f"{creds_file.name} has an invalid or truncated client_secret "
            f"(length {len(secret)}). Re-download the full JSON from Google Cloud Console: "
            "APIs & Services → Credentials → your Desktop client → Download JSON."
        )


def _load_credentials(*, allow_oauth: bool) -> Credentials | None:
    token_file = token_path()
    creds: Credentials | None = None

    if token_file.is_file():
        creds = Credentials.from_authorized_user_file(str(token_file), SCOPES)

    if creds and creds.expired and creds.refresh_token:
        creds.refresh(Request())
        token_file.write_text(creds.to_json(), encoding="utf-8")
        return creds

    if creds and creds.valid:
        return creds

    if not allow_oauth:
        return None

    creds_file = credentials_path()
    if not creds_file.is_file():
        raise FileNotFoundError(
            f"Missing {creds_file.name}. Place your Desktop OAuth credentials there."
        )

    _validate_credentials_file(creds_file)

    flow = InstalledAppFlow.from_client_secrets_file(str(creds_file), SCOPES)
    creds = flow.run_local_server(port=0)
    token_file.write_text(creds.to_json(), encoding="utf-8")
    return creds


def authenticate() -> None:
    _load_credentials(allow_oauth=True)


def get_calendar_service():
    creds = _load_credentials(allow_oauth=False)
    if creds is None or not creds.valid:
        raise RuntimeError("Not authenticated. Click Authenticate first.")
    return build("calendar", "v3", credentials=creds, cache_discovery=False)


def _event_body(event: ParsedEvent) -> dict:
    if event.all_day:
        return {
            "summary": event.summary,
            "start": {"date": event.start.date().isoformat()},
            "end": {"date": event.end.date().isoformat()},
        }

    return {
        "summary": event.summary,
        "start": {
            "dateTime": event.start.isoformat(),
            "timeZone": TIMEZONE,
        },
        "end": {
            "dateTime": event.end.isoformat(),
            "timeZone": TIMEZONE,
        },
    }


def insert_event(service, event: ParsedEvent) -> str:
    created = (
        service.events()
        .insert(calendarId=CALENDAR_ID, body=_event_body(event))
        .execute()
    )
    return created.get("htmlLink", created.get("id", "created"))


def insert_events(events: list[ParsedEvent]) -> list[tuple[ParsedEvent, str | Exception]]:
    service = get_calendar_service()
    results: list[tuple[ParsedEvent, str | Exception]] = []
    for event in events:
        try:
            link = insert_event(service, event)
            results.append((event, link))
        except Exception as exc:
            results.append((event, exc))
    return results
