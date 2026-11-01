---
description: Run claude, vim, the python REPL and interactive shells in zeropty sessions with zeropty-tty and a runtime adapter.
---

# Run claude and other terminal programs

By the end of this guide, you run programs that need a terminal in zeropty sessions: a python REPL, [Claude Code](https://docs.anthropic.com/en/docs/claude-code) (`claude`) and an interactive shell whose jobs you can stop with `C-c`.

## Before you begin

- zeropty installed on a Mac or on Linux. You need `zeropty-tty` and the shim (`shim.dylib`, or `shim.so` on Linux) in one folder: the release archive and `./build.sh` (in `dist/`) both put them there. `cargo install` does not. For details, see [Install and platform support](/guide/install).
- zeropty 0.1.5 or later for `C-c` to reach a job in a shell (step 4). Check with `zeropty -V`.
- For step 3, `claude` on your `PATH` (its native build) and a copy of [`tty-adapter.cjs`](https://github.com/CreatiCoding/zeropty/blob/main/examples/node/tty-adapter.cjs).

The commands below write `dist/zeropty-tty`. If you use the release folder, write the path of that folder instead.

## 1. See why a program needs a terminal

A session's terminal is a socket, not a PTY (pseudo-terminal). Programs that check `isatty()` decide they are not on a terminal: vim, less, the python REPL and claude all behave differently. For details, see [How a session gets a terminal](/guide/concepts/terminal).

**`zeropty-tty`** fixes this. It starts the program with a **shim** (`shim.dylib`, injected with `DYLD_INSERT_LIBRARIES` on macOS; `shim.so`, with `LD_PRELOAD` on Linux) that answers these calls as a terminal would:

| Call | What the program gets |
|---|---|
| `isatty`, `tcgetattr` | A terminal, on the session's own socket |
| `TIOCGWINSZ` | The session's size |
| `tcgetpgrp`, `tcsetpgrp` | The foreground process group |
| `/dev/tty` | The session's socket, for reading and writing |
| `readpassphrase`, `getpass` | A prompt that does not echo (since 0.1.4) |

Only the session's socket counts. A program whose stdout is piped (`zeropty-tty prog | cat`) or whose stdin is `/dev/null` sees no terminal there, as under tmux.

## 2. Start a program through zeropty-tty

Put `zeropty-tty` in front of the command:

```sh [Terminal]
zeropty new-session -d -s py -x 80 -y 10 "dist/zeropty-tty python3"
zeropty send-keys -t '=py:' -l 'print(6 * 7)'
zeropty send-keys -t '=py:' Enter
zeropty wait-pane -t '=py:' -m '42' -T 10000
```

vim works the same way: `zeropty new-session -d -s v "dist/zeropty-tty vim notes.txt"`. When the program switches to raw mode, the shim reports it, and keys pass through unchanged.

macOS does not let the shim into SIP-protected binaries (`/usr/bin`, `/bin`, `/sbin`, `/System`) or binaries signed with the hardened runtime. `zeropty-tty` runs those from a copy with only the signature changed, kept under `~/.zeropty/tty/` (`ZEROPTY_CACHE`). It rebuilds the copy when the original changes, and runs it under the name you gave, so `ps` and `pgrep` find it.

## 3. Start claude with a runtime adapter

claude's native build is signed with the hardened runtime, so `zeropty-tty` would run it from a re-signed copy. It is a Bun program, so a **runtime adapter** gives it a terminal instead, with its binary left as it is. Bun loads the adapter before claude:

```sh [Terminal]
zeropty new-session -d -s fix-1 -x 220 -y 80 -c ~/proj "BUN_OPTIONS='--preload $HOME/tty-adapter.cjs' exec claude"
```

Replace `~/proj` with the repository the agent works in, and `$HOME/tty-adapter.cjs` with where you put the adapter. The adapter removes `BUN_OPTIONS` again, so the commands claude runs do not load it. For how it works, see [How a session gets a terminal](/guide/concepts/terminal#a-runtime-adapter-instead-of-the-shim). Give the agent work and wait as in [Drive agents from the shell](/guide/shell).

## 4. Run an interactive shell and stop its jobs

To type commands at a shell in a session, start the shell through `zeropty-tty`:

```sh [Terminal]
zeropty new-session -d -s sh1 "dist/zeropty-tty zsh"
zeropty send-keys -t '=sh1:' -l 'sleep 30'
zeropty send-keys -t '=sh1:' Enter
zeropty send-keys -t '=sh1:' C-c
```

`C-c`, `C-\` and `C-z` go to the foreground job as signals, as a terminal sends them. The shell's `tcsetpgrp` reaches zeropty through the shim, so the shell itself must run with it. The job may be any program, `/usr/bin` included. `C-z` and `fg` work too, pipelines included. Only a process group with a member under the session's program is taken.

::: warning Programs typed at the shell
The shim does not get into `/usr/bin` programs typed at a shell in the session. They run without it. Signals still reach them, but `ssh`, `ssh-keygen` and `ssh-add` typed there still echo a passphrase onto the screen and the scrollback. Start them as their own session with `zeropty-tty ssh-keygen` instead.
:::

## Verify

Read the python session:

```sh [Terminal]
zeropty capture-pane -p -t '=py:'
```

You should see the REPL's banner, then the prompt, your line and the answer:

```text
>>> print(6 * 7)
42
>>>
```

The `>>>` prompt and line editing are the REPL's terminal mode, which it turns on only when it sees a terminal.

For the shell from step 4, `zeropty capture-pane -p -t '=sh1:'` shows a new prompt after `sleep 30` instead of a session still sleeping.

## Troubleshooting

| Symptom | Fix |
|---|---|
| A program behaves differently because it is "not a terminal" | Start it through `zeropty-tty`. For claude, use the runtime adapter (step 3). |
| A script started through `/usr/bin/env` loses the shim | SIP strips it there. Start the interpreter directly: `zeropty-tty python3 script.py`. |
| `top` or `ps` does not run | The copy has none of the original's privileges (setuid, entitlements). Programs that need them do not work. |
| `C-c` ends the shell's prompt but not `sleep 30` | The shell was started before the upgrade to 0.1.5 and keeps the old shim. Start the shell again. |
| A passphrase shows on screen | The program was typed at a shell and runs without the shim. Start it with `zeropty-tty`. |
| `zeropty-tty` cannot find the shim | Keep the folder together, or point `ZEROPTY_SHIM` at the shim (`shim.dylib` or `shim.so`). |
## Next steps

- [How a session gets a terminal](/guide/concepts/terminal) for the shim and the line discipline.
- [Drive agents from the shell](/guide/shell) to give the agent work and wait for it.
- [Environment variables](/reference/environment) for `ZEROPTY_SHIM` and `ZEROPTY_CACHE`.
