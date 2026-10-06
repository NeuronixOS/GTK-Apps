#!/bin/bash
# Install gtk-calendar runtime dependencies.

set -euo pipefail

echo "Installing gtk-calendar dependencies..."

if command -v apt-get >/dev/null 2>&1; then
    sudo apt-get update
    sudo apt-get install -y \
        python3 \
        python3-gi \
        gir1.2-gtk-4.0 \
        python3-googleapi \
        python3-google-auth \
        python3-google-auth-httplib2 \
        python3-google-auth-oauthlib \
        python3-httplib2 \
        python3-icalendar \
        python3-dateutil
else
    echo "apt-get not found. Install Python GTK4 + Google/ICS libraries manually."
fi

chmod +x "$(dirname "$0")/start.sh" "$(dirname "$0")/app.py"

echo "Installation complete. Run ./start.sh"
