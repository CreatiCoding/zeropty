# zeropty as a zsh function: commands go to the server over its socket from inside the shell
# (zsh/net/socket, shipped with zsh), so none starts a process. Source it from .zshrc:
#   source /path/to/zeropty/shell/zeropty.zsh
# The program runs instead for what needs it: attach, serve, restart-server, plugin-run, -V, help, and any command while
# no server is listening (new-session starts one). ZEROPTY_BIN overrides which program.

zmodload zsh/net/socket zsh/system || return
typeset -g _ZEROPTY_BIN=${${(%):-%x}:A:h:h}/dist/zeropty

zeropty() {
  emulate -L zsh
  # Lengths and slices in bytes: the server counts its reply in bytes.
  setopt no_multibyte
  local bin=${ZEROPTY_BIN:-$_ZEROPTY_BIN} sock=${ZEROPTY_SOCK:-$HOME/.zeropty/mux.sock}
  case $1 in
    (''|serve|restart-server|plugin-run|attach-session|attach|a|-V|--version) command $bin "$@"; return ;;
  esac
  # Help is the program's own (the server does not know the word).
  [[ $1 == (-h|--help|help) ]] && { command $bin "$@"; return }
  zsocket $sock 2>/dev/null || { command $bin "$@"; return }
  local fd=$REPLY a i head buf chunk
  local -a q
  # Each argument and the directory as a JSON string, with no subshell.
  for a in "$@" $PWD; do
    a=${a//\\/\\\\}
    a=${a//\"/\\\"}
    if [[ $a == *[[:cntrl:]]* ]]; then
      for i in {1..31}; do
        [[ $a == *${(#)i}* ]] && a=${a//${(#)i}/\\u00${(l:2::0:)$(( [##16] i ))}}
      done
    fi
    q+=(\"$a\")
  done
  print -rn -u $fd -- "{\"argv\":[${(j:,:)q[1,-2]}],\"cwd\":${q[-1]},\"raw\":true}"$'\n'
  # "<code> <out bytes> <err bytes>", then out and err as they are.
  if ! read -r -u $fd head; then
    exec {fd}>&-
    print -u2 "zeropty: lost server"
    return 1
  fi
  local -a h=(${=head})
  while (( ${#buf} < h[2] + h[3] )); do
    sysread -i $fd -s $(( h[2] + h[3] - ${#buf} )) chunk || break
    buf+=$chunk
  done
  exec {fd}>&-
  print -rn -- ${buf[1,h[2]]}
  print -rn -u2 -- ${buf[h[2]+1,-1]}
  return $h[1]
}
