#!/bin/bash
# One load test: N sessions on zeropty or tmux, then the server's footprint and its CPU averaged over ~20 s.
# usage: bench/load.sh <zeropty|tmux> <N> <idle|active|claude|trace>
#   idle   prints a recorded claude screen once, then sleeps
#   active redraws 31 lines once a second (a progress screen)
#   claude prints a whole recorded claude session (15 KB) every 2 s
#   trace  replays a real claude session (Korean answer, tests/streams/claude2.trace) at its own pace
set -u
KIND=$1; N=$2; MODE=$3
ROOT=$(cd "$(dirname "$0")/.." && pwd)
CUR=$(ps -U "$(id -u)" | wc -l); MAX=$(sysctl -n kern.maxprocperuid)
if [ $((CUR + N + 300)) -gt "$MAX" ]; then echo "stop: $CUR processes + $N + 300 > kern.maxprocperuid $MAX"; exit 1; fi
RAW=$ROOT/tests/streams/claude1.raw
case $MODE in
  idle) CMD="cat $RAW; exec sleep 100000" ;;
  trace) CMD="exec perl $ROOT/tests/replay_trace.pl $ROOT/tests/streams/claude2.trace" ;;  # phase added per session below
  claude) CMD="exec perl -e 'open(F,\"<$RAW\"); local \$/; \$d=<F>; \$|=1; while(1){ print \$d; sleep 2 }'" ;;
  active) CMD="exec perl -e '\$|=1; for(\$i=0;;\$i++){ printf \"\\033[H\\033[2J\\033[1;32mtick %d\\033[0m \\033[38;5;208mstatus\\033[0m\\n\", \$i; for(1..30){print \"line \$_ \" . (\"x\" x 80) . \"\\n\"} sleep 1 }'" ;;
  *) echo "unknown mode $MODE"; exit 1 ;;
esac
# trace: session i starts i * 0.618... of the way through the recording (mod 1), the same every run, so
# runs and the two servers replay the same mix (random starting points moved the footprint by 5-8%).
phase() { [ "$MODE" = trace ] && awk -v i="$1" 'BEGIN { x = i * 0.6180339887; printf " %.6f", x - int(x) }'; }
if [ "$KIND" = zeropty ]; then
  export ZEROPTY_SOCK=$HOME/.zeropty/bench.sock ZEROPTY_HISTORY=2000; C="$ROOT/dist/zeropty"
  for i in $(seq 1 "$N"); do $C new-session -d -s "s$i" -x 220 -y 80 "$CMD$(phase "$i")" || { echo "fail $i"; break; }; done
  PID=$(pgrep -f "zeropty serve" | head -1)
else
  C="tmux -L bench"; $C kill-server 2>/dev/null
  for i in $(seq 1 "$N"); do $C new-session -d -s "s$i" -x 220 -y 80 "$CMD$(phase "$i")" || { echo "fail $i"; break; }; done
  PID=$(pgrep -f "tmux -L bench" | head -1)
fi
sleep 15
cpu() { ps -o cputime= -p "$PID" | awk -F'[:.]' '{ if (NF==4) print ($1*3600+$2*60+$3)*100+$4; else print ($1*60+$2)*100+$3 }'; }
fp() { footprint "$PID" 2>/dev/null | awk '/phys_footprint:/{v=$2; if($3=="MB")v*=1024; if($3=="GB")v*=1048576; print int(v); exit}'; }
# The footprint moves with what the sessions are doing at the moment: averaged over ten samples.
now() { perl -MTime::HiRes=time -e 'printf "%.3f", time'; }
c0=$(cpu); t0=$(now); FP=0
for _ in 1 2 3 4 5 6 7 8 9 10; do sleep 2; FP=$((FP + $(fp))); done
c1=$(cpu); t1=$(now); FP=$((FP / 10))
printf "%-4s N=%-5s %-6s footprint=%8d KB  CPU=%6.2f%%  PTY=%s\n" "$KIND" "$N" "$MODE" "$FP" "$(echo "($c1-$c0)/($t1-$t0)" | bc -l)" "$(ls /dev/ttys* | wc -l | tr -d ' ')"
$C kill-server 2>/dev/null; sleep 2
