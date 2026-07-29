# gtk-configs

GTK4 Python editor for Neuronix config trees (`~/configs` or any `--root`).

Created by Kevin Hinds — [github.com/NeuronixOS/GTK-Apps](https://github.com/NeuronixOS/GTK-Apps)

## Local run

```bash
cd gtk-configs
python3 configs.py --root /path/to/Neuronix/Build/default/configs
```

Default without `--root` edits `~/configs` (merged live tree).

## Features

- Sidebar sections: Appearance, Hyprland, Waybar, Fuzzel, Mako, GTK/cursors, Terminal, MIME, Files
- **Colors** page: every color token with format-aware pickers (Hypr `rgb/rgba`, Fuzzel `RRGGBBAA`, `#hex`, CSS)
- Typed widgets for gaps, fonts, theme names, etc.
- **Raw** editor for full files (binds, waybar JSON, …) — preserves comments via targeted key rewrite for structured fields
- Save / Apply (`hyprctl reload`, waybar/mako SIGUSR2 when available)

## Dependencies

- `python3-gi`, `gir1.2-gtk-4.0`
- Optional: `gir1.2-gtksource-5`
