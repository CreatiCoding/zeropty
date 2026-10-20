#!/bin/bash
# Every load test, tmux and zeropty at 200 and 400 sessions, then zeropty at 1000 (tmux cannot: one PTY per
# session, and macOS allows at most 511 by default, 999 at most). About 15 minutes.
# usage: bench/suite.sh > bench/results.txt
B=$(dirname "$0")/load.sh
for mode in idle active claude trace; do
  for kind in tmux zeropty; do
    for n in 200 400; do bash "$B" $kind $n $mode 2>&1 | grep -E "footprint=|stop|fail"; done
  done
done
bash "$B" zeropty 1000 idle 2>&1 | grep -E "footprint=|stop|fail"
bash "$B" zeropty 1000 trace 2>&1 | grep -E "footprint=|stop|fail"
