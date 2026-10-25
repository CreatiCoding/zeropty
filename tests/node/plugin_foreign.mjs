// Run as root by tests/plugin.rs (attach_plugin_refuses_a_server_run_by_another_user): another user
// (nobody) listens at the socket first, and attachPlugin must refuse it before sending anything, as the
// CLI does. Exits 0 when it does; prints what went wrong and exits 1 otherwise.
import { spawn } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { attachPlugin } from '../../client/plugin.mjs'

const nobody = 65534
const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'zeropty-plugin-foreign-'))
fs.chmodSync(dir, 0o755)
const theirs = path.join(dir, 'theirs')
fs.mkdirSync(theirs)
fs.chownSync(theirs, nobody, nobody)
const sock = path.join(theirs, 's')
const got = path.join(theirs, 'got')
// Their server: records what it is sent, and answers as zeropty would so an attach goes through.
const server = `
  const net = require('node:net'), fs = require('node:fs')
  net.createServer((s) => s.on('data', (d) => { fs.appendFileSync(${JSON.stringify(got)}, d); s.write('{"code":0,"out":"","err":""}\\n') }))
    .listen(${JSON.stringify(sock)}, () => fs.chmodSync(${JSON.stringify(sock)}, 0o777))`
const child = spawn(process.execPath, ['-e', server], { uid: nobody, gid: nobody, stdio: 'inherit' })
const fail = (why) => {
  console.error(why)
  process.exitCode = 1
}
try {
  const start = Date.now()
  while (!fs.existsSync(sock) || (fs.statSync(sock).mode & 0o777) !== 0o777) {
    if (Date.now() - start > 5000) throw new Error('their server never listened')
    await new Promise((r) => setTimeout(r, 20))
  }
  const r = await attachPlugin({ name: 'notes', sock, commands: { 'notes-secret': () => 'token\n' } }).then(
    (p) => (p.close(), 'attached'),
    (e) => e.message,
  )
  if (!r.includes('runs as another user')) fail(`attachPlugin: ${r}`)
  await new Promise((r) => setTimeout(r, 200))
  if (fs.existsSync(got)) fail(`their server was sent: ${fs.readFileSync(got, 'utf8')}`)
} catch (e) {
  fail(String(e))
} finally {
  child.kill()
  fs.rmSync(dir, { recursive: true, force: true })
}
