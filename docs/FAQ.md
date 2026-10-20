# Frequently asked questions

**English** · [한국어](ko/FAQ.md)

[README](../README.md) · [Docs](README.md) · [Usage](USAGE.md) · [Plugins](PLUGINS.md) · [FAQ](FAQ.md) · [Benchmarks](BENCHMARKS.md) · [Design](DESIGN.md) · [Versioning](VERSIONING.md) · [Releasing](RELEASING.md)

**What it is**
- [Does it replace tmux?](#does-it-replace-tmux)
- [Can I use it without claude?](#can-i-use-it-without-claude)
- [Does it run on Linux or Windows?](#does-it-run-on-linux-or-windows)
- [Why does the README list only results of 10x or more?](#why-does-the-readme-list-only-results-of-10x-or-more)

**Using it**
- [How do I see the help?](#how-do-i-see-the-help)
- [What is missing when I install with `cargo install`?](#what-is-missing-when-i-install-with-cargo-install)
- [Can I use my tmux scripts as they are?](#can-i-use-my-tmux-scripts-as-they-are)
- [Can I use it without a server process?](#can-i-use-it-without-a-server-process)
- [How do I know when an agent is done?](#how-do-i-know-when-an-agent-is-done)
- [Doesn't `capture-pane` return a screen that is 300ms old?](#doesnt-capture-pane-return-a-screen-that-is-300ms-old)
- [Can I upgrade without losing sessions?](#can-i-upgrade-without-losing-sessions)
- [Can my sessions survive a restart of my app?](#can-my-sessions-survive-a-restart-of-my-app)

**Troubleshooting**
- [A program behaves differently because it is "not a terminal"](#a-program-behaves-differently-because-it-is-not-a-terminal)
- [`wait-pane -m` returns at once](#wait-pane--m-returns-at-once)
- [`wait-pane` ends with `session ended` although the text appeared](#wait-pane-ends-with-session-ended-although-the-text-appeared)
- [Input stops arriving after I send `C-d`](#input-stops-arriving-after-i-send-c-d)
- [`-t =work:` behaves oddly in zsh](#-t-work-behaves-oddly-in-zsh)
- [I get `no server running on ...`](#i-get-no-server-running-on-)
- [Starting many sessions fails](#starting-many-sessions-fails)
- [How do I stop the server?](#how-do-i-stop-the-server)

---

## Does it replace tmux?

**Not as a terminal multiplexer for people.** zeropty focuses on starting programs (coding agents in particular), reading their screens, sending them keys and waiting until they finish. It has no window splits, status line, copy mode or configuration file. In return, for that job it takes the same commands as tmux, is 10x or better on memory, CPU and latency across the whole measured range in 10 of the 15 items measured (1 more is borderline at 9.9\~11.7x, and 4 are under 10x) ([benchmarks](BENCHMARKS.md)), and has no PTY limit on the number of sessions. When a person wants to look in, `zeropty attach` connects to a session.

## Can I use it without claude?

Yes. A session can run any program (`sh`, `cat`, `python3`, a build command ...). The tests and the [examples](README.md#examples) all run without claude. `claude-zeropty` is only a small wrapper that starts claude without a terminal.

## Does it run on Linux or Windows?

Only macOS for now. The engine is built on kqueue, and the shim that fools programs which need a terminal uses macOS's `DYLD_INSERT_LIBRARIES`. Another platform would need both parts ported to its own mechanisms (epoll, `LD_PRELOAD`).

## Why does the README list only results of 10x or more?

The project's goal is "10x better than tmux on every item", and the README shows only the items that have reached it. The items below 10x are not hidden: [benchmarks](BENCHMARKS.md) lists every measurement, marked ⛔ where it falls short, with the reason for the low end. Numbers are written as the measured range, never rounded.

## How do I see the help?

`zeropty --help` shows every command and option on one screen (`-h` and `help` do the same). For one command, use `zeropty help send-keys`. It needs no server and does not start one. Each option's details and the key names are in [Usage](USAGE.md#command-reference). The command names and options are tmux's, so most of tmux's documentation applies as well ([feature comparison with tmux](USAGE.md#feature-comparison-with-tmux)).

## What is missing when I install with `cargo install`?

`cargo install --git https://github.com/CreatiCoding/zeropty` puts a single Rust binary, `zeropty` (the server and the CLI), in `~/.cargo/bin`. That alone starts the server, runs programs that need no terminal (`sh`, `python3`, build commands) as sessions, and gives you `send-keys`, `wait-pane`, `capture-pane` and `attach`.

This is what it leaves out. `cargo install` installs only a package's executables, and `./build.sh` builds or copies the files below separately.

| Left out | Why | Without it |
|---|---|---|
| `shim.dylib` | A C file (`shim/shim.c`) built separately with `clang` | Programs that need a terminal (claude, vim) decide they are "not on a terminal" |
| `zeropty-tty`, `claude-zeropty` | Shell scripts that look for `shim.dylib` in their own folder | claude cannot be started without a terminal |
| `libzeropty_c.dylib`, `zeropty.h`, `zeropty.node` | Libraries built by other crates of the workspace (`bindings/`) | No C or Node embedding (Rust embedding works through a git dependency) |
| `zeropty_bash.dylib` | A C file (`shell/zeropty_bash.c`) built separately | No bash builtin (the regular CLI still works) |

If you need any of these, build with `git clone` and `./build.sh` as in [Quick start](../README.md#quick-start) and use `dist/`.

## Can I use my tmux scripts as they are?

Mostly, by replacing `tmux` with `zeropty`. Commands that handle programs, such as `new-session`, `send-keys`, `capture-pane`, `list-sessions -F`, `display-message -p`, `load-buffer`, `paste-buffer` and `pipe-pane`, take the same arguments. The differences:

- There are no windows or window splits. One session runs one program (no `split-window`, no `new-window`).
- Target names must match exactly (no prefix matching as in tmux).
- Format conditionals (`#{?...}`) and modifiers (`#{t:...}`) are not supported.
- An unknown option is an error, not ignored.

Full comparison: [Usage: feature comparison with tmux](USAGE.md#feature-comparison-with-tmux)

## Can I use it without a server process?

Yes. Put the engine inside your own process (Node module, C ABI, Rust crate). Commands become function calls, so one `send-keys` from Node takes 0.04\~0.06ms (6.43\~10.37ms when tmux runs for each command). If needed, your program can also open a socket (`listen`) so the CLI and `attach` reach it too. [Usage: embedded](USAGE.md#inside-your-process-embedded)

## How do I know when an agent is done?

Use `wait-pane`. It answers when text appears on the screen (`-m`), when output has paused for a given time (`-q`), or both. The server checks only when output arrives, and only the rows that changed, so nothing polls.

```sh
zeropty wait-pane -t '=fix-1:' -q 5000 -T 600000   # until output pauses for 5 s, at most 10 minutes
```

To find which of many sessions is waiting for input, use `grep-panes` once.

## Doesn't `capture-pane` return a screen that is 300ms old?

No. To save CPU the engine reads output in batches every 300ms, but `capture-pane` (and `grep-panes`, `wait-pane`) first reads any pending output and then answers. A reader always gets the latest screen. Output also goes straight to a person watching with `attach`.

## Can I upgrade without losing sessions?

Yes. Run `zeropty restart-server` with the new binary: the new server takes over every running session and then the socket. The programs never stop and keep their pids. [Usage: moving sessions](USAGE.md#moving-sessions)

## Can my sessions survive a restart of my app?

An embedded engine's sessions end with the app. Before the app exits, hand them to a standalone server with `push-sessions -s <server socket>`, and take them back after it starts again with `pull-sessions -s <server socket>`.

## A program behaves differently because it is "not a terminal"

A session's terminal is a socket, not a PTY, so `isatty()` is false. Start the program through `zeropty-tty`:

```sh
zeropty new-session -d -s v "dist/zeropty-tty vim notes.txt"
```

System programs in `/usr/bin` run from a copy with only the signature changed. Programs that need special privileges, such as `top` or `ps`, do not work this way. [Usage: zeropty-tty](USAGE.md#programs-that-need-a-terminal-zeropty-tty)

## `wait-pane -m` returns at once

Typed text is echoed to the screen as with a PTY. If you send `send-keys -l 'echo done'` and then run `wait-pane -m done`, the `done` in the typed line matches at once. Wait for text that only the result prints, not the typed line. For example, send `echo "exit: $?"` and wait for `-m 'exit: 0'`.

## `wait-pane` ends with `session ended` although the text appeared

When a session prints the text being waited for and ends right away, the engine sometimes reports the end of the session before it has checked that output (a known issue; it reproduces about 5 times in 20). You then get `wait-pane: session ended` and exit code 1. Keep the session alive after the awaited text appears:

```sh
zeropty new-session -d -s build "sh -c 'sleep 1; echo build done; exec cat'"   # exec cat at the end
zeropty wait-pane -t build -m 'build done' -T 10000
zeropty kill-session -t build
```

The usual flow, starting a long-running program such as a shell (`sh`, `zsh`) or claude as the session and giving it work with `send-keys` ([Quick start](../README.md#quick-start)), does not have this problem.

## Input stops arriving after I send `C-d`

The session's socket has no way to signal "EOF once" the way a PTY does, so `C-d` on an empty line shuts the input. After that, nothing more can be sent to that session. Unless you mean to end the program, send `C-c` or the program's own quit command instead of `C-d`.

## `-t =work:` behaves oddly in zsh

zsh turns `=word` into the path of a command (`=` expansion). Quote it, as in `'=work:'`. The same goes for tmux.

## I get `no server running on ...`

No server is listening on that socket. The first `new-session` starts the server. To talk to a server on another socket (for example an embedded app's `listen`), name the socket: `ZEROPTY_SOCK=<socket> zeropty ...`.

## Starting many sessions fails

There is no PTY limit, but macOS does limit the number of processes per user. One claude session is about 4 processes.

```sh
sysctl kern.maxprocperuid     # process limit per user
ps -U $(id -u) | wc -l        # how many processes you have now
```

Often leftover test or benchmark servers are using up the limit. Send `kill-server` to their sockets to clean up.

## How do I stop the server?

```sh
zeropty kill-server
```

The server sends SIGHUP to every session's process group and exits. For a server on another socket, give `ZEROPTY_SOCK` as well.
