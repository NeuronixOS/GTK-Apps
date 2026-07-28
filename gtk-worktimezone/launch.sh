#!/usr/bin/env bash
# Launch WorkTimeZones GTK app.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec python3 "$SCRIPT_DIR/work_time_zones.py" "$@"
