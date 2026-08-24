# gtk-video

A GTK4 video trimmer: load a clip, drag in/out handles on the timeline, crop /
rotate / flip, and export the selected section as a new file.

Created by Kevin Hinds — [github.com/NeuronixOS/GTK-Apps](https://github.com/NeuronixOS/GTK-Apps)

- Open a file (dialog, CLI args, drag-and-drop)
- Preview with play / pause, seek, and loop-within-selection
- Drag the **in** and **out** sliders on the timeline (or `[` / `]` to mark the playhead)
- Rotate 90° (Ctrl+R / Ctrl+Shift+R), flip horizontal / vertical, crop with an overlay and aspect presets
- Export the range with ffmpeg — accurate re-encode (default, required when transforming) or fast stream copy
- Header bar, status bar, gear menu, suite Profile colors
- **Self Driving** (ꔮ / Ctrl+D) via gtk-neuron — ask the driver to open, trim, crop, rotate, or export

## Requirements

- A Rust toolchain (`rustc` / `cargo`) — install via [rustup](https://rustup.rs)
- GTK4 development libraries + `pkg-config`
- Runtime: `adwaita-icon-theme`, GStreamer GTK playback plugins, `ffmpeg`

### Install system dependencies

**Debian / Ubuntu / Neuronix:**

```bash
sudo apt install build-essential pkg-config libgtk-4-dev adwaita-icon-theme \
  gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav \
  gstreamer1.0-gtk4 ffmpeg
```

**Fedora:**

```bash
sudo dnf install gcc pkgconf-pkg-config gtk4-devel adwaita-icon-theme \
  gstreamer1-plugins-good gstreamer1-plugins-bad-free gstreamer1-libav ffmpeg
```

**Arch:**

```bash
sudo pacman -S base-devel gtk4 adwaita-icon-theme gst-plugins-good \
  gst-plugins-bad gst-libav gst-plugin-gtk4 ffmpeg
```

## Build

From this directory (`GTK-Apps/gtk-video`):

```bash
cargo build --release
```

The binary is produced at `target/release/gtk-video`.

## Run

```bash
# Debug build:
cargo run

# Optimized binary:
./target/release/gtk-video

# Open a file:
./target/release/gtk-video ~/Videos/clip.mp4

# Suite launcher:
../build-launch.sh gtk-video
```

### Install system-wide (optional)

```bash
cargo build --release
sudo install -Dm755 target/release/gtk-video /usr/local/bin/gtk-video
```

## Keyboard shortcuts

| Shortcut              | Action                         |
|-----------------------|--------------------------------|
| `Ctrl+O`              | Open video                     |
| `Ctrl+E` / `Ctrl+S`   | Export selection               |
| `Space`               | Play / Pause                   |
| `Left` / `Right`      | Seek 1 second                  |
| `Shift+Left` / `Right`| Seek 0.2 seconds               |
| `Home` / `End`        | Go to in / out point           |
| `[` / `]`             | Set in / out to playhead       |
| `Ctrl+R`              | Rotate clockwise               |
| `Ctrl+Shift+R`        | Rotate counterclockwise        |
| `Ctrl+Shift+C`        | Toggle crop overlay            |
| `Ctrl+D`              | Self Driving                   |
| `Ctrl+?`              | Keyboard shortcuts             |
| `Ctrl+Q`              | Quit                           |

Drag the left handle for the in point, the right handle for the out point, and
the playhead (triangle) to scrub. Playback stays inside the selected range.

## Configuration

Optional settings live in `~/.config/gtk-apps/gtk-video/`:

```bash
mkdir -p ~/.config/gtk-apps/gtk-video
cp examples/config.toml ~/.config/gtk-apps/gtk-video/config.toml
```

```toml
accurate_export = true
loop_selection = false
window_width = 960
window_height = 640
```

Optional GTK chrome CSS: `~/.config/gtk-apps/gtk-video/style.css`.

## Project layout

```
gtk-video/
├─ Cargo.toml
├─ README.md
├─ examples/
│  └─ config.toml
└─ src/
   ├─ main.rs      # app, CSS, about, shortcuts
   ├─ config.rs    # TOML config
   ├─ window.rs    # header bar, preview, actions, drop target
   ├─ timeline.rs  # draggable in/out handles + playhead
   ├─ crop.rs      # crop overlay + aspect presets
   ├─ neuron.rs    # Self Driving capability API
   └─ export.rs    # ffmpeg cut / crop / rotate / flip of the selected range
```

## License

GPL-3.0-or-later
