// The engine inside this Node process, no server: node examples/node/embedded.mjs
// Needs ./build.sh first (dist/zeropty.node). Same calls as quickstart.mjs; only the import differs.
import { zeropty } from '../../client/embedded.mjs'

const r = await zeropty(['new-session', '-d', '-s', 'w', '-x', '60', '-y', '8', 'exec cat'])
if (!r.ok) throw new Error(r.err)

await zeropty(['send-keys', '-t', '=w:', '-l', 'hello from an embedded engine'])
await zeropty(['send-keys', '-t', '=w:', 'Enter'])

// Commands are function calls: time a few hundred screen reads.
const n = 500
const t0 = performance.now()
for (let i = 0; i < n; i++) await zeropty(['capture-pane', '-p', '-t', '=w:'])
console.log(`capture-pane: ${((performance.now() - t0) / n * 1000).toFixed(1)} µs each`)

// cat prints the line back once it reads it.
for (let i = 0; i < 100; i++) {
  const { out } = await zeropty(['capture-pane', '-p', '-t', '=w:'])
  if (out.split('hello').length === 3) { console.log(out.trimEnd()); break }
  await new Promise((done) => setTimeout(done, 10))
}

// The sessions end with this process (their programs get EOF); kill-server ends them now.
await zeropty(['kill-server'])
