---
description: Every export of zeropty's Node.js modules (client/zeropty.mjs, client/embedded.mjs and the native zeropty.node), with arguments and results.
---

# Node.js API

Every export of zeropty's Node.js modules. For a first program and examples, see [Use zeropty from Node.js](/guide/node).

The build checks this list: `api/node.txt` is compared with the modules, and the site's build fails if an export in it is missing here. Supported Node.js: N-API 6 or later (`napi6`).

| Module | What it is | When to use it |
|---|---|---|
| [`client/zeropty.mjs`](#clientzeroptymjs) | A client of a running zeropty server, over its socket | The sessions live in the zeropty server, shared with the CLI. |
| [`client/embedded.mjs`](#clientembeddedmjs) | The engine inside your Node process | Your program owns the sessions, with no server process. |
| [`zeropty.node`](#zeroptynode) | The native module both of them use | You need a class the JS modules do not wrap. |

The two JS modules export the same `zeropty` and `zeroptySync`, so switching between them takes only a change of import.

## The result shape

```ts
{ ok: boolean, code: number, out: string, err: string }
```

A command's result, as the CLI's exit status and output: `code` is the exit code, `ok` is `code === 0`, `out` and `err` are standard output and standard error.

## `client/zeropty.mjs`

A client of the zeropty server. One connection to the server's socket stays open, and commands go over it one line each, replies coming back in order. The argv is the CLI's, so `tmux(...args)` call sites become `zeropty(args)`.

### `zeropty`

```ts
zeropty(argv: string[], options?: { cwd?: string, sock?: string, timeout?: number }): Promise<Result>
```

Runs one command and resolves its [result](#the-result-shape).

| Argument | Default | Meaning |
|---|---|---|
| `argv` | | The command line, its name first. |
| `options.cwd` | `process.cwd()` | The working directory the command sees. |
| `options.sock` | [`sockPath()`](#sockpath) | The server's socket. |
| `options.timeout` | `15000` | Milliseconds to wait for the reply. `wait-pane` has no timeout unless one is given here (its own `-T` sets the server's). |

A command the program runs itself rather than the server (`restart-server`, `serve`, `attach`, `-V`) runs the CLI.

### `zeroptySync`

```ts
zeroptySync(argv: string[], options?: { cwd?: string, sock?: string }): Result
```

The same, synchronously: the command is written and its reply read on this thread, blocking, with no event loop turn in between. It is faster than `zeropty` when Node is otherwise idle, and blocks the thread for the command's time (about 0.1 ms). Needs `dist/zeropty.node`. With no server running, the CLI runs, and it starts a server on `new-session`.

### `sockPath`

```ts
sockPath(): string
```

The server's socket: `$ZEROPTY_SOCK`, or `~/.zeropty/mux.sock`.

### `cliPath`

```ts
cliPath(): string
```

The CLI that commands the server cannot run go to: `$ZEROPTY_BIN`, or `dist/zeropty` next to the module.

### `VERSION`

```ts
VERSION: string
```

This module's version, following [Versioning](/guide/versioning).

## `client/embedded.mjs`

The engine inside your process, with no server process. The native module is loaded from `$ZEROPTY_NODE`, or `dist/zeropty.node` next to the module.

### `open`

```ts
open(options?: { history?: number, sizeDir?: string, polled?: boolean }): Engine
```

Starts the engine now; otherwise the first command does. Returns the [`Engine`](#engine).

| Option | Default | Meaning |
|---|---|---|
| `history` | | Rows of scrollback per session. |
| `sizeDir` | | Where resized sessions' window sizes are written for their programs. |
| `polled` | `true` | Node's event loop runs the engine, with no thread of its own: about 0.3 MB less, and output is parsed on the JS thread a slice at a time. `false` gives the engine a thread, for commands from worker threads. |

### `zeropty`

```ts
zeropty(argv: string[], options?: { cwd?: string }): Promise<Result>
```

Runs one command and resolves its [result](#the-result-shape). A plugin's command is waited for without holding the JS thread, so other work, and a plugin attached to this process through `listen`, go on meanwhile.

### `zeroptySync`

```ts
zeroptySync(argv: string[], options?: { cwd?: string }): Result
```

The same, returning at once: an engine command does not wait on anything. A plugin's command holds the JS thread until it answers, up to its deadline: use `zeropty` for those.

### `listen`

```ts
listen(sockPath: string): void
```

Answers commands on a Unix socket at `sockPath` too, as the zeropty server does. The zeropty CLI (`ZEROPTY_SOCK=SOCKPATH zeropty send-keys ...`), `attach` and `client/zeropty.mjs` then reach this process's sessions, with no server process. Throws if a server already answers there.

### `version`

```ts
version(): string
```

The loaded `zeropty.node`'s version. It should equal `VERSION`; a `dist/` built from another checkout may not.

### `VERSION`

```ts
VERSION: string
```

This module's version.

## `zeropty.node`

The native module. Load it with `createRequire`:

```js [app.mjs]
import { createRequire } from 'node:module'
const { Engine, Client, version } = createRequire(import.meta.url)('./dist/zeropty.node')
```

### `version`

```ts
version(): string
```

The engine's version, the same as `zeropty --version` and the C ABI's [`zeropty_version()`](/reference/c-abi#zeropty_version).

### `Engine`

zeropty's engine inside this process. `client/embedded.mjs` wraps one.

```ts
new Engine(options?: { history?: number, sizeDir?: string, polled?: boolean })
```

An engine with no sessions, run by Node's loop unless `polled` is `false`. The options are those of [`open`](#open).

```js
const engine = new Engine({ history: 2000 })
```

| Method | Returns | Meaning |
|---|---|---|
| `command(argv: string[], cwd?: string)` | `{ code, out, err }` | Runs one command synchronously, with no server round trip (`send-keys` p50 0.06–0.07 ms from Node, measured in [Benchmarks](/guide/benchmarks)). Waits for a plugin's command, up to its deadline. |
| `commandStart(argv: string[], cwd?: string)` | `Promise<{ code, out, err }>` | The same without holding the JS thread while a plugin answers. An engine command's promise is already resolved; a plugin's resolves when its reply comes. |
| `listen(path: string)` | | Answers commands on a Unix socket at `path` too, as the zeropty server does. Polled, its connections are served on Node's loop. |

```js
const { code, out } = engine.command(['capture-pane', '-p', '-t', '=w:'])
engine.listen(`${process.env.HOME}/.zeropty/app.sock`) // then: ZEROPTY_SOCK=... zeropty attach -t w
```

### `Client`

A connection to a zeropty server, used synchronously: each command is written and its reply read on the calling thread, blocking, with no event loop in between. `zeroptySync` in `client/zeropty.mjs` uses one.

```ts
new Client(sock: string)
```

`sock` is the server's socket path. It connects on the first command.

| Method | Returns | Meaning |
|---|---|---|
| `command(argv: string[], cwd?: string)` | `{ code, out, err }` | Runs one command on the server. Throws if there is no server, or it went away. |
| `close()` | | Closes the connection. The next command opens another. |

```js
const c = new Client(`${process.env.HOME}/.zeropty/mux.sock`)
console.log(c.command(['list-sessions']).out)
c.close()
```
