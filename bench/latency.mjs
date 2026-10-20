// Response times as a program sees them, from Node: tmux through execFile (a tmux process per command),
// tmux through control mode (`tmux -C`: one client kept open, commands on its stdin, the fastest way to
// drive tmux), zeropty through its socket (client/zeropty.mjs: async, and zeroptySync), and zeropty's engine inside this process
// (client/embedded.mjs). Both servers run N busy sessions replaying a real
// claude session, and every measurement alternates between them, so both see the same machine state
// (measuring one after the other drifted by 30% and more).
// usage: node bench/latency.mjs [N=200]
import { execFile, spawn } from 'node:child_process'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { zeropty, zeroptySync as muxSync } from '../client/zeropty.mjs'
import { zeroptySync, open } from '../client/embedded.mjs'

const N = Number(process.argv[2] ?? 200)
// The embedded engine is run by this process's event loop (polled, the default); ZEROPTY_THREAD=1 gives it a thread.
open({ polled: !process.env.ZEROPTY_THREAD, history: 2000 })
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const sock = path.join(os.homedir(), '.zeropty', 'latency.sock')
process.env.ZEROPTY_HISTORY = '2000'
const trace = `exec perl ${root}/tests/replay_trace.pl ${root}/tests/streams/claude2.trace`
const tmux = (...a) => new Promise((r) => execFile('tmux', ['-L', 'latency', ...a], { maxBuffer: 16 << 20 }, (e, out) => r({ ok: !e, out: String(out) })))
const mux = (...a) => zeropty(a, { sock })
// Control mode: replies come between %begin and %end (or %error); a block arriving with no command
// waiting (the attach's own) is dropped.
const ctl = { p: null, queue: [], buf: '', block: null }
const tmuxC = (...a) => new Promise((resolve) => {
  ctl.queue.push(resolve)
  ctl.p.stdin.write(a.map((x) => (/^[\w=:./-]+$/.test(x) ? x : `'${x.replace(/'/g, `'\\''`)}'`)).join(' ') + '\n')
})
const startControl = () => {
  ctl.p = spawn('tmux', ['-L', 'latency', '-C', 'attach', '-t', '=quiet'])
  ctl.p.stdout.on('data', (d) => {
    ctl.buf += d
    for (let i; (i = ctl.buf.indexOf('\n')) >= 0; ) {
      const line = ctl.buf.slice(0, i)
      ctl.buf = ctl.buf.slice(i + 1)
      if (ctl.block) {
        if (line.startsWith('%end') || line.startsWith('%error')) {
          ctl.queue.shift()?.({ ok: line.startsWith('%end'), out: ctl.block.join('\n') + '\n' })
          ctl.block = null
        } else ctl.block.push(line)
      } else if (line.startsWith('%begin')) ctl.block = []
    }
  })
}
const embedded = async (...a) => zeroptySync(a)
const muxSyncSide = async (...a) => muxSync(a, { sock })
const sides = { tmux, zeropty: mux, 'zeropty embedded': embedded }
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

for (const f of Object.values(sides)) await f('kill-server')
await sleep(500)
for (const f of Object.values(sides)) {
  for (let i = 0; i < N; i++) await f('new-session', '-d', '-s', `b${i}`, '-x', '220', '-y', '80', trace)
  await f('new-session', '-d', '-s', 'quiet', '-x', '220', '-y', '80', `cat ${root}/tests/streams/claude1.raw; exec sleep 1000`)
  await f('new-session', '-d', '-s', 'echo', '-x', '220', '-y', '80', 'exec cat')
}
await sleep(15000)
startControl()
await sleep(1000)
ctl.queue.length = 0
sides['tmux -C'] = tmuxC
// The same server as `zeropty`, through the native synchronous client (client/zeropty.mjs zeroptySync).
sides['zeropty sync'] = muxSyncSide
if (!(await tmuxC('display-message', '-p', 'ready')).out.includes('ready')) throw new Error('tmux control mode did not answer')

const ms = async (fn) => { const t = performance.now(); await fn(); return performance.now() - t }
// Keys sent, then the screen read until the echo shows: what pressing a dialog button and checking costs.
const echo = async (f) => {
  const tok = `k${Math.floor(performance.now() * 1000) % 1e9}`
  const t = performance.now()
  await f('send-keys', '-t', '=echo:', '-l', tok)
  await f('send-keys', '-t', '=echo:', 'Enter')
  while (!(await f('capture-pane', '-p', '-t', '=echo:')).out.includes(tok)) if (performance.now() - t > 3000) return NaN
  return performance.now() - t
}
const tests = {
  'capture-pane, busy session        ': (f) => ms(() => f('capture-pane', '-p', '-t', '=b0:')),
  'capture-pane -e -S -200, quiet one': (f) => ms(() => f('capture-pane', '-p', '-e', '-S', '-200', '-t', '=quiet:')),
  'send-keys                         ': (f) => ms(() => f('send-keys', '-t', '=echo:', '-l', 'x')),
  'keys -> on screen                 ': echo,
}
const pct = (v, p) => [...v].sort((a, b) => a - b)[Math.min(v.length - 1, Math.floor(v.length * p))]
console.log(`N=${N} busy sessions per server, alternating, 40 samples each; p50 / p95 in ms; ratios are p50`)
for (const [name, fn] of Object.entries(tests)) {
  const v = Object.fromEntries(Object.keys(sides).map((s) => [s, []]))
  for (let i = 0; i < 40; i++) for (const [s, f] of Object.entries(sides)) { await sleep(30); v[s].push(await fn(f)) }
  const a50 = pct(v.zeropty, 0.5)
  const cell = (s) => `${s} ${pct(v[s], 0.5).toFixed(2)} / ${pct(v[s], 0.95).toFixed(2)}`
  const e50 = pct(v['zeropty embedded'], 0.5)
  console.log(`  ${name} ${cell('tmux')} | ${cell('tmux -C')} | ${cell('zeropty')} (tmux ${(pct(v.tmux, 0.5) / a50).toFixed(1)}x, -C ${(pct(v['tmux -C'], 0.5) / a50).toFixed(1)}x) | ${cell('zeropty sync')} (-C ${(pct(v['tmux -C'], 0.5) / pct(v['zeropty sync'], 0.5)).toFixed(1)}x) | ${cell('zeropty embedded')} (tmux ${(pct(v.tmux, 0.5) / e50).toFixed(0)}x, -C ${(pct(v['tmux -C'], 0.5) / e50).toFixed(0)}x)`)
}
ctl.p.kill()
await tmux('kill-server')
await mux('kill-server')
zeroptySync(['kill-server'])
