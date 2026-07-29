# gtk-image

A GTK4 image viewer written in Rust, implementing the core feature set of
[Eye of GNOME](../Source-Apps/eog) (eog).

Created by Kevin Hinds — [github.com/NeuronixOS/GTK-Apps](https://github.com/NeuronixOS/GTK-Apps)

- Open a file or folder (dialog, CLI args, drag-and-drop)
- Browse sibling images in the directory (prev / next / first / last)
- Best-fit and free zoom (in / out / 100%), scroll-wheel zoom, drag to pan
- Rotate clockwise / counterclockwise, flip horizontal / vertical
- Fullscreen, copy to clipboard, move to trash, Save As
- Header bar, status bar (position + zoom), gear menu, context menu
- Optional TOML config and CSS theming

## Requirements

- A Rust toolchain (`rustc` / `cargo`) — install via [rustup](https://rustup.rs)
- GTK4 development libraries + `pkg-config`
- Runtime: `adwaita-icon-theme` (menu / toolbar / dialog icons)

### Install system dependencies

**Debian / Ubuntu:**

```bash
sudo apt install build-essential pkg-config libgtk-4-dev adwaita-icon-theme
```

**Fedora:**

```bash
sudo dnf install gcc pkgconf-pkg-config gtk4-devel adwaita-icon-theme
```

**Arch:**

```bash
sudo pacman -S base-devel gtk4 adwaita-icon-theme
```

## Build

From this directory (`GTK4-Apps/gtk-image`):

```bash
cargo build --release
```

The binary is produced at `target/release/gtk-image`.

## Run

```bash
# Debug build:
cargo run

# Optimized binary:
./target/release/gtk-image

# Open a file or folder:
./target/release/gtk-image ~/Pictures/photo.jpg
./target/release/gtk-image ~/Pictures/
```

### Install system-wide (optional)

```bash
cargo build --release
sudo install -Dm755 target/release/gtk-image /usr/local/bin/gtk-image
```

## Keyboard shortcuts

| Shortcut              | Action                         |
|-----------------------|--------------------------------|
| `Ctrl+O`              | Open image                     |
| `Ctrl+Shift+O`        | Open folder                    |
| `Ctrl+Shift+S`        | Save As                        |
| `Ctrl+C`              | Copy image                     |
| `Delete`              | Move to Trash                  |
| `Left` / `Backspace`  | Previous image                 |
| `Right` / `Space`     | Next image                     |
| `Home` / `End`        | First / last image             |
| `+` / `=`             | Zoom in                        |
| `-`                   | Zoom out                       |
| `1` / `Ctrl+0`        | Normal size (100%)             |
| `F`                   | Best fit                       |
| Scroll wheel          | Zoom in / out                  |
| `Ctrl+R`              | Rotate clockwise               |
| `Ctrl+Shift+R`        | Rotate counterclockwise        |
| `F11`                 | Toggle full screen             |
| `Ctrl+?`              | Keyboard shortcuts             |
| `Ctrl+Q`              | Quit                           |

## Menu

The header-bar menu (☰) contains Open / Open Folder / Save As, Copy / Trash /
Show in Folder, rotate & flip, zoom & fullscreen, shortcuts, and About.

Right-click the image for Copy, Save As, Trash, Show in Folder, and rotate.

## Configuration

Optional settings live in `~/.config/gtk-apps/gtk-image/`:

```bash
mkdir -p ~/.config/gtk-apps/gtk-image
cp examples/config.toml ~/.config/gtk-apps/gtk-image/config.toml
```

```toml
best_fit = true
zoom_step = 1.25
zoom_min = 0.05
zoom_max = 20.0
window_width = 960
window_height = 640
```

Optional GTK chrome CSS: `~/.config/gtk-apps/gtk-image/style.css`.

## Project layout

```
gtk-image/
├─ Cargo.toml
├─ README.md
├─ examples/
│  └─ config.toml
└─ src/
   ├─ main.rs        # app, CSS, about, shortcuts, open handling
   ├─ config.rs      # TOML config
   ├─ window.rs      # header bar, actions, dialogs, drop target
   ├─ image_list.rs  # directory collection / next-prev (eog list store)
   └─ image_view.rs  # zoom / pan / rotate display (eog scroll view)
```

## Not yet ported (from eog)

Slideshow, thumbnail gallery strip, EXIF/properties sidebar, print, plugins,
wallpaper, preferences dialog, and autorotate from EXIF.

## License

GPL-3.0-or-later
