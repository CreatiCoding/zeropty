// zeropty's engine inside this Node process (dist/zeropty.node): no server; a command is a function call
// (send-keys p50 0.04-0.06 ms, docs/BENCHMARKS.md).
// Same call and result as client/zeropty.mjs, so switching is the import:
//   import { zeropty } from './client/embedded.mjs'   // instead of './client/zeropty.mjs'
// Sessions live as long as this process (their programs get EOF when it exits), unlike the server's.
import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const require = createRequire(import.meta.url)
const native = require(process.env.ZEROPTY_NODE || path.join(here, '..', 'dist', 'zeropty.node'))
const { Engine } = native

/** This module's version (Semantic Versioning, docs/VERSIONING.md); scripts/release.mjs sets it. */
export const VERSION = '0.1.0'

/** The loaded zeropty.node's version; it should equal VERSION (a dist/ built from another checkout may not). */
export function version() {
  return native.version()
}

let engine = null

/** Starts the engine now, with options ({ history, sizeDir, polled }); otherwise the first command does.
 *  polled (default true): Node's event loop runs it, no thread of its own (~0.3 MB less; parsing takes
 *  the JS thread, a slice at a time). polled: false gives it a thread, for commands from worker threads. */
export function open(options) {
  engine ??= new Engine(options)
  return engine
}

/** Answers commands on a Unix socket at `sockPath` too, as the zeropty server does: the zeropty CLI
 *  (`ZEROPTY_SOCK=<sockPath> zeropty send-keys ...`), attach and client/zeropty.mjs reach this process's sessions,
 *  with no server process. Throws if a server already answers there. */
export function listen(sockPath) {
  open().listen(sockPath)
}

/** Runs one command; resolves { ok, code, out, err } like the CLI's exit status and output. A plugin's
 *  command (docs/PLUGINS.md) is waited for without holding the JS thread: other work, and a plugin
 *  attached to this process through `listen`, go on meanwhile. */
export async function zeropty(argv, { cwd = process.cwd() } = {}) {
  const r = await open().commandStart(argv, cwd)
  return { ok: r.code === 0, code: r.code, out: r.out, err: r.err }
}

/** The same, returning at once: an engine command does not wait on anything. A plugin's command holds the
 *  JS thread until it answers (up to its deadline): use `zeropty` for those. */
export function zeroptySync(argv, { cwd = process.cwd() } = {}) {
  const r = open().command(argv, cwd)
  return { ok: r.code === 0, code: r.code, out: r.out, err: r.err }
}
