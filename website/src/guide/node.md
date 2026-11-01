---
description: Drive zeropty sessions from Node.js over the server socket, or with the engine embedded in your Node process.
---

# Use zeropty from Node.js

By the end of this guide, you run an agent host from [Node.js](https://nodejs.org): you start a session, give it work, wait for it and read its screen. You do it first over the server's socket, then with the engine inside your Node process, by changing one import.

## Before you begin

- A Mac with zeropty built from source (`git clone` and `./build.sh`). The Node client loads `dist/zeropty.node` and runs `dist/zeropty`; the release archive carries neither `zeropty.node` nor the client. For details, see [Install and platform support](/guide/install).
- Node.js with N-API 6 or later (`napi6`). The examples in the repository are run with Node 18 or later.
- Run the commands from the zeropty folder. The imports below use `./client/...`; from your own project, import the files by their path in that folder.

## 1. Pick a client

The two modules take the same calls and return the same result, `{ ok, code, out, err }`:

| Module | Where sessions live | One `send-keys` | Use it when |
|---|---|---|---|
| `client/zeropty.mjs` | The zeropty server | 0.24~0.26ms | Sessions must outlive your app, or several programs share them |
| `client/embedded.mjs` | Your Node process (**embedded**) | 0.06~0.07ms | Your app owns the sessions |

For how the four interfaces compare, see [Four ways to drive zeropty](/guide/concepts/interfaces).

## 2. Drive a session over the socket

`client/zeropty.mjs` keeps one connection to the server's socket (`~/.zeropty/mux.sock`, or `ZEROPTY_SOCK`) and sends each command over it. If no server is running, it runs the CLI, so the first `new-session` starts one. This is [`examples/node/quickstart.mjs`](https://github.com/CreatiCoding/zeropty/blob/main/examples/node/quickstart.mjs):

```js [examples/node/quickstart.mjs]
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
```

The example uses a socket of its own, so it leaves your sessions alone. In your app, drop `sock` to use the default server.

- `zeropty(argv, { cwd, sock, timeout })` has a default timeout of 15 seconds.
- `wait-pane` opens a separate connection, so other commands keep flowing while it waits. It has no default timeout: set one with `-T` or `{ timeout }`.
- A failed command does not throw. Check `r.ok`, or `r.code` and `r.err`.

## 3. Call synchronously when you need to

`zeroptySync(argv, { cwd, sock })` sends the same command and blocks the thread until the reply, for 0.13~0.26ms per command. It needs `dist/zeropty.node`:

```js [app.mjs]
import { zeroptySync } from './client/zeropty.mjs'
const { out } = zeroptySync(['capture-pane', '-p', '-t', '=job:'])
```

`sockPath()` and `cliPath()` return the socket (`ZEROPTY_SOCK`) and CLI (`ZEROPTY_BIN`) the client uses.

## 4. Embed the engine instead

To put the engine inside your process, change only the import. This is [`examples/node/embedded.mjs`](https://github.com/CreatiCoding/zeropty/blob/main/examples/node/embedded.mjs), shortened:

```js [examples/node/embedded.mjs]
import { zeropty } from '../../client/embedded.mjs'

const r = await zeropty(['new-session', '-d', '-s', 'w', '-x', '60', '-y', '8', 'exec cat'])
if (!r.ok) throw new Error(r.err)

await zeropty(['send-keys', '-t', '=w:', '-l', 'hello from an embedded engine'])
await zeropty(['send-keys', '-t', '=w:', 'Enter'])

// cat prints the line back once it reads it.
for (let i = 0; i < 100; i++) {
  const { out } = await zeropty(['capture-pane', '-p', '-t', '=w:'])
  if (out.split('hello').length === 3) { console.log(out.trimEnd()); break }
  await new Promise((done) => setTimeout(done, 10))
}

// The sessions end with this process (their programs get EOF); kill-server ends them now.
await zeropty(['kill-server'])
```

Three things change with the engine embedded:

- **No `wait-pane`.** The function call has no loop that could defer the answer. Read the screen in a loop as above, or open a socket in step 5 and send `wait-pane` there.
- **Sessions end with your process.** To keep them, `push-sessions` them to a server before exiting. For details, see [Move sessions and upgrade the server](/guide/moving).
- **Options go to `open`.** Call `open({ history, sizeDir, polled })` before the first command. By default the engine is **polled**: Node's event loop drives it and it has no thread of its own. Pass `polled: false` to give it a thread, for calls from worker threads.

## 5. Open a socket for the CLI

`listen(path)` runs the same server as `zeropty serve` inside your app. Then the CLI, `attach` and other scripts reach your app's sessions:

```js [app.mjs]
import { zeropty, listen } from './client/embedded.mjs'
listen(`${process.env.HOME}/.zeropty/myapp.sock`)   // throws if another server already answers on this socket
```

```sh [Terminal]
ZEROPTY_SOCK=~/.zeropty/myapp.sock zeropty capture-pane -p -t '=w:'
ZEROPTY_SOCK=~/.zeropty/myapp.sock zeropty attach -t w
```

The path must be 103 bytes or less.

## Verify

Run the socket example:

```sh [Terminal]
node examples/node/quickstart.mjs
```

You should see the wait time, the screen and the `grep-panes` match. `NNN` stands for the milliseconds, which vary from run to run:

```text
wait-pane: done after NNN ms
for i in 1 2 3; do echo step $i; sleep 0.2; done; echo "finished: $?"
step 1
step 2
step 3
finished: 0
job: finished: 0
```

Then run `node examples/node/embedded.mjs`. It prints a `capture-pane: … µs each` timing, then the line twice: once echoed as typed, once printed back by `cat`.

## Troubleshooting

| Symptom | Fix |
|---|---|
| `Cannot find module …/dist/zeropty.node` | Run `./build.sh`, or set `ZEROPTY_NODE` to the module's path. |
| A `wait-pane` call never resolves | It has no default timeout. Pass `-T` or `{ timeout }`. |
| `wait-pane` fails on the embedded engine | Not available as a function call. Loop on `capture-pane`, or `listen` and use the socket client. |
| `listen` throws | Another server answers at that path. Pick another path, or stop that server. |
| A plugin command attached to your own `listen` socket times out | Call it with `await zeropty(...)`, not `zeroptySync`, which holds the JS thread the plugin runs on. |

## Next steps

- [Socket protocol](/reference/socket-protocol#from-other-programs-the-socket) to talk to the server from any language.
- [Node.js API](/reference/node-api) for every Node export and `open` option.
- [Embed zeropty in C or Rust](/guide/embed) for the same engine outside Node.
