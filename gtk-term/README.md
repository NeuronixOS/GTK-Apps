# gtk-term

A GTK4 + VTE terminal emulator written in Rust, implementing the core
feature set of GNOME Terminal.

Created by Kevin Hinds — [github.com/NeuronixOS/GTK-Apps](https://github.com/NeuronixOS/GTK-Apps)

- Tabbed interface with full-width, reorderable, closable tabs
- Find/search bar with regex, match case, and whole-word options
- Right-click context menu (copy, paste, select all, find, reset, …)
- Clickable URL detection (Ctrl+click to open links)
- Tab navigation: Ctrl+PgUp/PgDn, Alt+1–9, Ctrl+Shift+PgUp/PgDn to move
- Detach tab to a new window
- Set custom tab title
- Reset / Reset and Clear terminal
- Terminal size presets (80×24, 80×43, 132×24, 132×43)
- Confirm-close dialog for windows/tabs with running processes
- Menu with zoom, fullscreen, read-only, profiles, and advanced options
- Colors, font, scrollback, and cursor blink from a simple TOML file
- GTK chrome (window / header bar / tabs) themeable with CSS

## Requirements

- A Rust toolchain (`rustc` / `cargo`) — install via [rustup](https://rustup.rs)
- GTK4 and VTE (GTK4 build) development libraries + `pkg-config`
- Runtime: `adwaita-icon-theme` (menu / toolbar / dialog icons)

### Install system dependencies

**Debian / Ubuntu (incl. GamebianOS / GamebianUbuntu):**

```bash
sudo apt install build-essential pkg-config libgtk-4-dev libvte-2.91-gtk4-dev adwaita-icon-theme
```

**Fedora:**

```bash
sudo dnf install gcc pkgconf-pkg-config gtk4-devel vte291-gtk4-devel adwaita-icon-theme
```

**Arch:**

```bash
sudo pacman -S base-devel gtk4 vte4 adwaita-icon-theme
```

> The crate names `gtk4` and `vte4` in `Cargo.toml` must come from the **same
> gtk-rs generation**. This project uses `gtk4 = "0.9"` with `vte4 = "0.8"`
> (vte4 0.8 tracks the gtk4 0.9 generation). If `cargo build` reports a
> `gtk4-sys` links conflict, align these two crates to a matching pair.

## Build

From this directory (`GTK4-Apps/gtk-term`):

```bash
cargo build --release
```

The binary is produced at `target/release/gtk-term`.

## Run

```bash
# Run directly with cargo (debug build):
cargo run

# ...or run the optimized binary:
./target/release/gtk-term
```

### Install system-wide (optional)

```bash
cargo build --release
sudo install -Dm755 target/release/gtk-term /usr/local/bin/gtk-term
```

Then launch it as `gtk-term` from anywhere.

## Keyboard shortcuts

| Shortcut                   | Action                         |
|----------------------------|--------------------------------|
| `Ctrl+Shift+T`             | New tab                        |
| `Ctrl+Shift+W`             | Close tab                      |
| `Ctrl+Shift+N`             | New window                     |
| `Ctrl+Shift+D`             | Detach tab to new window       |
| `Ctrl+Shift+I`             | Set tab title                  |
| `Ctrl+PgDn` / `Ctrl+PgUp` | Next / previous tab            |
| `Ctrl+Shift+PgDn/PgUp`    | Move tab right / left          |
| `Alt+1` … `Alt+9`         | Switch to tab 1–9              |
| `Ctrl+Shift+C`             | Copy                           |
| `Ctrl+Shift+V`             | Paste                          |
| `Ctrl+Shift+A`             | Select all                     |
| `Ctrl+Shift+F`             | Find in scrollback             |
| `Enter` / `Shift+Enter`   | Previous / next match (in find)|
| `Escape`                   | Close search bar               |
| `Ctrl++` / `Ctrl+=`        | Zoom in                        |
| `Ctrl+-`                   | Zoom out                       |
| `Ctrl+0`                   | Reset zoom                     |
| `F11`                      | Toggle full screen              |
| `Ctrl+click` on link       | Open URL in browser            |
| Right-click                | Context menu                   |

## Menu

The header-bar menu (☰) contains:

- **Zoom row** — `−` / percentage (click to reset) / `+`
- **New Window** — open another terminal window
- **Full Screen** — toggle fullscreen (also `F11`)
- **Read-Only** — disable keyboard input to the current terminal
- **Set Title…** — rename the current tab
- **Profile** — switch the current terminal to a prebuilt color scheme:
  Tokyo Night, Dracula, Nord, Gruvbox Dark, Solarized Dark, Solarized Light
- **Advanced** submenu:
  - **Reset** — reset terminal state
  - **Reset and Clear** — reset terminal and clear scrollback
  - **Size presets** — 80×24, 80×43, 132×24, 132×43
- **Help** — keyboard-shortcut reference
- **About** — version info

## Right-click context menu

Right-clicking anywhere in the terminal opens a context menu with:

- Copy / Paste / Select All
- Find…
- Reset / Reset and Clear
- New Tab / Detach Tab / Set Title… / Close Tab

## URL detection

URLs (http, https, ftp, file, email addresses, www.* / ftp.* hostnames)
are automatically detected and highlighted with a pointer cursor. Hold
`Ctrl` and click to open the URL in your default browser via `xdg-open`.

Selecting a profile recolors the active terminal immediately. To make a scheme
the permanent default for new terminals, copy its colors into `config.toml`
(see below).

## Theming

There are two independent layers:

1. **`config.toml`** — the terminal grid itself (font, ANSI colors, foreground/background, scrollback, cursor).
2. **`style.css`** — the surrounding GTK widgets (window, header bar, tab bar).

Both live in `~/.config/gtk-apps/gtk-term/`. Neither is required; without them the
built-in Tokyo Night theme is used.

### 1. Terminal colors & font (`config.toml`)

Copy the sample and edit it:

```bash
mkdir -p ~/.config/gtk-apps/gtk-term
cp examples/config.toml ~/.config/gtk-apps/gtk-term/config.toml
```

```toml
font = "JetBrains Mono 12"   # any Pango font string: "<family> <size>"
scrollback_lines = 10000
cursor_blink = true

[colors]
foreground = "#c0caf5"
background = "#1a1b26"

# 16 ANSI colors: 0-7 normal, 8-15 bright.
palette = [
  "#15161e", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#a9b1d6",
  "#414868", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#c0caf5",
]
```

Colors accept any GDK-parseable string: `#rgb`, `#rrggbb`, `rgb(...)`,
`rgba(...)`, or a named color. Provide fewer than 16 palette entries and VTE
fills the rest with its defaults. Invalid values fall back gracefully.

### 2. GTK chrome (`style.css`)

```bash
cp examples/style.css ~/.config/gtk-apps/gtk-term/style.css
```

Useful CSS nodes:

- `window` — the top-level window
- `headerbar` — the title bar
- `notebook > header tab` / `tab:checked` — tab styling
- `vte-terminal` — the terminal widget (e.g. `padding` around the grid)

Example (add breathing room around the grid and accent the active tab):

```css
vte-terminal { padding: 8px; }
notebook > header tab:checked { box-shadow: inset 0 -2px 0 0 #7aa2f7; }
```

Changes are applied on the next launch.

## Project layout

```
gtk-term/
├─ Cargo.toml
├─ README.md
├─ examples/
│  ├─ config.toml   # sample terminal theme
│  └─ style.css     # sample GTK chrome theme
└─ src/
   ├─ main.rs       # app, window, tabs, actions/shortcuts, context menu
   ├─ config.rs     # TOML config loading + color parsing + profiles
   ├─ terminal.rs   # build/theme a terminal, URL matching, shell spawn
   └─ search.rs     # find/search bar widget
```

## Troubleshooting

- **`Package gtk4 was not found` / `vte-2.91-gtk4` errors at build time:**
  install the `-dev` packages listed under Requirements.
- **`gtk4-sys` links conflict / version selection error:** align the `gtk4` and
  `vte4` versions in `Cargo.toml` to the same gtk-rs generation (this project
  uses `gtk4 = "0.9"` + `vte4 = "0.8"`).
- **Blank window or no shell:** ensure `$SHELL` is set; gtk-term falls back to
  `/bin/bash` if it is not.

## License

MIT
