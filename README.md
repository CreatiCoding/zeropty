<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://cdn.creco.dev/zeropty/logo-en-dark.webp">
    <img alt="zeropty: the session engine for AI agents" src="https://cdn.creco.dev/zeropty/logo-en-light.webp">
  </picture>
</p>

<p align="center">
  <b>The session engine for AI agents.</b><br>
  Run 1000 agents on one machine: read their screens, send them keys, and wait until they finish.<br>
  It takes tmux's commands as they are, and uses <b>14.1\~15.6x less CPU and 12.0\~12.2x less memory</b> than tmux.
</p>

<p align="center"><b>English</b> · <a href="README.ko.md">한국어</a></p>

<p align="center">
  <!-- Status badges, each added once what it shows is real (no badge that claims a state it cannot track):
  - Release, once a GitHub release exists:
  <a href="https://github.com/CreatiCoding/zeropty/releases/latest"><img alt="release" src="https://img.shields.io/github/v/release/CreatiCoding/zeropty"></a>
  - crates.io: not published there (built with ./build.sh); no badge until it is. -->
  <img alt="platform: macOS" src="https://img.shields.io/badge/platform-macOS-111827?logo=apple&logoColor=white">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-engine-b7410e?logo=rust&logoColor=white">
  <img alt="embed: C ABI · Node" src="https://img.shields.io/badge/embed-C%20ABI%20%C2%B7%20Node-0891b2">
  <img alt="tmux compatible" src="https://img.shields.io/badge/tmux-compatible%20commands-1bb91f?logo=tmux&logoColor=white">
  <img alt="license: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-7c3aed">
</p>

<p align="center">
  <a href="#quick-start">Quick start</a> ·
  <a href="#benchmarks">Benchmarks</a> ·
  <a href="#install">Install</a> ·
  <a href="docs/USAGE.md">Usage</a> ·
  <a href="docs/PLUGINS.md">Plugins</a> ·
  <a href="docs/FAQ.md">FAQ</a> ·
  <a href="docs/DESIGN.md">Design</a> ·
  <a href="docs/README.md">All docs</a>
</p>

---

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://cdn.creco.dev/zeropty/demo-en-dark.webp">
    <img alt="Terminal demo: start a session with zeropty and send keys; wait-pane catches done: 42 after 1.01 seconds, and capture-pane reads the screen" src="https://cdn.creco.dev/zeropty/demo-en-light.webp" width="880">
  </picture>
</p>

Programs that run many coding agents (claude, codex ...) usually start them in tmux sessions and call `capture-pane` over and over to read their screens. With hundreds of sessions they hit the PTY limit, start a process for every command, and can tell that an agent finished only by polling. **zeropty is a session engine built for agent hosts from the start.** As the name says, it uses no PTY at all (zero PTY).

| What an agent host needs | tmux | zeropty |
|---|---|---|
| Hundreds to thousands of sessions | PTY limit of 511 (999 at most) | **1000 verified, no PTY limit** |
| Reading a screen, sending keys | Runs tmux for every command (`send-keys` 9.03\~9.56ms) | **One function call (`send-keys` 0.06\~0.07ms)** |
| Knowing that an agent finished | `capture-pane` in a loop (p50 86\~96ms late at a 200ms poll) | **`wait-pane`: 1ms, no polling** |
| Which session is waiting for input | `capture-pane` per session | **`grep-panes`: 1000 sessions in 16\~32ms** |
| Upgrading the server | Restarting the server ends the sessions | **`restart-server`: takes over the running sessions** |
| Putting it inside your program | ❌ | **C ABI · Node module** |

## Quick start

