# gtk-player

GTK4 player for video and audio. Playback uses GTK’s GStreamer backend (`gtk::MediaFile`), so the app plays whatever the installed GStreamer plugins can decode — H.264, H.265, other video containers, and audio such as MP3, FLAC, WAV, Opus, and sound-only MP4/M4A.

## Window

- **Open** replaces the playlist with one or more files and starts the first. **Add** appends. Dropping files appends. A folder adds the files inside it (not nested folders).
- The **playlist** is a narrow side panel. The header button, the menu, or F9 / Ctrl+L shows or hides it. Closing it gives that width back to the video. Open, add, remove, and clear live in the panel. Click a row to play it.
- Transport under the picture: previous, play/pause, next, stop, time, loop switches, and volume. The loop handles are on the time bar.
- **Loop track** repeats the current item between the two handles on the time bar. Dragging either handle turns loop track on. The playhead seeks.
- **Loop playlist** starts again at the first item after the last one.
- Audio-only files show the title instead of a blank video picture.

## Build

From this directory, with the suite pkg-config stubs on `PKG_CONFIG_PATH` (same as the other Rust apps):

```bash
../build-launch.sh gtk-player
# or
./run.sh
```

Runtime packages (Debian / Neuronix):

```bash
sudo apt install libgtk-4-media-gstreamer gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-libav
```

`gstreamer1.0-libav` is what decodes H.264 and H.265. Without a plugin for a codec, the status line shows the GStreamer error.

## Shortcuts

| Key | Action |
|-----|--------|
| Ctrl+O | Open (replaces the playlist) |
| Ctrl+Shift+O | Add to the playlist |
| Space | Play / pause |
| P / Ctrl+Left | Previous |
| N / Ctrl+Right | Next |
| Left / Right | Seek 5 seconds |
| Shift+Left / Right | Seek 1 second |
| `[` / `]` | Set loop start / end at the playhead |
| L | Loop this track |
| Shift+L | Loop the playlist |
| F9 / Ctrl+L | Show or hide the playlist |
| Delete | Remove the selected playlist item |

Settings are saved to `~/.config/gtk-apps/gtk-player/config.toml`.
