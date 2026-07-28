#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

if [[ ! -d ".venv" ]]; then
  echo "Missing .venv. Create it first:"
  echo "  python3 -m venv .venv"
  exit 1
fi

source ".venv/bin/activate"

if python -c "import gi" >/dev/null 2>&1; then
  exec python app.py
fi

# Debian/Ubuntu typically provide gi via apt (python3-gi), not pip.
if /usr/bin/python3 -c "import gi" >/dev/null 2>&1; then
  echo "Notice: gi is not available in .venv; using system python3 (has gi)."
  exec /usr/bin/python3 app.py
fi

echo "Missing GTK Python bindings (gi). Install system packages:"
echo "  sudo apt update && sudo apt install -y python3-gi gir1.2-gtk-4.0"
echo
echo "If you want to keep using .venv, recreate it with system packages visible:"
echo "  rm -rf .venv"
echo "  python3 -m venv --system-site-packages .venv"
echo "  source .venv/bin/activate"
echo "  pip install -r requirements.txt"
exit 1
