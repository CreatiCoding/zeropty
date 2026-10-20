# Changelog

**English** · [한국어](docs/ko/CHANGELOG.md)

Every user-visible change to zeropty is recorded here.

The format follows [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/) (the section names `Added`, `Changed`,
`Deprecated`, `Removed`, `Fixed` and `Security` are read by tools), and versions follow
[Semantic Versioning](https://semver.org/). What counts as a breaking change (CLI, socket protocol, C ABI, Node API,
environment variables, ...) is in [docs/VERSIONING.md](docs/VERSIONING.md), and how entries get here is in
[docs/RELEASING.md](docs/RELEASING.md).

Entries are written for people who use zeropty, not as a commit log. A user-visible change adds a line under
`[Unreleased]` in the same PR. `node scripts/release.mjs notes` drafts entries from the Conventional Commits since the last
release; polish that draft into entries written in English, and add the same entries in Korean to
[docs/ko/CHANGELOG.md](docs/ko/CHANGELOG.md). Breaking changes start with `**BREAKING**`.

## [Unreleased]

### Fixed

- `zeropty-tty`: `/dev/tty` opened for reading and writing (`fzf` with its stdin piped in) could read keys but every
  write failed with `EBADF`, so fzf drew no list and took no pick. A session's terminal is now one socket, as a PTY is
  one file: stdin, stdout and stderr are that socket (stdin was a pipe), and `/dev/tty` in any mode is it. `C-d` still
  ends input for good, and output keeps flowing after it. As on a PTY, `O_NONBLOCK` set on one of stdin, stdout and
  stderr is now set on all three. Sessions started by an older server and kept by `restart-server` keep their input
  pipe; sessions started by this version should not be handed back to 0.1.1 or earlier (`C-d` would not reach them).

## [0.1.1] - 2026-10-20

### Fixed

- `zeropty-tty`: the shim treated any program's fds 0-2 as a terminal, so a program whose stdout was piped or
  redirected got `\e]zeropty-tty;N\a` mode signals mixed into that output, a program reading `/dev/null` as stdin
  failed (kqueue `EINVAL`), and children inherited the effect. Now only the session's own pipe and socket count, named
  by the server in the new `ZEROPTY_TTY_ID` variable, and mode signals go only to the session's output.
- `zeropty-tty`: the signed copy took the name of the link's target (`claude` ran as `claude.exe`), so `ps` and
  `pgrep claude` did not find it. A program, copied or not, now runs under the name it was run by.
- `display-message` with `-t` naming a session that does not exist now prints the format with the session's
  variables empty and exits 0, as tmux does, instead of failing with exit 1. To test whether a session exists, use
  `has-session`. Every other command still fails with exit 1, as in tmux.
- A missing `-t` target is now reported in tmux's words. Without a `:`, a bare name gives `can't find pane: x`
  (`capture-pane`, `send-keys`, `pipe-pane`, `paste-buffer`, `resize-pane`) or `can't find window: x`
  (`resize-window`), an id gives the session, window or pane error for `$`, `@` or `%`, and `x.0` gives
  `can't find window: x`; `set-environment` gives `no such session: ` and the target as given. Exit codes are
  unchanged (1).

## [0.1.0] - 2026-10-20

The first public version: a session engine for programs that drive many terminal programs (coding agents, scripts, test
harnesses), which accepts tmux's command format without a PTY. macOS only. Tagged `v0.1.0` on the first public
commit, `c612089`.

Known limitations: macOS only (kqueue, the DYLD shim). One program per session, so there are no windows, panes, status
line, copy mode, key bindings or configuration file. `wait-pane` is not available as an embedded function call, only over
the socket (including an embedded `listen`). Programs protected by SIP or signed with the hardened runtime cannot load
the shim, so `zeropty-tty` runs a re-signed copy instead, and setuid or entitlement programs (`top`, `ps`) do not work.
End of file with `C-d` closes the session's input (a pipe has no one-time EOF). Before 1.0, a MINOR release may break
the interfaces in docs/VERSIONING.md.

### Added

- **Server and CLI** (`zeropty`), with the same arguments as tmux: `new-session`, `has-session`, `kill-session`,
  `list-sessions`, `kill-server`, `send-keys` (key names compared byte for byte with tmux 3), `capture-pane`
  (`-p -e -J -N -S -E -b -q`), `load-buffer`, `paste-buffer`, `display-message`, `pipe-pane`, `rename-session`,
  `resize-window`/`resize-pane`, `set-environment -g`/`show-environment -g`, `attach-session` (`-r` read-only), `-V`,
  and 25 tmux format variables. Unknown flags are errors. The first `new-session` starts the server.
- Commands tmux does not have: `wait-pane` (waits for text or for quiet without polling), `grep-panes` (searches the
  screens of all sessions), `pull-sessions`/`push-sessions` (moves running sessions to another process without touching
  the programs), `restart-server` (upgrades the server without ending sessions).
