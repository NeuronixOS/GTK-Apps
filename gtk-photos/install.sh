#!/bin/bash
# Installation script for Photo Organizer (gtk-photos)

set -e  # Exit on error

echo "Installing Photo Organizer dependencies..."

# Check if running on Debian/Ubuntu
if command -v apt-get &> /dev/null; then
    echo "Installing system packages..."
    sudo apt-get update
    sudo apt-get install -y \
        python3 \
        python3-pip \
        python3-gi \
        gir1.2-gtk-4.0 \
        gir1.2-gdkpixbuf-2.0 \
        gir1.2-gio-2.0 \
        ffmpeg
else
    echo "Warning: apt-get not found. Please install the following packages manually:"
    echo "  - python3"
    echo "  - python3-pip"
    echo "  - python3-gi"
    echo "  - gir1.2-gtk-4.0"
    echo "  - gir1.2-gdkpixbuf-2.0"
    echo "  - gir1.2-gio-2.0"
    echo "  - ffmpeg"
fi

# Install Python dependencies
echo "Installing Python packages..."
pip3 install --user -r requirements.txt

# Make start script executable
chmod +x start.sh

echo "Installation complete!"
echo "Run ./start.sh to launch the application."
