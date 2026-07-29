# gtk-photos

GTK4 **Photo Organizer** — browse and manage a photo drive, favorites, folders, and website thumbnails.

Created by Kevin Hinds — [github.com/NeuronixOS/GTK-Apps](https://github.com/NeuronixOS/GTK-Apps)

## Run

```bash
./start.sh
# or
../build-launch.sh gtk-photos
```

## Config

Under the suite config tree:

```
~/.config/gtk-apps/gtk-photos/config.json
~/.config/gtk-apps/gtk-photos/favorites.json
~/.config/gtk-apps/gtk-photos/recent.json
~/.config/gtk-apps/gtk-photos/best.json
~/.config/gtk-apps/gtk-photos/websites.json
~/.config/gtk-apps/gtk-photos/websites_thumbnail_overrides.json
~/.config/gtk-apps/gtk-photos/websites_img/
```

Example `config.json`:

```json
{
  "media_directory": "/path/to/sort",
  "explore_directory": "/path/to/photos",
  "explore_directory_features": "/path/to/photos/FEATURES",
  "photo_drive": "/path/to/photos"
}
```

`photo_drive` is the root folder the app browses (favorites, folders tab, create folder, etc.). There are no hardcoded drive mount paths in the app.

Folder thumbnails are cached under `~/.cache/gtk-apps/gtk-photos/`.

## Install deps

```bash
./install.sh
```
