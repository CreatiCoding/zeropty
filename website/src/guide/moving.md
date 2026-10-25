---
description: Upgrade the zeropty server without stopping your agents, and move running sessions between servers and embedded engines.
---

# Move sessions and upgrade the server

By the end of this guide, you upgrade the zeropty server while its programs keep running, and you move running sessions between two servers. You also know what travels with a session and what stays behind.

## Before you begin

- A Mac with zeropty installed and a server with at least one session. Commands below use a session named `work`; replace `work` with your session's name.
- For an upgrade, the new `zeropty` binary: a newer release folder or a fresh `./build.sh`. For details, see [Install and platform support](/guide/install).
- Moving sessions keeps programs alive, but it is not a backup. A session ends when its program exits, wherever it lives.

## 1. Note what you will check

Record the program's pid and the server's version before you change anything:

```sh [Terminal]
zeropty display -p -t '=work:' '#{pane_pid}'
zeropty display -p '#{version}'
```

`#{pane_pid}` is the program's pid, and `#{version}` is the version of the server that answers. `zeropty -V` prints the version of the CLI you ran, which may differ.

## 2. Upgrade with restart-server

Run `restart-server` from the **new** binary:

```sh [Terminal]
zeropty restart-server
```

A new server starts from the `zeropty` binary you just ran, takes over every session, then takes over the socket. The programs never stop and keep their pids. Commands that arrive during the few ms of the switch go to the old server.

If the new binary is not first on your `PATH`, run it by its path, for example `./zeropty-0.1.9-darwin-universal/zeropty restart-server`.

::: warning The new server has your shell's environment
`restart-server` starts the new server from the shell that runs it. Sessions created afterwards get that shell's environment, and values set with `set-environment -g` are dropped: set them again. Running sessions keep their own environment. `ZEROPTY_PLUGIN_DIR` and `ZEROPTY_SOCK` also come from this shell.
:::

## 3. Move sessions between servers

`pull-sessions` brings sessions from another server into the one your `zeropty` talks to. `push-sessions` sends them the other way. Name the other server by its socket with `-s`:

```sh [Terminal]
zeropty pull-sessions -s ~/.zeropty/other.sock            # all of the other server's sessions
zeropty pull-sessions -s ~/.zeropty/other.sock work build # only these
zeropty push-sessions -s ~/.zeropty/other.sock work       # this server's work to the other server
```

Replace `~/.zeropty/other.sock` with the other server's socket. If no server is running on your side, `pull-sessions` starts one.

The sending side releases only the sessions the receiver confirmed it took. If anything fails midway, sessions stay where they were.

## 4. Keep an embedded app's sessions across restarts

An embedded engine's sessions end with your app. The same commands work there, so hand them to a server before exiting and take them back after starting:

```js [app.mjs]
import { zeropty } from './client/embedded.mjs'

const server = `${process.env.HOME}/.zeropty/mux.sock`
await zeropty(['pull-sessions', '-s', server])     // on start: take the sessions back
// ... run your app ...
await zeropty(['push-sessions', '-s', server])     // before exit: hand them to the server
```

For the embedding side, see [Use zeropty from Node.js](/guide/node) and [Embed zeropty in C or Rust](/guide/embed).

## 5. Know what moves

| Moves with the session | Does not move |
|---|---|
| The running program and its pid | A `pipe-pane` pipe: it is closed when the session moves |
| The screen and scrollback, as bytes | Values from `set-environment -g`, on `restart-server` |
| The session's fds, by fd passing (SCM_RIGHTS) | A session whose name already exists on the other side |
| | The rest of an escape sequence that was half received at the moment of the move: it is printed as text |

Plugins are not handed over. With a plugin directory, the new server starts its own plugins after it takes over the socket, and for tens of ms plugin commands may return `unknown command`. Plugins see the move as events: `session-exited` with the reason `handed-over` on the sending side, `session-created` on the receiving side.

Under the hood, `pull-sessions`, `push-sessions` and `restart-server` use two internal requests on the socket, `export-sessions` and `import-sessions`. You rarely send them yourself, and the record format may change between versions. For details, see [Socket protocol: export-sessions, import-sessions](/reference/socket-protocol#socket-protocol-export-sessions-import-sessions).

## Verify

Run the commands from step 1 again:

```sh [Terminal]
zeropty display -p -t '=work:' '#{pane_pid}'
zeropty display -p '#{version}'
```

You should see the same pid as before and the new version. For example, after upgrading to 0.1.5 a program with pid 48213 reads:

```text
48213
0.1.5
```

`zeropty capture-pane -p -t '=work:'` shows the same screen as before the upgrade.

## Troubleshooting

| Symptom | Fix |
|---|---|
| The move prints `duplicate session: NAME` and exits with 1 | A session with that name exists on the other side. Rename one with `rename-session` and move it again. |
| `session record of another version` | The two sides run zeropty versions whose session records differ. Run both on the same version. |
| New sessions miss a variable after `restart-server` | The new server has the environment of the shell that ran it. Set the variable there, or use `set-environment -g` again. |
| A plugin command says `unknown command` right after the upgrade | The new server is still starting its plugins. Try again after a moment. |
| `C-d` no longer reaches a session after a downgrade | Sessions started by 0.1.2 or later should not be handed back to 0.1.1 or earlier. |

## Next steps

- [Sessions and the server](/guide/concepts/sessions) for why a session can change processes.
- [Server and moving sessions](/reference/commands#server-and-moving-sessions) for every option.
- [Moving sessions](/reference/socket-protocol#moving-sessions) for what happens on the socket.
