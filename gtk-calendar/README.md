# gtk-calendar

GTK4 month calendar for the Neuronix GTK-Apps suite. Sign in to Google Calendar and/or load `.ics` files and `webcal://` / `https://` feeds.

Created by Kevin Hinds — [github.com/NeuronixOS/GTK-Apps](https://github.com/NeuronixOS/GTK-Apps)

## Features

- Month view with event dots and a day agenda
- **Sign in with Google** (Calendar API, read-only) — choose which calendars to show
- Add **.ics** files or subscription URLs (Google “secret iCal address”, Outlook, Fastmail, etc.)
- Shared suite **Profile** theme menu (hamburger) via `gtk-theme`

## Run

```bash
./start.sh
# or
../build-launch.sh gtk-calendar
```

Config lives under `~/.config/gtk-apps/gtk-calendar/` (`config.json`, `google-token.json`). Token and client secret files are mode `0600`.

## Google sign-in

1. In [Google Cloud Console](https://console.cloud.google.com/) create a project, enable **Google Calendar API**, and create an **OAuth client ID** of type **Desktop app**.
2. In Calendar → **Calendars**, paste the client ID and secret (or load the downloaded JSON).
3. Click **Sign in with Google**. The browser completes OAuth on localhost; calendars appear as toggles.

Without Google libraries installed, `.ics` sources still work.

## Dependencies

```bash
# Debian / Ubuntu / Neuronix
sudo apt-get install python3-gi gir1.2-gtk-4.0 \
  python3-googleapi python3-google-auth python3-google-auth-oauthlib \
  python3-google-auth-httplib2 python3-icalendar python3-dateutil
```

Or `./install.sh`.
