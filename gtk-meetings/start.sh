#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

VENV="$SCRIPT_DIR/.venv"
PYTHON="$VENV/bin/python"

venv_usable() {
  [[ -x "$PYTHON" ]] || return 1
  # Broken Dropbox/copied venvs keep a shebang for another path
  "$PYTHON" -c "import sys" 2>/dev/null || return 1
  "$PYTHON" -m pip --version >/dev/null 2>&1 || return 1
}

need_deps() {
  # google.auth alone is not enough — httplib2 needs pyparsing at import time
  ! "$PYTHON" -c "import google.auth, googleapiclient.discovery, httplib2, pyparsing" 2>/dev/null
}

if ! venv_usable; then
  echo "Virtualenv missing or broken (often after moving the project)."
  echo "Run:  ./install.sh"
  exit 1
fi

if need_deps; then
  echo "Installing / repairing dependencies..."
  "$PYTHON" -m pip install -r requirements.txt
fi

exec "$PYTHON" app.py
