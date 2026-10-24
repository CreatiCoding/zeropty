---
description: Start agents from a shell script, send them keys, wait for them with wait-pane, read their screens and search many sessions at once.
---

# Drive agents from the shell

By the end of this guide, you drive several sessions from a shell script: you start them, type into them, wait until each one is done and find the ones that match. You also switch the script to the fast path, which talks to the socket without starting a process for every command.

## Before you begin

- A Mac with zeropty on your `PATH`. For details, see [Install and platform support](/guide/install).
- For the bash builtin, a build from source (`./build.sh` puts `dist/zeropty_bash.dylib` in place). The release archive does not carry it.
- The steps run `sh` in each session, so you do not need claude. A real script runs an agent instead, for example `dist/claude-zeropty` (see [Run claude and other terminal programs](/guide/terminal-programs)).

## 1. Start a session

A **session** is one program kept running by the **server**. The first `new-session` starts the server if none is running:

```sh [Terminal]
zeropty new-session -d -s build -x 80 -y 6 sh
```

`-d` leaves your terminal free, `-s build` names the session, and `-x`/`-y` set the screen size. For details, see [Sessions and the server](/guide/concepts/sessions).

## 2. Send keys

Every command names its session with a **target** (`-t`). In scripts, write it as `'=build:'`, quoted:

```sh [Terminal]
zeropty send-keys -t '=build:' -l 'ls / | head -3; echo "exit: $?"'
zeropty send-keys -t '=build:' Enter
```

Each argument to `send-keys` is either a key name or literal text:

| You write | What is sent |
|---|---|
| `Enter`, `C-c`, `Up`, `F5` | The key's bytes ([key names](/reference/keys-and-formats#key-names)) |
| `-l 'Enter'` | The five letters `Enter`, never a key |
| `-H 41 42 0a` | Raw bytes in hexadecimal (`AB` and a newline) |
| `-N 3 Down` | The whole argument list three times |

Use `-l` for any text that comes from a prompt or a variable, so a word such as `Enter` or `Space` in it is never read as a key. Send `Enter` as a separate command without `-l`.

::: warning Quote the target in zsh
zsh turns `=word` into the path of a command (`=` expansion). Write `'=build:'`, not `=build:`. tmux targets have the same problem.
:::

## 3. Wait for the result

`wait-pane` answers when text appears (`-m`), when output pauses (`-q`), or when both hold. `-T` is the timeout:

```sh [Terminal]
zeropty wait-pane -t '=build:' -m 'exit: 0' -T 10000
```

The server checks only when output arrives, so nothing polls. Typed keys are echoed to the screen, so wait for text that only the result prints. The typed line above holds `$?`, not `0`, so `exit: 0` matches only the output.

For an agent whose final text you cannot predict, wait for quiet instead:

```sh [Terminal]
zeropty wait-pane -t '=fix-1:' -q 5000 -T 600000   # until output pauses for 5 s, at most 10 minutes
```

`wait-pane` exits with code 1 on timeout (`wait-pane: timed out`) or when the session ends. For details, see [Screens, keys and waiting](/guide/concepts/screens).

## 4. Read the screen

`capture-pane -p` prints the screen. Pending output is processed first, so the screen is always current:

```sh [Terminal]
zeropty capture-pane -p -t '=build:'                 # the visible screen
zeropty capture-pane -p -e -S -200 -t '=build:'      # plus 200 lines of scrollback, with colors
zeropty capture-pane -p -J -S - -E - -t '=build:'    # everything, wrapped lines joined
```

| Option | Effect |
|---|---|
| `-p` | Print to standard output (without it, the screen goes into a buffer) |
| `-e` | Keep color and font escapes |
| `-J` | Join lines that were split by wrapping |
| `-N` | Keep trailing spaces |
| `-S`, `-E` | First and last line: 0 is the first screen line, negative numbers are scrollback, `-` means to the end |

For every option, see [Reading the screen](/reference/commands#reading-the-screen).

## 5. Drive many sessions

Start one session per job, then wait for each and search them all at once with `grep-panes`. Save this as `jobs.sh`:

```sh [jobs.sh]
#!/bin/sh
set -e
for n in 1 2 3; do
  zeropty new-session -d -s "job-$n" -x 80 -y 10 sh
  zeropty send-keys -t "=job-$n:" -l "sleep $n; echo \"finished: \$?\""
  zeropty send-keys -t "=job-$n:" Enter
done
for n in 1 2 3; do
  zeropty wait-pane -t "=job-$n:" -m 'finished: 0' -T 30000
done
zeropty grep-panes 'finished: 0'
```

`grep-panes` prints matches as `session: line`, sorted by session name, and exits with 1 when nothing matches. Add `-S -100` to search scrollback too. Across 1000 sessions it took 16~32ms. Use it to find which agents are waiting for input without reading each screen.

## 6. Use the fast path

Running the `zeropty` program starts a process for every command (3.59~4.23ms). The zsh function and the bash builtin send the command over the socket from inside the shell instead:

::: code-group

```sh [zsh (~/.zshrc)]
source /path/to/zeropty/shell/zeropty.zsh
```

```sh [bash]
enable -f /path/to/zeropty/dist/zeropty_bash.dylib zeropty
```

:::

Replace `/path/to/zeropty` with the folder you cloned. Your script stays the same; `zeropty` is now a function or a builtin.

| Path | Measured |
|---|---|
| zsh function | `send-keys -l` p50 0.12ms (50.9~52.7x faster than the tmux CLI); `has-session` 0.11~0.17ms; a 2000-line `capture-pane` 0.50~0.55ms |
| bash builtin | 0.041~0.138ms per command (48.7~157x faster than the tmux CLI), checked with macOS's `/bin/bash` 3.2 |

Both still run the program for `attach`, `serve`, `restart-server`, `-V`, `--help`, a call with no arguments, and any command while no server is running. Set `ZEROPTY_BIN` to point them at another program.

## Verify

Run the script, then read one screen:

```sh [Terminal]
sh jobs.sh
zeropty capture-pane -p -t '=job-2:'
```

You should see each session listed once by `grep-panes`:

```text
job-1: finished: 0
job-2: finished: 0
job-3: finished: 0
```

The screen of `job-2` shows the typed line, then `finished: 0`:

```text
sleep 2; echo "finished: $?"
finished: 0
```

Clean up with `zeropty kill-server`.

## Troubleshooting

| Symptom | Fix |
|---|---|
| `wait-pane -m` returns at once | The text is in the line you typed. Wait for text that only the result prints. |
| `wait-pane: session ended` although the text appeared | The program exited right after printing. Keep it alive, for example `sh -c 'make; echo build done; exec cat'`. |
| `can't find session: NAME` or `can't find pane: NAME` for a session you can see | Targets match exactly; there is no prefix matching. Check `zeropty ls`. |
| `no server running on ...` | No server listens on that socket. Start one with `new-session`, or set `ZEROPTY_SOCK`. |
| `send-keys: unknown flag -X` | Unknown options are errors. See [Sending keys](/reference/commands#sending-keys). |
| Input stops after `C-d` | `C-d` on an empty line shuts the session's input for good. Send `C-c` or the program's quit command instead. |

## Next steps

- [Run claude and other terminal programs](/guide/terminal-programs) to put agents in these sessions.
- [Use zeropty from Node.js](/guide/node) for the same flow without a process per command.
- [Commands](/reference/commands#waiting-and-searching) for every `wait-pane` and `grep-panes` option, and [Exit codes](/reference/keys-and-formats#exit-codes).
