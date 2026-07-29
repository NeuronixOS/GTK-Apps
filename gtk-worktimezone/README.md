# gtk-worktimezone

Track coworker local times across timezones.

Created by Kevin Hinds — [github.com/NeuronixOS/GTK-Apps](https://github.com/NeuronixOS/GTK-Apps)

## Run

```bash
./launch.sh
```

Or via the suite launcher:

```bash
../build-launch.sh gtk-worktimezone
```

## Data file

`~/.config/gtk-apps/gtk-worktimezone/work_time_zones_data.json` is created on first save. Until then the app runs with empty defaults:

```json
{
  "timezone_names": {},
  "selected_timezones": [],
  "default_timezone": null
}
```

Edit coworkers and timezones in the GTK UI or by editing the JSON directly.

## Dependencies

- Python 3.10+, GTK4, Libadwaita, PyGObject
