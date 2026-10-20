# Usage

**English** · [한국어](ko/USAGE.md)

[README](../README.md) · [Docs](README.md) · [Usage](USAGE.md) · [Plugins](PLUGINS.md) · [FAQ](FAQ.md) · [Benchmarks](BENCHMARKS.md) · [Design](DESIGN.md) · [Versioning](VERSIONING.md) · [Releasing](RELEASING.md)

This is the complete reference for zeropty's commands, options and protocols. If you are new, start with the [one-minute start in the README](../README.md#quick-start).

- [Basic concepts](#basic-concepts)
- [CLI at a glance](#cli-at-a-glance)
- [Command reference](#command-reference)
  - [Sessions](#sessions) · [Sending keys](#sending-keys) · [Reading the screen](#reading-the-screen) · [Waiting and searching](#waiting-and-searching) · [Buffers](#buffers) · [Viewing](#viewing-attach) · [Environment](#environment) · [Server and moving sessions](#server-and-moving-sessions) · [Diagnostics](#diagnostics)
- [Key names](#key-names) · [Format variables](#format-variables) · [Exit codes](#exit-codes)
- [Fast from the shell: zsh function, bash builtin](#fast-from-the-shell-zsh-function-bash-builtin)
- [Programs that need a terminal: zeropty-tty](#programs-that-need-a-terminal-zeropty-tty)
- [Key input and the line discipline](#key-input-and-the-line-discipline)
- [From other programs: the socket](#from-other-programs-the-socket)
- [Inside your process: embedded](#inside-your-process-embedded)
- [Moving sessions](#moving-sessions)
- [Environment variables](#environment-variables)
- [Feature comparison with tmux](#feature-comparison-with-tmux)

## Basic concepts

- **A session is one program.** `new-session` launches a single command with `/bin/sh -c`. There are no windows or panes; each session has one screen and its scrollback.
- **The server.** The `zeropty` CLI sends commands to a server (`~/.zeropty/mux.sock`). If no server is running, `new-session` (and `pull-sessions`) starts one, just like tmux. You can also put the same engine inside your own process ([embedded](#inside-your-process-embedded)).
- **Targets (`-t`).** A target names a session. `=work`, `=work:`, `work` and `work:0` all mean the session `work`. The name must **match exactly** (there is no tmux-style prefix matching). In scripts, prefer `=name:` as you would with tmux.
- **The command format is the same everywhere.** CLI arguments, the JSON `argv` on the socket, embedded function calls and the C ABI all take the same `["send-keys", "-t", "=work:", "Enter"]` and return the same result (`code`, `out`, `err`).
- **Unknown options are errors** (`send-keys: unknown flag -X`). They used to be silently ignored, which changed results; for example, `send-keys -N 3` would send only once.

## CLI at a glance

```sh
zeropty new-session -d -s work -x 220 -y 80 -c ~/proj "$PWD/dist/claude-zeropty"   # starts the server if none is running
zeropty capture-pane -p -e -S -200 -t '=work:'    # screen + 200 lines of scrollback, with colors
zeropty wait-pane -t '=work:' -m '>' -q 1000 -T 60000   # until '>' appears and the session is quiet for 1s (60s max)
zeropty send-keys -t '=work:' -l 'hello' && zeropty send-keys -t '=work:' Enter
zeropty attach -t work                            # attach to watch as a human; detach with Ctrl-b d
zeropty attach -r -t work                         # read-only (detach with q or Ctrl-c)
zeropty kill-session -t '=work:'
```

**Help and version.** `zeropty --help` (or `-h`, `help`) prints the list of commands and options, and `zeropty help <command>` prints that command's usage, what it does and a link to the matching section of this document. Both write to standard output and exit with code 0. Abbreviations (`help send`) work too. The program answers on its own, so no server is needed and none is started. The zsh function and the bash builtin behave the same way. `zeropty -V` (or `--version`) prints the version of the CLI itself (for the version of the running server, use `zeropty display -p '#{version}'`). What counts as compatible is described in [VERSIONING.md](VERSIONING.md), and what changed is in [CHANGELOG.md](../CHANGELOG.md). Each option is explained in the [command reference](#command-reference) below.

```sh
zeropty --help     # list of commands (exit code 0)
zeropty help send-keys   # one command: usage, what it does, doc link (exit code 2 for an unknown command)
zeropty -V         # zeropty 0.1.0 (--version does the same)
zeropty            # usage: zeropty serve | <tmux command> ...  (standard error, exit code 2)
```

## Command reference

Names in parentheses are the same aliases tmux uses. `<value>` after an option means the option takes a value.

### Sessions

| Command | Options | What it does |
|---|---|---|
| `new-session` (`new`) `[command]` | `-d` `-s <name>` `-x <cols>` `-y <rows>` `-c <dir>` `-e <VAR=value>` `-A` `-P` `-F <format>` | Creates a session. Without a command it runs `$SHELL` (or `/bin/sh` if unset). The default size is 80×24, and without a name it uses the number of sessions created so far (`0`, `1`, ...). `-A` succeeds and leaves the session alone if one with the same name exists. `-P` prints information about the new session (`-F` sets the format; the default is `name:`). `-e` can be repeated. `-n -E -D -X` are for windows and clients, so they are accepted and ignored |
| `has-session` (`has`) | `-t` | 0 if the session exists, 1 if not (`can't find session: name`) |
| `kill-session` | `-t` `-a` | Sends SIGHUP to the session's process group, then SIGKILLs any process still alive after 2 seconds. `-a` means every session except the `-t` session. `-C` is accepted and ignored |
| `list-sessions` (`ls`) | `-F <format>` | Lists sessions. The default output matches tmux: `name: 1 windows (created ...)` |
| `rename-session` (`rename`) `<new name>` | `-t` | Renames a session. An empty name, a name containing `:` or `.`, or a name already in use is an error |
| `resize-window` (`resizew`), `resize-pane` (`resizep`) | `-t` `-x <cols>` `-y <rows>` | Resizes the screen. The program receives SIGWINCH |
| `pipe-pane` (`pipep`) `[command]` | `-t` `-o` `-O` | Keeps feeding the session's output into the command's input (tmux's `-O` is the default). Calling it without a command closes the pipe. `-o` only closes it if one is already open. Output the command cannot accept yet is buffered up to 8MB, and anything beyond that is dropped. The pipe is closed when the session moves to another process |
| `kill-server` | | Sends SIGHUP to every session. A standalone server process also exits. An embedded engine ends only the sessions and keeps running |

When the program exits, the session ends too (the same as tmux with `remain-on-exit` off).

Environment the program in a session receives: `TERM=xterm-256color`, `COLUMNS`/`LINES`, `ZEROPTY_SESSION` (the session name), `ZEROPTY_COLS`/`ZEROPTY_ROWS`, and `ZEROPTY_SIZE_FILE` (the window size file the shim reads). `TMUX` and `TMUX_PANE` are removed (even if the server was started inside tmux, the session is not inside tmux).

### Sending keys

| Command | Options | What it does |
|---|---|---|
| `send-keys` (`send`) `<key>...` | `-t` `-l` `-H` `-N <count>` | Sends keys. For each argument, a [key name](#key-names) sends that key's bytes, and anything else is sent as literal text. `-l` sends the text literally without interpreting key names. `-H` reads the arguments as hexadecimal bytes (`-H 41 42 0a` is `AB` plus a newline). `-N` repeats the whole thing that many times |
| `paste-buffer` (`pasteb`) | `-t` `-b <buffer>` `-d` `-p` `-r` `-s <separator>` | Sends the buffer's contents as keys. Newlines are sent as `\r` (unchanged with `-r`; `-s` sets the separator). `-p` wraps the text in bracketed paste if the program has enabled it. `-d` deletes the buffer after sending |

Keys pass through the [line discipline](#key-input-and-the-line-discipline), just as with a PTY.

### Reading the screen

| Command | Options | What it does |
|---|---|---|
| `capture-pane` (`capturep`) | `-t` `-p` `-e` `-J` `-N` `-S <line>` `-E <line>` `-b <buffer>` `-q` | Reads the screen as text. Pending output is processed right before reading, so the screen is always current. `-e` includes color and font escapes, `-J` joins lines that were split by wrapping, and `-N` keeps trailing spaces. `-S`/`-E` use the same line numbers as tmux (0 is the first screen line, negative numbers are scrollback, `-` means to the end). Without `-p`, as in tmux, the screen goes into a buffer (`-b`, or `buffer0` by default) and nothing is printed |
| `display-message` (`display`) `[format]` | `-t` `-p` `-F <format>` | Prints the format with [format variables](#format-variables) filled in. Example: `display-message -p -t '=work:' '#{cursor_x},#{cursor_y}'` |

### Waiting and searching

These are zeropty commands that tmux does not have. They tell you, without polling, when an agent has finished or which session is waiting for input.

| Command | Options | What it does |
|---|---|---|
| `wait-pane` | `-t` `-m <text>` `-q <ms>` `-T <ms>` | `-m` waits until the text appears on screen (immediately if it is already there), and `-q` waits until the session has printed nothing for that long. With both, it answers once both hold. After `-T` it exits with code 1 (`wait-pane: timed out`), and it also exits with 1 if the session ends while waiting (`wait-pane: session ended`) |
| `grep-panes <text>` | `-S <line>` | Searches every session's screen for the text and shows matches as `session: line`. `-S` also searches scrollback, like `capture-pane -S` (`-S -100`). Results are sorted by session name, and the exit code is 1 when nothing matches (like grep) |

- `wait-pane` checks only when output arrives at the server, and only the rows that output changed. Text is matched within a row (text split across a line wrap is not found).
- The next command on the same connection is handled after `wait-pane` answers. `client/zeropty.mjs` opens a separate connection for each `wait-pane`, so other commands are not blocked.
- If a session exits right after printing its last output, the engine may report the session's end before it checks that output, and `wait-pane` then ends with `wait-pane: session ended` (exit code 1). This is a known issue. Keep the session alive after the text you are waiting for appears, for example: `sh -c 'make; echo build done; exec cat'`
- `wait-pane` is not available through the embedded engine's function call (there is no loop that could defer the answer). It works over the socket once you enable the [server plugin](#server-plugin-your-program-opens-the-socket-too). Rust hosts use `Engine::wait_start` and `wait_check` directly ([`examples/polled.rs`](../examples/polled.rs)).

### Buffers

| Command | Options | What it does |
|---|---|---|
| `load-buffer` (`loadb`) `<file>` | `-b <buffer>` | Loads a file's contents into a buffer (`buffer0` by default). Relative paths are resolved against the current directory of the side that sent the command |
| `paste-buffer` | | See [Sending keys](#sending-keys) |
| `capture-pane -b` | | Without `-p`, puts the screen into a buffer |

### Viewing (attach)

| Command | Options | What it does |
|---|---|---|
| `attach-session` (`attach`, `a`) | `-t` `-r` | Attaches the current terminal to a session. Detach with `Ctrl-b d`. `-r` is read-only; detach with `q` or `Ctrl-c`. Without `-t`, it attaches to the only session if there is one, and asks for a number if there are several |

While attached, the session follows your terminal's size, and it returns to its original size when you detach. Several terminals can attach to one session; the size follows the terminal that attached last.

### Environment

| Command | Options | What it does |
|---|---|---|
| `set-environment` (`setenv`) `<VAR> [value]` | `-g` `-u` | Sets an environment variable for sessions created afterwards. `-u` removes it. Without `-g`, even tmux applies the variable only to new windows, and since sessions never get new windows, it does nothing |
| `show-environment` (`showenv`) | `-g` | Shows the variables set with `-g`. Removed variables are shown as `-VAR` |

### Server and moving sessions

| Command | Options | What it does |
|---|---|---|
| `serve` | `--bind <socket>` | Starts a standalone server yourself (normally `new-session` starts one for you) |
| `pull-sessions` `[name...]` | `-s <socket>` | Brings sessions from another server into this one (all of them if no names are given) |
| `push-sessions` `[name...]` | `-s <socket>` | Sends this server's sessions to another server |
| `restart-server` | | A new server takes over every session and the socket. Use it for upgrades |

Details: [Moving sessions](#moving-sessions). The internal requests servers use with each other, `export-sessions` and `import-sessions`: [socket protocol](#socket-protocol-export-sessions-import-sessions)

### Plugins

| Command | Options | What it does |
|---|---|---|
| `list-plugins` | | One line per attached plugin: `name: state (api N) events=... commands=... access=... dropped=N`. The state is `running`, `stopped` or `failed (reason)` |
| `plugin-attach` | | Turns the socket connection into a plugin. Only meaningful on the socket |
| `plugin-run` `<library.dylib>` | | Loads a C plugin library in a separate process and attaches it to the server. It exits when the server exits |

Commands added by plugins are called from the CLI, the socket and the embedded engine just like built-in commands. If you set `ZEROPTY_PLUGIN_DIR`, `zeropty serve` starts the plugins in that directory and restarts them when they exit. **If it is not set (the default), nothing is run.** Pointing at a directory is consent to run the code in it as the same user as the server, so this is off by default (the same behavior as before this feature existed). How to attach plugins and the protocol: [PLUGINS.md](PLUGINS.md)

### Diagnostics

| Command | What it does |
|---|---|
| `stats` | One line of engine counters: `sessions`, `heap_bytes` (bytes requested from the allocator), `screen_bytes`, the number of `live`/`frozen` screens, the number of reads, and more |
| `mstats` | Allocator (mimalloc) statistics. Only on a standalone server (the command counts that process's allocator) |
| `mem -t <session>` | Memory breakdown of one screen. For a frozen screen: `frozen <bytes> B deep <true/false>` |

## Key names

`Enter`, `Escape`, `Tab`, `BTab`, `BSpace`, `Space`, `Up`, `Down`, `Left`, `Right`, `Home`, `End`, `PageUp` (`PgUp`, `PPage`), `PageDown` (`PgDn`, `NPage`), `IC` (`Insert`), `DC` (`Delete`), `F1`\~`F12`.
Combine them with the prefixes `C-` (Ctrl), `M-` (Alt) and `S-` (Shift) (`C-c`, `M-x`, `S-Up`; `^c` also works). Names are case-insensitive. The bytes sent were checked against tmux 3. Arguments that are not key names are sent as literal text.

When a program turns on application cursor mode (vim and others), the arrow keys switch to the `ESC O A` form, the same as tmux.

## Format variables

Used by `list-sessions -F`, `new-session -P -F` and `display-message`.

| Variable | Meaning |
|---|---|
| `session_name`, `session_id`, `session_created`, `session_attached`, `session_windows` | Session name, id starting with `$` (`pane_id` starts with `%`), creation time (Unix seconds), number of attached terminals, number of windows (always 1) |
| `window_index`, `window_id`, `window_name`, `window_width`, `window_height`, `window_panes` | Window information (there is always exactly one window) |
| `pane_id`, `pane_index`, `pane_pid`, `pane_width`, `pane_height` | Pane information and the program's pid |
| `pane_current_command`, `pane_current_path` | Name and directory of the command currently running |
| `pane_dead`, `pane_in_mode` | Always `0` (for tmux script compatibility; `window_index` and `pane_index` are always `0` too) |
| `cursor_x`, `cursor_y`, `alternate_on` | Cursor position (from 0) and whether the alternate screen is in use |
| `history_size`, `history_limit` | Number of scrollback lines and the limit |
| `version` | zeropty version of the answering server (or embedded engine) |

These were checked against tmux on the same sessions. A format that uses only `#{version}` works without a session (`zeropty display -p '#{version}'` gives the running server's version; `zeropty -V` gives the CLI's own version). Unknown variables become an empty string, as in tmux, and conditionals (`#{?...}`) and modifiers (`#{t:...}`) are not supported.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Failure: missing session, unknown option, `wait-pane` timeout or session end, "not found" from `grep-panes` or `has-session`, no server (`no server running on ...`) |
| 2 | Usage error: run without arguments, `zeropty help <unknown command>`, unknown argument to `attach` |

## Fast from the shell: zsh function, bash builtin

Running the `zeropty` program starts a process for every command (2.47\~2.60ms). Talking to the socket directly from inside the shell removes that cost.

- **zsh:** add `source /path/to/zeropty/shell/zeropty.zsh` to `.zshrc` and `zeropty` becomes a shell function (p50 `send-keys -l` 0.11ms, 42.3\~43.4x faster than the tmux CLI; `has-session` 0.11\~0.20ms, 21.7\~40.4x faster; a 2000-line `capture-pane` 0.45\~0.48ms, 10.3\~11.1x faster). It uses the `zsh/net/socket` module that ships with zsh.
- **bash:** run `enable -f /path/to/zeropty/dist/zeropty_bash.dylib zeropty` once and `zeropty` becomes a builtin (0.035\~0.115ms per command, 37.8\~133x faster than the tmux CLI). Verified with macOS's default `/bin/bash` (3.2).
- Both run the program for: running with no arguments, `attach` (`attach-session`, `a`), `serve`, `restart-server`, `-V`/`--version`, `-h`/`--help`/`help`, and any command when no server is running (because `new-session` has to start the server). Set `ZEROPTY_BIN` to change the program's location.
- In zsh, `=work:` is turned into a command path (`=` expansion). Quote it as `'=work:'`. The same applies to tmux.

## Programs that need a terminal: zeropty-tty

A session is a pipe, not a PTY, so programs that check for a terminal with `isatty()` (vim, less, the python REPL, claude) decide they are not on a terminal when launched directly. Launch them with `zeropty-tty` instead.

```sh
zeropty new-session -d -s v "dist/zeropty-tty vim notes.txt"
```

- The shim (`dist/shim.dylib`) answers `isatty`, `tcgetattr`, the window size (`TIOCGWINSZ`), `tcgetpgrp` and `/dev/tty` as a terminal would. It is injected with `DYLD_INSERT_LIBRARIES`.
- macOS does not let the shim into SIP-protected binaries such as those in `/usr/bin`, or into binaries signed with the hardened runtime. So `zeropty-tty` runs such programs from a copy with only the signature changed (`~/.zeropty/tty/`), and rebuilds the copy when the original changes.
- The copy does not carry the original's privileges, so `top` and `ps` do not work. For scripts that go through `/usr/bin/env`, SIP strips the shim; launch the interpreter directly with `zeropty-tty`.
- `dist/claude-zeropty` is a small wrapper that runs `zeropty-tty claude`. To use a different claude, give its path in `ZEROPTY_CLAUDE_REAL`.

## Key input and the line discipline

Keys pass through a line discipline, just as with a PTY. A new session starts in cooked mode, the same as a fresh PTY.

- Enter becomes a newline, so `read` returns.
- `C-c`, `C-\` and `C-z` go to the process group as signals.
- Input is collected line by line and can be edited with `BSpace`, `C-u`, `C-w` and `C-v`; `C-d` on an empty line is end of input (EOF).
- Typed characters are echoed to the screen. So if you send `send-keys -l 'echo hi'`, `echo hi` appears on screen first. When waiting for a result with `wait-pane -m`, pick text that is not in the line you typed.
- When a program switches to raw mode (claude, vim, the python REPL: the shim reports it), keys pass through unchanged.
- A program without the shim that wants raw mode prints `printf '\033]zeropty-tty;0\007'` (the equivalent of `stty raw -echo`).
- **One difference from a PTY:** a pipe has no way to signal EOF just once, so the EOF from `C-d` closes the input. No input is accepted after that.

## From other programs: the socket

Send one line of JSON to the server socket (`~/.zeropty/mux.sock`; change it with `ZEROPTY_SOCK`) and it answers with one line. One connection can carry many commands, and answers come back in order. The socket has 0600 permissions, so only the same user (and root) can connect.

```
→ {"argv":["capture-pane","-p","-t","=work:"],"cwd":"/"}
← {"code":0,"out":"...screen...","err":""}
```

- `cwd` is the base directory for relative paths (`new-session -c`, `load-buffer`).
- If the request includes `"raw":true`, the answer is not JSON but a line `<code> <out byte count> <err byte count>` followed by the raw out and err bytes. This is for clients that have a hard time parsing JSON (the zsh function).
- An `attach` connection exchanges frames after the first answer: `[kind u8][length u32 LE][bytes]`. Server to client: 0 (output), 1 (session ended). Client to server: 0 (keys), 1 (size: cols and rows as u16 LE), 2 (detach).
- A connection whose first request is `export-sessions` or `import-sessions` is used to hand sessions over (internal): [socket protocol](#socket-protocol-export-sessions-import-sessions).

```sh
# From any language: for example, with nc
printf '{"argv":["list-sessions"],"cwd":"/"}\n' | nc -U ~/.zeropty/mux.sock
```

### Node client

`client/zeropty.mjs` keeps one socket connection open and sends commands over it. Replace your tmux calls with `zeropty(args)`.

```js
import { zeropty, zeroptySync } from './client/zeropty.mjs'
const r = await zeropty(['capture-pane', '-p', '-t', '=work:'])   // { ok, code, out, err }
```

| Function | What it does |
|---|---|
| `zeropty(argv, { cwd, sock, timeout })` | Sends one command and resolves to `{ ok, code, out, err }`. If no server is running, it runs the CLI (`new-session` starts the server). Default timeout 15 seconds |
| `zeropty(['wait-pane', ...])` | Opens a separate connection, so other commands are still handled immediately on the shared connection while it waits. No default timeout (set one with `-T` or `{ timeout }`) |
| `zeroptySync(argv, { cwd, sock })` | The same, synchronously. Requires `dist/zeropty.node`, and blocks the thread for 0.11\~0.20ms per command |
| `sockPath()`, `cliPath()` | Paths of the socket (`ZEROPTY_SOCK`) and CLI (`ZEROPTY_BIN`) in use |

Runnable example: [`examples/node/quickstart.mjs`](../examples/node/quickstart.mjs)

## Inside your process: embedded

Put the engine inside your process, with no server. The command format and results are the same as on the socket. The one exception is `wait-pane`, which is not available as a function call (see [above](#waiting-and-searching)). Sessions live only as long as this process does (when the process exits, the programs get EOF). To keep them, hand them to a server before exiting (`push-sessions`, below).

**Node:** just change the import. Example: [`examples/node/embedded.mjs`](../examples/node/embedded.mjs)

```js
import { zeropty, open } from './client/embedded.mjs'   // instead of './client/zeropty.mjs'
await zeropty(['new-session', '-d', '-s', 'w', 'exec cat'])
```

| Function | What it does |
|---|---|
| `zeropty(argv, { cwd })`, `zeroptySync(argv, { cwd })` | Same calls and same results as the server client. `send-keys` p50 0.04\~0.06ms, `capture-pane` 0.06\~0.07ms (busy session 0.09\~0.12ms) ([measurements](BENCHMARKS.md)) |
| `open({ history, sizeDir, polled })` | Starts the engine with options before the first command (if you don't call it, the first command starts the engine with defaults) |
| `listen(socket)` | [Server plugin](#server-plugin-your-program-opens-the-socket-too) |

**C (or any language with an FFI):** the header is [`bindings/c/include/zeropty.h`](../bindings/c/include/zeropty.h), and the example is [`examples/c/hello.c`](../examples/c/hello.c).

```c
#include "zeropty.h"   // cc app.c -Idist -Ldist -lzeropty_c
zeropty *a = zeropty_open(NULL);   // or {"history": 2000, "size_dir": "/tmp"}
char *r = zeropty_command(a, "{\"argv\":[\"new-session\",\"-d\",\"-s\",\"x\",\"exec cat\"],\"cwd\":\"/\"}");
zeropty_free(r);                // r is {"code":0,"out":"","err":""}
zeropty_close(a);
```

| Function | What it does |
|---|---|
| `zeropty_open(options_json)` | An engine with its own engine thread. Options are `NULL` or `{"history": lines, "size_dir": "dir"}` |
| `zeropty_command(a, request_json)` | One command. The answer is a JSON string; release it with `zeropty_free` |
| `zeropty_listen(a, path)` | Also open a socket. Returns 0, or -1 on failure |
| `zeropty_close(a)` | Stops the engine. Sessions keep running until their programs exit (send `kill-server` first to end them) |
| `zeropty_open_polled`, `zeropty_fd`, `zeropty_timeout_ms`, `zeropty_process` | Run the engine from the host's loop, with no engine thread (below) |

Commands on a `zeropty_open` engine may be called from any thread. Reading the screen, sending keys and queries are handled right on the calling thread; everything else is handed to the engine thread.

**Rust:** the crate is not on crates.io yet, so add it as a git dependency. To stay on one commit, add `rev = "<commit>"`.

```toml
# Cargo.toml
[dependencies]
zeropty = { git = "https://github.com/CreatiCoding/zeropty" }
```

Then use `zeropty::embed::Embedded` (engine thread) or `zeropty::embed::Polled` (host loop). For API docs, run `cargo doc --no-deps --workspace --open` in this repository; for examples, `cargo run --release --example embed` and `cargo run --release --example polled`.

```rust
use zeropty::{embed::Embedded, engine::Options};

fn main() {
    let zp = Embedded::start(Options::default());
    let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let r = zp.command(&argv(&["new-session", "-d", "-s", "w", "exec cat"]), "/");
    assert_eq!(r.code, 0, "{}", r.err);
    zp.command(&argv(&["send-keys", "-t", "=w:", "-l", "hello"]), "/");
    print!("{}", zp.command(&argv(&["capture-pane", "-p", "-t", "=w:"]), "/").out);   // hello
    zp.command(&argv(&["kill-server"]), "/");
}
```

Building requires macOS and the Xcode Command Line Tools (`zstd-sys` compiles C code). The shim used to launch programs that need a terminal is not part of the crate; for that, use `dist/zeropty-tty` built with this repository's `./build.sh`.

**Polled: no engine thread (the Node default; 400 idle sessions go from 0.72MB to 0.39\~0.41MB).** The host's event loop drives the engine. Make every call from that loop's thread. In Node, libuv watches the engine fd, so there is nothing extra to do.

```js
open({ polled: false })   // to use an engine thread in Node (when calling from worker threads). Call before the first command
```

```c
zeropty *a = zeropty_open_polled(NULL);
for (;;) {   // host loop: call zeropty_process when the engine fd is readable or the timeout expires
  struct pollfd p = { zeropty_fd(a), POLLIN, 0 };
  poll(&p, 1, (int)zeropty_timeout_ms(a));
  zeropty_process(a);
}
```

Commands added by plugins can hold the calling thread while they wait for an answer (10-second deadline by default). To avoid holding the host loop, start them with `zeropty_command_start` and collect the answer with `zeropty_call_poll` after `zeropty_process` (Rust: `Polled::command_start`; Node: `await zeropty(...)`). Engine commands called this way already have their answer ready. Details are in [PLUGINS.md](PLUGINS.md#calling-plugin-commands-from-the-host).

### Server plugin: your program opens the socket too

Calling `listen(socket)` runs the same server as `zeropty serve` inside your program. The `zeropty` CLI, `zeropty attach`, the zsh function, the bash builtin and `client/zeropty.mjs` can then work with this program's sessions without a separate server process. Connections are handled on the same loop as the engine (the host loop when polled, the engine thread otherwise).

```js
import { zeropty, listen } from './client/embedded.mjs'
listen(`${process.env.HOME}/.zeropty/myapp.sock`)   // throws if another server already answers on this socket
```

```sh
ZEROPTY_SOCK=~/.zeropty/myapp.sock zeropty capture-pane -p -t '=w:'   # from another terminal
ZEROPTY_SOCK=~/.zeropty/myapp.sock zeropty attach -t w
```

```c
if (zeropty_listen(a, "/tmp/myapp.sock") != 0) { /* could not open it */ }
```

- The socket path must be 103 bytes or less. The socket gets 0600 permissions, and newly created directories get 0700.
- If a server already answers at that path, `listen` fails (Node throws, C returns -1). A socket file nobody answers on (left behind by a server that exited) is taken over; if a non-socket file is in the way, it fails without deleting it.
- `kill-server` from the socket ends only the sessions. Your program keeps running and the socket stays open (a standalone server process exits).
- Closing the engine (`zeropty_close`, or dropping Rust's `Embedded`/`Polled`) also removes the socket file. If another server has taken the same path in the meantime, its socket is left alone.
- Sessions still end together with your program. To keep them beyond its lifetime, `push-sessions` them to a standalone server before exiting.
- If the engine stops or the process exits, the socket file stays behind. Since nobody answers on it, the next `listen` takes it over.
- `mstats` is only on a standalone server.

## Moving sessions

Moves sessions to another process. **Running programs keep running** (even the pid stays the same). The screen and scrollback are transferred as bytes, and the pipes by fd passing (SCM_RIGHTS).

```sh
zeropty pull-sessions -s ~/.zeropty/other.sock [name...]   # another server's sessions into this one (all if no names)
zeropty push-sessions -s ~/.zeropty/other.sock [name...]   # this server's sessions to another server
zeropty restart-server                                  # a new server takes over every session and the socket
```

- The same commands work on an embedded engine. For example, a Node app can `pull-sessions` from the server when it starts and `push-sessions` them back when it exits, so sessions survive app restarts.
- `restart-server` starts the new server from the `zeropty` binary you just ran. That makes it the way to **upgrade without losing sessions**. Commands that arrive during the few ms of the switch go to the old server.
- The sending side releases only the sessions the receiver confirmed it took. If anything fails midway, the sessions stay where they were. Sessions whose name already exists on the other side are not moved.
- **Limitation:** if an escape sequence was half received at the moment of the move, the rest of it is printed as text.

### Socket protocol: export-sessions, import-sessions

These are **internal** requests that `pull-sessions`, `push-sessions` and `restart-server` use on the server socket. You will rarely send them yourself, and the session record format may change between versions (the first byte is the record version, and a record from another version is rejected with `session record of another version`). This section is kept as a human-readable reference. Implementation: `src/engine/handover.rs`.

- If the **first request line** of a connection is `{"argv":["export-sessions", name...],"cwd":""}` or `{"argv":["import-sessions"],"cwd":""}`, the connection is used only to hand over sessions and is closed when done. Meanwhile the server does nothing else (usually a few ms).
- Each side waits up to 10 seconds per step.
- Each session is sent as `[record length u32 LE][record]`. The pipe fds (whichever of input and output are open, 0\~2 of them) are attached to the 4-byte length with SCM_RIGHTS. A length of 0 marks the end. A record may not exceed 256MB.
- Every other line is a single line of JSON.

```
export-sessions (taker → server)
→ {"argv":["export-sessions","a","b"],"cwd":""}     all sessions if no names
← [length][record] ... [0]                          each session with its fds
← ["b"]                                             names the server does not have
→ {"taken":["a"],"failed":[]}                       sessions taken; the server releases only these

import-sessions (giver → server)
→ {"argv":["import-sessions"],"cwd":""}
← "ready"
→ [length][record] ... [0]
← {"taken":["a"],"failed":["duplicate session: b\n"]}   the giver releases only the taken ones
```

`failed` holds the reason a session was not taken (a session with the same name already exists, a different record version, and so on). If the connection drops or the format is wrong at any step, the receiver releases the sessions it took, and the sessions stay on the original side.

## Environment variables

| Variable | Default | Meaning |
|---|---|---|
| `ZEROPTY_SOCK` | `~/.zeropty/mux.sock` | Server socket (path of 103 bytes or less) |
| `ZEROPTY_HISTORY` | 500 | Number of scrollback lines |
| `ZEROPTY_COALESCE_MS` | 300 | Interval for batching output reads. Once 16KB or more has piled up it is read immediately, and screen reads always process pending output first. Attached terminals get output right away |
| `ZEROPTY_FREEZE_MS` | 250 | After this much quiet, the whole screen is frozen |
| `ZEROPTY_COLD` | file | Where compressed copies of long-quiet screens are kept. `memory` keeps them in process memory |
| `ZEROPTY_COLD_DIR` | temp directory | Directory where the compressed-copy file is created (its name is unlinked right after creation) |
| `ZEROPTY_RECORD` | – | Directory to record session output into as `<name>.raw`/`.trace` |
| `ZEROPTY_PURGE_MS` | 0 | Standalone server: how long to wait before returning freed memory to the OS |
| `ZEROPTY_BIN` | `dist/zeropty` | Program run by the Node client, the zsh function and the bash builtin |
| `ZEROPTY_NODE` | `dist/zeropty.node` | Native module loaded by the Node client |
| `ZEROPTY_SHIM` | `dist/shim.dylib` | Shim injected by `zeropty-tty` |
| `ZEROPTY_CACHE` | `~/.zeropty` | Where `zeropty-tty` keeps re-signed copies (under `tty/`) |
| `ZEROPTY_CLAUDE_REAL` | `claude` on `PATH` | The claude that `claude-zeropty` launches |
| `ZEROPTY_PLUGIN_DIR` | (unset: off) | Plugin directory the server launches. Only when set are the executables and `.dylib` files in it run. A server started by `restart-server` or `new-session` takes the value from the shell that ran that command ([PLUGINS.md](PLUGINS.md#autostart-the-plugin-directory)) |
| `ZEROPTY_PLUGIN_TIMEOUT_MS` | 10000 | Deadline for plugin commands and hooks. A plugin's `Host::command` waits up to half of it |

## Feature comparison with tmux

zeropty focuses on **keeping programs running, reading their screens and sending them keys**. Most features for humans sharing a terminal are not there.

| Feature | tmux | zeropty |
|---|---|---|
| Create, kill and list sessions, send keys, read the screen | ✅ | ✅ same arguments |
| `capture-pane` options | many | `-p -e -J -N -S -E -b -q` (others are errors) |
| Buffers | `load/paste/set/show/save-buffer` and more | `load-buffer`, `paste-buffer`, `capture-pane -b` |
| `pipe-pane`, `rename-session`, `resize-window`, `set-environment -g` | ✅ | ✅ (no `pipe-pane -I`) |
| Format variables `#{...}` | hundreds | 25 session, window, pane, cursor, history and process variables (no conditionals or modifiers) |
| Attach to watch as a human (`attach`) | ✅ | ✅ read-only (`-r`), Ctrl-b d, follows the terminal size |
| Windows and panes | ✅ | ❌ one program per session |
| Status line, copy mode, key bindings, config file, hooks | ✅ | ❌ |
| Several clients attached to one session | ✅ | ✅ (the size follows the terminal that attached last) |
| PTY | 1 per session (macOS limit 511\~999) | not used; 1000+ sessions |
| Full-screen programs (vim and others) | ✅ | Launch them with `zeropty-tty <program>`. Verified with vim (editing, saving), less (paging, quitting), the python REPL (line editing, history) and claude. System programs that need privileges (setuid, entitlements), such as `top` and `ps`, do not work |
| Access for programs | control mode (`-C`) | JSON socket protocol, Node client |
| Search text across every session's screen | ❌ `capture-pane` per session | ✅ `grep-panes` (12\~17ms for 1000) |
| Line discipline for key input (Enter, `C-c` signals, line editing, echo, `C-d`) | ✅ PTY | ✅ emulated by the engine (same keys checked against tmux). EOF from `C-d` closes the input |
| Wait until text appears on screen or output stops | ❌ repeatedly check with `capture-pane` | ✅ `wait-pane` (the server checks only when output arrives; detection 2ms) |
| Embed as a library | ❌ | ✅ C ABI, Node module, Rust crate |
| Move running sessions to another process | ❌ | ✅ `pull-sessions`, `push-sessions` |
| Keep sessions across server restarts and upgrades | ❌ | ✅ `restart-server` |
| Platforms | most Unix systems | macOS only (kqueue, DYLD shim) |
