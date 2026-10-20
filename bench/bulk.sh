#!/bin/bash
# Bulk output: a program prints 5.4 MB (60,000 colored lines) as fast as it can, in one session.
# usage: bench/bulk.sh <zeropty|tmux> [runs]
#   writer  how long the program took to print it all (it waits whenever the pipe is full)
#   screen  from the program's first write until capture-pane shows the last line (polled back to back;
#           each poll starts a process, ~2.5 ms for zeropty and ~4.5 ms for tmux, so that much +-)
set -u
KIND=$1; RUNS=${2:-3}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DIR=$(mktemp -d "${TMPDIR:-/tmp}/zeropty-bulk.XXXXXX")
perl -e 'for $i (0..59999) { printf "\033[%dmline %05d %s\033[0m\n", 31 + $i % 7, $i, "x" x 64 }' > "$DIR/big.txt"
if [ "$KIND" = zeropty ]; then
  export ZEROPTY_SOCK=$HOME/.zeropty/bulk.sock ZEROPTY_HISTORY=2000; C="$ROOT/dist/zeropty"
else
  C="tmux -L bulk"
fi
$C kill-server 2>/dev/null
for r in $(seq 1 "$RUNS"); do
  $C new-session -d -s "w$r" -x 220 -y 80 "perl -MTime::HiRes=time -e 'open(F,\"<$DIR/big.txt\"); \$|=1; \$s=time; open(S,\">$DIR/s$r\"); printf S \"%.6f\", \$s; close S; while(read(F,\$b,65536)){print \$b} open(O,\">$DIR/w$r\"); printf O \"%.1f\", (time-\$s)*1000'; exec sleep 100"
  # Polled from perl, so the time is taken the moment the line shows (not after starting a process).
  t1=$(perl -MTime::HiRes=time,sleep -e 'until (`@ARGV 2>/dev/null` =~ /line 59999/) { sleep 0.002 } printf "%.6f", time' $C capture-pane -p -t "=w$r:")
  until [ -s "$DIR/w$r" ]; do sleep 0.01; done
  echo "$KIND run $r: writer $(cat "$DIR/w$r") ms, screen $(perl -e "printf '%.0f', ($t1-$(cat "$DIR/s$r"))*1000") ms"
done
$C kill-server
