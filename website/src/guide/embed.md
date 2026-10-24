---
description: Put the zeropty engine inside a C or Rust program, with its own engine thread or driven from your event loop, and optionally open a socket for the CLI.
---

# Embed zeropty in C or Rust

By the end of this guide, you run sessions inside your own C or [Rust](https://www.rust-lang.org) program, with no server process. You send the same commands as the CLI, choose between an engine thread and your own loop, and open a socket so the CLI can reach your sessions.

## Before you begin

- A Mac with the Xcode Command Line Tools (`xcode-select --install`).
- **C:** a build from source. `./build.sh` puts `libzeropty_c.dylib` and `zeropty.h` in `dist/`. The release archive does not carry them.
- **Rust:** Rust 1.88 or later. The crate is not on crates.io, so you add it as a git dependency; `./build.sh` is not needed.
- To start programs that need a terminal (claude, vim), you also need `dist/zeropty-tty`, which is not part of the crate. For details, see [Run claude and other terminal programs](/guide/terminal-programs).

## 1. Add zeropty to your build

::: code-group

```sh [C (Terminal)]
git clone https://github.com/CreatiCoding/zeropty && cd zeropty
./build.sh                       # dist/libzeropty_c.dylib and dist/zeropty.h
```

```toml [Rust (Cargo.toml)]
[dependencies]
zeropty = { git = "https://github.com/CreatiCoding/zeropty" }
```

:::

In Rust, add `rev = "COMMIT"` to stay on one commit. Replace `COMMIT` with the commit hash.

## 2. Start an engine and send commands

An **embedded** engine takes the same commands as the CLI and returns the same `code`, `out` and `err`. In C, the request and the reply are the socket protocol's JSON lines. In Rust, they are an `argv` and a `Reply`.

::: code-group

```c [examples/c/hello.c]
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include "zeropty.h"

/* Runs one command and prints its reply. */
static void run(zeropty *zp, const char *request) {
  char *reply = zeropty_command(zp, request);
  printf("%s\n  -> %s\n", request, reply);
  zeropty_free(reply);
}

int main(void) {
  zeropty *zp = zeropty_open(NULL); /* or "{\"history\": 2000}" */
  if (!zp) return 1;
  run(zp, "{\"argv\":[\"new-session\",\"-d\",\"-s\",\"c\",\"-x\",\"40\",\"-y\",\"4\",\"exec cat\"],\"cwd\":\"/\"}");
  run(zp, "{\"argv\":[\"send-keys\",\"-t\",\"=c:\",\"-l\",\"hi from C\"],\"cwd\":\"/\"}");
  run(zp, "{\"argv\":[\"send-keys\",\"-t\",\"=c:\",\"Enter\"],\"cwd\":\"/\"}");
  usleep(100 * 1000); /* time for cat to print the line back */
  run(zp, "{\"argv\":[\"capture-pane\",\"-p\",\"-t\",\"=c:\"],\"cwd\":\"/\"}");
  run(zp, "{\"argv\":[\"kill-server\"],\"cwd\":\"/\"}");
  zeropty_close(zp);
  return 0;
}
```

```rust [examples/embed.rs]
use std::time::{Duration, Instant};
use zeropty::embed::Embedded;
use zeropty::engine::{Options, Reply};

fn main() {
    let zp = Embedded::start(Options::default());
    let run = |args: &[&str]| -> Reply { zp.command(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "/") };

    // A session is a program on pipes: `sh` here, any program works (`exec cat`, a coding agent...).
    let r = run(&["new-session", "-d", "-s", "demo", "-x", "60", "-y", "10", "sh"]);
    assert_eq!(r.code, 0, "{}", r.err);

    // Keys go through the engine's line discipline, as through a PTY: Enter ends the line.
    run(&["send-keys", "-t", "=demo:", "-l", "echo \"answer: $((6 * 7))\""]);
    run(&["send-keys", "-t", "=demo:", "Enter"]);

    // capture-pane reads the output pending first, so the screen is never stale.
    let start = Instant::now();
    let screen = loop {
        let screen = run(&["capture-pane", "-p", "-t", "=demo:"]).out;
        if screen.contains("answer: 42") || start.elapsed() > Duration::from_secs(5) {
            break screen;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    print!("{}", screen.trim_end_matches('\n'));
    println!();

    // Every session's name and size, in tmux's format language.
    print!("{}", run(&["list-sessions", "-F", "#{session_name} #{window_width}x#{window_height}"]).out);

    // kill-server ends the sessions; dropping `zp` stops the engine's thread.
    run(&["kill-server"]);
}
```

:::

`zeropty_open` (Rust: `Embedded::start`) gives the engine its own **engine thread**. You may call commands from any thread: reading the screen, sending keys and queries run on the calling thread, and everything else goes to the engine thread. In C, free every reply with `zeropty_free`, never with `free()`.

The function call has no `wait-pane`, so these examples read the screen in a loop. Step 3 shows the polled engine's way to wait.

## 3. Drive the engine from your own loop

A **polled** engine has no thread of its own. Your event loop waits on the engine's fd and calls `process` when it is readable or its timeout passes. Make every call from that loop's thread. With 400 idle sessions, polled mode took memory from 0.74MB to 0.45~0.46MB.

::: code-group

```c [app.c]
zeropty *a = zeropty_open_polled(NULL);
for (;;) {   // host loop: call zeropty_process when the engine fd is readable or the timeout expires
  struct pollfd p = { zeropty_fd(a), POLLIN, 0 };
  poll(&p, 1, (int)zeropty_timeout_ms(a));
  zeropty_process(a);
}
```

```rust [examples/polled.rs]
let mut zp = Polled::start(Options::default());
let r = zp.command(&argv(&["new-session", "-d", "-s", "job", "sh -c 'sleep 0.5; echo build done; exec cat'"]), "/");
assert_eq!(r.code, 0, "{}", r.err);

// wait-pane -t job -m 'build done' -T 5000, without the command name.
let wait = zp.engine().wait_start(&argv(&["-t", "=job", "-m", "build done", "-T", "5000"])).unwrap_or_else(|r| panic!("{}", r.err));
let reply = loop {
    if let Some(reply) = zp.engine().wait_check(&wait) {
        break reply;
    }
    let mut p = libc::pollfd { fd: zp.fd(), events: libc::POLLIN, revents: 0 };
    unsafe { libc::poll(&mut p, 1, zp.timeout_ms().min(i32::MAX as u64) as i32) };
    zp.process();
};
zp.engine().wait_end(&wait);
```

:::

In Rust, `Engine::wait_start` and `wait_check` give you `wait-pane` inside the loop, with no polling of the screen. The session ends with `exec cat` so it stays alive after printing; a session that exits right after its last output can be reported as ended before that output is matched.

## 4. Open a socket for the CLI

The **server plugin** runs the same server as `zeropty serve` inside your program. The CLI, `attach`, the shell function and the Node client then reach your sessions, and `wait-pane` works over that socket:

::: code-group

```c [app.c]
if (zeropty_listen(a, "/tmp/myapp.sock") != 0) { /* could not open it */ }
```

```sh [Terminal]
ZEROPTY_SOCK=/tmp/myapp.sock zeropty capture-pane -p -t '=w:'
ZEROPTY_SOCK=/tmp/myapp.sock zeropty attach -t w
```

:::

- The path must be 103 bytes or less. If a server already answers there, `zeropty_listen` returns -1.
- `kill-server` from the socket ends only the sessions; your program keeps running.
- Sessions still end with your program. `push-sessions` them to a standalone server before exiting to keep them ([Move sessions](/guide/moving)).

## Verify

Build and run each example from the zeropty folder (the Rust excerpt in step 3 is shortened from `examples/polled.rs`):

::: code-group

```sh [C]
cc examples/c/hello.c -Idist -Ldist -lzeropty_c -o /tmp/zeropty-hello
DYLD_LIBRARY_PATH=dist /tmp/zeropty-hello
```

```sh [Rust]
cargo run --release --example embed
cargo run --release --example polled
```

:::

The C program prints each request followed by its reply. You should see `{"code":0,"out":"","err":""}` for the first three, and the `capture-pane` reply holds `hi from C` twice: once echoed as typed, once printed back by `cat`.

`embed` prints the typed line, the answer and the session list:

```text
echo "answer: $((6 * 7))"
answer: 42
demo 60x10
```

`polled` prints the wait result (`NNN` is the milliseconds) and the screen:

```text
wait-pane: code 0 after NNN ms
build done
```

## Troubleshooting

| Symptom | Fix |
|---|---|
| `Library not loaded: … libzeropty_c.dylib` | Set `DYLD_LIBRARY_PATH=dist`, or link with `-Wl,-rpath,"$PWD/dist"`. |
| `zstd-sys` fails to build | Install the Xcode Command Line Tools. |
| Sessions vanish when the program exits | They live in your process. `push-sessions` them to a server first. |
| A call from another thread misbehaves on a polled engine | Every call must come from the loop's thread. Use `zeropty_open` for calls from many threads. |
| A plugin command holds your loop | Start it with `zeropty_command_start` and collect it with `zeropty_call_poll`. See [Write a plugin](/guide/plugins). |

## Next steps

- [C ABI](/reference/c-abi) for every function, type and macro of the C ABI.
- [Rust API](/reference/rust-api) for the Rust crate and building its rustdoc.
- [Four ways to drive zeropty](/guide/concepts/interfaces) to compare embedding with the socket and the CLI.
- [Write a plugin](/guide/plugins) to add commands to an embedded engine.
