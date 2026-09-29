# Installed into interactive bash/zsh. __GTK_TERM_GIT_LS_LIB__ is a
# shell-quoted path to git-ls-lib.bash, substituted when the file is written.

if [ "${GTK_TERM_GIT_LS:-1}" = "0" ]; then
  return 0
fi
if [ -n "${GTK_TERM_LS_WRAPPED:-}" ]; then
  return 0
fi

_gtk_term_ls_alias=$(alias ls 2>/dev/null || true)
GTK_TERM_LS_EXTRA=""
if [ -n "$_gtk_term_ls_alias" ]; then
  case "$_gtk_term_ls_alias" in
    *eza*|*exa*|*lsd*)
      unset _gtk_term_ls_alias
      return 0
      ;;
  esac
  _gtk_term_ls_body=${_gtk_term_ls_alias#alias }
  case "$_gtk_term_ls_body" in
    ls=*) _gtk_term_ls_body=${_gtk_term_ls_body#ls=} ;;
    *)
      unset _gtk_term_ls_alias _gtk_term_ls_body
      return 0
      ;;
  esac
  case "$_gtk_term_ls_body" in
    \'*\')
      _gtk_term_ls_body=${_gtk_term_ls_body#\'}
      _gtk_term_ls_body=${_gtk_term_ls_body%\'}
      ;;
    \"*\")
      _gtk_term_ls_body=${_gtk_term_ls_body#\"}
      _gtk_term_ls_body=${_gtk_term_ls_body%\"}
      ;;
  esac
  case "$_gtk_term_ls_body" in
    ls|ls\ *|command\ ls|command\ ls\ *|/bin/ls|/bin/ls\ *|/usr/bin/ls|/usr/bin/ls\ *)
      ;;
    *)
      unset _gtk_term_ls_alias _gtk_term_ls_body
      return 0
      ;;
  esac
  GTK_TERM_LS_EXTRA=$_gtk_term_ls_body
  GTK_TERM_LS_EXTRA=${GTK_TERM_LS_EXTRA#command }
  GTK_TERM_LS_EXTRA=${GTK_TERM_LS_EXTRA#/usr/bin/ls}
  GTK_TERM_LS_EXTRA=${GTK_TERM_LS_EXTRA#/bin/ls}
  GTK_TERM_LS_EXTRA=${GTK_TERM_LS_EXTRA#ls}
  GTK_TERM_LS_EXTRA=${GTK_TERM_LS_EXTRA#"${GTK_TERM_LS_EXTRA%%[![:space:]]*}"}
fi
unset _gtk_term_ls_alias _gtk_term_ls_body
unalias ls 2>/dev/null || true
export GTK_TERM_LS_EXTRA
export GTK_TERM_LS_WRAPPED=1

ls() {
  if [ ! -t 1 ] || [ "${GTK_TERM_GIT_LS:-1}" = "0" ]; then
    command ls "$@"
    return $?
  fi
  # Alias flags are a single string. Bash splits unquoted expansions; zsh does
  # not, unless SH_WORD_SPLIT is on. noglob so a flag like --ignore='*' is literal.
  if [ -n "${ZSH_VERSION:-}" ]; then
    setopt local_options shwordsplit noglob
  else
    case $- in
      *f*) _gtk_term_had_noglob=1 ;;
      *) _gtk_term_had_noglob=0 ;;
    esac
    set -f
  fi
  # shellcheck disable=SC2086
  bash __GTK_TERM_GIT_LS_LIB__ $GTK_TERM_LS_EXTRA "$@"
  local _gtk_term_status=$?
  if [ -z "${ZSH_VERSION:-}" ] && [ "${_gtk_term_had_noglob:-0}" = 0 ]; then
    set +f
  fi
  return $_gtk_term_status
}
