// A Node host whose engine runs on its own loop (polled), with a plugin attached to it from the same
// process through `listen`: `zeropty()` waits for the plugin's command without holding the JS thread, so
// the plugin (served by that same thread) answers, and timers go on meanwhile. Run by tests/plugin.rs.
import { listen, open, zeropty } from '../../client/embedded.mjs'
import { attachPlugin } from '../../client/plugin.mjs'

const sock = process.argv[2]
listen(sock)
const plugin = await attachPlugin({
  name: 'later',
  sock,
  commands: { later: (argv) => new Promise((done) => setTimeout(() => done(`later ${argv[1]}\n`), 300)) },
})
let ticks = 0
const timer = setInterval(() => ticks++, 10)
const start = Date.now()
const r = await zeropty(['later', 'x'])
const took = Date.now() - start
clearInterval(timer)
// commandStart has one shape: an engine command's reply comes as an already resolved Promise too.
const started = open().commandStart(['list-sessions'], '/')
const promised = started instanceof Promise
const engine = await zeropty(['list-sessions'])
console.log(JSON.stringify({ out: r.out, err: r.err, ticks, took, engine: engine.ok, promised, code: (await started).code }))
plugin.close()
process.exit(0)
