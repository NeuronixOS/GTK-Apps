#!/bin/bash
# Git-status colors for directory names in `ls` output.
#
# Matches gtk-files: blue #3b82f6 untracked, orange #ea580c modified,
# red #dc2626 conflict. A child folder takes the strongest status of
# anything underneath it. Clean directories keep the normal `ls` color.
#
# Invoked as `bash git-ls-lib.bash [ls args...]` from the interactive
# shell wrapper. Sourced by tests to call the helpers directly.

# Blue / orange / red, bold so dirty folders stay as heavy as other dirs.
gtk_term_sgr() {
  case "$1" in
    untracked) printf '\033[1;38;2;59;130;246m' ;;
    modified) printf '\033[1;38;2;234;88;12m' ;;
    conflict) printf '\033[1;38;2;220;38;38m' ;;
  esac
}

gtk_term_rank() {
  case "$1" in
    untracked) printf '1' ;;
    modified) printf '2' ;;
    conflict) printf '3' ;;
    *) printf '0' ;;
  esac
}

# Strongest of two decor names. Empty loses to anything set.
gtk_term_merge() {
  local a="$1" b="$2" ra rb
  ra=$(gtk_term_rank "$a")
  rb=$(gtk_term_rank "$b")
  if [ "$rb" -gt "$ra" ]; then
    printf '%s' "$b"
  else
    printf '%s' "$a"
  fi
}

# Porcelain XY column → untracked | modified | conflict | empty.
# Same rules as gtk-files classify_xy.
gtk_term_classify_xy() {
  local xy="$1" a b added=0 changed=0
  [ "${#xy}" -eq 2 ] || return 0
  a=${xy:0:1}
  b=${xy:1:1}
  if [ "$a" = "U" ] || [ "$b" = "U" ] || [ "$xy" = "AA" ] || [ "$xy" = "DD" ]; then
    printf '%s' conflict
    return 0
  fi
  if [ "$xy" = "??" ]; then
    printf '%s' untracked
    return 0
  fi
  if [ "$xy" = "!!" ]; then
    return 0
  fi
  if [ "$a" = "A" ] || [ "$b" = "A" ]; then
    added=1
  fi
  case "$a" in
    M|D|R|T|C) changed=1 ;;
  esac
  case "$b" in
    M|D|R|T|C) changed=1 ;;
  esac
  if [ "$added" = 1 ] && [ "$changed" = 1 ]; then
    printf '%s' modified
    return 0
  fi
  if [ "$added" = 1 ]; then
    printf '%s' untracked
    return 0
  fi
  if [ "$changed" = 1 ] || [ "$a" != " " ] || [ "$b" != " " ]; then
    printf '%s' modified
    return 0
  fi
}

# Path half of a porcelain v1 line, unquoted, no trailing slash.
gtk_term_porcelain_path() {
  local line="$1" xy rest
  [ "${#line}" -ge 3 ] || return 0
  xy=${line:0:2}
  rest=${line:3}
  case "$xy" in
    *R*|*C*)
      case "$rest" in
        *" -> "*) rest=${rest##*" -> "} ;;
      esac
      ;;
  esac
  gtk_term_unquote "$rest"
}

