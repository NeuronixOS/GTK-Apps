#!/usr/bin/env bash
# Stop, rebuild (compiled apps only), and relaunch GTK-Apps for local testing.
#
# Usage:
#   ./build-launch.sh                         # all apps
#   ./build-launch.sh gtk-files               # one app
#   ./build-launch.sh gtk-files gtk-meetings
#
# Rust apps (Cargo.toml): cargo build --release, then run the binary.
# Python / script apps: skip build; run via launch/start/run.sh or python entrypoint.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Ensure cargo is on PATH when launched from a minimal shell.
if ! command -v cargo >/dev/null 2>&1; then
  # shellcheck source=/dev/null
  [[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"
fi

# Suite apps (gtk-theme is a library — not launched).
ALL_APPS=(
  gtk-calc
  gtk-edit
  gtk-files
  gtk-image
  gtk-term
  gtk-theme-editor
  gtk-meetings
  gtk-workspaces
  gtk-worktimezone
  gtk-photos
  gtk-colors
)

is_known_app() {
  local candidate=$1 app
  for app in "${ALL_APPS[@]}"; do
    [[ "$app" == "$candidate" ]] && return 0
  done
  return 1
}

app_kind() {
  local app=$1 dir="$ROOT/$1"
  if [[ -f "$dir/Cargo.toml" ]]; then
    echo rust
  else
    echo script
  fi
}

# Resolve how to launch a non-Rust app.
app_launch_cmd() {
  local app=$1 dir="$ROOT/$1"
  if [[ -x "$dir/launch.sh" ]]; then
    echo "$dir/launch.sh"
  elif [[ -x "$dir/start.sh" ]]; then
    echo "$dir/start.sh"
  elif [[ -x "$dir/run.sh" ]]; then
    echo "$dir/run.sh"
  elif [[ -f "$dir/app.py" ]]; then
    echo "python3 $dir/app.py"
  elif [[ -f "$dir/colors.py" ]]; then
    echo "python3 $dir/colors.py"
  elif [[ -f "$dir/work_time_zones.py" ]]; then
    echo "python3 $dir/work_time_zones.py"
  else
    return 1
  fi
}

# How to stop a running instance: exact:<name> | pattern:<regex> | cwd:<dir>
app_stop_spec() {
  local app=$1
  case "$app" in
    gtk-calc | gtk-edit | gtk-files | gtk-image | gtk-term)
      echo "exact:$app"
      ;;
    gtk-theme-editor)
      # Binary name is 16 chars; Linux truncates comm to 15, so `pgrep -x`
      # never matches. Match the full launched binary path instead.
      echo "pattern:${ROOT}/gtk-theme-editor/target/release/gtk-theme-editor"
      ;;
    gtk-meetings)
      echo "pattern:${ROOT}/gtk-meetings/app.py"
      ;;
    gtk-workspaces)
      echo "pattern:${ROOT}/gtk-workspaces/app.py"
      ;;
    gtk-worktimezone)
      echo "pattern:${ROOT}/gtk-worktimezone/work_time_zones.py"
      ;;
    gtk-photos)
      # Launched as `python3 -m src.main` from the app directory.
      echo "cwd:${ROOT}/gtk-photos"
      ;;
    gtk-colors)
      echo "pattern:${ROOT}/gtk-colors/colors.py"
      ;;
    *)
      echo "pattern:${ROOT}/${app}"
      ;;
  esac
}

stop_by_cwd() {
  local target=$1 signal=${2:-TERM} pid cwd
  for pid in $(pgrep -f 'python3|python' 2>/dev/null || true); do
    cwd="$(readlink -f "/proc/$pid/cwd" 2>/dev/null || true)"
    if [[ "$cwd" == "$target" ]]; then
      kill "-$signal" "$pid" 2>/dev/null || true
    fi
  done
}