You need macOS, [Rust](https://rustup.rs) (stable) and the Xcode Command Line Tools (`xcode-select --install`). You do not need claude.

```sh
git clone https://github.com/CreatiCoding/zeropty && cd zeropty
./build.sh                       # puts zeropty and the embedding libraries in dist/
export PATH="$PWD/dist:$PATH"
```

The first `./build.sh` took 23\~27 seconds on an Apple M1 (8 cores), measured three times from a clean clone with an empty cargo cache (including downloading 49 dependency crates). The five commands below finished in 1.07 seconds, `sleep 1` included.

Start a session as you would in tmux, give it work, wait until it is done, and read the screen:

```sh
zeropty new-session -d -s demo -x 80 -y 6 sh                # starts the server if none is running
zeropty send-keys -t demo -l 'sleep 1; echo "done: $((6 * 7))"'
zeropty send-keys -t demo Enter
zeropty wait-pane -t demo -m 'done: 42' -T 10000            # no polling: returns the moment the text appears
zeropty capture-pane -p -t demo
```

```text
sleep 1; echo "done: $((6 * 7))"
done: 42
```

The screen is 6 rows (`-y 6`), so 4 empty rows follow (left out above). For the list of commands run `zeropty --help`; for one command, `zeropty help send-keys`.

To watch it yourself, attach with `zeropty attach -t demo` and detach with `Ctrl-b d`. When you are done, `zeropty kill-server`. The same flow in Node, C and Rust is in the [examples](docs/README.md#examples).

The server keeps the environment of the process that started it (the first command that starts it, usually `new-session`), as tmux's does: later sessions get that environment, not the one of the shell running each `new-session`. Pass a variable with `new-session -e VAR=value`, or set it for every new session with `set-environment -g` ([environment](docs/USAGE.md#environment)).

## Benchmarks

<!-- bench-v4-*.webp and how-v3-*.webp are docs/assets/ files of the same name, uploaded to the CDN; a redrawn picture gets a new name (docs/assets/webp.sh) so this page never shows numbers older than its tables. This page uses the English set (*-en-*); README.ko.md uses the Korean one. -->
<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://cdn.creco.dev/zeropty/bench-v4-en-dark.webp">
    <img alt="Measured ranges against tmux: with 400 claude jobs, CPU 54.30~54.91% vs 3.51~3.84% (14.1~15.6x), memory 32.4~32.5MB vs 2.66~2.70MB (server, 12.0~12.2x) and 1.63~1.64MB (embedded, 19.7~19.9x); one send-keys from Node 9.03~9.56ms vs 0.24~0.26ms over the socket (34.7~39.8x) and 0.06~0.07ms embedded (128~159x)" src="https://cdn.creco.dev/zeropty/bench-v4-en-light.webp">
  </picture>
</p>

Measured against tmux 3.7c on the same machine (Apple M1) under the same load, **interleaved**. The 400 sessions replay recordings of real claude work.

| | tmux | zeropty | |
|---|---|---|---|
| **CPU**, replaying 400 claude jobs | 54.30\~54.91% | 3.51\~3.84% | **14.1\~15.6×** |
| **Memory**, replaying 400 claude jobs (server) | 32.4\~32.5MB | 2.66\~2.70MB | **12.0\~12.2×** |
| **Memory**, replaying 400 claude jobs (embedded) | 32.4\~32.5MB | 1.63\~1.64MB | **19.7\~19.9×** |
| **Memory**, 400 screens updating | 251.2\~251.4MB | 1.71\~1.74MB | **144\~147×** |
| **Memory**, 400 idle (embedded) | 8.0MB | 0.45\~0.46MB | **17.3\~17.7×** |
| **`send-keys`** from a Node program (vs running tmux per command, socket) | 9.03\~9.56ms | 0.24\~0.26ms | **34.7\~39.8×** |
| **`send-keys`** from a Node program (vs running tmux per command, embedded) | 9.03\~9.56ms | 0.06\~0.07ms | **128\~159×** |
| **`send-keys`** from a bash script | 6.02\~6.31ms | 0.043\~0.048ms | **125\~146×** |
| Until **5.4MB of output** is done | 166\~176ms | 3.0\~4.6ms | **36.0\~58.6×** |

Every ratio follows one rule: divide by the ends of the value ranges in the table (from fastest tmux ÷ slowest zeropty to the other way round) and round down, never up. Only the items that stay at 10x or more down to the low end under this rule are in this table ([how ratios are computed](docs/BENCHMARKS.md#how-multipliers-are-computed)).

With 1000 sessions replaying real claude work, zeropty stays at **4.65MB of memory and 6.69\~7.10% CPU**. tmux cannot start 1000 because of the PTY limit.

→ Every measurement, how it was measured, and the items under 10x with the reasons: [docs/BENCHMARKS.md](docs/BENCHMARKS.md)

## Driving one agent

```js
import { zeropty } from './client/zeropty.mjs'   // the first new-session starts the server if none is running

// Start an agent (claude-zeropty: a wrapper that starts claude without a terminal)
await zeropty(['new-session', '-d', '-s', 'fix-1', '-c', '/path/to/repo', 'dist/claude-zeropty'])

// Give it work
await zeropty(['send-keys', '-t', '=fix-1:', '-l', 'Fix the failing test'])
await zeropty(['send-keys', '-t', '=fix-1:', 'Enter'])

// Wait until it is done: output paused for 5 seconds (10 minutes at most). No polling
await zeropty(['wait-pane', '-t', '=fix-1:', '-q', '5000', '-T', '600000'])

// Read the result
const { out } = await zeropty(['capture-pane', '-p', '-t', '=fix-1:'])
```

Targets are written `=name:` as in tmux scripts (`=fix-1:` is the session named exactly `fix-1`; `=fix-1` and `fix-1` name the same session). In the shell, do the same by writing `zeropty` where you would write tmux: `zeropty new-session ...`, `zeropty send-keys ...`. To watch, attach with `zeropty attach -t fix-1`. A version you can run right away without claude: [`examples/node/quickstart.mjs`](examples/node/quickstart.mjs)

## Embedding it in your program

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://cdn.creco.dev/zeropty/how-v3-en-dark.webp">
    <img alt="The zeropty engine inside an agent app handles 1000 sessions; optionally it opens a socket so people and scripts can attach too" src="https://cdn.creco.dev/zeropty/how-v3-en-light.webp">
  </picture>
</p>

With the engine inside your process there is no server process, and a command is a function call. Open a socket (`listen`) and the CLI, `attach` and other programs work on the same sessions.

```js
import { zeropty, listen } from './client/embedded.mjs'   // only the import changes
await zeropty(['new-session', '-d', '-s', 'w', 'dist/claude-zeropty'])
const { out } = await zeropty(['capture-pane', '-p', '-t', '=w:'])   // 0.06~0.07ms (busy session 0.10~0.17ms)
listen(`${process.env.HOME}/.zeropty/app.sock`)   // optional: ZEROPTY_SOCK=... zeropty attach -t w
```

```c
zeropty *a = zeropty_open(NULL);   // C ABI: any language with an FFI (Python ctypes, Go cgo, Swift ...)
char *r = zeropty_command(a, "{\"argv\":[\"send-keys\",\"-t\",\"=w:\",\"Enter\"],\"cwd\":\"/\"}");
zeropty_free(r);
```

```rust
let zp = zeropty::embed::Embedded::start(Default::default());   // Rust: the same engine, the same commands
let r = zp.command(&["capture-pane", "-p", "-t", "=w:"].map(String::from), "/");
```

→ Embedding from Node, C and Rust, and running from your host loop with no engine thread (polled): [docs/USAGE.md](docs/USAGE.md#inside-your-process-embedded) · runnable examples: [`examples/`](docs/README.md#examples)

## Plugins

You can attach code that listens to events such as a session being created, ending or printing output, and that adds new commands. A program in any language can connect to the socket, a C library built from `zeropty.h` alone can be started with `zeropty plugin-run` (neither needs zeropty recompiled), or a C function or Rust trait can go inside your own process. Point `ZEROPTY_PLUGIN_DIR` at a plugin directory and the server starts the plugins in it, and starts them again if they die or after `restart-server`. Each plugin runs on its own thread (or in its own process), so the engine keeps running when a plugin is slow, hangs or panics. A plugin can do only what the permissions it declared allow (by default, look only). With no plugins there is no cost. A program that embeds the engine can also start a plugin command without waiting for it and pick up the answer later (`command_start`, `zeropty_command_start`).

```sh
python3 examples/plugins/hello.py &   # a 68-line plugin using only the standard library
zeropty hello you                     # the plugin's command, used like a built-in one
zeropty list-plugins
export ZEROPTY_PLUGIN_DIR=~/.zeropty/plugins   # plugins to start with the server (on only when set)
cc -dynamiclib -Ibindings/c/include -o ~/.zeropty/plugins/tally.dylib examples/plugins/tally.c   # a C plugin that starts with the server
```

> **Compatibility and security note:** `zeropty serve` (and `new-session` when it starts the server) runs the executables and `.dylib` files in `ZEROPTY_PLUGIN_DIR` only when that variable is set. Without it, nothing is run, as before (even if `~/.zeropty/plugins` exists). Setting the directory is your consent to run the code in it with this user's privileges. Directories and files that other users can write to are skipped.

→ Step-by-step guide, protocol, API: [docs/PLUGINS.md](docs/PLUGINS.md)

## Install

**Download a release: no Rust needed.** Each [release](https://github.com/CreatiCoding/zeropty/releases) since 0.1.3 carries one archive for every Mac (Apple silicon and Intel, macOS 11 or later; tested on macOS 15 and 26) and its `SHA256SUMS`. The archive holds one folder: `zeropty`, `shim.dylib`, `zeropty-tty`, `claude-zeropty` and the licenses.

```sh
v=0.1.8   # or the latest: v=$(curl -fsSLo /dev/null -w '%{url_effective}' https://github.com/CreatiCoding/zeropty/releases/latest | sed 's|.*/v||')
curl -fLO "https://github.com/CreatiCoding/zeropty/releases/download/v$v/zeropty-$v-darwin-universal.tar.gz"
curl -fLO "https://github.com/CreatiCoding/zeropty/releases/download/v$v/SHA256SUMS"
shasum -a 256 -c --ignore-missing SHA256SUMS   # zeropty-0.1.8-darwin-universal.tar.gz: OK
tar -xzf "zeropty-$v-darwin-universal.tar.gz"
export PATH="$PWD/zeropty-$v-darwin-universal:$PATH"   # add it to ~/.zshrc to keep it
zeropty -V
```

- The folder works wherever you unpack it: `zeropty-tty` finds `shim.dylib` next to itself. Put the folder on your PATH rather than symlinking `zeropty-tty` or `claude-zeropty` elsewhere, or point `ZEROPTY_SHIM` at the shim.
- To pin a version in a script, fix `v` and the archive's hash from that release's `SHA256SUMS`, and compare before unpacking.
- **Gatekeeper:** the files are signed ad hoc, not notarized. `curl` adds no quarantine mark, so the steps above run as they are. A browser download is marked, `tar` passes the mark on to every unpacked file, and macOS then refuses `zeropty` with a dialog (the command waits on it). Clear the mark before the first run: `xattr -dr com.apple.quarantine zeropty-$v-darwin-universal`.

**On Linux (x86_64, glibc 2.29 or later: Ubuntu 20.04, Debian 11 and later)** the release carries `zeropty-$v-linux-x86_64.tar.gz`, in the same `SHA256SUMS`. Its folder holds `shim.so` in place of `shim.dylib`, loaded with `LD_PRELOAD`: no copy and no signing, and any dynamically linked program sees a terminal (a statically linked one, such as most Go binaries, does not).

```sh
curl -fLO "https://github.com/CreatiCoding/zeropty/releases/download/v$v/zeropty-$v-linux-x86_64.tar.gz"
curl -fLO "https://github.com/CreatiCoding/zeropty/releases/download/v$v/SHA256SUMS"
sha256sum -c --ignore-missing SHA256SUMS     # zeropty-0.1.8-linux-x86_64.tar.gz: OK
tar -xzf "zeropty-$v-linux-x86_64.tar.gz" && export PATH="$PWD/zeropty-$v-linux-x86_64:$PATH"
```

**If you need only the CLI and the server and have Rust, it is one line.** That is enough to run programs that need no terminal, such as `sh`, `python3` and build commands.

```sh
cargo install --git https://github.com/CreatiCoding/zeropty   # ~/.cargo/bin/zeropty
```

If you have no Rust, install it first with `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` ([rustup](https://rustup.rs)). There is no Homebrew tap yet.

**For everything, including C and Node embedding and the bash builtin, build from source.** The release archive does not carry those, and `cargo install` installs none of the shim for programs that need a terminal (claude, vim), `claude-zeropty`, embedding or the builtin ([why?](docs/FAQ.md#what-is-missing-when-i-install-with-cargo-install)). Building is the `git clone` + `./build.sh` of [Quick start](#quick-start) (macOS, Rust stable, Xcode Command Line Tools). To keep using it, put `dist/` on your PATH:

```sh
echo "export PATH=\"$PWD/dist:\$PATH\"" >> ~/.zshrc   # from inside the zeropty folder
```

To use it from inside your shell, faster and with no process:

```sh
source shell/zeropty.zsh                          # zsh: send-keys 0.12ms (p50)
enable -f "$PWD/dist/zeropty_bash.dylib" zeropty  # bash: send-keys 0.043~0.048ms
```

| In `dist/` | For |
|---|---|
| `zeropty` | The server and the CLI |
| `libzeropty_c.dylib`, `zeropty.h` | Embedding the engine through the C ABI |
| `zeropty.node` | Embedding the engine in Node (loaded by `client/embedded.mjs`) |
| `shim.dylib`, `zeropty-tty`, `claude-zeropty` | Starting programs that need a terminal (vim, claude) |
| `zeropty_bash.dylib` | The bash builtin |

To use it from a Rust program, one git dependency is enough, with no `./build.sh` (it is not on crates.io yet). Example: [docs/USAGE.md](docs/USAGE.md#inside-your-process-embedded)

```toml
[dependencies]
zeropty = { git = "https://github.com/CreatiCoding/zeropty" }
```

## Why it is fast

- **Sockets instead of PTYs.** No kernel PTY limit. Programs that need a terminal are fooled by a small shim that intercepts `isatty` and `ioctl`.
- **Quiet screens are frozen.** Most of 1000 sessions sit still. A screen quiet for 250ms is kept in about 1KB, and after 10 seconds it is compressed once more with zstd.
- **6 bytes per cell.** The screen parser is our own (the `vt100` crate uses 32 bytes). Its results are compared with `vt100` on recorded claude sessions and thousands of sets of random input.
- **One thread, one kqueue.** Output is gathered for 300ms and read at once. Readers always get the latest screen (pending output is handled just before reading).
- **No process to start.** Put the engine in your program, or talk to the socket straight from your shell (zsh function, bash builtin).

→ Architecture diagrams and the numbers behind them: [docs/DESIGN.md](docs/DESIGN.md) · all commands and the protocol: [docs/USAGE.md](docs/USAGE.md) · plugins: [docs/PLUGINS.md](docs/PLUGINS.md)

## How it differs from tmux

zeropty concentrates on **starting programs and driving them**. It has no window splits, status line, copy mode or configuration file, and one session runs one program. It supports macOS and Linux (kqueue or epoll, a DYLD or LD_PRELOAD shim). Feature by feature: [docs/USAGE.md](docs/USAGE.md#feature-comparison-with-tmux)

## FAQ

<details>
<summary><b>Can I use my tmux scripts as they are?</b></summary>

Mostly, by replacing `tmux` with `zeropty`. The differences: there are no windows or window splits, target names must match exactly, there are no format conditionals (`#{?...}`), and an unknown option is an error. [More](docs/FAQ.md#can-i-use-my-tmux-scripts-as-they-are)
</details>

<details>
<summary><b>What about programs that need a terminal, such as vim or claude?</b></summary>

Start them through `zeropty-tty`, as in `zeropty new-session -d -s v "dist/zeropty-tty vim notes.txt"`. For claude, `dist/claude-zeropty` does the same. [More](docs/USAGE.md#programs-that-need-a-terminal-zeropty-tty)
</details>

<details>
<summary><b>Doesn't <code>capture-pane</code> return a screen that is 300ms old?</b></summary>

No. Output is read in batches to save CPU, but commands that read the screen handle pending output first and then answer. [More](docs/FAQ.md#doesnt-capture-pane-return-a-screen-that-is-300ms-old)
</details>

<details>
<summary><b>Does it run on Linux?</b></summary>

macOS and Linux (x86_64), not Windows. On Linux the engine waits on epoll and the shim loads with `LD_PRELOAD`. [More](docs/FAQ.md#does-it-run-on-linux-or-windows)
</details>

→ More questions: [docs/FAQ.md](docs/FAQ.md)

## Documentation

| | |
|---|---|
| [Usage](docs/USAGE.md) | All commands and options, key names, format variables, socket protocol, embedding API, environment variables |
| [Plugins](docs/PLUGINS.md) | Plugins that listen to session events and add commands: how to attach them, permissions, protocol, API |
| [FAQ](docs/FAQ.md) | Frequently asked questions and troubleshooting |
| [Benchmarks](docs/BENCHMARKS.md) | Every measurement and how to reproduce it |
| [Design](docs/DESIGN.md) | Architecture diagrams, why it is fast, code map |
| [Versioning](docs/VERSIONING.md) · [Changelog](CHANGELOG.md) | What compatibility covers and the MSRV, what changed in each version |
| [API docs](docs/README.md#rust-api-docs) | rustdoc of the Rust library and the C and Node bindings, with a runnable example for every public function. Open it from the repository with `cargo doc --no-deps --workspace --open` |
| [한국어 문서](README.ko.md) | Korean translations of the README and of every document in [docs/ko/](docs/ko/README.md) |

## Contributing

Bug reports, measurements and code are all welcome. [Contributing](CONTRIBUTING.md) explains how to build, test and measure. Please report security issues privately as the [security policy](SECURITY.md) describes. The community follows the [code of conduct](CODE_OF_CONDUCT.md).

```sh
cargo test --release
```

There is no hosted CI: `scripts/check.sh` runs every check (the one hosted job builds the release archive). It covers the build, tests (including doctests), the 6 examples, `cargo doc -D warnings`, `clippy -D warnings`, `cargo fmt --check`, the MSRV (Rust 1.88), the public API baseline, links between the docs, and commit messages and the changelog. After `git config core.hooksPath .githooks`, `.githooks/pre-push` runs it on every push. The steps are described in [Contributing](CONTRIBUTING.md#pull-requests).

Screens are compared with the `vt100` crate (recorded claude sessions plus random input), and the server, embedding and session moves are checked with ordinary programs such as `cat` and `printf`. None of it needs claude.

## Versions and changelog

zeropty follows [Semantic Versioning](docs/VERSIONING.md). Which of the CLI, socket protocol, C ABI, Node API and environment variables compatibility covers, and the MSRV (Rust 1.88), are in [docs/VERSIONING.md](docs/VERSIONING.md); what changed is in [CHANGELOG.md](CHANGELOG.md). To check the version: `zeropty --version`, C `zeropty_version()`, Node `version()`. How to contribute (commit format): [CONTRIBUTING.md](CONTRIBUTING.md) · release procedure: [docs/RELEASING.md](docs/RELEASING.md)

## License

Your choice of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).
