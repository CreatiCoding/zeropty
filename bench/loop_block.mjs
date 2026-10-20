// How long the embedded engine holds Node's event loop, with the engine polled (run by this loop) or on
// its own thread: N sessions replaying real claude work, then one printing 5.4 MB at once. Reports the
// event loop's delay (perf_hooks, 1 ms resolution) over each phase.
// usage: node bench/loop_block.mjs [N=200] [polled|thread]
import { monitorEventLoopDelay } from 'node:perf_hooks'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { open, zeroptySync } from '../client/embedded.mjs'

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..')
const N = Number(process.argv[2] || 200), polled = (process.argv[3] || 'polled') === 'polled'
open({ polled, history: 2000 })
for (let i = 0; i < N; i++) {
  const phase = ((i * 0.6180339887) % 1).toFixed(6)
  zeroptySync(['new-session', '-d', '-s', `b${i}`, '-x', '220', '-y', '80', `exec perl ${root}/tests/replay_trace.pl ${root}/tests/streams/claude2.trace ${phase}`])
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const report = (label, h) => console.log(`${polled ? 'polled' : 'thread'} N=${N} ${label.padEnd(26)} loop delay p50 ${(h.percentile(50) / 1e6).toFixed(1)} ms  p99 ${(h.percentile(99) / 1e6).toFixed(1)} ms  max ${(h.max / 1e6).toFixed(1)} ms`)
await sleep(3000)
let h = monitorEventLoopDelay({ resolution: 1 }); h.enable(); await sleep(10000); h.disable(); report('busy sessions', h)
const big = `perl -e 'for $i (0..59999) { printf "\\033[%dmline %05d %s\\033[0m\\n", 31 + $i % 7, $i, "x" x 64 }'; exec sleep 100`
h = monitorEventLoopDelay({ resolution: 1 }); h.enable()
zeroptySync(['new-session', '-d', '-s', 'bulk', '-x', '220', '-y', '80', big])
await sleep(3000); h.disable(); report('+ 5.4 MB printed at once', h)
console.log('  bulk on screen:', zeroptySync(['capture-pane', '-p', '-t', '=bulk:', '-S', '-5']).out.includes('line 59999'))
zeroptySync(['kill-server'])
process.exit(0)