- **No PTY**: a session's program talks to pipes, and the engine emulates the line discipline (Enter, the `C-c` signal,
  line editing, echo, `C-d`). For programs that need a terminal (claude, vim, less, python), `shim.dylib`,
  `claude-zeropty` and `zeropty-tty` answer `isatty`, `tcgetattr`, the window size and `/dev/tty`.
- **Screen emulator**: 6 bytes per cell, compared with the `vt100` crate (0.16) on recorded claude sessions and random
  input. Quiet screens are folded away and later compressed with zstd.
- **Socket protocol**: one JSON line per request and per reply on `~/.zeropty/mux.sock` (`ZEROPTY_SOCK`), an optional
  raw reply format, and attach frames.
- **Embedding**: the C ABI (`libzeropty_c.dylib`, `zeropty.h`: `zeropty_open`, `zeropty_command`, `zeropty_listen`, and
  `zeropty_open_polled`/`zeropty_fd`/`zeropty_timeout_ms`/`zeropty_process` for polled use) and the Node module
  (`zeropty.node`, `client/embedded.mjs`) run the engine inside your process. `listen` serves the same socket protocol
  from the host process.
- **Clients**: `client/zeropty.mjs` (Node, keeps one connection open), and a zsh function (`shell/zeropty.zsh`) and a
  bash builtin (`shell/zeropty_bash.c`) that talk to the socket without starting a process.
- Environment variables `ZEROPTY_SOCK`, `ZEROPTY_HISTORY`, `ZEROPTY_COALESCE_MS`, `ZEROPTY_FREEZE_MS`, `ZEROPTY_COLD`,
  `ZEROPTY_COLD_DIR`, `ZEROPTY_RECORD` (docs/USAGE.md).
- Benchmarks measured interleaved with tmux 3.7c on one Mac mini M1 (docs/BENCHMARKS.md): 11 of the 14 measured items
  are at 10x or better, and 3 are not there yet (memory with 400 idle sessions on the standalone server at 6.2x, and
  `send-keys` against tmux control mode at 8~10x embedded and 2.2~2.8x over the socket). The reasons are in that
  document.
- A plugin system ([docs/PLUGINS.md](docs/PLUGINS.md)): plugins receive session create, exit, rename and output events and
  add new commands. The Rust `Plugin` trait (`API_VERSION` 1), `plugin-attach` over the socket (any language, out of
  process), C callbacks with `zeropty_add_plugin`, dylib plugins with `zeropty plugin-run`/`zeropty_load_plugin`, an
  opt-in autostart directory `ZEROPTY_PLUGIN_DIR`, and a status check with `list-plugins`. With no plugins there is no
  cost, and a plugin that panics, hangs or is slow cannot hold up the engine.
- Command calls that do not wait: C `zeropty_command_start`, `zeropty_call_poll`, `zeropty_call_due_ms`,
  `zeropty_call_free`, and Node `Engine.commandStart`.
- `zeropty --version`: the same output as `zeropty -V` (`zeropty 0.1.0`). The zsh function, the bash builtin and
  `client/zeropty.mjs` also answer it by running the program, as they do for `-V`.
- The format variable `#{version}`: the version of the running server (or embedded engine). A `display-message -p` that
  uses only this variable works without a session (`zeropty display -p '#{version}'`).
- C ABI: `ZEROPTY_VERSION_MAJOR`, `_MINOR`, `_PATCH`, `_PRERELEASE`, `_IS_PRERELEASE`, `_STRING` and `_NUMBER` in
  `zeropty.h`, and `zeropty_version()`, which returns the version of the linked library. `ZEROPTY_VERSION_NUMBER` does not
  include the prerelease, so `0.2.0-rc.1` and `0.2.0` have the same value; tell them apart with
  `ZEROPTY_VERSION_IS_PRERELEASE`.
- Node: `version()` in `zeropty.node` and `client/embedded.mjs`, and `VERSION` in `client/zeropty.mjs` and
  `client/embedded.mjs`.
- The versioning policy `docs/VERSIONING.md`: the compatibility surface whose changes count as breaking (the CLI's
  arguments, output and exit codes, the socket protocol, the session record, the C ABI, the Node API, environment
  variables, `dist/` names) and the MSRV (Rust 1.88). Before 1.0, a MINOR release may break this surface, and such
  changes are announced as `**BREAKING**` entries.
- `zeropty --help` (`-h`, `help`) prints the list of commands and options, and `zeropty help <command>` prints that
  command's usage and a link to its documentation. It answers without a server.
- Documentation: a reworked Korean README, a documentation index (`docs/README.md`), a FAQ, runnable examples
  (`examples/`), and rustdoc with examples for every public Rust item.

### Fixed

- The CLI no longer panics when the reader closes the pipe early (`zeropty capture-pane -p ... | head -1`).

[Unreleased]: https://github.com/CreatiCoding/zeropty/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/CreatiCoding/zeropty/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/CreatiCoding/zeropty/releases/tag/v0.1.0
