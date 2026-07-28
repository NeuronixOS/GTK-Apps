# gtk-meetings

GTK4 calendar import for the Neuronix GTK-Apps suite.

Created by Kevin Hinds — [github.com/khinds10-Neuronix/GTK-Apps](https://github.com/khinds10-Neuronix/GTK-Apps)

Paste JSON events and add them to your Google Calendar (primary calendar) via OAuth.

## Prerequisites

```bash
sudo apt install python3-gi gir1.2-gtk-4.0 python3-venv
```

## Google Calendar credentials

This app uses the **Google Calendar API** with OAuth. You need:

| File | What it is |
|------|------------|
| `credentials.json` | OAuth client config (Desktop app) from Google Cloud Console |
| `token.json` | User access token (created when you click **Authenticate**) |

Place `credentials.json` at:

```
~/.config/gtk-apps/gtk-meetings/credentials.json
```

Both files are gitignored. Do not commit or share them.

### Setup OAuth (once)

1. Open [Google Cloud Console](https://console.cloud.google.com/) and create or select a project.
2. Enable **Google Calendar API**.
3. Configure the **OAuth consent screen** (External or Internal). Add yourself as a test user if the app is in Testing mode.
4. Create **Credentials → OAuth client ID → Desktop app**, then download the JSON.
5. Rename it to `credentials.json` and move it into `~/.config/gtk-apps/gtk-meetings/`.

Example shape (values will differ):

```json
{
  "installed": {
    "client_id": "123456789-xxxx.apps.googleusercontent.com",
    "project_id": "your-project-id",
    "auth_uri": "https://accounts.google.com/o/oauth2/auth",
    "token_uri": "https://oauth2.googleapis.com/token",
    "client_secret": "GOCSPX-...",
    "redirect_uris": ["http://localhost"]
  }
}
```

Use a **Desktop app** client (not Web).

## Run

```bash
./start.sh
```

Or:

```bash
.venv/bin/python app.py
```

`start.sh` creates a venv with `--system-site-packages` so system `python3-gi` works.

First run: click **Authenticate**, sign in with the Google account whose primary calendar should receive events, and approve access. `token.json` is saved for later runs.

## Usage

Paste JSON, then **Preview** or **Import**.

- **Plain array** — always targets today.
- **Day-keyed object** — uses today's key when present; otherwise imports the date(s) in the JSON.

### Format A: day-keyed

```json
{
  "Thu, July 9th, 2026": [
    ["9:00 AM - 10:00 AM: Team standup"],
    ["Dentist follow-up"]
  ]
}
```

### Format B: plain today array

```json
[
  ["9:00 AM - 10:00 AM: Team standup"],
  ["Dentist follow-up"]
]
```

### Event text rules

| Text | Meaning |
|------|---------|
| `9:00 AM - 10:30 AM: Title` | Timed event with start and end |
| `9:00 AM: Title` | Timed event, 1-hour default duration |
| `Title` | All-day event |

Times use `America/New_York` by default.

## Notes

- Imports go to the authenticated user's **primary** calendar.
- Re-importing the same JSON creates duplicate events.
