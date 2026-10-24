---
description: zeropty is a session engine for AI agents that takes tmux commands and runs sessions without a PTY.
---

# Introduction

zeropty is a **session engine** for AI agents. It starts programs such as coding agents, reads their screens, sends them keys and tells you when they finish. It takes the same commands as [tmux](https://github.com/tmux/tmux), but it uses no PTY (pseudo-terminal) at all, which is where the name comes from.

zeropty is written in [Rust](https://www.rust-lang.org) and runs on macOS only. You can use it as a command-line tool, talk to its socket from any language, or put the engine inside your own program.

## How does it work?

### Sessions and the server

A **session** is one program with one screen and its scrollback. There are no windows or panes. The `zeropty` CLI sends commands to a **server** that owns every session, and the first `new-session` starts that server for you, as tmux does. Commands name a session with a **target** (`-t =name:`), and the name must match exactly.

For details, see [Sessions and the server](/guide/concepts/sessions).

### How a session gets a terminal

tmux gives every window a PTY, and macOS allows only 511 of them by default (999 at most). zeropty connects each session through a socketpair instead, so there is no such limit. Programs that check for a terminal, such as vim or [Claude Code](https://docs.anthropic.com/en/docs/claude-code), run through `zeropty-tty`, which injects a small library that answers terminal calls. The engine itself does what the PTY did: Enter becomes a newline, `C-c` becomes a signal, and typed text is echoed.

For details, see [How a session gets a terminal](/guide/concepts/terminal).

### Screens, keys and waiting

The engine parses each session's output into its own screen structure of 6 bytes per cell. Quiet screens are folded into about 1KB. `send-keys` and `capture-pane` work as in tmux. `wait-pane` answers the moment text appears or output pauses, and `grep-panes` searches every screen at once, so nothing has to poll.

For details, see [Screens, keys and waiting](/guide/concepts/screens).

### Four ways to drive zeropty

Every command has the same form everywhere: a list of arguments such as `["send-keys", "-t", "=work:", "Enter"]`. You can send it from the CLI, as one line of JSON on the server socket, or as a function call into an embedded engine (C ABI, Node.js module or Rust crate). Plugins listen to session events and add their own commands.

For details, see [Four ways to drive zeropty](/guide/concepts/interfaces).

## Why zeropty?

Programs that run many coding agents usually start them in tmux sessions and call `capture-pane` over and over to read their screens. With hundreds of sessions, that approach runs into the problems below. zeropty was built for agent hosts from the start.

| What an agent host needs | tmux | zeropty |
|---|---|---|
| Hundreds to thousands of sessions | PTY limit of 511 (999 at most) | 1000 verified, no PTY limit |
| Reading a screen, sending keys | Runs tmux for every command (`send-keys` 9.03~9.56ms from Node) | One function call (`send-keys` 0.06~0.07ms embedded) |
| Knowing that an agent finished | `capture-pane` in a loop (p50 86~96ms late at a 200ms poll) | `wait-pane`: p50 1ms, no polling |
| Which session is waiting for input | `capture-pane` per session | `grep-panes`: 1000 sessions in 16~32ms |
| Upgrading the server | Restarting the server ends the sessions | `restart-server` takes over the running sessions |
| Putting it inside your program | Not possible | C ABI, Node module, Rust crate |

- **The PTY limit.** tmux needs one PTY per session. zeropty uses none, and 1000 sessions replaying real claude work were measured at 4.65MB of memory and 6.69~7.10% CPU. tmux cannot start 1000.
- **A process per command.** Running tmux for each command costs a process start every time. zeropty answers over one socket connection (0.24~0.26ms per `send-keys` from Node) or as a function call (0.06~0.07ms).
- **Polling to know an agent finished.** With 200 sessions, 200ms polling noticed a finished task p50 86~96ms late. `wait-pane` noticed it in 1ms.
- **Finding who waits for input.** Reading 1000 screens one by one through the CLI took 4.2~4.3 seconds. `grep-panes` took 16~32ms.
- **Restarts end sessions.** `restart-server` hands every running session to a new server. The programs keep running and keep their pids.
- **Embedding.** With the engine inside your process there is no server process, and a command is a function call.

The cost is lower too. With 400 sessions replaying recordings of real claude work, measured against tmux 3.7c on an Apple M1, zeropty used 3.51~3.84% CPU against 54.30~54.91% (14.1~15.6x less), and 2.66~2.70MB of server memory against 32.4~32.5MB (12.0~12.2x less). For every measurement and how it was taken, see [Benchmarks](/guide/benchmarks).

zeropty does not replace tmux as a terminal multiplexer for people. It has no window splits, status line, copy mode or configuration file. When you want to look at a session, `zeropty attach` connects your terminal to it.

## Next steps

- [Get started](/guide/getting-started): build zeropty, run a session, wait for its output and read the screen.
- [Install and platform support](/guide/install): release archives, what each file is for, and supported Macs.
- [Sessions and the server](/guide/concepts/sessions): the first concept page.
- [Coming from tmux](/guide/from-tmux): what changes in your existing scripts.
