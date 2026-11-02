# AGENTS.md

> **THIS IS A PUBLIC REPOSITORY.** Everything committed here is visible to anyone, history included.
> Never commit secrets, tokens, personal paths or names, private hostnames, or recordings of real
> sessions without checking them (a recorded screen shows the recorder's skills, prompts and folders).
> Before every commit: `git diff --cached` and look.

Rules for agents (and people) working on zeropty. Read README.md first for what zeropty is.

## Goal

zeropty is a tool of its own, used like tmux by anyone: people at a terminal, scripts, and programs in any
language (embedded as a library or through the server). It must stay usable with up to 1000 claude
sessions on one macOS machine. The bar is
**10x better than tmux in every measured category**: memory, CPU and response time, at 200 / 400 / 1000
sessions. "Better than tmux" is not done. Never change the goal to something easier; if an item has a hard
floor (the cost of starting a process, the memory of an empty process), say so in one line in
docs/BENCHMARKS.md and keep going on everything else.

## Benchmarks live in docs/BENCHMARKS.md

- docs/BENCHMARKS.md always carries every current benchmark table. A change that moves memory, CPU or
  latency numbers updates it in the same commit (date, commit, machine, ratios).
- There, mark every item below 10x, and every item not measured yet. Never round a ratio up or drop a
  bad row.
- README.md is the front page: it shows only items measured at 10x or better against tmux (the owner's
  choice), with their measured ranges, and links to docs/BENCHMARKS.md for the rest. Update its rows
  in the same commit; an item that falls below 10x leaves README, never gets rounded up to stay.
- Details go in docs/: USAGE.md (commands, protocols), DESIGN.md (why it is fast, with the numbers).
- Reproduce with `bench/suite.sh` (memory/CPU, ~15 min) and `node bench/latency.mjs 200` (latency).

## Documentation

- Docs prose is English in the main files (README.md, docs/*.md, CONTRIBUTING.md, SECURITY.md,
  CODE_OF_CONDUCT.md, CHANGELOG.md). The Korean translations live in README.ko.md and docs/ko/ (same file
  names); each Korean file links back to its English file on the line under the title, and vice versa.
- Keep both in step: a change to a document makes the same change, in the same sections, to its Korean copy
  in the same commit. A CHANGELOG entry goes into CHANGELOG.md in English and docs/ko/CHANGELOG.md in Korean.
- English files link to English files and Korean copies to Korean copies. After changing docs, run
  `node scripts/check-links.mjs` (every relative link and `#anchor` must resolve; headings change anchors).
- Code, commands, identifiers and file names stay in English in both languages, and every number is copied
  exactly.

## Measuring

- Compare tmux and zeropty **interleaved under the same load** (`bench/latency.mjs` does). Running one
  after the other drifted by 30% and more on this machine.
- The server often runs on efficiency cores: single runs vary by +-10-15%. Repeat before concluding,
  and prefer the per-session increase between 200 and 400 sessions for memory.
- Check the per-user process limit first (`kern.maxprocperuid`, 2666 here; a claude session is about 4
  processes). `bench/load.sh` stops before crossing it. Leftover benchmark servers have exhausted it
  before: kill them with `kill-server` on their socket when done, and check `ps -U $(id -u) | wc -l`.
- Profile with `sample <pid> 20 -file out.txt`; `zeropty stats` reports `heap_bytes` (bytes asked of the
  allocator) to compare with the footprint, `zeropty mem -t <session>` breaks one screen down.
- Verify a hypothesis with a measurement before acting on it. Several "obvious" causes here were wrong
  (kernel time, freeze churn, the allocator) and only the profile or a counter showed the real one.

## Correctness

- The screen must match the `vt100` crate (0.16). `cargo test` replays recorded claude sessions and
  random streams against it, with random freeze / thaw / packing between reads. Raise coverage with
  `SEEDS=20000 cargo test --release random_streams`.
- After adding a code path, break it on purpose once (a mutation) and check that a test fails; restore
  it after. Add a test when none fails.
- vte 0.15.0 drops a character when a read ends inside a UTF-8 character and the next read continues
  with another; zeropty feeds bytes >= 0x80 to vte one at a time, and the oracle gets one byte at a time.
- `capture-pane` output must stay byte-identical to what it was for the same screen unless the change
  is meant to alter it (compare against a saved binary).
- End to end: start a real claude in a session through the runtime adapter (docs/USAGE.md), then check send-keys,
  paste-buffer, `capture-pane -e -S`, ONLCR (`echo` lines start at column 0) and kill-session.

## Independence

- No code, names, paths or settings specific to any one user or program of zeropty; no test or benchmark
  that needs one. Programs use zeropty the way they use tmux.
- The interface is tmux's command format (`new-session`, `send-keys`, `capture-pane`, ...), the same
  through the CLI, the socket, and the embedded library.
- claude is optional: plain programs (a shell, `cat`) run as sessions; the shim and the runtime adapter are
  add-ons for programs that need a terminal. Nothing in the code is specific to claude.
- Hosts decide policy (when to hand sessions over, when to exit); the engine only provides the means.

## Versions and change notes

- Semantic Versioning; the compatibility surface (CLI, socket protocol, session record, C ABI, Node API, env
  vars, dist/ names) and the MSRV are in docs/VERSIONING.md. A change to any of them is breaking: mark it.
- Commits are Conventional Commits (CONTRIBUTING.md): `feat(cli): ...`, `fix(term)!: ...`. scripts/check.sh rejects others.
- A user-visible change (feat, fix, perf, revert, breaking) adds a line under CHANGELOG.md's `## [Unreleased]`
  in the same commit or PR; breaking ones start with `**BREAKING**`.
- The C ABI and Node exports are listed in api/*.txt and scripts/check.sh compares them with the build
  (`node scripts/check-api.mjs`): a changed interface updates them (`--update`) in the same commit, and a line
  that goes away needs a breaking mark.
- Never edit version numbers by hand: `node scripts/release.mjs bump ...` (docs/RELEASING.md) writes every
  place, and tests/version.rs fails when they disagree. No tags, releases or publishing from agents.

## Code

- Layers, each depending only on the ones below it (src/lib.rs has the table; every module's `//!` docs
  say what it owns and its invariants):
  - `src/term/`: the screen emulator, pure (bytes in, a screen out): `text`, `codec`, `attrs`,
    `palette`, `cell`, `row` (packed format), `grid/` (storage, `edit`, `read`, `save`), `screen`, `csi`,
    `vt` (the dispatcher), `frozen` (freeze/thaw/export), `cold` (the compressed store; a failing file
    falls back to memory).
  - `src/sys/`: the OS (kqueue, fds, processes, time, and `tty`, the attaching user's terminal, public
    as `zeropty::tty`); nothing else in the crate or the binary calls libc for I/O, and every `unsafe`
    block anywhere has a `// SAFETY:` comment (`clippy::undocumented_unsafe_blocks` is denied).
  - `src/engine/`: sessions and every command, a library any host can run. Its seams to the OS are
    three traits (`backend`), bundled as `Engine<B: Backend>`: `Poller` (the queue), `Clock` (the time)
    and `Programs` (starting, signalling and reaping programs, their size file). `B` defaults to
    `System` (kqueue, monotonic clock, real processes); the engine never calls `sys::process` or writes
    a file itself, so its scheduling, kill timers, resizes and commands are unit-tested against fakes
    (`engine::testing`). Pure parts (`args`, `keys`, `ldisc`, `format`, `render`, `record`, `policy`,
    `table`, `reply`) are unit-tested on their own; `session`, `spawn` (behind `SystemPrograms`),
    `commands` (the command table), `viewers`, `wait`, `transfer` do the I/O. Handlers return
    `Result<Reply, CmdError>`. A wait whose change was refused is retried without it; a failure of the
    queue itself is returned (`try_poll`), never swallowed.
  - `src/protocol.rs`: the socket's bytes (request/reply lines, typed attach frames `ServerFrame` /
    `ClientFrame` with length caps), shared by both ends.
  - `src/server.rs`: the socket server (clients, attach, wait-pane, handover), which borrows an engine.
    `src/host.rs`: one turn of a host's loop (events, server, reads, attached terminals, pending
    changes), used by every host so the order lives in one place; generic over the engine's `Backend`
    and the `Service` it drives (the server), so the order is unit-tested with fakes. `src/engine/handover.rs`: sessions
    between processes. `src/embed.rs`: the engine in a host (its own thread, or polled by the host's
    loop), behind the `Frontend` trait the bindings call.
  - `src/plugin.rs`, `src/plugin/`: plugins, above the engine and beside the server (docs/PLUGINS.md,
    docs/PLUGINS-INTERNALS.md). `api.rs` is the stable surface; `__private/` is zeropty's own.
  - `src/main.rs` and `src/cli/`: the `zeropty` process (standalone server, CLI, attach, `plugin-run`).
- Fds are typed everywhere: `OwnedFd` (owned, closed on drop; a session's are `Option<OwnedFd>`, 4 bytes
  like a `RawFd` thanks to the niche of -1) or `BorrowedFd` (lent). A `RawFd` appears only as a number
  in a kqueue `Change` or `Event`.
- Moving or renaming a public item keeps the old path as a `#[deprecated]` re-export (src/lib.rs lists
  them).
- `bindings/c`, `bindings/node`: the embedded engine for other languages. `shim/shim.c`: the isatty
  shim; `examples/node/tty-adapter.cjs`: the runtime adapter; `client/zeropty.mjs`: the Node client.
- A new command is a row in `src/engine/commands.rs`'s `COMMANDS` and a handler beside it.
- `tests/server.rs` drives a real server through the CLI and the socket with plain programs.
- Comments say why, name the gotcha, and keep the measured number that justified a choice.
- `./build.sh` deletes binaries in `dist/` before copying: macOS kills a binary overwritten in place.
- Work in a git worktree, not the main checkout (other sessions may use it).
- Never force-push `main`.
