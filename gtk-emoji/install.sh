#!/usr/bin/env bash
# Install gtk-emoji runtime dependencies (GTK4 + clipboard helpers).
set -euo pipefail
echo "Installing gtk-emoji dependencies..."
if command -v apt-get >/dev/null 2>&1; then
	sudo apt-get update
	sudo apt-get install -y python3 python3-gi python3-gi-cairo gir1.2-gtk-4.0 wl-clipboard xclip
else
	echo "apt-get not found. Install python3-gi, gir1.2-gtk-4.0, wl-clipboard (or xclip) manually."
fi
chmod +x "$(dirname "$0")/start.sh" "$(dirname "$0")/app.py" "$(dirname "$0")/emoji_picker.py"
echo "Installation complete. Run ./start.sh or gtk-emoji"
