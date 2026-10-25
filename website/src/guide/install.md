---
description: Which Macs and Linux systems zeropty runs on, the three ways to install it, and what each installed file is for.
---

# Install and platform support

This page lists the platforms zeropty supports, compares the ways to install it, and explains each file you get. For a first install with every command, see [Get started](/guide/getting-started#1-install).

## Supported platforms

| Platform | Support |
|---|---|
| macOS 11 or later, Apple silicon | Supported. The release binaries load on macOS 11 and later |
| macOS 11 or later, Intel | Supported by the release archive (`zeropty` and `shim.dylib` carry an `x86_64` slice) |
| Linux x86_64, glibc 2.29 or later, kernel 5.3 or later | Supported since 0.1.6 by the Linux release archive (Ubuntu 20.04, Debian 11 and later) |
| Tested on | macOS 15 and macOS 26; Ubuntu 22.04 and 24.04 |
| Linux arm64 | Not yet |
| Windows | Not supported |

The engine waits on kqueue on macOS and on epoll on Linux. The **shim** that fools programs which need a terminal is `shim.dylib`, loaded with `DYLD_INSERT_LIBRARIES`, on macOS, and `shim.so`, loaded with `LD_PRELOAD`, on Linux. On Linux a statically linked program, such as most Go binaries, never loads the shim and sees no terminal.

Building from source needs [Rust](https://www.rust-lang.org) 1.88 or later, the minimum supported Rust version (MSRV). For details, see [Versioning](/guide/versioning).

## Install methods

| Method | Needs | You get | Leaves out |
|---|---|---|---|
| Release archive | `curl`, `shasum` (`sha256sum` on Linux) | `zeropty`, the shim, `zeropty-tty`, `claude-zeropty` | C and Node embedding libraries, bash builtin |
| `./build.sh` from source (macOS) | Rust stable, Xcode Command Line Tools | Everything, in `dist/` | Nothing |
| `cargo install --git` | Rust | `zeropty` only, in `~/.cargo/bin` | The shim, `zeropty-tty`, `claude-zeropty`, embedding libraries, bash builtin |

- **Release archive.** Each release since 0.1.3 carries `zeropty-VERSION-darwin-universal.tar.gz`, and since 0.1.6 also `zeropty-VERSION-linux-x86_64.tar.gz`, with one `SHA256SUMS` for both. Replace `VERSION` with the release number, such as `0.1.9`. Check the archive before you unpack it: `shasum -a 256 -c --ignore-missing SHA256SUMS` on macOS, `sha256sum -c --ignore-missing SHA256SUMS` on Linux (`--ignore-missing` skips the archive you did not download). To pin a version in a script, fix the version and the archive's hash from that release's `SHA256SUMS`.
- **`./build.sh`.** Clone the repository and run `./build.sh`. It builds the workspace in release mode and copies the results into `dist/`. To keep using it, put `dist/` on your `PATH`.
- **`cargo install`.** One line, if you need only the CLI and the server:

```sh [Terminal]
cargo install --git https://github.com/CreatiCoding/zeropty   # ~/.cargo/bin/zeropty
```

This is enough to run programs that need no terminal, such as `sh`, `python3` and build commands. `cargo install` installs only a package's executables, so it leaves out the files that `./build.sh` builds separately. For details, see [FAQ](/guide/faq#what-is-missing-when-i-install-with-cargo-install).

There is no Homebrew tap, and the crate is not on crates.io. A Rust program can depend on the engine through a git dependency. For details, see [Embed zeropty in C or Rust](/guide/embed).

On Linux, the same steps with the Linux archive:

```sh [Terminal]
v=0.1.9
curl -fLO "https://github.com/CreatiCoding/zeropty/releases/download/v$v/zeropty-$v-linux-x86_64.tar.gz"
curl -fLO "https://github.com/CreatiCoding/zeropty/releases/download/v$v/SHA256SUMS"
sha256sum -c --ignore-missing SHA256SUMS     # zeropty-0.1.9-linux-x86_64.tar.gz: OK
tar -xzf "zeropty-$v-linux-x86_64.tar.gz"
export PATH="$PWD/zeropty-$v-linux-x86_64:$PATH"   # add it to ~/.bashrc to keep it
```

::: warning Gatekeeper (macOS)
The release files are signed ad hoc, not notarized. A browser download carries a quarantine mark, and macOS then refuses `zeropty`. Clear it with `xattr -dr com.apple.quarantine FOLDER`, replacing `FOLDER` with the unpacked folder. `curl` adds no mark.
:::

## What each file is for

| File | In the archive | In `dist/` | For |
|---|---|---|---|
| `zeropty` | Yes (arm64 and x86_64) | Yes | The server and the CLI |
| `shim.dylib` (macOS) | Yes (arm64, arm64e, x86_64) | Yes (arm64, arm64e) | Makes a session's socket look like a terminal to the program |
| `shim.so` (Linux) | Yes (x86_64) | No | The same, loaded with `LD_PRELOAD` |
| `zeropty-tty` | Yes | Yes | Starts a program with the shim injected (vim, less, claude) |
| `claude-zeropty` | Yes | Yes | Runs `zeropty-tty claude` |
| `libzeropty_c.dylib`, `zeropty.h` | No | Yes | Embedding the engine through the C ABI (application binary interface) |
| `zeropty.node` | No | Yes | Embedding the engine in Node.js, loaded by `client/embedded.mjs` |
| `zeropty_bash.dylib` | No | Yes | The bash builtin: `enable -f dist/zeropty_bash.dylib zeropty` |
| `LICENSE-MIT`, `LICENSE-APACHE` | Yes | No | The licenses |

Keep these files together. `zeropty-tty` finds the shim next to itself, and `claude-zeropty` finds `zeropty-tty` the same way. Put the folder on your `PATH` rather than symlinking `zeropty-tty` or `claude-zeropty` elsewhere, or point `ZEROPTY_SHIM` at the shim.

The zsh function, `shell/zeropty.zsh`, is in the repository rather than in `dist/`. Load it with `source shell/zeropty.zsh`.

## Files zeropty creates

| Path | What |
|---|---|
| `~/.zeropty/mux.sock` | The server socket (change it with `ZEROPTY_SOCK`) |
| `~/.zeropty/tty/` | macOS only: re-signed copies of programs that `zeropty-tty` runs, such as those in `/usr/bin` (change the parent with `ZEROPTY_CACHE`). Linux runs every program as it is |

## Stop and uninstall

1. Stop the server and every session in it:

   ```sh [Terminal]
   zeropty kill-server
   ```

   The server sends SIGHUP to every session's process group and exits. For a server on another socket, set `ZEROPTY_SOCK` as well.

2. Remove the folder you unpacked or cloned, and remove the `PATH` line you added to `~/.zshrc`.
3. If you installed with `cargo install`, run `cargo uninstall zeropty`.
4. Remove `~/.zeropty` to delete the socket file and the copies made by `zeropty-tty`.

## Next steps

- [Get started](/guide/getting-started): run your first session.
- [How a session gets a terminal](/guide/concepts/terminal): why the shim and `zeropty-tty` exist.
- [Embed zeropty in C or Rust](/guide/embed): use the libraries that `./build.sh` puts in `dist/`.
