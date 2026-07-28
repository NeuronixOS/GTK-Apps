# GTK4 Apps (Neuronix)

Suite of GTK4 desktop apps for Neuronix. Apps share color profiles and UI chrome via [`gtk-theme`](gtk-theme/) (Rust crate + Python helpers). User settings live under `~/.config/gtk-apps/`.

## Apps

| App | Stack | Role |
|-----|-------|------|
| `gtk-calc` | Rust | Calculator (GNOME Calculator–style: basic / advanced / programming modes, history, CLI `--solve`) |
| `gtk-edit` | Rust | Text editor (gedit-style: tabs, GtkSourceView, find/replace, plugins) |
| `gtk-files` | Rust | File manager (Nautilus-style: places, tabs, list/grid, trash, embedded terminal) |
| `gtk-image` | Rust | Image viewer (Eye of GNOME–style: browse folder, zoom/rotate, fullscreen) |
| `gtk-term` | Rust | Terminal emulator (GNOME Terminal–style: VTE tabs, search, profiles, URLs) |
| `gtk-theme-editor` | Rust | Edit suite color profiles (fg/bg + 16-color palette); save custom profiles and apply suite-wide |
| `gtk-colors` | Python | Color picker and format converter (RGB/Hex and many color spaces, palette harmonies) |
| `gtk-configs` | Python | Neuronix config tree editor (`~/configs` or `--root`): Hyprland, Waybar, Fuzzel, Mako, colors, raw files |
| `gtk-meld` | Python | Visual diff / merge tool (Meld-based, GTK4) |
| `gtk-photos` | Python | Photo organizer (drive browse, favorites, folders, website thumbnails) |
| `gtk-meetings` | Python | Paste JSON events and import them into Google Calendar (OAuth) |
| `gtk-workspaces` | Python | Named workspace launcher (run command sets) |
| `gtk-worktimezone` | Python | Coworker timezone tracker (edit/view local times across zones) |

Shared library (not launched as an app):

| Path | Role |
|------|------|
| `gtk-theme` | Color profiles, Profile menu, Adwaita icon helpers for Rust and Python suite apps |

## Runtime icons

Menus and labeled buttons resolve **symbolic icon names** through GTK’s `IconTheme`. Artwork comes from the system package:

- **Debian / Ubuntu / Neuronix:** `adwaita-icon-theme`
- **Fedora:** `adwaita-icon-theme`
- **Arch:** `adwaita-icon-theme`

Install example:

```bash
sudo apt install adwaita-icon-theme
```

Apps do not vendor Adwaita SVGs. At startup, `gtk-theme` adds Adwaita’s icon directories to GTK’s search path so symbolic chrome icons still resolve when the session theme is something else (e.g. Faenza). Window/taskbar icons use Freedesktop / GNOME names (`accessories-calculator`, `org.gnome.eog`, …).

Shared helpers live in `gtk-theme` (`IconMenu`, `labeled_button`, `icon_for_action`). GTK4 does not show icons on ordinary `Gio.Menu` rows, so `IconMenu` binds custom `Image` + `Label` children into `PopoverMenu` / `PopoverMenuBar`.

## Build / launch

```bash
./build-launch.sh                 # all apps listed in the script
./build-launch.sh gtk-files       # one app
```

Rust apps build with `cargo`; Python apps skip compile and launch via `launch.sh` / `start.sh` / `run.sh` or their entrypoint. `gtk-meld` builds separately with `./gtk-meld/build.sh` (meson).

See each app’s `README.md` for build dependencies.

## Config

User settings live under `~/.config/gtk-apps/` (created on first run with defaults). Do not commit personal JSON/TOML into this tree — use `*.dist.json` / `examples/config.toml` as templates.
