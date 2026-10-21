#!/bin/zsh
# Finding text across sessions: 1000 idle claude screens, 3 with a marker, found four ways (grep-panes,
# Node capturing each, the zsh function capturing each, the CLI capturing each). Server footprint after each.
# usage: bench/grep.zsh [N=1000]
zmodload zsh/datetime
ROOT=${0:A:h:h}; B=$ROOT/dist/zeropty; RAW=$ROOT/tests/streams/claude1.raw
export ZEROPTY_SOCK=$HOME/.zeropty/grep.sock ZEROPTY_HISTORY=2000 ZEROPTY_BIN=$B
source $ROOT/shell/zeropty.zsh
$B kill-server 2>/dev/null
N=${1:-1000}
for i in {1..$N}; do
  if (( i % 333 == 0 )); then c="cat $RAW; printf '\\n MARKER-ASK %d\\n' $i; exec sleep 100000"; else c="cat $RAW; exec sleep 100000"; fi
  zeropty new-session -d -s s$i -x 220 -y 80 "$c" || { echo fail $i; break }
done
PID=$(lsof -t $ZEROPTY_SOCK | head -1)  # this socket's server, not another one running
fp() { footprint $PID 2>/dev/null | awk '/phys_footprint:/{print $2,$3; exit}' }
sleep 14; echo "idle (deep frozen): $(fp)"
t() { local s=$EPOCHREALTIME; "$@"; printf '  %.0f ms  footprint %s\n' $(( (EPOCHREALTIME - s) * 1000 )) "$(fp)" }
names=(${(f)"$(zeropty list-sessions | cut -d: -f1)"})
cli() { for n in $names; do $B capture-pane -p -t "=$n:" | grep -q MARKER-ASK && echo $n; done | wc -l | tr -d ' ' }
fn() { for n in $names; do zeropty capture-pane -p -t "=$n:" | grep -q MARKER-ASK && echo $n; done | wc -l | tr -d ' ' }
fn2() { local n o c=0; for n in $names; do o=$(zeropty capture-pane -p -t "=$n:"); [[ $o == *MARKER-ASK* ]] && ((c++)); done; echo $c }
node_() { node -e "
import('$ROOT/client/zeropty.mjs').then(async ({ zeropty }) => {
  const names = (await zeropty(['list-sessions'])).out.trim().split('\n').map(l => l.split(':')[0])
  const r = await Promise.all(names.map(n => zeropty(['capture-pane', '-p', '-t', '=' + n + ':'])))
  console.log(r.filter(x => x.out.includes('MARKER-ASK')).length); process.exit(0) })" }
srv() { zeropty grep-panes MARKER-ASK | wc -l | tr -d ' ' }
for round in 1 2; do
  echo "round $round"
  echo -n "server grep-panes:     "; t srv; sleep 14
  echo -n "node pipelined:        "; t node_; sleep 14
  echo -n "zsh function loop:     "; t fn2; sleep 14
  echo -n "CLI loop:              "; t cli; sleep 14
done
$B kill-server
