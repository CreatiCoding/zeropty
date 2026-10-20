#!/bin/zsh
# Commands from a shell: the zsh function (shell/zeropty.zsh, no process per command), the zeropty program,
# and tmux, 200 calls each, two rounds alternating. Times in ms (p50 / p95).
# usage: bench/shell.zsh   (then the same from bash, with the builtin)
zmodload zsh/datetime
ROOT=${0:A:h:h}
export ZEROPTY_SOCK=$HOME/.zeropty/shellbench.sock ZEROPTY_HISTORY=2000
source $ROOT/shell/zeropty.zsh
B=$ROOT/dist/zeropty; T=(tmux -L shellbench)
$B kill-server 2>/dev/null; $T kill-server 2>/dev/null
cmd='seq 1 3000; exec sleep 1000'
$B new-session -d -s z -x 120 -y 40 $cmd; $T new-session -d -s z -x 120 -y 40 $cmd
sleep 1
stat() { local -a s=(${(no)@}); printf "%.3f / %.3f" ${s[$(( (${#s} + 1) / 2 ))]} ${s[$(( ${#s} * 95 / 100 ))]} }
run() {
  local label=$1 i s; shift; local -a t
  for i in {1..200}; do s=$EPOCHREALTIME; "$@" >/dev/null 2>&1; t+=($(( (EPOCHREALTIME - s) * 1000 ))); done
  printf '%-36s %s\n' $label "$(stat $t)"
}
for round in 1 2; do
  for what in has-session send-keys capture; do
    case $what in
      (has-session) args=(has-session -t '=z') ;;
      (send-keys) args=(send-keys -t '=z:' -l x) ;;
      (capture) args=(capture-pane -p -S -2000 -t '=z:') ;;
    esac
    run "$what: zsh function" zeropty $args
    run "$what: zeropty program" $B $args
    run "$what: tmux" $T $args
  done
done
$B kill-server; $T kill-server

# bash: the builtin (dist/zeropty_bash.dylib), the program, tmux; bash 3.2 has no microsecond clock, so
# the mean of 200 calls: one bash loop timed whole, less the same loop doing nothing.
$B new-session -d -s z -x 120 -y 40 $cmd; $T new-session -d -s z -x 120 -y 40 $cmd
sleep 1
bash_mean() {
  local loop="enable -f $ROOT/dist/zeropty_bash.dylib zeropty; B=$B; for i in {1..200}; do $1 >/dev/null 2>&1; done" s e s0 e0
  s=$EPOCHREALTIME; /bin/bash -c "$loop"; e=$EPOCHREALTIME
  s0=$EPOCHREALTIME; /bin/bash -c "${loop/$1/:}"; e0=$EPOCHREALTIME
  printf '%.3f' $(( ((e - s) - (e0 - s0)) * 1000 / 200 ))
}
for round in 1 2; do
  printf '%-36s %s\n' "has-session (bash): builtin" "$(bash_mean "zeropty has-session -t =z") mean"
  printf '%-36s %s\n' "has-session (bash): zeropty program" "$(bash_mean "\$B has-session -t =z") mean"
  printf '%-36s %s\n' "has-session (bash): tmux" "$(bash_mean "tmux -L shellbench has-session -t =z") mean"
  printf '%-36s %s\n' "send-keys (bash): builtin" "$(bash_mean "zeropty send-keys -t =z: -l x") mean"
  printf '%-36s %s\n' "send-keys (bash): tmux" "$(bash_mean "tmux -L shellbench send-keys -t =z: -l x") mean"
  printf '%-36s %s\n' "capture (bash): builtin" "$(bash_mean "zeropty capture-pane -p -S -2000 -t =z:") mean"
  printf '%-36s %s\n' "capture (bash): tmux" "$(bash_mean "tmux -L shellbench capture-pane -p -S -2000 -t =z:") mean"
done
$B kill-server; $T kill-server
