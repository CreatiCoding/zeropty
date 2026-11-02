// An agent host in a few lines, through the zeropty server: node examples/node/quickstart.mjs
// Needs ./build.sh first (dist/zeropty). Uses a server of its own on a temporary socket, so it leaves
// your sessions alone, and ends it at the end.
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { zeropty } from '../../client/zeropty.mjs'

const sock = path.join(os.tmpdir(), `zeropty-quickstart-${process.pid}.sock`)
const zp = (args, opts) => zeropty(args, { sock, ...opts })

// Start a session; with no server on the socket, new-session starts one (as tmux does).
// A real host runs an agent here, or any program.
let r = await zp(['new-session', '-d', '-s', 'job', '-x', '80', '-y', '20', 'sh'])
if (!r.ok) throw new Error(r.err)

// Give it work.
await zp(['send-keys', '-t', '=job:', '-l', 'for i in 1 2 3; do echo step $i; sleep 0.2; done; echo "finished: $?"'])
await zp(['send-keys', '-t', '=job:', 'Enter'])

// Wait until it is done: the text shows up (the typed line holds `$?`, not `0`) and the output
// stays quiet for 300 ms. No polling: the server checks as output comes in.
const t0 = performance.now()
r = await zp(['wait-pane', '-t', '=job', '-m', 'finished: 0', '-q', '300', '-T', '10000'])
console.log(`wait-pane: ${r.ok ? 'done' : r.err.trim()} after ${Math.round(performance.now() - t0)} ms`)

// Read the result.
const { out } = await zp(['capture-pane', '-p', '-t', '=job:'])
console.log(out.trimEnd())

// Which sessions show a given text, across all of them at once.
console.log((await zp(['grep-panes', 'finished: 0'])).out.trimEnd())

await zp(['kill-server'])
fs.rmSync(sock, { force: true })
