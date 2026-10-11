# Appended after the user's stock interactive startup; no widgets replaced.
[[ -o interactive && -o zle ]] || return
zmodload zsh/zle
zmodload zsh/system
autoload -Uz add-zsh-hook add-zle-hook-widget

typeset -ga _amux_pending=()
typeset -g _amux_active=0 _amux_last_partial=-1 _amux_prompt_generation=0 _amux_current_execution=''
typeset -g _amux_wakeup_pending=0 _amux_lost=0 _amux_submit_key=$'\e[amux-'"$AMUX_TERMINAL"'~'
exec {_amux_fd}<>"$AMUX_INTEGRATION/commands"

_amux_report() {
  emulate -L zsh
  [[ $1 == lost ]] && _amux_lost=1
  "$AMUX_EXECUTABLE" internal shell "$@" >/dev/null 2>/dev/null
}
_amux_precmd() {
  local _amux_last_status=$?
  emulate -L zsh
  _amux_active=0
  # SIGINT can skip the rest of an accepted source line, including its suffix.
  [[ -n $_amux_current_execution ]] && _amux_complete "$_amux_current_execution" "$_amux_last_status"
  _amux_report context "$$" "$PWD" || _amux_report lost "$$"
}
_amux_preexec() { emulate -L zsh; _amux_active=0; _amux_report busy "$$"; }
_amux_finish() { emulate -L zsh; _amux_active=0; _amux_report busy "$$"; }
_amux_redraw() {
  emulate -L zsh
  (( _amux_active )) || return
  local partial=0
  [[ -n $BUFFER || $CONTEXT != start || $PENDING -gt 0 || $KEYS_QUEUED_COUNT -gt 0 ]] && partial=1
  if (( partial != _amux_last_partial )); then
    _amux_last_partial=$partial
    _amux_report ready "$$" "$partial" "$_amux_prompt_generation"
  fi
  _amux_schedule
}
_amux_schedule() {
  emulate -L zsh
  local entry id queued generation epoch response
  while (( _amux_active && ${#_amux_pending} && !_amux_wakeup_pending )); do
    entry=$_amux_pending[1]
    id=${entry%%:*}; generation=${entry##*:}; queued=${${entry#*:}%%:*}
    if [[ $queued == 0 && $generation != $_amux_prompt_generation ]]; then
      _amux_pending[1]=()
      _amux_report rejected "$id" busy
      continue
    fi
    [[ -f "$AMUX_INTEGRATION/$id.zsh" ]] || { _amux_pending[1]=(); continue; }
    # Reject before a wake key can follow typeahead into a new foreground app.
    if [[ -n $BUFFER || $CONTEXT != start || $PENDING -gt 0 || $KEYS_QUEUED_COUNT -gt 0 ]]; then
      [[ $queued == 1 ]] && return
      _amux_pending[1]=()
      _amux_report rejected "$id" partial-input
      continue
    fi
    # Drain older daemon input, then sample its epoch before a fresh native check.
    if ! epoch=$("$AMUX_EXECUTABLE" internal shell epoch "$$" "$_amux_prompt_generation" "$id" 2>/dev/null); then
      _amux_active=0
      _amux_report lost "$$"
      return
    fi
    [[ $epoch == deferred ]] && return
    if [[ -n $BUFFER || $CONTEXT != start || $PENDING -gt 0 || $KEYS_QUEUED_COUNT -gt 0 ]]; then
      [[ $queued == 1 ]] && return
      _amux_pending[1]=()
      _amux_report rejected "$id" partial-input
      continue
    fi
    _amux_wakeup_pending=1
    if ! response=$("$AMUX_EXECUTABLE" internal shell wake "$$" "$_amux_prompt_generation" "$id" "$epoch" 2>/dev/null); then
      _amux_active=0
      _amux_wakeup_pending=0
      _amux_report lost "$$"
    elif [[ $response == deferred ]]; then
      _amux_wakeup_pending=0
      zle -R
    fi
    return
  done
}
_amux_complete() {
  emulate -L zsh
  local _amux_execution=$1 _amux_status=$2
  _amux_current_execution=''
  command rm -f -- "$AMUX_INTEGRATION/$_amux_execution.zsh"
  _amux_report completed "$_amux_execution" "$_amux_status" || _amux_report lost "$$"
  return $_amux_status
}
_amux_dispatch() {
  emulate -L zsh
  _amux_wakeup_pending=0
  local entry id queued generation source_path
  (( _amux_active )) || return
  while (( ${#_amux_pending} )); do
    entry=$_amux_pending[1]
    id=${entry%%:*}; generation=${entry##*:}
    queued=${${entry#*:}%%:*}
    if [[ $queued == 0 && $generation != $_amux_prompt_generation ]]; then
      _amux_pending[1]=()
      _amux_report rejected "$id" busy
      continue
    fi
    # The final check runs inside native ZLE, never in the daemon cache.
    if [[ $CONTEXT != start || -n $BUFFER || $PENDING -gt 0 || $KEYS_QUEUED_COUNT -gt 0 ]]; then
      [[ $queued == 1 ]] && return
      _amux_pending[1]=()
      _amux_report rejected "$id" partial-input
      continue
    fi
    _amux_pending[1]=()
    [[ -f "$AMUX_INTEGRATION/$id.zsh" ]] || continue
    source_path="$AMUX_INTEGRATION/$id.zsh"
    _amux_active=0
    BUFFER="builtin source ${(q)source_path}; _amux_complete ${(q)id} \$?"
    if ! _amux_report accepted "$id"; then
      BUFFER=""
      return
    fi
    _amux_current_execution=$id
    zle .accept-line
    return
  done
}
_amux_receive() {
  emulate -L zsh
  setopt extendedglob
  local entry
  if [[ -n $2 ]]; then
    zle -F "$1"
    _amux_active=0
    _amux_report lost "$$"
    return
  fi
  while IFS= read -r -t 0.01 -u $_amux_fd entry; do
    [[ $entry == [[:xdigit:]-]##:[01]:[0-9]## ]] && _amux_pending+=("$entry")
  done
  _amux_schedule
}
_amux_start() {
  emulate -L zsh
  (( _amux_lost )) && return
  (( ++_amux_prompt_generation ))
  _amux_active=1
  _amux_last_partial=-1
  _amux_redraw
  _amux_receive
}
add-zsh-hook precmd _amux_precmd
add-zsh-hook preexec _amux_preexec
add-zsh-hook zshexit _amux_finish
add-zle-hook-widget line-init _amux_start
add-zle-hook-widget line-finish _amux_finish
add-zle-hook-widget line-pre-redraw _amux_redraw
zle -N _amux_dispatch
for _amux_keymap in ${(f)"$(bindkey -l)"}; do
  [[ $_amux_keymap == .safe ]] || bindkey -M "$_amux_keymap" "$_amux_submit_key" _amux_dispatch
done
unset _amux_keymap
zle -N _amux_receive
zle -F -w $_amux_fd _amux_receive

# Retain a user's existing launcher and forward the explicit extension flag.
# A pre-existing alias is left intact rather than silently changing its meaning.
# Such aliases must forward -e themselves; command omp deliberately bypasses us.
if (( ${+functions[omp]} )); then
  functions -c omp _amux_user_omp
  functions[omp]='_amux_user_omp -e "$AMUX_OMP_EXTENSION" "$@"'
else
  functions[omp]='command omp -e "$AMUX_OMP_EXTENSION" "$@"'
fi
