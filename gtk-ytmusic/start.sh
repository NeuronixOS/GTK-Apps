#!/usr/bin/env bash
# Launch YouTube Music in a dedicated Chromium/Chrome app window.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"
export PYTHONUNBUFFERED=1
exec python3 "$SCRIPT_DIR/app.py" "$@"
