#!/bin/sh
# zeropty from a shell script, the way tmux is used: sh examples/shell/quickstart.sh
# Needs ./build.sh first. Runs a server of its own on a temporary socket and ends it at the end.
set -e
cd "$(dirname "$0")/../.."
ZP=$PWD/dist/zeropty
# A socket of its own, so your sessions are left alone (a stale one is replaced on the next run).
export ZEROPTY_SOCK="${TMPDIR:-/tmp}/zeropty-quickstart.sock"

"$ZP" new-session -d -s build -x 80 -y 6 sh           # no server yet: this starts one
"$ZP" send-keys -t '=build:' -l 'ls / | head -3; echo "exit: $?"'
"$ZP" send-keys -t '=build:' Enter
"$ZP" wait-pane -t '=build:' -m 'exit: 0' -T 10000      # returns as soon as the line shows, no polling
"$ZP" capture-pane -p -t '=build:'
"$ZP" list-sessions
"$ZP" kill-server
