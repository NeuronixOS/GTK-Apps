# gtk-files

A GTK4 file manager written in Rust — a feature port of **GNOME Files (Nautilus)**
(see `../Source-Apps/nautilus`).

Created by Kevin Hinds — [github.com/NeuronixOS/GTK-Apps](https://github.com/NeuronixOS/GTK-Apps)

- Places sidebar (Home, XDG dirs, Computer, Trash, USB Devices, **Network** mounts, bookmarks / favorites / recent in `places.toml`)
- **Connect to Server** (SFTP / FTP / SMB / WebDAV via GVFS) — sidebar **Connect to Network…** lists remembered remotes; mounts add a `~/Network/<name>` shortcut
- Tabbed browsing with closable, reorderable tabs
- Breadcrumb path bar and editable location entry (`Ctrl+L`)
- Back / forward / up / home navigation history
- List view (name, size, type, modified) and icon/grid view
- Folder search filter (`Ctrl+F`) and Find in Files content search (`Ctrl+Shift+F`, regex supported)
- Sort by name, size, type, or modified date; folders-first
- Show/hide hidden files (`Ctrl+H`)
- Cut / copy / paste, rename, new folder / document
- Move to Trash / permanent delete with confirmation
- Empty Trash, file properties dialog
- Open with default application (`GtkFileLauncher`)
- Open With… dialog (MIME type, current default, set new default)
- Bottom terminal / Find in Files panel that follows the focused folder
- Drag and drop files (out to other apps, into folders / current view)
- Context menus, preferences (TOML), keyboard shortcuts
- Multi-window support

## Requirements

- Rust toolchain (`rustc` / `cargo`) — [rustup](https://rustup.rs)
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
sudo pacman -S base-devel gtk4
```

## Build

```bash
cd GTK4-Apps/gtk-files
cargo build --release
```

Binary: `target/release/gtk-files`

## Run

```bash
cargo run
# or
./target/release/gtk-files
./target/release/gtk-files ~/Documents
```

### Install system-wide (optional)

```bash
cargo build --release
sudo install -Dm755 target/release/gtk-files /usr/local/bin/gtk-files
```

## Configuration

Settings live under XDG config (same pattern as the other gtk-* apps):

| File | Purpose |
|------|---------|
| `~/.config/gtk-apps/gtk-files/config.toml` | App preferences (window, view, behavior) |
| `~/.config/gtk-apps/gtk-files/places.toml` | Favorites / recent folders |
| `~/.config/gtk-apps/gtk-files/style.css` | Optional CSS overrides |
| `~/.config/gtk-apps/theme.toml` | Shared theme profile (all gtk-* apps) |

On first launch, an older `config.toml` from the process working directory or `~/.config/rusty-files/` is copied into `~/.config/gtk-apps/gtk-files/` if present. See [`examples/config.toml`](examples/config.toml) for the schema.

Thumbnail sizes in grid view: `small` (48px), `medium` (64px), `large` (96px), `larger` (128px), `largest` (192px) — also under **View → Thumbnail Size**.

## Keyboard shortcuts

| Shortcut | Action |
|----------|--------|
| `Alt+←` / `Alt+→` | Back / Forward |
| `Alt+↑` | Parent folder |
| `Alt+Home` | Home |
| `Ctrl+L` | Enter location |
| `Ctrl+Alt+S` | Connect to Server |
| Sidebar → Sync → Setup Sync | Launch gtk-sync installer (server or client) |
| Sidebar → Sync (status / folder) | Active server status; click client folder to open it |
| Sync ✕ / eject | Uninstall local server; disconnect client (files kept) |
| Sync folder emblems / Sync column | Up to date, Syncing, Pending, Deleted (from client status.json) |
| Context → Show Deleted | Ghost tombstone rows (dimmed); Restore Previous Version… |
| `Ctrl+R` / `F5` | Reload |
| `Ctrl+T` / `Ctrl+W` | New / close tab |
| `Ctrl+N` | New window |
| `Ctrl+Shift+N` | New folder |
| `Ctrl+C` / `X` / `V` | Copy / Cut / Paste |
| `Delete` | Move to Trash |
| `Shift+Delete` | Delete permanently |
| `F2` | Rename |
| `Alt+Enter` | Properties |
| `Ctrl+A` | Select all |
| `Ctrl+H` | Show hidden files |
| `Ctrl+F` | Search folder (filter names) |
| `Ctrl+Shift+F` | Find in Files (content search) |
| `Ctrl+1` / `Ctrl+2` | Toggle list/grid |
| `Ctrl+Q` | Quit |

## Architecture

| Module | Role |
|--------|------|
| `window` | Main window, menus, actions, tabs |
| `sync_setup` | Launch gtk-sync install dialog; probe systemd server/client status |
| `tab` | `GtkDirectoryList` + list/grid views, filter, sort |
| `sidebar` | Places / bookmarks |
| `pathbar` | Breadcrumbs + location entry |
| `file_ops` | Create, rename, trash, delete, paste |
| `clipboard` | Cut/copy state + `text/uri-list` |
| `search` | In-folder search bar |
| `properties` / `prefs` | Dialogs |
| `config` | TOML preferences |

Uses only **Rust** and **GTK4** (no libadwaita), matching the other `gtk-*` apps in this tree.
