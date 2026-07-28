#!/usr/bin/env bash
# Install Meetings GTK app: system deps, fresh venv, and Python packages.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

VENV="$SCRIPT_DIR/.venv"
PYTHON="$VENV/bin/python"

echo "=== Meetings install ==="
echo "App dir: $SCRIPT_DIR"
echo

# System packages (Debian/Ubuntu) — skip if already available
need_system_pkgs=0
for pkg_check in python3 python3-venv; do
  command -v "${pkg_check%%-*}" >/dev/null 2>&1 || need_system_pkgs=1
done
/usr/bin/python3 -c "import gi" 2>/dev/null || need_system_pkgs=1

if [[ "$need_system_pkgs" -eq 1 ]]; then
  if command -v apt-get >/dev/null 2>&1; then
    echo "Installing system packages (may prompt for sudo)..."
    sudo apt-get update
    sudo apt-get install -y \
      python3 \
      python3-pip \
      python3-venv \
      python3-gi \
      gir1.2-gtk-4.0
  else
    echo "Warning: apt-get not found. Ensure these are installed:"
    echo "  python3 python3-pip python3-venv python3-gi gir1.2-gtk-4.0"
  fi
else
  echo "System packages already present (python3, venv, gi)."
fi

echo
echo "Recreating virtualenv..."
rm -rf "$VENV"
python3 -m venv --system-site-packages "$VENV"
"$PYTHON" -m pip install --upgrade pip
"$PYTHON" -m pip install -r requirements.txt

chmod +x "$SCRIPT_DIR/start.sh"

echo
echo "Installation complete."
echo "Launch with:  ./start.sh"
