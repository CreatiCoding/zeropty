---
description: Switch a tmux script to zeropty, learn which commands map one to one, what does not exist, and the differences that break scripts.
---

# Coming from tmux

By the end of this guide, you run an existing [tmux](https://github.com/tmux/tmux) script on zeropty, replace its polling loop with `wait-pane`, and know the few differences that make a script behave differently.

## Before you begin

- A Mac with zeropty on your `PATH`. For details, see [Install and platform support](/guide/install).
- A shell script that drives programs through tmux: `new-session`, `send-keys`, `capture-pane` and the like.
- If the script starts programs that need a terminal (claude, vim), `zeropty-tty` from the release archive or `./build.sh`.

## 1. Check what maps one to one

zeropty takes tmux's command names, aliases and arguments for driving programs. The format variables were checked against tmux on the same sessions, and the key bytes against tmux 3.

| Works the same | Notes |
|---|---|
| `new-session`, `kill-session`, `has-session`, `list-sessions`, `rename-session` | `new-session` accepts and ignores `-n -E -D -X` |
| `send-keys`, `paste-buffer`, `load-buffer` | Same [key names](/reference/keys-and-formats#key-names) |
| `capture-pane` | Only `-p -e -J -N -S -E -b -q`; other options are errors |
| `display-message -p`, `list-sessions -F` | 25 [format variables](/reference/keys-and-formats#format-variables) |
| `pipe-pane`, `resize-window`, `set-environment -g` | No `pipe-pane -I` |
| `attach` | Detach with `Ctrl-b d`; `-r` is read-only |

## 2. Check what does not exist

zeropty keeps programs running, reads their screens and sends them keys. Features for people sharing a terminal are not there:

- **No windows or panes.** One session runs one program. There is no `split-window` or `new-window`. `window_index` and `pane_index` are always `0`.
- **No status line, copy mode, key bindings, config file or hooks.**
- **No format conditionals or modifiers** (`#{?...}`, `#{t:...}`). Unknown variables become an empty string, as in tmux.
- **No control mode** (`-C`). Programs use the JSON socket protocol or the Node client instead.

For the full table, see [tmux compatibility](/reference/tmux-compatibility).

## 3. Switch the script

Replace `tmux` with `zeropty`. A one-line variable keeps the change small:

```sh [agents.sh]
#!/bin/sh
set -e
T=zeropty                                   # was: T=tmux
$T new-session -d -s fix-1 -x 220 -y 80 -c ~/proj "BUN_OPTIONS='--preload $HOME/tty-adapter.cjs' exec claude"
$T send-keys -t '=fix-1:' -l 'Fix the failing test'
$T send-keys -t '=fix-1:' Enter
```

Replace `~/proj` with your repository. Programs that need a terminal must start through `zeropty-tty`; claude loads a runtime adapter instead (`$HOME/tty-adapter.cjs` here). For details, see [Run claude and other terminal programs](/guide/terminal-programs).

The first `new-session` starts the server if none is running, as tmux does.

## 4. Replace polling with wait-pane

A tmux script learns that an agent is done by calling `capture-pane` in a loop. zeropty answers when it happens:

::: code-group

```sh [Before (tmux)]
until tmux capture-pane -p -t '=fix-1:' | grep -q 'All tests pass'; do
  sleep 0.2
done
```

```sh [After (zeropty)]
zeropty wait-pane -t '=fix-1:' -m 'All tests pass' -T 600000
```

:::

`-m` waits for text, `-q` waits until output pauses, and `-T` sets a timeout in milliseconds. The server checks only when output arrives (detection p50 1ms, p95 2ms). To see which of many sessions shows a text, use `grep-panes` once instead of a `capture-pane` per session. For details, see [Screens, keys and waiting](/guide/concepts/screens).

## 5. Fix the differences that break scripts

| Difference | What to change |
|---|---|
| **Targets match exactly.** There is no prefix matching: `-t fix` does not find `fix-1`. | Use full names. Write targets as `'=name:'`. |
| **Unknown options are errors** (`send-keys: unknown flag -X`), not ignored. | Remove options zeropty does not list. They used to be ignored silently, which changed results. |
| **The server keeps the environment of the process that started it.** | Pass variables with `new-session -e VAR=value`, or `set-environment -g VAR value` for every new session. |
| **`=name:` in zsh** turns into a command path (`=` expansion). | Quote it: `'=fix-1:'`. The same applies to tmux. |
| **`C-d` closes a session's input for good**, since the socket cannot signal EOF just once. | Send `C-c` or the program's quit command unless you mean to end it. |
| **No PTY.** A program run directly sees no terminal. | Start it with `zeropty-tty`. |

`TMUX` and `TMUX_PANE` are removed from a session's environment, so a program in a session does not think it runs inside tmux. Exit codes follow tmux: 0 for success, 1 for failure. For details, see [Exit codes](/reference/keys-and-formats#exit-codes).

## Verify

Run the switched script and wait for the agent, or use this check with `sh` in place of claude:

```sh [Terminal]
zeropty new-session -d -s demo -x 80 -y 6 sh
zeropty send-keys -t '=demo:' -l 'echo "exit: $?"'
zeropty send-keys -t '=demo:' Enter
zeropty wait-pane -t '=demo:' -m 'exit: 0' -T 10000
zeropty list-sessions
```

You should see the session in tmux's own list format, with the creation time in place of `...`:

```text
demo: 1 windows (created ...)
```

 `zeropty capture-pane -p -t '=demo:'` shows the typed line and `exit: 0`.

## Troubleshooting

| Symptom | Fix |
|---|---|
| `can't find session` or `can't find pane` for a short name | Targets match exactly. Use the full name. |
| `unknown flag` on an option tmux accepted | zeropty does not have it. Check [Commands](/reference/commands#command-reference). |
| `split-window` or `new-window` fails | There are no windows. Start one session per program. |
| A program behaves as if it is "not a terminal" | Start it with `zeropty-tty`. |
| A variable from your shell is missing in a new session | The server has the environment it started with. Use `-e` or `set-environment -g`. |
| A format prints an empty string | The variable is unknown, or uses a conditional or modifier. See [Format variables](/reference/keys-and-formats#format-variables). |

## Next steps

- [tmux compatibility](/reference/tmux-compatibility) for the feature-by-feature table.
- [Drive agents from the shell](/guide/shell) for `wait-pane`, `grep-panes` and the fast path.
- [FAQ: Can I use my tmux scripts as they are?](/guide/faq#can-i-use-my-tmux-scripts-as-they-are)
