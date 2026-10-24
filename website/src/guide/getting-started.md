---
description: Install zeropty, start a session, send keys, wait for output with wait-pane and read the screen.
---

# Get started

By the end of this guide, you have zeropty installed, a session running a shell, and the screen of that session read back after waiting for its output. The whole flow takes five commands and about a second.

## Before you begin

- A Mac (macOS 11 or later, Apple silicon or Intel) or Linux (x86_64, glibc 2.29 or later). For details, see [Install and platform support](/guide/install).
- One of these:
  - **The release archive.** You need only `curl` and `shasum`, which come with macOS. No Rust is needed.
  - **A build from source (macOS).** You need [Rust](https://rustup.rs) (stable) and the Xcode Command Line Tools (`xcode-select --install`).
- You do not need claude or any other agent. This guide runs `sh`.

## 1. Install

Pick one of the two ways below.

### Option A: download a release

Each [release](https://github.com/CreatiCoding/zeropty/releases) since 0.1.3 carries one archive for every Mac and its `SHA256SUMS`. Download both, check the hash, then unpack:

```sh [Terminal]
v=0.1.7   # or the latest: v=$(curl -fsSLo /dev/null -w '%{url_effective}' https://github.com/CreatiCoding/zeropty/releases/latest | sed 's|.*/v||')
curl -fLO "https://github.com/CreatiCoding/zeropty/releases/download/v$v/zeropty-$v-darwin-universal.tar.gz"
curl -fLO "https://github.com/CreatiCoding/zeropty/releases/download/v$v/SHA256SUMS"
shasum -a 256 -c --ignore-missing SHA256SUMS   # zeropty-0.1.7-darwin-universal.tar.gz: OK
tar -xzf "zeropty-$v-darwin-universal.tar.gz"
export PATH="$PWD/zeropty-$v-darwin-universal:$PATH"   # add it to ~/.zshrc to keep it
zeropty -V
```

`v` holds the version. Replace `0.1.7` with another version to pin it. The last command prints the version, for example:

```text
zeropty 0.1.7
```

::: warning Downloaded with a browser?
The files are signed ad hoc, not notarized. `curl` adds no quarantine mark, so the steps above run as they are. A browser download is marked, and macOS then refuses `zeropty` with a dialog. Clear the mark before the first run: `xattr -dr com.apple.quarantine zeropty-$v-darwin-universal`.
:::

On Linux (x86_64), download `zeropty-$v-linux-x86_64.tar.gz` instead, check it with `sha256sum -c --ignore-missing SHA256SUMS`, and put `zeropty-$v-linux-x86_64` on your `PATH`. For the full commands, see [Install and platform support](/guide/install#install-methods).

### Option B: build from source

Building gives you everything, including the embedding libraries and the bash builtin:

```sh [Terminal]
git clone https://github.com/CreatiCoding/zeropty && cd zeropty
./build.sh                       # puts zeropty and the embedding libraries in dist/
export PATH="$PWD/dist:$PATH"
```

The first `./build.sh` took 23~27 seconds on an Apple M1 (8 cores), from a clean clone with an empty cargo cache.

## 2. Start a session

Start a session named `demo` that runs `sh` on a screen of 80 columns and 6 rows:

```sh [Terminal]
zeropty new-session -d -s demo -x 80 -y 6 sh                # starts the server if none is running
```

`-d` keeps your terminal free, and `-s demo` names the session. No server was running, so this command also starts one. For details, see [Sessions and the server](/guide/concepts/sessions).

## 3. Send keys

Type a command into the session, then press Enter:

```sh [Terminal]
zeropty send-keys -t demo -l 'sleep 1; echo "done: $((6 * 7))"'
zeropty send-keys -t demo Enter
```

`-t demo` is the **target**, the session to send to. `-l` sends the text literally. `Enter` on its own is a key name, so it sends the Enter key.

## 4. Wait for output with wait-pane

Wait until `done: 42` appears on the screen, for at most 10 seconds:

```sh [Terminal]
zeropty wait-pane -t demo -m 'done: 42' -T 10000            # no polling: returns the moment the text appears
```

The command returns about one second later, as soon as the shell prints the line. `-T` is the timeout in milliseconds. After it, `wait-pane` exits with code 1.

::: tip Wait for the result, not the typed line
Typed text is echoed to the screen, as on a real terminal. Wait for text that only the result prints. Here, the typed line contains `$((6 * 7))`, not `42`.
:::

## 5. Read the screen

Print the current screen of the session:

```sh [Terminal]
zeropty capture-pane -p -t demo
```

`-p` prints the screen to standard output. Without it, the screen goes into a buffer, as in tmux.

## Verify

You should see the typed line and its result:

```text
sleep 1; echo "done: $((6 * 7))"
done: 42
```

The screen is 6 rows (`-y 6`), so 4 empty rows follow, which are left out above. If `capture-pane` prints nothing, check that the session exists with `zeropty ls`.

## 6. Attach and detach, then stop the server

To watch the session yourself, attach your terminal to it:

```sh [Terminal]
zeropty attach -t demo
```

Detach with `Ctrl-b d`. The session keeps running. When you are done, stop the server and every session in it:

```sh [Terminal]
zeropty kill-server
```

For the list of commands, run `zeropty --help`. For one command, run `zeropty help send-keys`.

## Next steps

- [Drive agents from the shell](/guide/shell): the same flow with real agents, plus the faster zsh function and bash builtin.
- [Run claude and other terminal programs](/guide/terminal-programs): start programs that need a terminal with `zeropty-tty`.
- [Use zeropty from Node.js](/guide/node): drive sessions from a program over the socket or embedded.
- [Screens, keys and waiting](/guide/concepts/screens): how `wait-pane` knows without polling.
