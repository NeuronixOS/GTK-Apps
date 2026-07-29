# GTK Meld

GTK4 visual diff and merge tool (based on [Meld](https://meld.app/)), with the shared suite Profile menu via `gtk-theme`.

Created by Kevin Hinds — [github.com/NeuronixOS/GTK-Apps](https://github.com/NeuronixOS/GTK-Apps)

## Build

```bash
./build.sh
```

Installs into `./target/`. Executable:

```text
gtk-meld/target/gtk-meld
```

## Configuration

Unlike upstream Meld (which stores preferences in dconf), GTK Meld keeps all of
its settings in a keyfile at `~/.config/gtk-apps/gtk-meld/settings.ini`, alongside the
rest of the GTK-Apps suite. `meld/settings.py` redirects every `GSettings`
schema (`org.gnome.meld` and `org.gnome.meld.WindowState`) to this keyfile via a
`GKeyfileSettingsBackend` (see `get_keyfile_backend`).

Optional git difftool settings can live in `~/.config/gtk-apps/gtk-meld/gitconfig`
and be included from `~/.gitconfig`.

## Dependencies

- Python ≥ 3.10, GTK 4, GtkSourceView 5, libadwaita, PyGObject, pycairo
- Build: `meson`, `ninja`, `glib-compile-resources`
