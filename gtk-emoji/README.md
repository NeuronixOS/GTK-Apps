# Emoji Script

A Python script to show an emoji picker dialog. When you select an emoji, it's automatically copied to your clipboard.

## Features

- 🎨 **Native GTK Interface**: Beautiful, native Linux UI with stable threading and professional styling
- 🔍 **Search Functionality**: Real-time search through all available emojis
- 📝 **Recent Emojis**: Automatically saves and displays your most recently used emojis
- 📋 **Clipboard Integration**: Selected emojis are automatically copied to clipboard
- ⚙️ **Configurable**: Customizable settings via JSON configuration file

## Requirements

- Python 3.6+
- Linux with desktop environment (GNOME, KDE, etc.)
- X11 display server
- Clipboard utility: xclip, xsel, or wl-copy (xclip is installed automatically)
- GTK for native GUI (installed automatically)

## Installation

1. **Clone or download this repository**
   ```bash
   cd /path/to/Emoji-Picker
   ```

2. **Make the installation script executable and run it**
   ```bash
   chmod +x install.sh
   ./install.sh
   ```

The installation script will:
- Install required Python packages

## Usage

### Basic Usage
1. **Trigger the emoji picker**: Run the script to open the window
2. **Search for emojis**: Type in the search box to filter emojis
3. **Select an emoji**: Double-click on any emoji to copy it to clipboard
4. **Use recent emojis**: Check the "Recent Emojis" panel for quick access

### Keyboard Shortcuts
- `Ctrl+F`: Focus search box
- `Ctrl+R`: Focus recent emojis list
- `Ctrl+A`: Focus all emojis list
- `Escape`: Close dialog
- `Enter`: Select highlighted emoji

## Configuration

The service creates a configuration file at `~/.emoji_service/config.json`:

```json
{
  "hotkey": "mouse6",
  "max_recent": 20,
  "window_width": 600,
  "window_height": 500
}
```

### Configuration Options
- `max_recent`: Maximum number of recent emojis to remember
- `window_width`: Width of the emoji picker window
- `window_height`: Height of the emoji picker window

## Troubleshooting

### Display Issues
- Ensure you're running an X11 display server
- The service requires a graphical session to run
- Check that `DISPLAY` environment variable is set

### Dependencies
The service uses the following Python packages:
- `emoji`: Emoji data and utilities
- `pyautogui`: GUI automation
- `keyboard`: Keyboard event handling
- `pyperclip`: Clipboard operations

## License

This project is open source. Feel free to modify and distribute as needed.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.
