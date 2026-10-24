// zeropty from Node without starting a process per command: one connection to the server's socket is kept
// open and commands go over it one line each, replies coming back in order. Same argv as the CLI, so
// `tmux(...args)` call sites become `zeropty(args)`.
import fs from 'node:fs'
import net from 'node:net'
import os from 'node:os'
import path from 'node:path'
import { execFile, spawnSync } from 'node:child_process'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
/** This client's version (Semantic Versioning, docs/VERSIONING.md); scripts/release.mjs sets it. */
export const VERSION = '0.1.7'
export const sockPath = () => process.env.ZEROPTY_SOCK || path.join(os.homedir(), '.zeropty', 'mux.sock')
export const cliPath = () => process.env.ZEROPTY_BIN || path.join(here, '..', 'dist', 'zeropty')

/**
 * @typedef {{ ok: boolean, code: number, out: string, err: string }} Result
 * A command's result: `ok` when `code` is 0, as the CLI's exit status and output.
 */

/** A reply line from the server as a Result. */
function parseReply(line) {
  try {
    const r = JSON.parse(line)
    return { ok: r.code === 0, code: r.code, out: r.out, err: r.err }
  } catch (e) {
    return failed(`zeropty: bad reply: ${e.message}\n`)
  }
}

/** A failure that never reached the server (or whose reply was lost). */
const failed = (err) => ({ ok: false, code: 1, out: '', err })

/** What the program does itself rather than the server: these run the CLI. */
const programOnly = new Set(['restart-server', 'serve', 'attach-session', 'attach', 'a', '-V', '--version'])

/** Open connections by socket path: { socket, ready, waiting: [{ resolve, timer }], buf } */
const conns = new Map()

function connection(sock) {
  let c = conns.get(sock)
  if (c) return c
  if (foreign(sock)) return null
  const socket = net.createConnection(sock)
  c = { socket, waiting: [], chunks: [] }
  c.ready = new Promise((resolve) => {
    socket.once('connect', () => resolve(true))
    socket.once('error', () => resolve(false))
  })
  const drop = (err) => {
    if (conns.get(sock) === c) conns.delete(sock)
    for (const w of c.waiting.splice(0)) { clearTimeout(w.timer); w.resolve(failed(err)) }
    socket.destroy()
  }
  socket.on('data', (d) => {
    // Replies are one JSON object per line, in the order the commands went out.
    let from = 0
    for (let i = d.indexOf(10); i >= 0; i = d.indexOf(10, from)) {
      c.chunks.push(d.subarray(from, i))
      const line = Buffer.concat(c.chunks).toString('utf8')
      c.chunks = []
      from = i + 1
      const w = c.waiting.shift()
      if (!w) continue
      clearTimeout(w.timer)
      w.resolve(parseReply(line))
    }
    if (from < d.length) c.chunks.push(d.subarray(from))
    if (!c.waiting.length) socket.unref()
  })
  socket.on('error', () => drop('zeropty: lost server\n'))
  socket.on('close', () => drop('zeropty: lost server\n'))
  socket.unref()
  conns.set(sock, c)
  return c
}

/** The socket's owner is another user (not root): whoever listens there first would get every command,
 *  keys included, and answer what it likes. The callers leave it to the CLI, which refuses it and says why. */
const foreign = (sock) => {
  try {
    const { uid } = fs.statSync(sock)
    return uid !== 0 && uid !== process.geteuid()
  } catch {
    return false
  }
}

/**
 * wait-pane on a connection of its own, closed after: it holds its connection until answered, which
 * would hold up every command sent after it on the shared one. No timeout unless one is given (-T is
 * the server's).
 */
function waitPane(argv, cwd, sock, timeout) {
  if (foreign(sock)) return Promise.resolve(failed(`zeropty: the server on ${sock} runs as another user\n`))
  return new Promise((resolve) => {
    const socket = net.createConnection(sock)
    let buf = '', done = false
    const finish = (r) => { if (done) return; done = true; clearTimeout(timer); socket.destroy(); resolve(r) }
    const timer = timeout ? setTimeout(() => finish(failed('zeropty: timeout\n')), timeout) : null
    socket.on('connect', () => socket.write(JSON.stringify({ argv, cwd }) + '\n'))
    socket.on('data', (d) => {
      buf += d
      const i = buf.indexOf('\n')
      if (i < 0) return
      finish(parseReply(buf.slice(0, i)))
    })
    socket.on('error', () => finish(failed(`no server running on ${sock}\n`)))
    socket.on('close', () => finish(failed('zeropty: lost server\n')))
  })
}

/**
 * Runs one command; resolves { ok, code, out, err } like the CLI's exit status and output.
 * @param {string[]} argv the command line, its name first
 * @param {{ cwd?: string, sock?: string, timeout?: number }} [options]
 * @returns {Promise<Result>}
 */
export async function zeropty(argv, { cwd = process.cwd(), sock = sockPath(), timeout } = {}) {
  if (argv[0] === 'wait-pane') return waitPane(argv, cwd, sock, timeout)
  timeout ??= 15_000
  const c = programOnly.has(argv[0]) ? null : connection(sock)
  if (!c || !(await c.ready)) {
    // No server yet (or a stale socket): the CLI starts one on new-session, as tmux does.
    conns.delete(sock)
    return new Promise((resolve) =>
      execFile(cliPath(), argv, { cwd, timeout, maxBuffer: 64 * 1024 * 1024, env: { ...process.env, ZEROPTY_SOCK: sock } }, (err, out, errOut) =>
        resolve({ ok: !err, code: err ? (typeof err.code === 'number' ? err.code : 1) : 0, out: String(out), err: String(errOut) })))
  }
  return new Promise((resolve) => {
    const w = { resolve }
    // A command that never answers takes the connection down with it, so later ones are not stuck behind.
    w.timer = setTimeout(() => { c.socket.emit('error', new Error('timeout')) }, timeout)
    c.waiting.push(w)
    c.socket.ref()
    c.socket.write(JSON.stringify({ argv, cwd }) + '\n')
  })
}

/** Native synchronous connections by socket path (dist/zeropty.node's Client). */
const syncConns = new Map()
let Native

/**
 * The same, synchronously: the command is written and its reply read on this thread, blocking, with
 * no event loop turn in between (faster than `zeropty` when Node is otherwise idle; it blocks the thread
 * for the command's time, ~0.1 ms). Needs dist/zeropty.node. With no server, the CLI runs (it starts one
 * on new-session).
 */
export function zeroptySync(argv, { cwd = process.cwd(), sock = sockPath() } = {}) {
  if (!Native) {
    const require = createRequire(import.meta.url)
    Native = require(process.env.ZEROPTY_NODE || path.join(here, '..', 'dist', 'zeropty.node')).Client
  }
  let c = syncConns.get(sock)
  if (!c) syncConns.set(sock, (c = new Native(sock)))
  try {
    if (programOnly.has(argv[0])) throw new Error('the program')
    const r = c.command(argv, cwd)
    return { ok: r.code === 0, code: r.code, out: r.out, err: r.err }
  } catch {
    const r = spawnSync(cliPath(), argv, { cwd, maxBuffer: 64 * 1024 * 1024, env: { ...process.env, ZEROPTY_SOCK: sock } })
    return { ok: r.status === 0, code: r.status ?? 1, out: String(r.stdout), err: String(r.stderr) }
  }
}
