# Contributing

**English** · [한국어](docs/ko/CONTRIBUTING.md)

Thank you for your interest in zeropty. Bug reports, documentation fixes, shared measurements and code contributions are all welcome.

- [Before you start](#before-you-start)
- [Building and testing](#building-and-testing)
- [Finding your way around the code](#finding-your-way-around-the-code)
- [Making a change](#making-a-change)
- [Commit messages: Conventional Commits](#commit-messages-conventional-commits)
- [Changelog: CHANGELOG.md](#changelog-changelogmd)
- [Changes that affect performance](#changes-that-affect-performance)
- [Documentation](#documentation)
- [Pull requests](#pull-requests)

## Before you start

- **This is a public repository.** Never commit secrets, tokens, personal paths (`/Users/<name>/...`) or names, internal host names, or recordings of real sessions you have not checked. A recorded screen shows the recorder's prompts and folders. Always look at `git diff --cached` before you commit.
- **The goal is 10x better than tmux in every measured category.** "Better than tmux" is not done. We never change the goal to something easier.
- **No code for one particular user or program.** Programs use zeropty the way they use tmux. claude is optional, and tests and benchmarks must run with plain programs such as `cat` and `printf`.
- The authoritative code rules, measurement methods and the 10x goal are in [AGENTS.md](AGENTS.md) (the same rules apply to agents and people).
- For a large change, it helps to agree on the direction in an [issue](https://github.com/CreatiCoding/zeropty/issues) first.
- Do not report security problems as issues; follow the [security policy](SECURITY.md) instead.
- Everyone who takes part follows the [code of conduct](CODE_OF_CONDUCT.md).

## Building and testing

You need macOS, Rust (stable, edition 2024; the minimum supported Rust version (MSRV) is 1.88) and the Xcode Command Line Tools. Node 18 or later is useful for running the examples.

```sh
git clone https://github.com/CreatiCoding/zeropty && cd zeropty
git config core.hooksPath .githooks   # checks the message on commit and runs scripts/check.sh before push (needs node)
./build.sh                  # zeropty, the shim, the C and Node bindings, and the bash builtin into dist/
cargo test --release --workspace   # vt100 comparison, server, embedded, session handover, plugins, doc examples
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace   # API docs: must build without warnings
```

- `./build.sh` deletes the binaries in `dist/` before copying. On macOS, overwriting a running binary in place kills that process.
- If you changed the screen emulator, run more random input: `SEEDS=20000 cargo test --release random_streams`
- We recommend working in a `git worktree` so you do not disturb a checkout that other sessions are using.
- Tests run in parallel and start session processes at the same time. Do not stop at a single passing run: if you changed code that depends on timing (sockets, process exit, waits), run the whole suite several times. For example, even after a listener is closed, a child process that another thread has just started holds a copy of the fd until it calls exec, so a socket at the same path can briefly accept connections as if it were a "live server". For the same reason, a test that closes one end of an fd and then immediately checks for the end (EOF, EPIPE) on the other does both the close and the check inside a `sys::fd::no_spawns()` guard. No process is started while the guard is held, so no copy of the closed fd is left behind (see `keys_and_buffers_reach_the_program` in `engine::commands`; the measurements are in the comment on `no_spawns`).

## Finding your way around the code

| File | What it does |
|---|---|
| `src/term/` | The screen emulator (same results as the `vt100` crate 0.16) |
| `src/sys/` | The OS boundary (kqueue, fds, processes, time, `tty`) |
| `src/engine/` | The engine: sessions and every command. A new command is one row in `COMMANDS` in `commands.rs` plus one handler |
| `src/protocol.rs`, `src/server.rs` | The socket protocol (bytes) and the socket server |
| `src/host.rs`, `src/embed.rs` | One turn of the host loop; the engine inside a host (on its own thread or in the host's loop) |
| `src/engine/handover.rs` | Moving sessions between processes |
| `src/plugin.rs`, `src/plugin/` | Plugins ([docs/PLUGINS.md](docs/PLUGINS.md)). The stable API for plugin authors is `api.rs`; for hosts, `Plugins` and `Call` |
| `src/main.rs`, `src/cli/` | The `zeropty` program (standalone server, CLI, attach, `plugin-run`, `--help`, `--version`) |
| `shim/shim.c` | The shim that intercepts `isatty` and friends |
| `bindings/c`, `bindings/node`, `client/` | The C ABI, the Node module, the Node client |
| `tests/` | `tests/diff.rs` (vt100 comparison), `tests/server.rs`, `tests/embed.rs`, `tests/handover.rs`, `tests/version.rs` (the version number agrees everywhere) |
| `api/`, `scripts/` | The public API baseline (`api/*.txt`), release and check scripts ([docs/RELEASING.md](docs/RELEASING.md)) |
| `bench/` | Benchmarks |

The architecture diagram is in [Design](docs/DESIGN.md#overall-architecture).

## Making a change

**Correctness**

- The screen must match the `vt100` crate (0.16). `cargo test` feeds recorded claude sessions and random input to both and compares them, with random freeze, thaw and packing mixed in between reads.
- If you added a new code path, break it on purpose once (a mutation) and check that a test fails. If none fails, add a test. Restore the code afterwards.
- Unless the change is meant to alter it, `capture-pane` output must stay byte-identical for the same screen (compare against a saved result).
- End to end: if you can, start a real claude in a session through `dist/claude-zeropty` and check `send-keys`, `paste-buffer`, `capture-pane -e -S`, ONLCR (`echo` lines start at column 0) and `kill-session`.

**Compatibility**

- Keep the CLI behavior, socket protocol, session record, C ABI (`bindings/c/include/zeropty.h`), Node API, environment variables and `dist/` names backward compatible (what counts as the compatibility surface is in [docs/VERSIONING.md](docs/VERSIONING.md)). Adding is fine, but changing or removing is a breaking change: discuss it in an issue first and mark the commit as breaking.
- If you changed the C ABI or the Node exports, update `api/*.txt` in the same commit with `./build.sh && node scripts/check-api.mjs --update`.
- Never edit version numbers by hand. At release time, `node scripts/release.mjs bump ...` updates every place ([docs/RELEASING.md](docs/RELEASING.md)).
- The interface is tmux's command format. It must behave the same through the CLI, the socket and the embedded library.

**Code style**

- Write comments in English and record **why** something is done, what the gotcha is, and the measurement that justified the choice. For example: `// v2: 400 busy claude sessions took 3.5 MB against 4.5 MB with v3`
- All code is rustfmt-formatted (configured in `rustfmt.toml`). Run `cargo fmt --all` before committing; `scripts/check.sh` verifies it with `cargo fmt --all --check` ([docs/RELEASING.md](docs/RELEASING.md#formatting)).
- `cargo clippy --workspace --all-targets -- -D warnings` must pass. Lints allowed for the whole crate, with their reasons, are in `Cargo.toml`; a new exception gets an `#[allow(...)]` with a reason right where it applies.
- On hot paths (the engine's tick and output handling, the screen emulator, capture), do not add dynamic dispatch or allocations. Use static dispatch through generics and traits where needed.
- Give public items doc comments (`///`). The crate has `#![warn(missing_docs)]` enabled.
- Write commit messages in English, in the [Conventional Commits](#commit-messages-conventional-commits) format.

## Commit messages: Conventional Commits

We follow [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/). Messages are written in English.

```
<type>(<scope>)!: <what was done, imperative, lowercase>

<why it changed. The measured numbers, if any.>

BREAKING CHANGE: <for a breaking change, what users have to change>
Refs: #123
```

- The first line is at most 100 characters, with no period at the end. Separate it from the body with a blank line.
- A `!` or a `BREAKING CHANGE:` footer marks a **breaking change**. What counts as breaking is in [docs/VERSIONING.md](docs/VERSIONING.md) (CLI, socket protocol, session record, C ABI, Node API, environment variables, build outputs).
- One thing per commit. Keep refactoring and behavior changes in separate commits.

| type | When | CHANGELOG | Release level (before / after 1.0) |
|---|---|---|---|
| `feat` | A new command, flag, function or environment variable | Added | PATCH / MINOR |
| `fix` | A bug fix (`fix(security)` for a security fix) | Fixed (Security) | PATCH / PATCH |
| `perf` | Faster or smaller, same behavior | Changed | PATCH / PATCH |
| `revert` | Reverting an earlier commit | Changed | PATCH / PATCH |
| `refactor` | A code change with the same behavior | – | – |
| `docs` | Documentation only | – | – |
| `test` | Tests only | – | – |
| `build` | `build.sh`, Cargo settings, dependencies | – | – |
| `ci` | Check scripts and hook settings (`scripts/check.sh`, `.githooks/`) | – | – |
| `chore` | Anything else users do not see (`chore(release): X.Y.Z`) | – | – |
| `style` | Formatting only | – | – |
| any type + `!` | A breaking change | Changed, `**BREAKING**` | MINOR / MAJOR |

The scope is optional and lowercase. Common ones: `cli`, `server`, `engine`, `term`, `embed`, `plugin`, `protocol`, `handover`, `c-abi`, `node`, `shim`, `shell`, `bench`, `release`, `ci`.

Examples (hypothetical changes):

```
feat(cli): add --version as an alias of -V
fix(term): keep the cursor column after a wide character at the margin
perf(engine): read coalesced output in one kevent batch

400 replaying sessions: <before>% -> <after>% CPU (bench/suite.sh, interleaved with tmux, twice).
refactor(c-abi)!: take options as a struct in zeropty_open

BREAKING CHANGE: zeropty_open(const char *) is now zeropty_open(const zeropty_options *); pass NULL for defaults.
```

## Changelog: CHANGELOG.md

- The format is [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). A user-visible change (`feat`, `fix`, `perf`, `revert`, a breaking change) adds one line **in the same PR** under the matching heading (Added, Changed, Deprecated, Removed, Fixed, Security) below `## [Unreleased]`. The commit type decides the heading: `feat` → Added, `fix` → Fixed (`fix(security)` → Security), `perf` and `revert` → Changed, a breaking change → a `**BREAKING**` line under Changed or Removed. `scripts/check.sh` checks the headings of new entries with the same rules as the git-cliff draft ([docs/RELEASING.md](docs/RELEASING.md#why-it-is-semi-automatic)).
- Do not paste the commit log; write what users need to know, **in English**, and add the same entry in Korean to the Korean copy, [docs/ko/CHANGELOG.md](docs/ko/CHANGELOG.md). `node scripts/release.mjs notes` produces a draft for you (from the English commit messages).
- If you changed the C ABI or the Node exports, update `api/*.txt` with `./build.sh && node scripts/check-api.mjs --update` and include it in the same commit. A line that disappears is a breaking change.
- A breaking change starts with `- **BREAKING** ...` and says what users have to change.
- For performance numbers, use only values measured in [docs/BENCHMARKS.md](docs/BENCHMARKS.md).
- PRs that users do not see (`refactor`, `docs`, `test`, `ci` ...) need no entry. When a single commit has nothing to record, such as a fix to a feature that has not been released yet, end its message with a `Changelog: skip` footer (not allowed for breaking changes); when the whole PR is like that, say so in the PR description. The maintainer then checks it with `ZEROPTY_SKIP_CHANGELOG=1 scripts/check.sh`.
- Do not touch version numbers or `## [X.Y.Z]` headings. `scripts/release.mjs` handles them at release time ([docs/RELEASING.md](docs/RELEASING.md)).

## Changes that affect performance

A change that moves memory, CPU or response-time numbers updates [docs/BENCHMARKS.md](docs/BENCHMARKS.md) in the same commit (date, commit, machine, ratios).

- Measure tmux and zeropty **interleaved under the same load** (`bench/latency.mjs` does this). Measuring one after the other drifted by more than 30% on this machine.
- The server often runs on efficiency cores, so a single measurement varies by ±10\~15%. Repeat before drawing a conclusion, and for memory, prefer the per-session increase between 200 and 400 sessions.
- Check the per-user process limit (`sysctl kern.maxprocperuid`) first. Stop leftover benchmark servers with `kill-server` on their socket and check with `ps -U $(id -u) | wc -l`.
- Profiling: `sample <pid> 20 -file out.txt`. Compare `heap_bytes` from `zeropty stats` (bytes requested from the allocator) with the footprint, and break one screen down with `zeropty mem -t <session>`.
- Confirm a hypothesis with a measurement before acting on it. Several "obvious" causes turned out to be wrong; a profile or a counter showed the real one.
- In BENCHMARKS.md, mark items that fall short of 10x (⛔) and items not measured yet. Never round a ratio up or drop a bad row.
- README shows only items measured at 10x or better against tmux, with the measured ranges as they are. An item that falls below 10x is removed from README.

How to reproduce: [Benchmarks: reproducing the measurements](docs/BENCHMARKS.md#reproducing-the-measurements)

## Documentation

- Documentation prose is written in English in README.md and `docs/`. The Korean translations live in [README.ko.md](README.ko.md) and [docs/ko/](docs/ko/) and are kept in step with the English files (same sections) whenever a document changes. Code, commands and identifiers stay in English in both.
- After changing documents, run `node scripts/check-links.mjs` to check that every relative link and `#anchor` resolves (in both the English and the Korean files).
- Run every command and piece of code before putting it in a document.
- The command reference is in [docs/USAGE.md](docs/USAGE.md), the design and the numbers behind it in [docs/DESIGN.md](docs/DESIGN.md), and every measurement in [docs/BENCHMARKS.md](docs/BENCHMARKS.md).
- README's images (logo, benchmarks, architecture) are loaded from absolute `https://cdn.creco.dev/zeropty/` URLs so that they also show in the GitHub mobile app. The sources are in `docs/assets/`; regenerate them with `node docs/assets/make.mjs` followed by `sh docs/assets/webp.sh`. Overwriting a file under the same name would change the CDN file and README at different times, and the front page would show an old image (with old numbers) in between. So a redrawn image gets a new name by bumping `rev` in `webp.sh` (for example `bench-v3-light.webp` → `bench-v4-light.webp`), and README's `<picture>` URLs change in the same commit. The maintainer uploads the new files to the CDN and then merges, so the numbers in the front-page images never disagree with the tables. `sh docs/assets/cdn-check.sh` checks that every CDN image in README opens with 200 and names the files not uploaded yet. The last step of `scripts/check.sh` runs the same check (skipped with `--quick`). The maintainer merges a PR that changes images after uploading them to the CDN and seeing this check pass.
- The terminal demo is recorded by actually running the one-minute quick start with `node docs/assets/demo.mjs` after `./build.sh` (needs Google Chrome and `img2webp`). The resulting `demo-{light,dark}.webp` files are not committed to the repository; the maintainer uploads them to the CDN, and they go into README as a `<picture>` with absolute URLs, like the other images.

## Pull requests

1. Work on a branch (do not push straight to `main`; no force-push).
2. Run `scripts/check.sh` and make sure everything passes. The repository has no hosted CI: this script is every check that runs before something lands on main (the one hosted job, `.github/workflows/release.yml`, builds the release archive and also runs on pull requests that change what goes into it), and the maintainer runs the same script before merging. With `git config core.hooksPath .githooks`, `.githooks/pre-push` runs this script on every push (`--quick` with `ZEROPTY_PRE_PUSH=quick`).

   ```sh
   scripts/check.sh            # every check (takes a few minutes; the first MSRV check compiles from scratch)
   scripts/check.sh --quick    # without the examples, MSRV, rustdoc, CDN images and the git-cliff draft
   ```

   The steps run in order from the first; on a failure, it names the step and stops.
   - **build:** `./build.sh` (release build of the workspace, `dist/`).
   - **tests:** `ZEROPTY_REQUIRE_ALL=1 cargo test --release --workspace --locked` (including doctests). Tests that would be skipped because a tool (cc, node, python3) is missing fail instead, so a pass means every test ran.
   - **versions agree:** `node scripts/release.mjs check`. If HEAD has a tag, it also checks the tag against the version with `--tag <tag>`.
   - **public API:** `node scripts/check-api.mjs`, plus `--base <base>` when there is a base branch (whether lines that disappeared are marked as breaking).
   - **release tooling tests:** `node --test scripts/test/*.test.mjs`.
   - **version smoke tests:** `scripts/smoke-version.sh` (CLI, server, C and Node versions).
   - **clippy:** `cargo clippy --workspace --all-targets --locked -- -D warnings`.
   - **rustfmt:** `cargo fmt --all --check`.
   - **commit messages and changelog:** whether every commit after the base branch is a Conventional Commit and, if there are user-visible commits, whether `[Unreleased]` has new entries under the headings that match their types (`node scripts/check-commits.mjs --changelog`).
   - From here on, steps are skipped with `--quick`. **rustdoc:** `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace` (undocumented public items, broken doc links). **examples:** the six shell, Node, C and Rust examples. **MSRV:** `rustup run 1.88.0 cargo check --workspace --all-targets --locked` (needs `rustup toolchain install 1.88.0`). **git-cliff draft:** whether `node scripts/release.mjs notes` produces a draft. **README images:** `docs/assets/cdn-check.sh`.

   The base branch is `$ZEROPTY_CHECK_BASE`; if unset, the current branch's upstream; if there is none, `origin/main`.
3. Each example uses a server on its own temporary socket or an engine inside the process and cleans up when done, so it never touches sessions in use.
4. PRs are squash-merged by default, so **the PR title becomes the commit message on main**: write it as a Conventional Commit. Before merging, the maintainer checks the title (the squash message) with `node scripts/check-commits.mjs --message "<title>"`.
5. Fill in the checklist in the PR template. If performance is affected, attach the numbers from before and after.
6. Address review comments by adding commits.

Contributed code is distributed under the same [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) license as the repository.
