# zeropty documentation

**English** · [한국어](ko/README.md)

[README](../README.md) · [Docs](README.md) · [Usage](USAGE.md) · [Plugins](PLUGINS.md) · [FAQ](FAQ.md) · [Benchmarks](BENCHMARKS.md) · [Design](DESIGN.md) · [Versioning](VERSIONING.md) · [Releasing](RELEASING.md)

The documents are written in English. Korean translations of all of them live in [docs/ko/](ko/README.md) (and [README.ko.md](../README.ko.md) for the front page), kept in step section by section.

## Documents

| Document | Contents |
|---|---|
| [README](../README.md) | What zeropty is and why it is fast, quick start, install |
| [Usage](USAGE.md) | Full reference of commands and options, key names, format variables, socket protocol, embedding (Node, C, Rust), moving sessions, environment variables, feature comparison with tmux |
| [Plugins](PLUGINS.md) | Plugins that listen to session events and add commands: socket protocol, C libraries (`plugin-run`), in-host callbacks, permissions, API reference |
| [Plugin internals](PLUGINS-INTERNALS.md) | For people changing zeropty or writing their own host loop: the layers, measured cost with no plugin |
| [FAQ](FAQ.md) | Frequently asked questions and troubleshooting |
| [Benchmarks](BENCHMARKS.md) | Every measurement (including items under 10x), how it was measured, how to reproduce it |
| [Design](DESIGN.md) | Architecture diagrams, how memory, CPU and latency were cut and the numbers behind it, code map |
| [Versioning](VERSIONING.md) | Semantic Versioning, what is covered by compatibility (CLI, socket protocol, session records, C ABI, Node API, environment variables), MSRV |
| [Releasing](RELEASING.md) | Bumping the version and the release procedure (`scripts/release.mjs`) |
| [Changelog](../CHANGELOG.md) | User-visible changes in each version |
| [Contributing](../CONTRIBUTING.md) | Building, testing, measuring, pull request rules |
| [Security policy](../SECURITY.md) | How to report a vulnerability privately |
| [Code of conduct](../CODE_OF_CONDUCT.md) | Community rules |

## Find by task

| I want to | Where to look |
|---|---|
| See all commands and options at a glance | `zeropty --help`, `zeropty help <command>`, [Usage: command reference](USAGE.md#command-reference) |
| Try it from the shell like tmux | [README: quick start](../README.md#quick-start), [`examples/shell/quickstart.sh`](../examples/shell/quickstart.sh) |
| Run several agents from a Node program | [Usage: Node client](USAGE.md#node-client), [`examples/node/quickstart.mjs`](../examples/node/quickstart.mjs) |
| Put it inside my program, with no server | [Usage: embedded](USAGE.md#inside-your-process-embedded), [`examples/node/embedded.mjs`](../examples/node/embedded.mjs), [`examples/c/hello.c`](../examples/c/hello.c), [`examples/embed.rs`](../examples/embed.rs) |
| Run the engine from my host's event loop | [Usage: polled](USAGE.md#inside-your-process-embedded), [`examples/polled.rs`](../examples/polled.rs) |
| Wait until an agent is done | [Usage: waiting and finding](USAGE.md#waiting-and-searching) |
| Start programs that need a terminal, such as vim or claude | [Usage: zeropty-tty](USAGE.md#programs-that-need-a-terminal-zeropty-tty) |
| Upgrade the server without losing sessions | [Usage: moving sessions](USAGE.md#moving-sessions) |
| Move existing tmux scripts over | [Usage: feature comparison with tmux](USAGE.md#feature-comparison-with-tmux), [FAQ](FAQ.md#can-i-use-my-tmux-scripts-as-they-are) |
| Listen to session events, add my own commands (plugins) | [Plugins](PLUGINS.md), [Usage: plugins](USAGE.md#plugins) |
| Check the version, see what changed | `zeropty --version`, [Changelog](../CHANGELOG.md), [Versioning](VERSIONING.md) |
| Reproduce the benchmarks | [Benchmarks](BENCHMARKS.md), [Contributing: measuring](../CONTRIBUTING.md#changes-that-affect-performance) |

## Examples

All run from the repository root after `./build.sh`. None needs claude.

| Example | Run | Shows |
|---|---|---|
| [`examples/shell/quickstart.sh`](../examples/shell/quickstart.sh) | `sh examples/shell/quickstart.sh` | Creating a session from the CLI, sending keys, `wait-pane`, reading the screen |
| [`examples/node/quickstart.mjs`](../examples/node/quickstart.mjs) | `node examples/node/quickstart.mjs` | The same from the Node client, plus `grep-panes` |
| [`examples/node/embedded.mjs`](../examples/node/embedded.mjs) | `node examples/node/embedded.mjs` | The engine inside Node with no server, and how long one command takes |
| [`examples/c/hello.c`](../examples/c/hello.c) | See the comment at the top of the file | The C ABI |
| [`examples/embed.rs`](../examples/embed.rs) | `cargo run --release --example embed` | Embedding the engine in Rust |
| [`examples/polled.rs`](../examples/polled.rs) | `cargo run --release --example polled` | Running from a host loop with no engine thread, `wait-pane` without polling |

The other files in `examples/` (`*_bench.rs`, `sizes.rs`, `zstd_ratio.rs`, `claude_loop.rs`) are for measuring and profiling.

## Rust API docs

The rustdoc of the three crates `zeropty`, `zeropty_c` and `zeropty_node`. They are not published on crates.io (macOS only, built with `./build.sh`). There is no hosted copy; build and open it from the repository:

```sh
cargo doc --no-deps --workspace --open
```

Each crate's front page has an overview and runnable examples, and every public function and type has an example (except the `#[deprecated]` items kept under their old paths). The 200 or so Rust examples are doctests, so `cargo test --release --doc` really runs them all. Every public item is documented (`#![warn(missing_docs)]` in all three crates), and `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace` finishes without warnings. `scripts/check.sh` checks both (rustdoc when not run with `--quick`).

Where to start:

| I want to | Where to look in rustdoc |
|---|---|
| Put the engine in my process, send commands from several threads | `embed::Embedded`, `Embedded::command`, `Embedded::with` |
| Run it from my host's event loop, with no thread | `embed::Polled`, `Polled::fd`, `Polled::timeout_ms`, `Polled::process` |
| Wait for text to appear without polling | `engine::Engine::wait_start`, `wait_check`, `wait_due_ms`, `wait_end` |
| Let the CLI and socket clients reach my sessions | `Embedded::listen`, `Polled::listen`, the `server` module |
| Move running sessions to another process | `engine::handover::pull`, `push`, `Engine::export_session` |
| Write a loop around the engine alone, wait on my own fds in the same queue | `engine::Engine`, `Engine::try_poll`, `Engine::watch`, `host::HostLoop` |
| Attach my terminal to a session | `Engine::attach`, `viewer_events`, `zeropty::tty` |
| Write a socket client myself | the `protocol` module (`Request`, `reply_line`, `ClientFrame`, `ServerFrame`) |
| Test the engine against a fake OS | `engine::Backend`, `Poller`, `Clock`, `Programs`, `Engine::with_backend` |
| Write a plugin, attach it to a host | the `plugin` module (`plugin::api`), `Embedded::add_plugin`, `Polled::command_start` |
| Use only the terminal emulator | `term::Term`, `Term::freeze`, `Term::export` |
| Use it from C or Node | the front pages of the `zeropty_c` and `zeropty_node` crates |
