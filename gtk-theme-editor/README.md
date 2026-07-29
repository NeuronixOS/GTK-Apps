# gtk-theme-editor

A GTK4 front-end for the shared [`gtk-theme`](../gtk-theme) suite color profiles.

Load any built-in profile (Gruvbox, Tokyo Night, Dracula, Nord, …) or one of
your own custom profiles, tweak the **foreground**, **background**, and the full
**16-color ANSI palette**, and watch the whole window recolor as you edit. Save
your result under a custom name and it becomes available in every GTK-Apps
suite app's *Profile* menu.

Created by Kevin Hinds — [github.com/NeuronixOS/GTK-Apps](https://github.com/NeuronixOS/GTK-Apps)

## Features

- **Load a profile** — pick any built-in or custom profile from the *Base:*
  dropdown in the header.
- **Live preview** — every color change re-themes the editor chrome
  immediately (header bar, buttons, entries, list, scrollbars) plus a dedicated
  preview pane with a palette strip and foreground/background swatches.
- **Edit everything** — foreground, background, and all 16 palette slots, each
  with a color picker and a `#rrggbb` hex entry that stay in sync.
- **Save with a custom name** — writes to
  `~/.config/gtk-apps/custom-profiles.json`. Re-saving a loaded custom profile
  edits it in place; editing a built-in creates a new custom profile.
- **Apply to Suite** — saves, then switches every running suite app to the
  profile at once (via the shared `~/.config/gtk-apps/theme.toml`).
- **Delete** — remove a custom profile (built-ins are protected).

## How it works

Custom profiles live alongside the shared `theme.toml` in
`~/.config/gtk-apps/`. The `gtk-theme` library exposes them through
`all_profiles()` / `profile_by_id()`, so any app that builds its Profile menu
from the library automatically lists them.

> Note: other suite apps must be rebuilt against the updated `gtk-theme`
> library (run `./build-launch.sh` from `GTK-Apps/`) before they can resolve
> and display custom profiles.

## Build & run

```bash
cargo build --release
./target/release/gtk-theme-editor
```

Or via the suite helper:

```bash
../build-launch.sh gtk-theme-editor
```
