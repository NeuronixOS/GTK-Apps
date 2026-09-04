# gtk-ytmusic

Dedicated [YouTube Music](https://music.youtube.com/) player for the GTK-Apps suite.

Created by Kevin Hinds — [github.com/NeuronixOS/GTK-Apps](https://github.com/NeuronixOS/GTK-Apps)

## Why not just open a browser tab?

Chrome/Firefox often **skip or gap** on YT Music when:

- Extensions (AdBlock, etc.) interfere with media requests
- Background-tab / occluded-window timer throttling kicks in
- The tab shares a profile with dozens of other sites

This app opens music.youtube.com in a **Chromium/Chrome `--app` window** with:

- An isolated profile under `~/.config/gtk-ytmusic/chromium-profile` (no extensions)
- Flags that disable background media throttling
- Autoplay allowed without an extra gesture

## Run

```bash
./start.sh
# or
python3 app.py
```

Optional:

```bash
./start.sh --webkit          # embed in WebKitGTK (GTK3; experimental)
./start.sh 'https://music.youtube.com/playlist?list=…'
GTK_YTMUSIC_BROWSER=chromium ./start.sh
```

## Requirements

- `chromium` or `google-chrome` (default mode)
- For `--webkit`: `gir1.2-webkit2-4.1` + GTK3 (already on Debian 13)

## Note on gaps

YouTube Music itself does not offer true gapless/crossfade like Spotify. This app
targets **browser-caused** skips (throttling / extensions), not server-side
silence between tracks.
