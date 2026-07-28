#!/usr/bin/env bash
# Launch gtk-edit. Prefer the out-of-Dropbox cargo target (Dropbox marks
# target/release/deps immutable and blocks linking).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
for candidate in \
  /tmp/gtk-edit-target/release/gtk-edit \
  "$ROOT/target/release/gtk-edit"
do
  if [[ -x "$candidate" ]]; then
    exec "$candidate" "$@"
  fi
done
echo "gtk-edit binary not found — run: cargo build --release" >&2
exit 1
