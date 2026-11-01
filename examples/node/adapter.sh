#!/bin/sh
# A Node program sees a terminal in a session through the runtime adapter, with no shim and its binary
# untouched: sh examples/node/adapter.sh
# Needs ./build.sh first and node. Runs a server of its own on a temporary socket and ends it at the end.
set -e
cd "$(dirname "$0")/../.."
ZP=$PWD/dist/zeropty
export ZEROPTY_SOCK="${TMPDIR:-/tmp}/zeropty-adapter.sock"
ADAPTER=$PWD/examples/node/tty-adapter.cjs

# Reports what it sees, again on each resize; raw mode makes each key arrive as it is typed.
PROG='const s = () => console.log(`tty ${process.stdout.isTTY} ${process.stdout.columns}x${process.stdout.rows}`);
s(); process.stdout.on("resize", s);
process.stdin.setRawMode(true);
process.stdin.on("data", (d) => { console.log(`key ${JSON.stringify(String(d))}`); if (String(d) === "q") process.exit(0); });'

"$ZP" new-session -d -s app -x 60 -y 8 "NODE_OPTIONS='--require $ADAPTER' exec node -e '$PROG'"
"$ZP" wait-pane -t '=app:' -m 'tty true 60x8' -T 10000
"$ZP" resize-window -t '=app:' -x 50 -y 9
"$ZP" wait-pane -t '=app:' -m 'tty true 50x9' -T 10000
"$ZP" send-keys -t '=app:' -l 'x'                # raw: no Enter needed
"$ZP" wait-pane -t '=app:' -m 'key "x"' -T 10000
"$ZP" capture-pane -p -t '=app:'
"$ZP" kill-server
