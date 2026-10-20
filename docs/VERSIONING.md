# Versioning

**English** · [한국어](ko/VERSIONING.md)

[README](../README.md) · [Docs](README.md) · [Usage](USAGE.md) · [Plugins](PLUGINS.md) · [FAQ](FAQ.md) · [Benchmarks](BENCHMARKS.md) · [Design](DESIGN.md) · [Versioning](VERSIONING.md) · [Releasing](RELEASING.md)

zeropty follows [Semantic Versioning 2.0.0](https://semver.org/). This document defines **what the public interface is, and which changes break compatibility**. The change history is in [CHANGELOG.md](../CHANGELOG.md), and the release procedure in [RELEASING.md](RELEASING.md).

## One version

- The whole repository uses **one version**. The `zeropty`, `zeropty-c` and `zeropty-node` crates inherit `[workspace.package] version` from `Cargo.toml`, and the C header's `ZEROPTY_VERSION_*`, the Node clients' `VERSION` and `CHANGELOG.md` carry the same value.
- The version is never edited by hand; it changes only through `node scripts/release.mjs bump ...`. If the places disagree, `cargo test` (`tests/version.rs`) and `scripts/check.sh` fail.
- Tags are `v` + the version (`v0.1.0`). Prereleases are written like `0.2.0-rc.1`.

How to check the version at run time:

| Where | How | Result |
|---|---|---|
| CLI | `zeropty --version` or `zeropty -V` | `zeropty 0.1.0` (the CLI program itself) |
| Running server | `zeropty display-message -p '#{version}'` (the same format variable as tmux. Only a format that contains `#{version}` and no session variables answers without a session; with no `-t` and no session, any other format still exits with code 1, as before) | `0.1.0` |
| C ABI | `zeropty_version()` (run time), `ZEROPTY_VERSION_STRING`, `_MAJOR`, `_MINOR`, `_PATCH`, `_PRERELEASE`, `_IS_PRERELEASE`, `_NUMBER` (compile time) | `"0.1.0"`. `ZEROPTY_VERSION_NUMBER` is `MAJOR*1000000 + MINOR*1000 + PATCH`, so it **does not include the prerelease**: `0.2.0-rc.1` and `0.2.0` have the same value. Tell them apart with `ZEROPTY_VERSION_PRERELEASE` (`"rc.1"`, `""` for a final release) or `#if ZEROPTY_VERSION_IS_PRERELEASE` |
| Node (embedded) | `version()` (`zeropty.node`, `client/embedded.mjs`) | `'0.1.0'` |
| Node (client modules) | `VERSION` (`client/zeropty.mjs`, `client/embedded.mjs`) | `'0.1.0'` |

The header and the library must come from the same build. A library with a different MAJOR (a different MINOR before 1.0) may not match the header's declarations, so an embedding program does well to compare `zeropty_version()` with `ZEROPTY_VERSION_STRING` at startup.

## Rules for bumping the version

| Change | Before 1.0 (now, `0.MINOR.PATCH`) | After 1.0 |
|---|---|---|
| A change that breaks compatibility of the public interface below | **MINOR** (`0.1.x` → `0.2.0`) | **MAJOR** |
| New features (new commands, flags, functions, environment variables), backward compatible | PATCH | MINOR |
| Bug fixes, performance improvements, documentation | PATCH | PATCH |
| Raising the MSRV, changing the supported macOS versions | MINOR | MINOR |

- Before 1.0 we use the same interpretation as Cargo: `0.1.x` versions are compatible with each other, and `0.2.0` may break them.
- The level to bump is computed from the commit messages (`node scripts/release.mjs next`). A `!` or a `BREAKING CHANGE:` footer is a breaking change, `feat` is a new feature, and `fix`, `perf` and `revert` are fixes. The detailed format is in [CONTRIBUTING.md](../CONTRIBUTING.md).
- A breaking change gets a CHANGELOG line that starts with `**BREAKING**` and says what users have to change. `scripts/check.sh` checks this.

## Public interface (what stays compatible)

Changing or removing anything below is a **breaking change**. Adding something new (a new command, flag, function, JSON field or environment variable) is not.

### 1. CLI

- Command names and aliases (`new-session`/`new`, `attach-session`/`attach`/`a`, ...), flags and their meaning, and tmux-compatible argument parsing (including the `-t =name:` target format).
- Exit codes: 0 on success, 1 on failure (`has-session` finds nothing, `wait-pane` times out, `grep-panes` finds nothing, an unknown option, ...), and 2 when run without arguments.
- Output formats that programs read: the default output of `list-sessions` and `-F`, the format variables of `display-message -p`, `new-session -P`, the `session: line` output of `grep-panes`, and `zeropty X.Y.Z` from `zeropty --version`.
- The bytes of `capture-pane` output. The same screen must give the same bytes. Making output that differed from tmux match it is a bug fix (`fix`); changing the format on purpose is a breaking change.
- The default socket path (`~/.zeropty/mux.sock`) and the behavior of the first `new-session` starting the server.
- Error messages meant for people (`err`) are not part of the public interface. Decide by the exit code.

### 2. Socket protocol

- A request is one line: `{"argv": [...], "cwd": "...", "raw": bool}`. A reply is one line: `{"code", "out", "err"}`. Several requests per connection, with replies in the order the requests were sent.
- The `"raw": true` reply format (`<code> <out byte count> <err byte count>\n` + bytes).
- The `attach` frame format and kind values (`FRAME_OUTPUT`, `FRAME_ENDED`, `FRAME_INPUT`, `FRAME_RESIZE`, `FRAME_DETACH`).
- New request fields may be added. When an old client does not send them, everything must work exactly as it does now.

### 3. Session handover record

- The session record exchanged by `pull-sessions`, `push-sessions` and `restart-server` (`RECORD_VERSION`, currently 1). **`restart-server` is the upgrade path**, so if a newer server cannot read an older record, you cannot upgrade without losing running sessions.
- That is why raising `RECORD_VERSION` is a breaking change. If it has to go up, keep reading the older record too, and note in the CHANGELOG from which version to which version `restart-server` works.

### 4. C ABI (`bindings/c/include/zeropty.h`, `libzeropty_c.dylib`)

- Function names, argument and return types, meaning, and threading rules (`zeropty_open` from any thread; polled use from one loop thread).
- Memory ownership: the result of `zeropty_command` is freed with `zeropty_free`; the string from `zeropty_version()` is not freed.
- The keys of the options JSON (`history`, `size_dir`) and the request and reply JSON (the same as the socket protocol).
- `zeropty` is an opaque type. Its size and internal layout are not part of the public interface.
- The names and meaning of the header's `ZEROPTY_VERSION_*` macros.
- Adding functions is not a breaking change. Instead of changing the signature of an existing function, add one under a new name.

### 5. Node API

- The exports of `zeropty.node`: `Engine` (constructor options `history`, `sizeDir`, `polled`, `command`, `listen`), `Client` (`command`, `close`), `version()`.
- `client/zeropty.mjs`: `zeropty`, `zeroptySync`, `sockPath`, `cliPath`, `VERSION`, the result shape `{ ok, code, out, err }`, and the options (`cwd`, `sock`, `timeout`).
- `client/embedded.mjs`: `zeropty`, `zeroptySync`, `open`, `listen`, `version`, `VERSION`. The two modules must be interchangeable by changing only the import.
- Supported Node: N-API 6 or later (`napi6`).

### 6. Environment variables

| Public (name, meaning and default are kept) | Internal (may change without notice) |
|---|---|
| `ZEROPTY_SOCK`, `ZEROPTY_HISTORY`, `ZEROPTY_COALESCE_MS`, `ZEROPTY_FREEZE_MS`, `ZEROPTY_COLD`, `ZEROPTY_COLD_DIR`, `ZEROPTY_RECORD` ([USAGE.md](USAGE.md#environment-variables)) | `ZEROPTY_COLS`, `ZEROPTY_ROWS`, `ZEROPTY_SIZE_FILE`, `ZEROPTY_TTY_ID` (values the engine passes to the shim, used only within the same build) |
| `ZEROPTY_BIN`, `ZEROPTY_NODE` (the binary and module the clients use), `ZEROPTY_SHIM`, `ZEROPTY_CACHE`, `ZEROPTY_CLAUDE_REAL` (`zeropty-tty`, `claude-zeropty`) | `ZEROPTY_MALLOC_ADDED` (for re-executing the server), `ZEROPTY_PURGE_MS` (for measurements) |
| `ZEROPTY_SESSION` (the session name that programs inside a session read) | Every other `ZEROPTY_*` used only by tests and benchmarks |

Changing a default is also a breaking change if behavior changes. The exception is defaults for performance tuning (`ZEROPTY_COALESCE_MS`, `ZEROPTY_FREEZE_MS`): if what you read from the screen stays the same, the change counts as a fix. It still goes under Changed in the CHANGELOG.

### 7. Build outputs and shell integration

- The file names `./build.sh` produces in `dist/`: `zeropty`, `libzeropty_c.dylib`, `zeropty.h`, `zeropty.node`, `shim.dylib`, `zeropty_bash.dylib`, `claude-zeropty`, `zeropty-tty`.
- The names and behavior of the zsh function (`shell/zeropty.zsh`) and the bash builtin (`enable -f ... zeropty`) (they must behave like the CLI).

## Machine checks

The rules are not left to human eyes alone.

| What | How | Where |
|---|---|---|
| C ABI (section 4) | Compares the `zeropty_*` symbols from `nm -gU dist/libzeropty_c.dylib` and the declarations in `zeropty.h` (without parameter names, comments and line breaks) and the macro names with `api/c-abi.txt` | `node scripts/check-api.mjs` (the public API step of `scripts/check.sh`) |
| Node API (section 5) | Compares the export names and kinds of `zeropty.node`, `client/zeropty.mjs` and `client/embedded.mjs`, and the methods of their classes, with `api/node.txt` | Same |
| Breaking-change marks | If a line in `api/*.txt` disappears or changes in a PR, it passes only when a commit (or the PR title) has `!` or `BREAKING CHANGE:` and `[Unreleased]` has a new `- **BREAKING**` line | `node scripts/check-api.mjs --base <base>` (the public API step of `scripts/check.sh`) |
| Version agreement | The crates, `Cargo.lock`, the header macros, Node `VERSION`, `--version`, `#{version}`, the CHANGELOG | `tests/version.rs`, `node scripts/release.mjs check` |
| MSRV | `cargo check --workspace --all-targets --locked` with Rust 1.88.0 | The MSRV step of `scripts/check.sh` |
| CLI, socket protocol, record | `tests/server.rs`, `tests/handover.rs` and `tests/embed.rs` check the behavior with a real server and clients | `cargo test` |

A new function or export only adds a line to the baseline, so it is not a breaking change; but `scripts/check.sh` fails when the build and the baseline differ, so put the `api/` updated with `node scripts/check-api.mjs --update` in the same commit. The Rust library API is not part of the public interface (see below), so we do not use `cargo-semver-checks`. If we decide to make it public, we will add it to `scripts/check.sh` then.

## What is not public interface

- **The Rust library API** (`zeropty::engine`, `zeropty::term`, `zeropty::server`, ...). It is not published on crates.io, and the bindings and the server change together in the same repository. Whether to make it public will be decided separately before 1.0.
- Performance numbers. Getting slower is treated as a regression, not a compatibility issue (the 10x goal in [AGENTS.md](../AGENTS.md), [BENCHMARKS.md](BENCHMARKS.md)).
- The contents of `bench/`, `examples/`, `tests/` and `scripts/`, and the output format of the diagnostic commands (`stats`, `mstats`, `mem`).
- Error messages, logs, and the internal format of record files (`ZEROPTY_RECORD`).

## Deprecation

- After 1.0, something is deprecated for at least one MINOR release before it is removed. It keeps working during that time, and the CHANGELOG's **Deprecated** section names the replacement. It is removed under **Removed** in the next MAJOR.
- Before 1.0, a MINOR release may remove things right away, but we follow the same procedure when we can.

## MSRV (minimum supported Rust version)

- **Rust 1.88** (`rust-version` in `Cargo.toml`). The dependency `napi` 3 requires 1.88. The edition is 2024.
- The MSRV step of `scripts/check.sh` runs `cargo check --locked` on the whole workspace with exactly 1.88.0 (`rustup run 1.88.0`).
- The MSRV is raised only in a MINOR release and noted under Changed in the CHANGELOG. When raising it, change `rust-version` in `Cargo.toml`, the MSRV step of `scripts/check.sh` and this document in the same commit.
- Platform: macOS only (kqueue, the DYLD shim). The checks (`scripts/check.sh`) run on the maintainer's Apple Silicon Mac.

## Examples

| Change | Classification | Commit |
|---|---|---|
| Add a `-C` flag to `capture-pane` | New feature | `feat(cli): support capture-pane -C` |
| Change the `wait-pane` timeout exit code from 1 to 2 | Breaking change | `fix(cli)!: exit 2 when wait-pane times out` |
| `display -p '#{version}'`, which failed (1) without a session, now prints the server version (0); other formats still give 1 | New feature (only input that used to be an error now works; the results of input that already succeeded are unchanged) | `feat(cli): add the #{version} format variable` |
| `display -p -t =gone: '...'`, which failed (1), now prints the format with the session's variables empty (0), as tmux does | Bug fix (input that used to be an error now works as in tmux; nothing that succeeded changes) | `fix(cli): answer display-message on a missing target as tmux does` |
| The exit code or output format of a command that already succeeded changes | Breaking change | `feat(cli)!: ...` |
| Add `zeropty_command_len()` to the C ABI | New feature | `feat(c-abi): add zeropty_command_len` |
| Add an argument to `zeropty_open` | Breaking change (adding a new function is recommended instead) | `feat(c-abi)!: ...` |
| Add a `"pid"` field to the reply JSON | New feature | `feat(protocol): ...` |
| `RECORD_VERSION` to 2, still reads 1 | New feature | `feat(handover): ...` |
| `RECORD_VERSION` to 2, cannot read 1 | Breaking change | `feat(handover)!: ...` |
| Make `capture-pane -e` bytes that differed from tmux on the same screen match | Fix | `fix(term): ...` |
| Screen parsing twice as fast | Fix (performance) | `perf(term): ...` |
| Rename `ZEROPTY_SOCK` | Breaking change | `refactor!: ...` or `feat!: ...` |
| MSRV 1.88 → 1.90 | MINOR | `build: raise MSRV to 1.90` |
