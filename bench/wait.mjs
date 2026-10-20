// Waiting for sessions to finish: polling capture-pane against wait-pane.
// N sessions replay a real claude session (tests/streams/claude2.trace) for 5-25 s each, then print
// "TASK-DONE <ms>". The host learns when each is done, either by capturing every session every P ms
// (what a tmux host has to do) or by one wait-pane per session. Reports the server's and the host's CPU
// over the run and how late the host noticed (from the moment the line was printed).
// usage: node bench/wait.mjs [N=200] [rounds=2]
import path from 'node:path'
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { zeropty } from '../client/zeropty.mjs'

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..')
const N = Number(process.argv[2] || 200), ROUNDS = Number(process.argv[3] || 2)
const sock = process.env.ZEROPTY_SOCK = path.join(process.env.HOME, '.zeropty', 'waitbench.sock')
const bin = path.join(root, 'dist', 'zeropty')
const run = (...a) => { try { return execFileSync(bin, a, { encoding: 'utf8' }) } catch { return '' } }

function cpuMs(pid) {
  const t = execFileSync('ps', ['-o', 'cputime=', '-p', String(pid)], { encoding: 'utf8' }).trim()
  const [m, s] = t.split(':')
  return (Number(m) * 60 + Number(s)) * 1000
}

function start() {
  run('kill-server')
  for (let i = 0; i < N; i++) {
    const secs = 5 + ((i * 7919) % 2000) / 100
    const phase = ((i * 0.6180339887) % 1).toFixed(6)
    run('new-session', '-d', '-s', `s${i}`, '-x', '220', '-y', '80',
      `perl ${root}/tests/replay_trace.pl ${root}/tests/streams/claude2.trace ${phase} & P=$!; sleep ${secs}; kill $P; ` +
      `perl -MTime::HiRes=time -e 'printf "\\nTASK-DONE %d\\n", time*1000'; exec sleep 1000`)
  }
  return Number(execFileSync('pgrep', ['-f', `zeropty serve`], { encoding: 'utf8' }).trim().split('\n')[0])
}

const doneAt = (screen) => Number(/TASK-DONE (\d+)/.exec(screen)?.[1])

async function poll(every) {
  const late = []
  const left = new Set(Array.from({ length: N }, (_, i) => i))
  while (left.size) {
    const t = Date.now()
    await Promise.all([...left].map(async (i) => {
      const r = await zeropty(['capture-pane', '-p', '-t', `=s${i}:`])
      const at = doneAt(r.out)
      if (at) { late.push(Date.now() - at); left.delete(i) }
    }))
    await new Promise((r) => setTimeout(r, Math.max(0, every - (Date.now() - t))))
  }
  return late
}

async function waitOne(i) {
  const r = await zeropty(['wait-pane', '-t', `=s${i}`, '-m', 'TASK-DONE'])
  const noticed = Date.now()
  if (!r.ok) throw new Error(r.err)
  return noticed - doneAt((await zeropty(['capture-pane', '-p', '-t', `=s${i}:`])).out)
}
const waitAll = () => Promise.all(Array.from({ length: N }, (_, i) => waitOne(i)))

const pct = (a, p) => { const s = [...a].sort((x, y) => x - y); return s[Math.min(s.length - 1, Math.floor(s.length * p))] }
for (let round = 1; round <= ROUNDS; round++) {
  const modes = { 'poll 500 ms': () => poll(500), 'wait-pane': waitAll, 'poll 200 ms': () => poll(200) }
  const only = process.env.ONLY ? process.env.ONLY.split(',') : Object.keys(modes)
  for (const [label, how] of Object.entries(modes).filter(([l]) => only.includes(l))) {
    const pid = start()
    const c0 = cpuMs(pid), h0 = process.cpuUsage(), t0 = Date.now()
    const late = await how()
    const secs = (Date.now() - t0) / 1000
    const server = (cpuMs(pid) - c0) / 1000, h = process.cpuUsage(h0), host = (h.user + h.system) / 1e6
    console.log(`N=${N} ${label.padEnd(12)} ${secs.toFixed(1)} s  server CPU ${server.toFixed(2)} s (${(server / secs * 100).toFixed(1)}%)  host CPU ${host.toFixed(2)} s (${(host / secs * 100).toFixed(1)}%)  noticed late p50 ${pct(late, 0.5)} ms p95 ${pct(late, 0.95)} ms`)
  }
}
run('kill-server')
process.exit(0)