is_running() {
  local app=$1 spec pattern cwd
  spec="$(app_stop_spec "$app")"
  case "$spec" in
    exact:*)
      pgrep -x "${spec#exact:}" >/dev/null 2>&1
      ;;
    pattern:*)
      pgrep -f "${spec#pattern:}" >/dev/null 2>&1
      ;;
    cwd:*)
      cwd="${spec#cwd:}"
      for pid in $(pgrep -f 'python3|python' 2>/dev/null || true); do
        [[ "$(readlink -f "/proc/$pid/cwd" 2>/dev/null || true)" == "$cwd" ]] && return 0
      done
      return 1
      ;;
  esac
}

# Normalize args like "gtk-files/" or "./gtk-files" to "gtk-files".
resolve_apps() {
  if (($# == 0)); then
    APPS=("${ALL_APPS[@]}")
    return
  fi

  APPS=()
  local raw name
  for raw in "$@"; do
    name="${raw%/}"
    name="${name##*/}"
    if ! is_known_app "$name"; then
      echo "Unknown app: $raw" >&2
      echo "Known apps: ${ALL_APPS[*]}" >&2
      exit 1
    fi
    if [[ ! -d "$ROOT/$name" ]]; then
      echo "Missing directory: $ROOT/$name" >&2
      exit 1
    fi
    APPS+=("$name")
  done
}

stop_one() {
  local app=$1 signal=${2:-TERM} spec pattern
  spec="$(app_stop_spec "$app")"
  case "$spec" in
    exact:*)
      pattern="${spec#exact:}"
      if pgrep -x "$pattern" >/dev/null 2>&1; then
        echo "    stopping $app (SIG${signal})"
        pkill "-$signal" -x "$pattern" || true
      fi
      ;;
    pattern:*)
      pattern="${spec#pattern:}"
      if pgrep -f "$pattern" >/dev/null 2>&1; then
        echo "    stopping $app (SIG${signal})"
        pkill "-$signal" -f "$pattern" || true
      fi
      ;;
    cwd:*)
      if is_running "$app"; then
        echo "    stopping $app (SIG${signal})"
        stop_by_cwd "${spec#cwd:}" "$signal"
      fi
      ;;
  esac
}

stop_apps() {
  echo "==> Stopping: ${APPS[*]}"
  local app
  for app in "${APPS[@]}"; do
    stop_one "$app" TERM
  done

  sleep 0.5

  for app in "${APPS[@]}"; do
    if is_running "$app"; then
      stop_one "$app" KILL
    fi
  done
}

# Prefer system pkg-config; fall back to gtk-edit's vendored .deps if present.
setup_pkg_config() {
  if pkg-config --exists gtksourceview-5 2>/dev/null; then
    return
  fi
  local deps_pc="$ROOT/gtk-edit/.deps/usr/lib/x86_64-linux-gnu/pkgconfig"
  if [[ -f "$deps_pc/gtksourceview-5.pc" ]]; then
    echo "==> Using PKG_CONFIG_PATH from gtk-edit/.deps (system gtksourceview-5.pc missing)"
    export PKG_CONFIG_PATH="${deps_pc}${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
  fi
}

# Dropbox often sets +i (immutable) on cargo outputs under the synced tree,
# which makes incremental builds / linking fail with "Permission denied".
make_mutable() {
  local path=$1
  [[ -e "$path" ]] || return 0
  if command -v chattr >/dev/null 2>&1; then
    chattr -R -i "$path" 2>/dev/null || true
  fi
  chmod -R u+w "$path" 2>/dev/null || true
}

