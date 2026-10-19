#!/usr/bin/env bash
# Launch the in-tree release binary (same path cargo / build-launch.sh use).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
BIN="$ROOT/target/release/gtk-player"
if [[ ! -x "$BIN" ]]; then
  echo "gtk-player binary not found at $BIN — run: ../build-launch.sh gtk-player" >&2
  exit 1
fi
exec "$BIN" "$@"
