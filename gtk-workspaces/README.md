# gtk-workspaces

GTK4 launcher for named workspaces — each workspace runs a configured set of commands (apps, scripts, layouts).

Created by Kevin Hinds — [github.com/NeuronixOS/GTK-Apps](https://github.com/NeuronixOS/GTK-Apps)

## Run

```bash
./run.sh
```

Or via the suite launcher:

```bash
../build-launch.sh gtk-workspaces
```

## Data file

`~/.config/gtk-apps/gtk-workspaces/workspaces.json` is created on first save (empty `{}` until you add workspaces). Edit workspaces in the GTK UI or by editing the JSON directly.

## Dependencies

- Python 3.10+, GTK4, PyGObject