# Where cargo will write for this crate (honours .cargo/config.toml target-dir).
cargo_target_dir() {
  local dir=$1
  (cd "$dir" && cargo metadata --format-version=1 --no-deps 2>/dev/null \
    | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p' \
    | head -1) || true
}

# Clear immutable bits on in-tree and out-of-tree cargo targets before build.
make_cargo_targets_mutable() {
  echo "==> Marking cargo build trees mutable..."
  local app dir kind target
  make_mutable "$ROOT/gtk-theme/target"
  for app in "${APPS[@]}"; do
    kind="$(app_kind "$app")"
    [[ "$kind" == rust ]] || continue
    dir="$ROOT/$app"
    make_mutable "$dir/target"
    target="$(cargo_target_dir "$dir")"
    if [[ -n "$target" && "$target" != "$dir/target" ]]; then
      make_mutable "$target"
    fi
    # Common out-of-Dropbox dirs used by this suite.
    make_mutable "/tmp/${app}-target"
  done
}

# Copy the freshly built binary into $app/target/release/ for launchers that
# expect the in-tree path (and clear +i on the destination first).
install_rust_binary() {
  local app=$1 dir="$ROOT/$1"
  local target bin dest
  target="$(cargo_target_dir "$dir")"
  [[ -n "$target" ]] || target="$dir/target"
  bin="$target/release/$app"
  dest="$dir/target/release/$app"
  if [[ ! -f "$bin" ]]; then
    echo "    warn: built binary missing: $bin" >&2
    return 1
  fi
  mkdir -p "$dir/target/release"
  make_mutable "$dest"
  if [[ "$(readlink -f "$bin" 2>/dev/null || echo "$bin")" != \
        "$(readlink -f "$dest" 2>/dev/null || echo "$dest")" ]]; then
    cp -f "$bin" "$dest"
  fi
  chmod u+wx "$dest" 2>/dev/null || true
  make_mutable "$dest"
}

build_apps() {
  echo "==> Building compiled apps (release)..."
  local app dir kind
  FAILED_APPS=()
  READY_APPS=()
  for app in "${APPS[@]}"; do
    kind="$(app_kind "$app")"
    dir="$ROOT/$app"
    if [[ "$kind" == rust ]]; then
      echo "    building $app..."
      make_mutable "$dir/target"
      make_mutable "/tmp/${app}-target"
      if (cd "$dir" && cargo build --release) && install_rust_binary "$app"; then
        echo "    $app OK"
        READY_APPS+=("$app")
      else
        echo "    $app FAILED" >&2
        FAILED_APPS+=("$app")
      fi
    else
      echo "    skip build $app (script/python)"
      READY_APPS+=("$app")
    fi
  done
}

launch_apps() {
  echo "==> Launching apps..."
  local app kind bin cmd launched=0
  for app in "${READY_APPS[@]:-}"; do
    [[ -n "$app" ]] || continue
    kind="$(app_kind "$app")"
    if [[ "$kind" == rust ]]; then
      # Prefer run.sh when present (resolves /tmp vs in-tree target-dir).
      if [[ -x "$ROOT/$app/run.sh" ]]; then
        echo "    launching $app (run.sh)"
        "$ROOT/$app/run.sh" >/dev/null 2>&1 &
        disown || true
        launched=$((launched + 1))
        continue
      fi
      bin="$ROOT/$app/target/release/$app"
      if [[ ! -x "$bin" ]]; then
        echo "    skip $app (binary missing)" >&2
        continue
      fi
      echo "    launching $app"
      "$bin" >/dev/null 2>&1 &
      disown || true
      launched=$((launched + 1))
    else
      if ! cmd="$(app_launch_cmd "$app")"; then
        echo "    skip $app (no launch script / entrypoint)" >&2
        continue
      fi
      echo "    launching $app ($cmd)"
      # shellcheck disable=SC2086
      (cd "$ROOT/$app" && $cmd) >/dev/null 2>&1 &
      disown || true
      launched=$((launched + 1))
    fi
  done
  echo "    launched $launched app(s)"
}

resolve_apps "$@"
setup_pkg_config
stop_apps
make_cargo_targets_mutable
build_apps
launch_apps

if ((${#FAILED_APPS[@]} > 0)); then
  echo "==> Done with failures: ${FAILED_APPS[*]}" >&2
  exit 1
fi
echo "==> Done. Running: ${READY_APPS[*]}"
