#!/usr/bin/env bash
# Build and install gtk-meld into ./target (suite-local, not ~/.local).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"

PYVER="$(python3 -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")')"
PREFIX="$ROOT/target"
PURELIB="$PREFIX/lib/python${PYVER}/site-packages"

if [[ ! -f _build/build.ninja ]]; then
  meson setup _build --prefix="$PREFIX" -Dpython.purelibdir="$PURELIB"
else
  meson configure _build --prefix="$PREFIX" -Dpython.purelibdir="$PURELIB"
fi

ninja -C _build install

# Suite-style entrypoint: target/gtk-meld (same idea as gtk-edit's target/release/…)
# Theme path is relocatable: source tree (target/../../gtk-theme) or installed
# sibling (apps/gtk-meld/../../gtk-theme → gtk-apps/gtk-theme).
cat > "$PREFIX/gtk-meld" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
PYVER="$(python3 -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")')"
THEME=""
for cand in "$ROOT/../../gtk-theme/python" "$ROOT/../gtk-theme/python"; do
  if [[ -f "$cand/gtk_theme.py" ]]; then
    THEME="$cand"
    break
  fi
done
export PYTHONPATH="${THEME:+$THEME:}$ROOT/lib/python${PYVER}/site-packages${PYTHONPATH:+:$PYTHONPATH}"
export XDG_DATA_DIRS="$ROOT/share${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}"
export GSETTINGS_SCHEMA_DIR="$ROOT/share/glib-2.0/schemas${GSETTINGS_SCHEMA_DIR:+:$GSETTINGS_SCHEMA_DIR}"
exec "$ROOT/bin/gtk-meld" "$@"
EOF
chmod +x "$PREFIX/gtk-meld"

echo "Installed: $PREFIX/gtk-meld"
