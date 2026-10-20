// A zeropty plugin from Node in a few lines: attaches to a server's socket (plugin-attach, docs/PLUGINS.md),
// hands events to `onEvent`, answers the commands it adds with the functions given, and runs commands of
// its own on the same connection.
//
//   import { attachPlugin } from './client/plugin.mjs'
//   const p = await attachPlugin({
//     name: 'notes',
//     events: ['session-exited'],
//     onEvent: (ev) => console.log(ev.session, ev.reason),
//     commands: { 'notes-count': async (argv, { plugin }) => `${(await plugin.command(['list-sessions'])).out.split('\n').length - 1}\n` },
//   })
import net from 'node:net'
import { sockPath } from './zeropty.mjs'

/**
 * `access` lists what `plugin.command` may run besides looking (read): 'input', 'manage', 'call', 'all'.
 * Resolves the attached plugin { command(argv, cwd) -> Promise<{ ok, code, out, err }>, close(), closed },
 * or rejects with zeropty's reason (a name taken, a built-in command...).
 * A command function returns a string (the output), { code, out, err }, or throws (code 1, the message).
 */
export function attachPlugin({ name, events = [], commands = {}, access = [], onEvent = () => {}, sock = sockPath() }) {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection(sock)
    const waiting = new Map()
    let nextId = 1, attached = false, rest = ''
    const send = (msg) => socket.write(JSON.stringify(msg) + '\n')
    let closedResolve
    const plugin = {
      command(argv, cwd = '/') {
        const id = nextId++
        return new Promise((done) => { waiting.set(id, done); send({ id, argv, cwd }) })
      },
      close() { socket.end() },
      closed: new Promise((r) => (closedResolve = r)),
    }
    const answer = async (msg) => {
      let r
      try {
        const fn = commands[msg.argv[0]]
        r = await fn(msg.argv, { cwd: msg.cwd, plugin })
        r = typeof r === 'string' || r === undefined ? { code: 0, out: r ?? '', err: '' } : { code: 0, out: '', err: '', ...r }
      } catch (e) {
        r = { code: 1, out: '', err: `${e?.message ?? e}\n` }
      }
      send({ reply: msg.call, ...r })
    }
    const line = (text) => {
      const msg = JSON.parse(text)
      if (!attached) {
        attached = true
        if (msg.code !== 0) { socket.destroy(); reject(new Error(msg.err.trim())); return }
        resolve(plugin)
        return
      }
      if (msg.event) onEvent(msg, plugin)
      else if (msg.call !== undefined) answer(msg)
      else if (msg.id !== undefined && waiting.has(msg.id)) {
        const done = waiting.get(msg.id)
        waiting.delete(msg.id)
        done({ ok: msg.code === 0, code: msg.code, out: msg.out, err: msg.err })
      }
    }
    socket.setEncoding('utf8')
    socket.on('connect', () => {
      const argv = ['plugin-attach', '-n', name]
      if (events.length) argv.push('-e', events.join(','))
      if (Object.keys(commands).length) argv.push('-c', Object.keys(commands).join(','))
      // What plugin.command may run besides looking: 'input', 'manage', 'call' (docs/PLUGINS.md).
      if (access.length) argv.push('-a', access.join(','))
      send({ argv, cwd: '/' })
    })
    socket.on('data', (d) => {
      rest += d
      for (let i = rest.indexOf('\n'); i >= 0; i = rest.indexOf('\n')) {
        const text = rest.slice(0, i)
        rest = rest.slice(i + 1)
        if (text) line(text)
      }
    })
    socket.on('error', (e) => { if (!attached) reject(e) })
    socket.on('close', () => {
      for (const done of waiting.values()) done({ ok: false, code: 1, out: '', err: 'zeropty: lost server\n' })
      waiting.clear()
      closedResolve()
    })
  })
}
