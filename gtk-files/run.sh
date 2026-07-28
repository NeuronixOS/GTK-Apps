#!/usr/bin/env bash
# Launch the in-tree release binary (same path cargo / build-launch.sh use).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
BIN="$ROOT/target/release/gtk-files"
if [[ ! -x "$BIN" ]]; then
  echo "gtk-files binary not found at $BIN — run: ../build-launch.sh gtk-files" >&2
  exit 1
fi
exec "$BIN" "$@"