gtk_term_unquote() {
  local s="$1" out="" i=0 c n
  s=${s%$'\r'}
  s=${s#"${s%%[![:space:]]*}"}
  s=${s%"${s##*[![:space:]]}"}
  if [ "${#s}" -ge 2 ] && [ "${s:0:1}" = '"' ] && [ "${s: -1}" = '"' ]; then
    s=${s:1:$((${#s} - 2))}
    while [ "$i" -lt "${#s}" ]; do
      c=${s:$i:1}
      if [ "$c" = '\' ]; then
        n=${s:$((i + 1)):1}
        case "$n" in
          n) out+=$'\n' ;;
          t) out+=$'\t' ;;
          *) out+="$n" ;;
        esac
        i=$((i + 2))
      else
        out+="$c"
        i=$((i + 1))
      fi
    done
    s=$out
  fi
  s=${s%/}
  printf '%s' "$s"
}

# First path component of `path` (repo-relative) under `rel` (listed dir
# relative to the repo, empty when listing the repo root).
gtk_term_immediate_child() {
  local rel="$1" path="$2" rest="$path"
  if [ -n "$rel" ]; then
    if [ "$path" = "$rel" ]; then
      return 1
    fi
    case "$path" in
      "$rel"/*) rest=${path#"$rel"/} ;;
      *) return 1 ;;
    esac
  fi
  [ -n "$rest" ] || return 1
  printf '%s' "${rest%%/*}"
}

gtk_term_note() {
  local name="$1" decor="$2" prev
  [ -n "$name" ] && [ -n "$decor" ] || return 0
  prev=${COLORS[$name]:-}
  COLORS[$name]=$(gtk_term_merge "$prev" "$decor")
}

# Stdin: porcelain. $1 listed abs dir, $2 repo root.
gtk_term_apply_porcelain() {
  local listed_abs="$1" repo="$2" rel="" line path decor child
  case "$listed_abs" in
    "$repo") rel="" ;;
    "$repo"/*) rel=${listed_abs#"$repo"/} ;;
    *) return 0 ;;
  esac
  while IFS= read -r line || [ -n "$line" ]; do
    [ "${#line}" -ge 3 ] || continue
    decor=$(gtk_term_classify_xy "${line:0:2}")
    [ -n "$decor" ] || continue
    path=$(gtk_term_porcelain_path "$line")
    [ -n "$path" ] || continue
    if ! child=$(gtk_term_immediate_child "$rel" "$path"); then
      continue
    fi
    if [ -d "$listed_abs/$child" ]; then
      gtk_term_note "$child" "$decor"
    fi
  done
}

gtk_term_repo_worst() {
  local dir="$1" line decor worst=""
  while IFS= read -r line || [ -n "$line" ]; do
    [ "${#line}" -ge 3 ] || continue
    decor=$(gtk_term_classify_xy "${line:0:2}")
    [ -n "$decor" ] || continue
    worst=$(gtk_term_merge "$worst" "$decor")
  done < <(GIT_TERMINAL_PROMPT=0 GIT_OPTIONAL_LOCKS=1 git -C "$dir" --no-pager status --porcelain=v1 2>/dev/null || true)
  printf '%s' "$worst"
}

# Color a child that is its own git repo by that repo's worst status.
gtk_term_apply_nested() {
  local listed_abs="$1" d child decor
  for d in "$listed_abs"/* "$listed_abs"/.[!.]* "$listed_abs"/..?*; do
    [ -d "$d" ] || continue
    [ -e "$d/.git" ] || continue
    child=$(basename -- "$d")
    [ "$child" = "." ] || [ "$child" = ".." ] || [ "$child" = ".git" ] && continue
    decor=$(gtk_term_repo_worst "$d")
    gtk_term_note "$child" "$decor"
  done
}

gtk_term_scan_dir() {
  local listed_abs="$1" repo porc
  command -v git >/dev/null 2>&1 || return 0
  repo=$(git -C "$listed_abs" rev-parse --show-toplevel 2>/dev/null || true)
  repo=${repo%$'\r'}
  repo=${repo%$'\n'}
  if [ -n "$repo" ]; then
    porc=$(GIT_TERMINAL_PROMPT=0 GIT_OPTIONAL_LOCKS=1 git -C "$repo" --no-pager status --porcelain=v1 2>/dev/null || true)
    # Here-string, not a pipe: a pipe would run the updater in a subshell
    # and drop the COLORS assignments.
    gtk_term_apply_porcelain "$listed_abs" "$repo" <<<"$porc"
  fi
  gtk_term_apply_nested "$listed_abs"
}

gtk_term_abs_dir() {
  local d="$1"
  (cd -- "$d" 2>/dev/null && pwd -P)
}

# Drop --color so we can force --color=always. Sets GTK_TERM_SKIP_RECOLOR
# when the user asked for no color, and GTK_TERM_SKIP_GIT for -d / -R.
gtk_term_filter_args() {
  GTK_TERM_LS_ARGS=()
  GTK_TERM_OPERANDS=()
  GTK_TERM_SKIP_RECOLOR=0
  GTK_TERM_SKIP_GIT=0
  local skip=0 ended=0 arg last
  for arg in "$@"; do
    if [ "$ended" = 1 ]; then
      GTK_TERM_LS_ARGS+=("$arg")
      GTK_TERM_OPERANDS+=("$arg")
      continue
    fi
    if [ "$skip" = 1 ]; then
      skip=0
      GTK_TERM_LS_ARGS+=("$arg")
      continue
    fi
    case "$arg" in
      --)
        ended=1
        GTK_TERM_LS_ARGS+=("$arg")
        ;;
      --color=never|--color=no|--color=none)
        GTK_TERM_SKIP_RECOLOR=1
        GTK_TERM_LS_ARGS+=("$arg")
        ;;
      --color|--color=*)
        ;;
      --directory|--recursive)
        GTK_TERM_SKIP_GIT=1
        GTK_TERM_LS_ARGS+=("$arg")
        ;;
      --ignore|--width|--tabsize|--time-style|--quoting-style|--format|--sort|--block-size|--hide|--indicator-style|--time)
        skip=1
        GTK_TERM_LS_ARGS+=("$arg")
        ;;
      --*=*|--*)
        GTK_TERM_LS_ARGS+=("$arg")
        ;;
      -*)
        # Only short clusters: -d / -R (and -ld, -lR). Long options were handled above.
        case "$arg" in
          *d*|*R*) GTK_TERM_SKIP_GIT=1 ;;
        esac
        GTK_TERM_LS_ARGS+=("$arg")
        case "$arg" in
          -I|-w|-T) skip=1 ;;
          -*I|-*w|-*T)
            last=${arg: -1}
            case "$last" in
              I|w|T) skip=1 ;;
            esac
            ;;
        esac
        ;;
      *)
        GTK_TERM_LS_ARGS+=("$arg")
        GTK_TERM_OPERANDS+=("$arg")
        ;;
    esac
  done
}

gtk_term_ends_with_sgr() {
  local s="$1" tail body
  case "$s" in
    *$'\033['*) ;;
    *) return 1 ;;
  esac
  tail=${s##*$'\033['}
  [ "${tail: -1}" = "m" ] || return 1
  body=${tail%m}
  case "$body" in
    *[!0-9\;]*) return 1 ;;
  esac
  return 0
}

gtk_term_strip_sgr() {
  local s="$1" without_m
  without_m=${s%m}
  GTK_TERM_STRIPPED=${without_m%$'\033['*}
}

# Replace the SGR wrapped around a directory name. Leaves other uses of
# that text alone (the reset must follow the name immediately).
gtk_term_recolor_name() {
  local name="$1" sgr="$2" rest="$GTK_TERM_TEXT" out="" head after reset
  local r0=$'\033[0m' r00=$'\033[00m' rm=$'\033[m'
  [ -n "$name" ] || return 0
  while [ -n "$rest" ]; do
    case "$rest" in
      *"$name"*) ;;
      *)
        out+="$rest"
        break
        ;;
    esac
    head=${rest%%"$name"*}
    after=${rest#*"$name"}
    reset=""
    if gtk_term_ends_with_sgr "$head"; then
      if [ "${after#"$r0"}" != "$after" ]; then
        reset=$r0
      elif [ "${after#"$r00"}" != "$after" ]; then
        reset=$r00
      elif [ "${after#"$rm"}" != "$after" ]; then
        reset=$rm
      fi
    fi
    if [ -n "$reset" ]; then
      gtk_term_strip_sgr "$head"
      out+="$GTK_TERM_STRIPPED$sgr$name$reset"
      after=${after#"$reset"}
      rest=$after
      continue
    fi
    out+="$head${name:0:1}"
    rest="${name:1}$after"
  done
  GTK_TERM_TEXT=$out
}

gtk_term_apply_colors() {
  local name sgr
  for name in "${!COLORS[@]}"; do
    sgr=$(gtk_term_sgr "${COLORS[$name]}")
    [ -n "$sgr" ] || continue
    gtk_term_recolor_name "$name" "$sgr"
  done
}

gtk_term_ls_main() {
  declare -gA COLORS=()
  gtk_term_filter_args "$@"
  if [ "$GTK_TERM_SKIP_RECOLOR" = 1 ]; then
    command ls "$@"
    return $?
  fi

  local err status out abs target
  err=$(mktemp)
  out=$(
    unset NO_COLOR
    if [ -z "${TERM:-}" ] || [ "$TERM" = "dumb" ]; then
      export TERM=xterm-256color
    fi
    command ls --color=always "${GTK_TERM_LS_ARGS[@]}" 2>"$err"
    ec=$?
    printf x
    exit "$ec"
  )
  status=$?
  out=${out%x}
  if [ -s "$err" ]; then
    cat "$err" >&2
  fi
  rm -f "$err"

  if [ "$GTK_TERM_SKIP_GIT" != 1 ]; then
    if [ "${#GTK_TERM_OPERANDS[@]}" -eq 0 ]; then
      GTK_TERM_TARGETS=(".")
    else
      GTK_TERM_TARGETS=()
      for target in "${GTK_TERM_OPERANDS[@]}"; do
        if [ -d "$target" ]; then
          GTK_TERM_TARGETS+=("$target")
        fi
      done
    fi
    for target in "${GTK_TERM_TARGETS[@]}"; do
      abs=$(gtk_term_abs_dir "$target") || continue
      [ -n "$abs" ] || continue
      gtk_term_scan_dir "$abs"
    done
  fi

  GTK_TERM_TEXT=$out
  if [ "${#COLORS[@]}" -gt 0 ]; then
    gtk_term_apply_colors
  fi
  printf '%s' "$GTK_TERM_TEXT"
  return "$status"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  gtk_term_ls_main "$@"
  exit $?
fi
