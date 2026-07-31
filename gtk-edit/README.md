# gtk-edit

A GTK4 + GtkSourceView 5 text editor written in Rust — a feature port of classic **gedit 3.5.1** (see `../Source-Apps/gedit`).

Created by Kevin Hinds — [github.com/NeuronixOS/GTK-Apps](https://github.com/NeuronixOS/GTK-Apps)

- Tabbed MDI editing with tab groups and multi-window
- Syntax highlighting, encodings, backups, autosave
- Incremental find, replace, go-to-line
- Side/bottom panels, documents list, statusbar language & tab-width
- Print / print preview
- Preferences (TOML under `~/.config/gtk-apps/gtk-edit/`)
- Plugin engine (libpeas-style activatable traits) with bundled plugins:
  changecase, docinfo, filebrowser, modelines, sort, spell, time,
  externaltools, pythonconsole, quickopen, snippets, terminal

## Requirements

- Rust toolchain (`rustc` / `cargo`) — [rustup](https://rustup.rs)
- GTK4 and GtkSourceView 5 development libraries + `pkg-config`
- Runtime: `adwaita-icon-theme` (menu / toolbar / dialog icons)
- Optional: `aspell` (spell plugin), `python3` (Python console plugin), `libvte-2.91-gtk4`

### Install system dependencies

**Debian / Ubuntu:**

```bash
sudo apt install build-essential pkg-config libgtk-4-dev libgtksourceview-5-dev \
  libvte-2.91-gtk4-dev libenchant-2-dev aspell aspell-en adwaita-icon-theme
```

**Fedora:**

```bash
sudo dnf install gcc pkgconf-pkg-config gtk4-devel gtksourceview5-devel \
  vte291-gtk4-devel enchant2-devel aspell aspell-en adwaita-icon-theme
```

**Arch:**

```bash
sudo pacman -S base-devel gtk4 gtksourceview5 vte4 enchant aspell adwaita-icon-theme
```

> `gtk4` and `sourceview5` crate versions must match the same gtk-rs generation.
> This project uses `gtk4 = "0.9"` with `sourceview5 = "0.9"`.

## Build

```bash
cd GTK4-Apps/gtk-edit
cargo build --release
```

Binary: `target/release/gtk-edit`

## Run

```bash
cargo run
# or
./target/release/gtk-edit
./target/release/gtk-edit file.txt
./target/release/gtk-edit --encoding UTF-8 +42 file.txt
```

### Install system-wide (optional)

```bash
cargo build --release
sudo install -Dm755 target/release/gtk-edit /usr/local/bin/gtk-edit
```

## Configuration

Config file: `~/.config/gtk-apps/gtk-edit/config.toml`  
See [examples/config.toml](examples/config.toml) for defaults (mirrors classic gedit GSettings keys).

## Plugins

Built-in plugins are toggled under **Edit → Preferences → Plugins**.

Defaults: `docinfo`, `modelines`, `filebrowser`, `spell`, `time`, `terminal`.

External plugins can be dropped in:

```text
~/.local/share/gtk-edit/plugins/<name>/<name>.plugin
~/.local/share/gtk-edit/plugins/<name>/lib<name>.so   # optional cdylib
```

Example `.plugin` metadata:

```ini
[Plugin]
Module=myplugin
Name=My Plugin
Description=Example external plugin
Library=libmyplugin.so
```

Third-party plugins use the same activatable lifecycle as builtins
(`AppActivatable` / `WindowActivatable` / `ViewActivatable`). They are **not**
binary-compatible with upstream gedit/libpeas Python or C plugins.

## Keyboard shortcuts

| Shortcut | Action |
|----------|--------|
| Ctrl+N | New |
| Ctrl+O | Open |
| Ctrl+S | Save |
| Ctrl+Shift+S | Save As |
| Ctrl+W | Close tab |
| Ctrl+Q | Quit |
| Ctrl+Z / Ctrl+Shift+Z | Undo / Redo |
| Ctrl+F | Find |
| Ctrl+G / Ctrl+Shift+G | Find next / previous |
| Ctrl+H | Replace |
| Ctrl+I | Go to line |
| Ctrl+U / Ctrl+L / Ctrl+T | Upper / lower / title case (selection) |
| Ctrl+P | Print |
| F11 | Fullscreen |
| Ctrl+PageUp/Down | Previous/next document |
| Ctrl+Alt+O | Quick Open (plugin) |
| Ctrl+B | Expand snippet (plugin) |

## Source mapping

Feature checklist and behavior are taken from `GTK4-Apps/Source-Apps/gedit`
(classic gedit 3.5.1). UI and APIs are remapped to GTK4 / GtkSourceView 5 /
`gio::SimpleAction`, following the same Cargo-only style as `gtk-term`.

Skipped (not Linux product features in that tree): Windows/macOS tooling,
`checkupdate`, Zeitgeist, Tag list (not shipped upstream here).
