# Plugins

**English** · [한국어](ko/PLUGINS.md)

[README](../README.md) · [Docs](README.md) · [Usage](USAGE.md) · [Plugins](PLUGINS.md) · [FAQ](FAQ.md) · [Benchmarks](BENCHMARKS.md) · [Design](DESIGN.md) · [Versioning](VERSIONING.md) · [Releasing](RELEASING.md)

A plugin is code that lives outside zeropty. It listens to what happens to sessions (creation, exit, rename, output) and adds new commands. The commands it adds are called like built-in commands, from the CLI, the socket, or an embedded call. You can attach a plugin without recompiling zeropty.

```sh
python3 examples/plugins/hello.py &      # a program written in any language attaches to the socket
zeropty hello you                        # → hello, you! sessions: ...
zeropty list-plugins                     # → hello: running (api 1) events=session-created,session-exited commands=hello access=read dropped=0
```

## Ways to attach

| Method | Language | Where it runs | Recompile | Example |
|---|---|---|---|---|
| **Socket (out of process)** | Any language (socket + JSON) | Separate process | Not needed | [`examples/plugins/hello.py`](../examples/plugins/hello.py), [`examples/plugins/notify.mjs`](../examples/plugins/notify.mjs) |
| **Plugin library (`.dylib`)** | C, or any language that can export C functions | A separate process via `zeropty plugin-run`, or inside the host process | Not needed (only `zeropty.h` is required) | [`examples/plugins/tally.c`](../examples/plugins/tally.c) |
| **C ABI callback (in process)** | C, or any language that can pass C functions | Host process, on a dedicated plugin thread | Not needed (`libzeropty_c.dylib`) | [`examples/plugins/count.c`](../examples/plugins/count.c) |
| **Rust trait (in process)** | Rust | Host process, on a dedicated plugin thread | Only the host program | [`examples/plugin_activity.rs`](../examples/plugin_activity.rs) |

All four share the same registry, the same events, the same access levels and the same isolation rules. Internally, a socket plugin is wrapped in a single Rust plugin (`Remote` in `src/plugin/__private/remote.rs`), so queues, deadlines and failure handling apply identically. Plugins that should always run alongside the server go in the [plugin directory](#autostart-the-plugin-directory).

## What plugin authors need to know

These are all the rules you need to write a plugin. The numbers are defaults; if you change the deadline (`ZEROPTY_PLUGIN_TIMEOUT_MS`, or `Plugins::set_timeout` in Rust; 10 seconds by default), they scale proportionally. Why things work this way is explained in [Behavior model](#behavior-model).

| Rule | Default | When exceeded |
|---|---|---|
| Plugin code never runs on the engine thread. Each plugin has its own thread (for out-of-process plugins, its own process), and hooks are called one at a time. Hooks may block | | |
| Rust `manifest()` is called once at registration, first thing on the plugin's new thread. `add` waits for it meanwhile. Build it directly from constants and do slow setup in `start` | 1 second (or the deadline, if shorter) | `add` returns a `ManifestTimeout` error and the plugin is not registered. Only the caller of `add` (the host loop, for `Polled`) waits that long; the engine does not wait. A panic gives a `ManifestPanicked` error |
| Events arrive through a per-plugin queue | 1024 events, 8MB of output | The event is dropped and counted in `dropped=` of `list-plugins`. The engine does not wait |
| Answering an added command (`call`) | 8.75 seconds (7/8 of the deadline) | Only that call fails with `plugin timed out`; a late reply is discarded. The plugin stays `running` |
| Time a single hook may run on its own time | 10 seconds (the deadline) | `failed (stuck in a hook ...)`; it receives no more events or commands |
| Commands a plugin asks the engine to run (`Host::command`, socket `id`, C `zeropty_host_command`) | 5 seconds (half the deadline), only within its access | Only that command is answered with an error. Time spent waiting does not count toward the hook's time |
| A socket plugin must read the lines zeropty sends it | 8.75 seconds | `failed (connection: not read ...)` |
| A panic in a hook, or a crash of an out-of-process plugin | | Only that plugin becomes `failed` or is removed. The engine and other plugins are unaffected. It can reattach under the same name |
| Calls where plugins wait on each other (A → B → A) | | Rejected immediately with a `call cycle` error |

- For long-running work, reply "started" first and let the result be fetched with another command.
- A command that the caller gave up on is never run later (the error ends in `not run`).
- On shutdown, `stop` arrives. Out-of-process plugins get EOF when the server exits.
- With no plugins there is no cost: the output path adds one branch, and commands read one extra atomic variable ([measurements](#cost-with-no-plugins)).

## Calling plugin commands from the host

Commands added by plugins are called from the same places as built-in commands. The difference is that you can choose whether the calling thread is held while the plugin works.

| Host | Blocking (does not return until the reply) | Non-blocking (returns immediately) |
|---|---|---|
| Rust `Embedded` | `command(argv, cwd)` | `command_start(argv, cwd) -> Call` |
| Rust `Polled` | `command(argv, cwd)` (keeps running the engine while it waits) | `command_start(argv, cwd) -> Call` |
| C | `zeropty_command` | `zeropty_command_start` → `zeropty_call_poll` → `zeropty_call_free` |
| Node (`client/embedded.mjs`) | `zeroptySync(argv)` | `await zeropty(argv)` (`Engine.commandStart`: always `Promise<Reply>`) |

- Engine commands (commands that do not belong to a plugin) run immediately even through `command_start`, and come back as a `Call` that already holds the reply. So you can handle every command with the same code.
- `Call::poll()` returns `None` while waiting, and returns `Some(Reply)` once when the reply arrives, the plugin stops, or the deadline passes. On a thread that may block, use `Call::wait()`. `Call::due_ms()` is the time left until the deadline.
- On a polled engine, a plugin's reply makes `fd()` readable, and `timeout_ms()` takes the deadlines of started calls into account. So a loop that sleeps on `fd()` and `timeout_ms()` notices a reply or a deadline right away, with no separate wakeup needed.

```rust
let call = polled.command_start(&argv, "/");       // returns immediately
loop {                                              // the host's event loop
    poll_readable(polled.fd(), polled.timeout_ms()); // whatever poll/kqueue/libuv the host uses
    polled.process();
    if let Some(reply) = call.poll() { break reply; }
}
```

```c
zeropty_call *c = zeropty_command_start(a, "{\"argv\":[\"hello\",\"you\"],\"cwd\":\"/\"}");
for (;;) {
    struct pollfd p = { zeropty_fd(a), POLLIN, 0 };
    poll(&p, 1, (int)zeropty_timeout_ms(a));
    zeropty_process(a);
    char *r = zeropty_call_poll(c);                 // NULL while waiting
    if (r) { puts(r); zeropty_free(r); break; }
}
zeropty_call_free(c);                               // freeing before the reply means a command not yet started is never run
```

- In Node, if you attached a plugin from **the same process** (`attachPlugin`) to a socket opened with `listen`, you must call it with `await zeropty(...)`. `zeroptySync` holds the JS thread while it waits, and since that plugin also runs on the same JS thread, it cannot reply before the deadline.

## Access

The commands a plugin can run through `Host::command` (the `id` command over the socket, `zeropty_host_command` in C) are limited to the access it declared at registration. The default is view-only (`read`).

| Access | Rust | Allowed commands |
|---|---|---|
| `read` (default) | `Access::READ` | `capture-pane`, `display-message`, `has-session`, `list-sessions`, `grep-panes`, `show-environment`, `stats`, `mem`, `list-plugins` |
| `input` | `Access::INPUT` | `send-keys`, `paste-buffer`, `load-buffer` |
| `manage` | `Access::MANAGE` | `new-session`, `kill-session`, `rename-session`, `resize-*`, `set-environment`, `pipe-pane` (runs a program), and any other command not listed above |
| `call` | `Access::CALL` | Commands added by other plugins |
| `all` | `Access::ALL` | All of the above |

- Where to declare it: Rust `Manifest::new("x").access(Access::INPUT)`, socket `plugin-attach ... -a input,call`, C and library manifests `"access":["input"]`, Node `attachPlugin({ access: ['input'] })`.
- A command outside the plugin's access is not run and is answered with `send-keys: plugin x lacks access input (...)`. `access=` in `list-plugins` shows each plugin's access.
- Not allowed under any access level: `kill-server`, `pull-sessions`, `push-sessions` (handing sessions over holds the engine thread), `attach`, `plugin-attach`, and the plugin's own commands.
- Access is a boundary that makes a plugin's intent visible and prevents mistakes. An out-of-process plugin running as the same user can also connect to the socket as an ordinary client, so the security boundary is the socket (accessible only to this user).

## 1. From another process: the socket protocol (any language)

You can attach whenever a socket is open, whether it is `zeropty serve` or an embedded engine that called `listen`. One connection is one plugin. Every message is a single line of JSON (terminated by `\n`).

**Step 1: connect.** Connect to `ZEROPTY_SOCK` (or `~/.zeropty/mux.sock` if unset) as a Unix socket. These are the same rules as the CLI.

**Step 2: attach.** Send `plugin-attach` as the first command.

```json
{"argv":["plugin-attach","-n","hello","-e","session-created,session-exited","-c","hello"],"cwd":"/"}
```

| Option | Meaning |
|---|---|
| `-n name` | Plugin name (required). Letters, digits, `- _ . :`, up to 64 characters |
| `-e list` | Events to receive, comma-separated. `sessions` (create, exit, rename) and `all` also work. If omitted, no events are delivered |
| `-c list` | Command names to add, comma-separated. Built-in command names and names owned by other plugins cannot be used |
| `-a list` | Access levels (table above), comma-separated. Defaults to `read` |
| `-v version` | The API version this plugin was written for (default 1). Rejected if higher than zeropty's |

The reply is a line like any ordinary command reply. On success it is `{"code":0,"out":"zeropty-plugin 1\n","err":""}`, and by the time this reply arrives, registration is already complete. On failure it is `code` 1 with the reason (the connection stays a client, so you can fix the request and send it again).

**Step 3: exchange messages.** After attaching, the following messages flow over this connection. The first key identifies the message type.

| Direction | Message | What to do |
|---|---|---|
| zeropty → plugin | `{"event":"session-created","session":"w"}` | An event (table below) |
| zeropty → plugin | `{"call":7,"argv":["hello","you"],"cwd":"/"}` | An added command was called. Reply with the same number |
| plugin → zeropty | `{"reply":7,"code":0,"out":"hello\n","err":""}` | Reply to a `call` |
| plugin → zeropty | `{"id":3,"argv":["capture-pane","-p","-t","=w:"],"cwd":"/"}` | Run a zeropty command (within access) |
| zeropty → plugin | `{"id":3,"code":0,"out":"...","err":""}` | Reply to that command (in the order sent) |

- You may send `id` commands while answering a `call` (for example, `hello.py` gets the session list with `list-sessions` and puts it in its reply). While you wait for that reply, events or other `call`s may arrive first, so queue them in the order received and handle them afterwards.
- `wait-pane` cannot be used from plugins. Use events for waiting.
- Closing the connection removes the plugin from the registry, and its commands become `unknown command` again. When the server exits, the plugin gets EOF.
- Socket writes by zeropty have a deadline (`reply_wait`, 7/8 of the deadline). If a plugin stops reading and holds out, only that plugin is marked failed.
- Reply to a `call` within 7/8 of the deadline (8.75 seconds by default). If you are late, only that call fails with `plugin timed out` and the late `reply` is discarded. The plugin stays attached. For long-running work, reply "started" first and let the result be fetched with another command.

**Python, standard library only:** [`examples/plugins/hello.py`](../examples/plugins/hello.py) is the whole protocol (68 lines).

```sh
python3 examples/plugins/hello.py &
zeropty hello you
```

**Node:** `attachPlugin` in [`client/plugin.mjs`](../client/plugin.mjs) handles connecting, line splitting and matching reply numbers.

```js
import { attachPlugin } from './client/plugin.mjs'
const plugin = await attachPlugin({
  name: 'notes',
  events: ['session-exited'],
  access: [],                       // view only. Use ['input'] if you need to send keys
  onEvent: (ev) => console.log(`${ev.session} ${ev.reason}`),
  commands: {
    // Return a string for output, or { code, out, err }. An exception becomes code 1 with its message.
    'notes-count': async (argv, { plugin }) => {
      const { out } = await plugin.command(['list-sessions', '-F', '#{session_name}'])
      return `${out.split('\n').filter(Boolean).length}\n`
    },
  },
})
```

[`examples/plugins/notify.mjs`](../examples/plugins/notify.mjs) adds a log of session creation and exit (`zeropty notify-log`) and a finder for quiet sessions (`zeropty notify-idle 10`), and with `--desktop` it shows a macOS notification when a session ends.

## 2. Plugin libraries (`.dylib`)

Build a plugin written in C into a library with nothing but `zeropty.h`. It does not link against `libzeropty_c`. There is a single entry point.

```c
#include "zeropty.h"

static const zeropty_plugin_api *zp;   // functions provided by the host: host_command, free

static char *on_message(void *user, const char *msg, zeropty_host *host) {
    if (strncmp(msg, "{\"call\"", 7) != 0) return NULL;          // start, events, stop
    char *r = zp->host_command(host, "{\"argv\":[\"list-sessions\"],\"cwd\":\"/\"}");
    zp->free(r);
    return strdup("{\"code\":0,\"out\":\"hi\\n\",\"err\":\"\"}");  // malloc'd reply: zeropty frees it
}

int zeropty_plugin_init(const zeropty_plugin_api *api, zeropty_plugin_def *def) {
    if (api->api < 1) return 1;
    zp = api;
    def->manifest_json = "{\"name\":\"hi\",\"events\":[\"session-created\"],\"commands\":[\"hi\"]}";
    def->callback = on_message;
    def->user = NULL;
    return 0;                                                    // nonzero means the library is not loaded
}
```

1. Build: `cc -dynamiclib -O2 -Ibindings/c/include -o tally.dylib examples/plugins/tally.c`
2. Attach to the server: `zeropty plugin-run ./tally.dylib &`. `plugin-run` loads the library into its own process and attaches to the server over the socket protocol. If the library crashes, only that process dies, and the server just sees a closed connection. When the server exits, `plugin-run` exits too.
3. Keep it running: copy the library into the [plugin directory](#autostart-the-plugin-directory) (set with `ZEROPTY_PLUGIN_DIR`), and the server starts it with `plugin-run` on startup and restarts it if it dies.
4. Load it into your own process: a program that embeds the engine loads the same library with `zeropty_load_plugin(a, "tally.dylib")` (in this case, a crash in the library is a crash of that process).

- `size` in `zeropty_plugin_api` is the size of the struct passed by the host. Later versions only add fields at the end.
- The messages the callback receives are the same as the socket protocol, plus `{"start":{"api":1}}` first and `{"stop":true}` last. For a `call`, return a malloc'd `{"code","out","err"}` reply.
- The callback is called one at a time (on one thread). `host` is valid only for the duration of that call.

## 3. Inside your process: C ABI callbacks

A program that embeds the engine with `zeropty_open` (or `zeropty_open_polled`) adds a plugin with a single function. The callback and messages are the same as in section 2, and commands are run with `zeropty_host_command`.

```c
#include "zeropty.h"   // cc app.c -Ldist -lzeropty_c

static char *on_message(void *user, const char *msg, zeropty_host *host) {
    if (strncmp(msg, "{\"call\"", 7) == 0) {
        char *r = zeropty_host_command(host, "{\"argv\":[\"list-sessions\"],\"cwd\":\"/\"}");  // ask the engine
        zeropty_free(r);
        return strdup("{\"code\":0,\"out\":\"hi\\n\",\"err\":\"\"}");   // malloc'd reply: zeropty frees it
    }
    return NULL;   // NULL for events, start, stop
}

char *err = zeropty_add_plugin(a, "{\"name\":\"hi\",\"events\":[\"session-created\"],\"commands\":[\"hi\"]}", on_message, NULL);
if (err) { fprintf(stderr, "%s", err); zeropty_free(err); }   // NULL means it is registered
```

- Use `zeropty_host_command` only inside the callback (`host` is valid only for that long). It is safe with both the engine-thread and polled modes (it runs through the engine loop).
- Full example: [`examples/plugins/count.c`](../examples/plugins/count.c) (after `./build.sh`, `cc -O2 -o /tmp/count examples/plugins/count.c -Idist -Ldist -lzeropty_c -Wl,-rpath,"$PWD/dist"`).
- Embedded Node (`client/embedded.mjs`) opens a socket with `listen` and attaches with method 1. Call that plugin's commands with `await zeropty(...)`.

## 4. Inside your process: Rust

Plugins use only `zeropty::plugin` (`Reply`, `ok` and `fail` are re-exported there too). Plugin code stays the same even if the engine's module layout changes.

```rust
use zeropty::embed::Embedded;
use zeropty::engine::Options;
use zeropty::plugin::{ok, Access, Event, Events, Host, Manifest, Plugin, Reply};

struct Bell;

impl Plugin for Bell {
    fn manifest(&self) -> Manifest {
        Manifest::new("bell").events(Events::SESSION_EXITED).command("bell-test").access(Access::READ)
    }
    fn event(&mut self, event: &Event, _host: &Host) {
        if let Event::SessionExited { session, reason } = event {
            eprintln!("\x07{session} {}", reason.as_str());
        }
    }
    fn command(&mut self, _argv: &[String], _cwd: &str, host: &Host) -> Reply {
        let r = host.command(&["list-sessions"], "/");   // an engine command, run on the engine thread
        ok(format!("{} sessions\n", r.out.lines().count()))
    }
}

let e = Embedded::start(Options::default());
e.add_plugin(Bell)?;                        // events are delivered from the moment this line returns
e.command(&["bell-test".into()], "/");      // callable the same way from the socket (listen) and the CLI
```

- `Polled::add_plugin` works the same way. The polled engine keeps running even while waiting for a plugin command. To avoid holding the host thread, use `command_start` ([Calling plugin commands from the host](#calling-plugin-commands-from-the-host)).
- If a hook waits on something outside zeropty (another process, the network), wait no longer than `zeropty::plugin::reply_wait(host.deadline())`. Then, if you run late, only that call fails and the watchdog does not cut off the plugin (the server side of socket plugins does exactly this).
- Try it: `cargo run --release --example plugin_activity` (an `activity` command that counts the bytes each session has output). Pass a socket path as an argument and it stays up, so you can call it with `ZEROPTY_SOCK=path zeropty activity`.
- A host with its own loop (a program that drives `Engine` directly) attaches the registry to `host::HostLoop` as a service. It takes three lines, and `src/cli/serve.rs` does exactly this.

  ```rust
  let plugins = Plugins::new();
  plugins.attach(&engine)?;                  // lets plugins wake the loop (one user event on the engine queue)
  server.set_plugins(plugins.clone());       // socket: list-plugins, plugin commands, plugin-attach
  let mut host = HostLoop::new(Some(server), Waiting::WithEngine);
  host.set_plugins(plugins.clone());         // pump after the server on every turn; watchdog cap on sleep time
  loop { host.turn(&mut engine, Source::Wait(None), Yield::Never, |_, _, _| {})?; }
  ```

  A host without a socket attaches plugins with `Plugins::add`, and calls plugin commands the way `Plugins::list` and the `command`/`command_start` of `Embedded` and `Polled` do.
- When registration fails, `PluginError` tells you why. For example, on `Err(PluginError::CommandTaken { owner, .. })` retry with a different name, and on `Err(PluginError::Api { supported, .. })` lower the version with `.api(supported)`. Handle the rest with a `_` arm, such as `Err(e) => eprintln!("{e}")`.

## Autostart: the plugin directory

If you set a directory with `ZEROPTY_PLUGIN_DIR`, `zeropty serve` starts the plugins in it on startup. **If it is not set, nothing is started (the default).**

> **Security and compatibility:** Setting the directory means you agree that the code in it may run with the same user privileges as the server. It is off by default so that existing commands (`serve`, and `new-session`, which starts the server) never silently run files from a well-known path. So the default behavior is the same as before this feature existed.

```sh
export ZEROPTY_PLUGIN_DIR=~/.zeropty/plugins               # put this in your shell config (~/.zshrc etc.) to keep it always on
mkdir -p ~/.zeropty/plugins
cp /tmp/tally.dylib ~/.zeropty/plugins/                  # a .dylib is started with zeropty plugin-run
printf '#!/bin/sh\nexec python3 /path/to/hello.py\n' > ~/.zeropty/plugins/hello
chmod +x ~/.zeropty/plugins/hello                          # an executable is started as is
zeropty restart-server                                     # takes effect from the next server
```

- The server inherits the environment of the command that started it. `ZEROPTY_PLUGIN_DIR` must be set in the shell where `new-session` starts the server, or where `restart-server` starts the new server (the same rule as `ZEROPTY_SOCK`). A relative path is made absolute relative to the directory where the server started.

- Executable files and `.dylib` files are started in name order. Files starting with `.` and files without execute permission are skipped.
- Each program receives the server socket in `ZEROPTY_SOCK`, runs with the directory as its working directory, and its output goes to `<name>.log` (rewritten from scratch once it exceeds 1MB). Each program runs `plugin-attach` itself.
- A program that exits is restarted. If it exits quickly, the wait before restarting starts at 0.5 seconds and doubles up to 30 seconds; if it ran for 30 seconds or more, it is restarted after 0.5 seconds. Files removed from the directory are not restarted.
- On `kill-server`, the server sends SIGTERM to the programs it started (each in its own process group).
- **Plugins survive `restart-server`.** When the old server exits, its plugins get EOF and exit, and the new server starts its own plugins after it takes over the socket (after `mux.sock.new` disappears). Plugins need no reconnect logic of their own. In between, for a brief moment (tens of ms), plugin commands may return `unknown command`.
- If the directory or a file is writable by other users (group or other write permission, or a different owner), it is skipped and the reason is logged to the server's stderr.
- The plugin directory is a feature of `zeropty serve`. Its implementation (`zeropty::plugin::__private::launch`) is internal to zeropty and is not a stable API (see the API reference below).

## Events

| Name (`-e`) | Rust | Fields | When |
|---|---|---|---|
| `session-created` | `Event::SessionCreated` | `session` | `new-session`, or when taken over from another server |
| `session-exited` | `Event::SessionExited` | `session`, `reason`: `exited` · `killed` · `handed-over` | When the program exits (`kill-server` is also `exited`), `kill-session`, or when handed over to another server |
| `session-renamed` | `Event::SessionRenamed` | `session` (new name), `from` | `rename-session` |
| `output` | `Event::Output` | `session`, `data` (socket and C: base64) | Every time bytes output by the program are read. They are read in batches together with the screen, so for quiet sessions this is in units of \~300ms. A single event holds at most 64KB, so a large read arrives as several events. A UTF-8 character can be split across two events |
| `activity` | `Event::Activity` | `session` | Every time reading output changes the screen. It copies no bytes, so it is cheaper than `output` |
| `sessions` | `Events::SESSIONS` | | The three lifecycle events above |
| `all` | `Events::ALL` | | Everything |

The engine copies the bytes it reads only when some plugin wants `output`. If you only need to know that the screen changed, use `activity` and read with `capture-pane` when needed.

- **`output` can be lossy.** Because of the memory budgets (16MB per handoff from the engine, 8MB per plugin queue), even a plugin that reads promptly loses part of a very large burst of output (tens of MB or more). If you need every byte of output, use `pipe-pane` (which sends session output as-is to a file or command) instead of a plugin.
- **`dropped=` is a single total.** Events dropped because the queue was full (1024 events or 8MB of output), output dropped because the engine exceeded its 16MB budget, and events dropped because the engine-side queue overflowed are all added to the same number. They are not counted separately by reason.

## Commands

- `list-plugins`: one line per plugin, `name: state (api N) events=... commands=... access=... dropped=N`. Works over the socket, the CLI and embedded calls. There are three states.
  - `running`: receiving events and commands.
  - `stopped`: being removed. Shown briefly after the connection closed or the plugin was `remove`d but before the host loop has taken it off the list. Commands are no longer routed to it.
  - `failed (reason)`: a panic (`panicked in hook: message`), a hook that exceeded the deadline (`stuck in a hook for more than N ms`), or a connection error (`connection: ...`). It stays on the list so you can see the reason, and gives up its slot when a plugin reattaches under the same name. Only the 16 most recent failed entries are kept, and the oldest are removed first (so a program that keeps reattaching under new names cannot grow the registry without bound).
- `plugin-attach`: section 1 above. Meaningful only on a socket connection (sent from the CLI, the connection is closed right after the reply, so the plugin is removed immediately).
- `zeropty plugin-run library.dylib`: section 2 above. Exits when the server exits.

## API reference (Rust, `zeropty::plugin`)

The stability boundaries match the module and file boundaries of the code.

| File | Path | Used by | Stability |
|---|---|---|---|
| `src/plugin/api.rs` | `zeropty::plugin::api` (= `zeropty::plugin::*`) | Plugin authors | Stable API (versioning below) |
| `src/plugin/registry.rs`, `src/plugin/hosting.rs` | `Plugins`, `Call` | Hosts that embed the engine | Public methods are stable API |
| `src/host.rs` | `HostLoop::set_plugins`, `impl Service for Plugins` | Hosts with their own loop | Same as the host loop API (`zeropty::host`) |
| `src/plugin/__private/{remote,native,launch}.rs`, `src/plugin/wire.rs` | `zeropty::plugin::__private` (`#[doc(hidden)]`, not in the docs) | The zeropty binary, C bindings, tests | **Unstable**, outside semver |
| `src/engine/tap.rs` | (only `Event` and `ExitReason` are public, via `zeropty::plugin`) | Engine | Queues and methods are crate-internal |

**Stable API.** The parts used by plugin authors and by hosts that embed the engine. Within the same `API_VERSION`, things are only added (see versioning below).

| Item | Description |
|---|---|
| `API_VERSION` | The version of this interface (currently 1). `Manifest::api` and `plugin-attach -v` are compared against it |
| `trait Plugin: Send + 'static` | `manifest()` (required), `start`, `event`, `command`, `stop` (with default implementations). The four hooks (`start`, `event`, `command`, `stop`) are called one at a time on the plugin thread. `manifest()` is called once at registration, first thing on the plugin's new thread, and `add` waits for its answer only until the manifest deadline (the registry deadline, at most 1 second). If it takes longer, registration is rejected with `PluginError::ManifestTimeout(ms)`, and the plugin is dropped on its own thread as soon as `manifest()` finishes. So even a stuck `manifest()` holds the caller of `add` for at most 1 second, and never holds the engine. Build it directly from constants and do slow setup in `start`, which runs under the deadline. A panic is caught and becomes `PluginError::ManifestPanicked` |
| `Manifest::new(name).events(Events).command(name).access(Access).api(v)` | Name, events to receive, commands to add, access, version. `events` and `access` add to what is already there. The fields `events: Events` and `access: Access` (which always includes `READ`) are readable. `#[non_exhaustive]` |
| `Manifest::from_json(text)`, `attach_argv()` | Parse the JSON manifest of C plugins and libraries; the `plugin-attach` command for the same manifest |
| `Event` | `#[non_exhaustive]` enum. `kind() -> Events` (the bit for that single event), `name()`, `session()`, `to_json()` |
| `ExitReason` | `Exited`, `Killed`, `HandedOver`. `as_str()`. `#[non_exhaustive]`, so a `match` needs a `_` arm |
| `PluginError` | Registration and manifest errors. `#[non_exhaustive]` enum: `BadName`, `BadCommand`, `Api { api, supported }`, `UnknownEvent`, `UnknownAccess`, `BuiltinCommand`, `DuplicateCommand`, `NameTaken`, `CommandTaken { owner }`, `BadManifest`, `ManifestPanicked`, `ManifestTimeout(ms)`, `Thread`, `Unsupported` (a frontend that does not run plugins). `Display` gives a human-readable reason; `String::from` appends a newline, like a reply's `err`. Implements `std::error::Error` |
| `Events` | A set of events to receive (typed bit flags). Constants `Events::{SESSION_CREATED, SESSION_EXITED, SESSION_RENAMED, OUTPUT, ACTIVITY, SESSIONS, ALL}`. `Copy`, `Eq`, `Hash`, `Debug` (`Events(session-created \| output)`), `Default` (the empty set), `Display` (the same comma-separated list as the socket). Operators `\|`, `\|=`, `&`, `&=`, `-` (difference), `!` (complement within the known bits). Methods `empty()`, `all()`, `is_empty()`, `contains(other)`, `intersects(other)`, `union`, `difference`, `bits() -> u32`, `from_bits(u32) -> Option<Events>` (`None` if there are unknown bits), `from_bits_truncate`, `parse("a,b") -> Result<Events, PluginError>` (`FromStr` is the same), `names() -> String` |
| `Access` | A set of access levels. Constants `Access::{READ, INPUT, MANAGE, CALL, ALL}`. Same traits and methods as `Events` (`parse("read,input")`, `names()`) |
| `Host::command(argv, cwd) -> Reply` | Runs an engine command within the plugin's access and waits for the reply (up to half the deadline; time spent waiting does not count toward the hook's time). Commands of other plugins require `CALL`, and calls that would form a cycle are rejected |
| `Host::deadline() -> Duration` | The deadline (`set_timeout`, `ZEROPTY_PLUGIN_TIMEOUT_MS`). A hook that runs longer than this on its own time is cut off |
| `reply_wait(deadline) -> Duration` | How long a hook may wait on something outside zeropty: the deadline minus 1/8 of it (at least 20ms, at most half). Ensures it gives up clearly before the watchdog fires |
| `Host::name()`, `Host::detach()`, `Host::fail(why)` | The name; detach itself; mark itself failed and detach (no commands arrive from that moment on) |
| `Reply`, `ok(out)`, `fail(err)`, `Reply::new(code, out, err)` | A command's reply (read its `code`, `out` and `err` fields). Re-exported from the engine module. It is exactly the reply of the CLI and the socket, so the three fields are fixed (you may also build it directly as `Reply { code, out, err }`) |
| `is_reserved(name) -> bool` | Whether a name is answered directly by the engine or the host (server, CLI), and so cannot be owned by a plugin. The engine side reads the engine's command table (`COMMANDS`, including aliases) directly, so new engine commands are reserved automatically. The list itself is not API: it grows as zeropty adds commands, so use your own prefix, such as `myplugin-...` |
| `Plugins` | The registry. `add`, `add_boxed`, `check` (`Result<(), PluginError>`), `remove` (`false` if absent), `names`, `list`, `set_timeout`, `timeout`, `set_shutdown_wait`, `shutdown`, `attach(&engine)` (lets plugins wake that engine's loop; it takes the engine so that the queue type, currently kqueue, does not leak into the API), `watchdog_ms()` (the longest the loop may sleep while plugins exist). Implements `host::Service`. All public methods are stable API |
| `HostLoop::set_plugins(plugins)`, `HostLoop::plugins()` | Makes the host loop run the registry as a service (`zeropty serve`, `Embedded` and `Polled` use this) |
| `Embedded::add_plugin`, `add_plugin_boxed`, `Embedded::plugins()`, `Polled::add_plugin`, `add_plugin_boxed`, `Polled::plugins()` | Attach to an embedded engine (`Result<(), PluginError>`). The `embed::Frontend` used by the bindings also has `add_plugin_boxed` and `command_start`. Both have default implementations, so code that implemented `Frontend` before plugins existed still compiles (the default `add_plugin_boxed` is `Err(PluginError::Unsupported)`, and the default `command_start` answers immediately with `command`) |
| `Embedded::command_start`, `Polled::command_start` `-> Call` | Call a command without waiting. Engine commands come back as a `Call` that holds the reply |
| `Call` | A command in progress. `poll() -> Option<Reply>` (gives the reply once), `wait() -> Reply`, `due_ms()`, `answered()`, `Call::ready(reply)`. Dropping it before the reply means a command not yet started is never run. `Polled::timeout_ms()` only counts the deadlines of `Call`s still waiting for a reply. A `Call` that has been answered or dropped does not wake the loop |

**`zeropty::plugin::__private` (not stable API).** The parts used by zeropty's own binary (`plugin-run`, the plugin directory), the C bindings and the tests are gathered in this one module. It is **unstable and outside semver**: it can change or disappear at any time without a version bump. The stable plugin API is only `api` (= `zeropty::plugin::*`), `Plugins` and `Call`, and plugins and hosts do not use `__private`. This follows the same convention as serde's `__private`: it is `#[doc(hidden)]` and does not appear in the API docs, and no item of the stable API takes or returns a type from this module. Using a path with `__` in its name is itself the signal that you are using something unstable.

Why it is not hidden behind a cargo feature: the binary (`src/main.rs`) and the bindings are separate crates from the library, so they need a public path, and cargo has no way for a package to enable a feature only for its own binaries. Putting it behind a feature would mean either moving the binary to another package (which breaks `cargo install --path .`) or adding `required-features` (which makes `cargo install` skip the binary), so we chose to keep the install path unchanged. That is why the binary and its tests stay in the root package `zeropty` as before:

```sh
cargo install --path .              # install the zeropty binary
cargo build --release -p zeropty    # target/release/zeropty
```

| Item | Description |
|---|---|
| `remote` (`Remote`, `parse_attach`) | The server side of socket plugins (`plugin-attach`) |
| `launch::{Autostart, configured_dir, programs}` | The plugin directory (`ZEROPTY_PLUGIN_DIR`) |
| `unsafe native::{load, run}`, `unsafe native::Library::from_parts`, `native::{Api, Def, ENTRY}` | Loading plugin libraries (`zeropty_plugin_init`), `plugin-run`. Loading a library runs its code, hence `unsafe` (conditions under `# Safety`). From other languages, use the C ABI (`zeropty_load_plugin`) or `zeropty plugin-run` |
| `call_json`, `reply_json`, `reply_from_json`, `base64` | For implementing the protocol |
| `set_engine_events`, `take_engine_events`, `has_engine_events` | Test functions that inspect the engine-side event queue without a registry |

Not visible outside the crate: the engine-side queue (`Engine::set_tap`, `take_tap`, `has_tap_events`), the registry's `pump`, `call` and `owns`, and the wakeup event tag (`PLUGIN_TAG`) are `pub(crate)`.

C ABI (`bindings/c/include/zeropty.h`): `zeropty_add_plugin`, `zeropty_host_command`, `zeropty_load_plugin`, the library entry point `zeropty_plugin_init` with `zeropty_plugin_api` and `zeropty_plugin_def`, and the non-blocking calls `zeropty_command_start`, `zeropty_call_poll`, `zeropty_call_due_ms`, `zeropty_call_free`. Node (`zeropty.node`): `Engine.commandStart(argv, cwd)` always returns a `Promise<Reply>`. For engine commands, the Promise is already resolved. `Reply` and Promise are never mixed, so you can always just `await`.

Registration rules: names and command names use letters, digits and `- _ . :` (up to 64 characters, and may not start with `-`). Built-in commands (names for which `is_reserved` is true) and the names and commands of other running plugins cannot be used. Names of failed or stopped plugins can be reused (reconnecting).

Operating limits: plugin queue 1024 events and 8MB of output (output copying resumes once it drains below 256 events and 2MB), 64KB per `output` event, engine-side queue 16384 events and 16MB of output (a single read is also copied up to 16MB), deadline 10 seconds (`Host::command` 5 seconds; `call` replies and socket writes for socket plugins 8.75 seconds), watchdog period 1/4 of the deadline (10ms\~1 second), shutdown wait 1 second, 16 failed entries kept.

## Versioning

- A plugin declares the version it was written for (`Manifest::api`, `-v`, `api` in the manifest). If it is newer than zeropty's, registration is rejected. Older versions keep being accepted.
- Within a version, things are only added: new hooks come with default implementations, and `Event`, `ExitReason`, `PluginError` and `Manifest` are `#[non_exhaustive]`, so existing plugins still compile when new events, exit reasons, error kinds or fields appear (which is why a `match` on these enums needs a `_` arm). `Reply` is both the reply of the CLI and the socket and a type that embedders build as a struct literal, so it is fixed at the three fields `code`, `out` and `err`. If more needs to be reported, a new type is added alongside it rather than adding fields to `Reply`. Socket plugins just need to ignore unknown keys and unknown `event`s. C's `zeropty_plugin_api` only adds fields at the end and announces them through `size`.
- Things that need byte-level compatibility (the message key order, with `event`, `call`, `reply` or `id` first) are pinned by tests.
- **`zeropty::plugin::__private` is outside this policy.** It exists for zeropty's own binary, bindings and tests, does not follow semver, and can change or disappear without a version bump. If you use it, the next release may break you. The compiler does not stop you (the same convention as serde's `__private`), so code that uses this name takes that risk on itself.
- Events and access are the `Events` and `Access` types, not raw `u32` bits. These types hold only the bits this version knows (`from_bits` returns `None` for unknown bits), so a manifest with unknown bits cannot be built. That is why `PluginError::UnknownEventBits` and `UnknownAccessBits`, which used to reject such manifests, were removed before the first release (they are in a `#[non_exhaustive]` enum, so no `match` breaks from this change). The socket, C and JSON formats (names, comma-separated lists) are unchanged.
- The JSON manifest of C plugins and dylibs (`Manifest::from_json`) is strict. Keys other than the five (`name`, `events`, `commands`, `access`, `api`), list values that are not strings, and values of the wrong type are registration errors (`bad manifest: ...`). This prevents a typo such as `"event"` from silently registering a plugin with zero events.

## Relationship to the socket server

The socket server (`src/server.rs`, `listen`) and the plugin registry are attached to the same mechanism. Both are `host::Service`s, and on every turn the host loop (`HostLoop`) runs the server and then the registry. The server, however, is not built on the `Plugin` trait.

- The server handles connections on the same thread and the same kqueue as the engine, so a command never crosses threads (that is a \~0.1ms difference). A `Plugin`, by definition, runs on a thread separate from the engine.
- The server needs direct access to engine internals (`Engine<B>`) for things like attach, wait-pane and session handover. `Plugin` is a boundary designed to operate only through public commands (`Host::command`) and within its access (`access`).

So making the server a plugin would cost either latency or the boundary. Instead, "riding the same loop next to the engine" (`Service`) and "listening to events and adding commands, separate from the engine" (`Plugin`) are kept apart, and the server is the entry point through which out-of-process plugins come in (`plugin-attach`). Plugin commands attached to the server are available to every client of the server.

## Tests

```sh
cargo test --release --workspace                        # everything: root package, C bindings, Node module
cargo test --release --test plugin                      # registration, events, command extension (embedded, socket, CLI), examples, isolation, deadlines
cargo test --release --lib -- plugin tap                # engine-side queue, manifest and protocol, order in which the host loop runs the registry
cargo test --release -p zeropty-c                       # C callback plugins, plugin libraries, calling from a polled engine
cargo test --release -p zeropty-node                    # Node module: await zeropty() does not block the event loop while waiting for a plugin
ZEROPTY_REQUIRE_ALL=1 cargo test --release --workspace  # release check: everything, with no skipped tests
```

- **Hooks and command extension**: event order and kinds, no unwanted events delivered, commands outside access rejected, registration already done when the attach success reply arrives, the plugin directory.
- **Bundled examples**: all of them run in the tests. `hello.py` is attached to a real `zeropty serve` and called from the CLI, and `tally.c` is attached with `plugin-run`. `count.c` is linked against the built C library (`libzeropty_c.dylib`), run, and its output (`created 2, ended 1, live 1`) checked. `notify.mjs` is attached to a real server, its logging of session creation, rename and exit and `notify-idle` (a command that queries the server from inside the plugin) are checked from the CLI, and the test confirms it exits on its own when the server exits. For `--desktop` notifications, the test checks that session names containing quotes (`"`) and `&` are passed as literal text rather than AppleScript code (by inspecting the arguments a fake `osascript` received, and running the same arguments through the real `osascript`).
- **Reserved names**: every engine command name and alias is reserved, and every reserved name that does not belong to the host is answered by the engine (both directions, `plugin::api` unit tests).
- **Isolation**: a panic in a hook; server peak memory staying within budget with an `output` plugin that never reads and 200MB of output; a stuck hook (engine commands are answered within 100ms meanwhile); a stuck `manifest()` (`add` returns `ManifestTimeout` within 1 second); an `output` plugin with a full queue; socket plugins that never reply or never read; call cycles; abandoned or dropped commands are never run; the watchdog runs even on an idle engine.
- **Host-side waiting**: a polled engine answers other clients and does not spin the CPU while waiting for a plugin command; deadlines of `Call`s that were answered or dropped do not shorten `timeout_ms()` (`embed` unit tests); Node `await zeropty()` does not block the event loop (`bindings/node/tests/plugin.rs`).
- **Cost**: with no plugins, the engine queues no events at all (`engine::tap` unit tests). Latency is covered in [Cost with no plugins](#cost-with-no-plugins).
- **C library and Node module**: the `count.c` test uses the C library, and the Node module tests use `zeropty.node`. `cargo test --workspace` builds them before the tests (because the lib of both packages is `cdylib`+`rlib`). A plain `cargo test --release` builds only the root package, so the `count.c` test uses the library built by `./build.sh` or `cargo build --release --workspace`. Either way, a library older than its sources is not used, so the tests never exercise stale code. The tests do not invoke cargo themselves.
- **Skipped tests**: if `python3`, `node`, `cc` or the built C library is missing, the tests that need it print `SKIPPED <test name>: <reason>` and move on. This line goes straight to the process's stderr rather than through `eprintln!`, which the test harness captures, so it shows up next to the results of a plain `cargo test`. With `ZEROPTY_REQUIRE_ALL=1`, they fail instead. The memory measurement that is sensitive to machine load (`a_burst_to_a_plugin_that_never_reads_stays_within_the_budgets`) also runs only in that case.

## Behavior model

This section details the rules in [What plugin authors need to know](#what-plugin-authors-need-to-know). The tests that verify each rule are listed in [Tests](#tests). How the registry, the server and the engine are connected is described in [PLUGINS-INTERNALS.md](PLUGINS-INTERNALS.md).

**Threads**

- Plugin code never runs on the engine thread. Each plugin has one dedicated thread, and the hooks (`start`, `event`, `command`, `stop`) are called one at a time on that thread. Hooks may block.
- Commands a plugin asks the engine to run (`Host::command`, the `id` command over the socket, `zeropty_host_command` in C) run on the engine thread just like any other command.
- The engine thread never blocks on a plugin socket. The success reply to `plugin-attach` and any replies backed up on that connection are sent by the plugin thread, and by the time the success reply goes out, registration is already complete.

**Events**

- The engine does not create events of kinds that no plugin wants.
- Events go to a per-plugin queue (1024 events). They are pushed with `try_send` on each turn of the host loop (`pump`); if the queue is full, the event is dropped and counted in `dropped=` of `list-plugins`. A slow plugin only loses events; it cannot hold up the engine.
- Queues are bounded by bytes as well as by count. `output` events are split into pieces of 64KB or less, and each plugin queue holds at most 8MB of output (pieces beyond that are dropped and counted in `dropped=`). The engine also copies at most 16MB of a single read, and counts the rest as one dropped event. So even with a plugin that never reads, the extra memory the server uses stays within these budgets (tens of MB combined). Previously a single event was everything read in one go, so 200MB of `yes` plus one `output` plugin that never read took server memory from 141MB to 656MB. `a_burst_to_a_plugin_that_never_reads_stays_within_the_budgets` pins the server's peak memory (`ri_lifetime_max_phys_footprint`) to within 64MB of the no-plugin case (101MB vs 131MB on this machine). This measurement can be affected by machine load, so it runs only with `ZEROPTY_REQUIRE_ALL=1`; the budgets themselves are always checked by deterministic unit tests (`bursts_come_in_bounded_pieces` in `engine::tap`, and the registry's byte-budget tests).
- When every plugin that wants `output` (or `activity`) has a full queue, the engine neither copies output nor creates events, and only counts `dropped=`. As soon as one of those queues drains below a quarter (256 events), copying resumes. Lifecycle events (`session-*`) are never switched off this way; they are counted one by one at the queue.
- The engine-side queue also has a limit (16384 events, 16MB of output). Events that overflow because the host calls `pump` rarely are added to `dropped=` of the plugins that wanted that kind.

**Commands and deadlines**

- A caller of a plugin command that gets no reply within the deadline receives a `plugin timed out` error.
- The socket server waits for plugin command replies asynchronously. Only that connection waits; other clients are served as usual. The next command on the same connection is processed after it, to preserve order.
- A host that embeds the engine can call in two ways. `command` does not return until the reply or the deadline. `command_start` (C `zeropty_command_start`; Node `commandStart` always returns `Promise<Reply>`) returns immediately, and the reply is fetched later with `Call::poll`. Details are in [Calling plugin commands from the host](#calling-plugin-commands-from-the-host).
- The polled engine's `Polled::command` keeps doing the work of `process` while it waits (reading session output, clients of a `listen`ed socket, engine commands issued by plugins, the watchdog). In between, it sleeps on the engine's kqueue and is woken by the plugin's reply, session output, clients or engine timers. It never spins the CPU while waiting.
- `call` replies from socket plugins and `plugin-run` libraries are awaited up to `reply_wait(deadline)` (7/8 of the deadline, 8.75 seconds by default). Past that, only that call fails with `plugin timed out (no reply within N ms; a later reply is ignored)`, and a `reply` that arrives late is discarded. The plugin stays `running`. Writing one line (event, call, reply) to a plugin is also given this same time, once in total. If the socket buffer is full and the write cannot complete within that time, the plugin becomes `failed (connection: not read for N ms)`. Since this happens before the deadline, the watchdog never cuts it off as `stuck in a hook`. (With a time limit on each `write`, macOS reports a `write` that sent even a few bytes as successful, so `write_all` waited out the limit several times and the watchdog cut in first.)
- `Host::command` waits up to half the deadline. Past that, it gets `command: no answer within N ms (half the plugin deadline)`. The remaining half is used to answer whoever called this plugin, with the reason.
- A command the caller gave up on is never run later. A command not yet started is discarded, and `not run` is appended to the error (`busy; not run` if it was only waiting in the plugin's queue). If it had already started, `started, may still take effect` is appended (`plugin timed out` for plugin commands). Starting and giving up are decided exactly once through a single atomic variable, so a command reported as failed never runs afterwards. The same applies to a `Call` dropped before its reply (C `zeropty_call_free`).
- Calls that would form a cycle are rejected immediately. If B calls one of A's commands while A is waiting on one of B's commands (with `call` access), it gets `command: call cycle (b -> a -> b): plugin a is waiting for b`. Longer cycles are handled the same way. Both plugins stay `running`.

**Failure isolation**

- If a hook panics, only that plugin becomes `failed (panicked in hook: message)`, and the caller gets an error. From the moment it fails, no more commands are routed to that plugin, and the same name can be registered again.
- If a single hook runs longer than the deadline on its own time, the plugin becomes `failed (stuck in a hook for more than N ms)` and receives no more events or commands. A Rust thread cannot be terminated from outside, so that thread ends when it returns from the hook.
- A hook that never returns holds its thread forever. This is unavoidable (Rust has no way to kill a thread from outside). The engine is still unaffected. The watchdog marks the plugin `failed` and closes its queue, so no more events or commands go to it, and the engine never waits on plugin queues or threads in the first place (it only does `try_send`). Callers of that plugin's commands get an error at the deadline, and `shutdown` detaches the thread and returns once its wait (1 second by default) is over. What remains is one stuck thread and whatever memory the plugin holds, which go away when the process exits. If the plugin needs to restart, it can be registered again under the same name. If this worries you for a given plugin, run it out of process (as a socket plugin or with `plugin-run`), where the OS can terminate a stuck process.
- `manifest()` also runs on the plugin thread. `add` starts the plugin thread first, calls `manifest()` there, and waits for the answer only until the manifest deadline (the shorter of the registry deadline and 1 second). Past that, it rejects with `ManifestTimeout` and leaves the name free. When `manifest()` returns, that thread drops the plugin and exits (if it never returns, one thread is left behind, like the stuck hook above). The registry lock is taken only after `manifest()` has answered, so other plugin registrations, commands and the engine are not blocked while waiting.
- Only the hook's own time counts. Time spent inside a hook waiting on the engine or another plugin through `Host::command` is excluded. The slow party is judged by its own hook time.
- The watchdog runs in the host loop's `pump`. While there is at least one plugin, the host loop never sleeps longer than a quarter of the deadline (10ms\~1 second) (`HostLoop::timeout_ms` takes `Plugins::watchdog_ms` into account; for the polled engine this is in `timeout_ms()`, and for the server and `Embedded` it is in the loop's sleep time). So even on an idle engine, a stuck hook is cut off within 1.25 times the deadline. With no plugins, this cap does not exist either.
- When an out-of-process plugin dies, the server just sees a closed connection. That plugin is removed from the registry.

**Shutdown**

- `Plugins::shutdown` (and dropping `Embedded` or `Polled`) sends `stop` to each plugin and waits for the threads to finish for up to `set_shutdown_wait` (1 second by default; 0 means no waiting). Threads still stuck in a hook at that point are detached, and it returns. Such a thread exits without `stop` once it returns from the hook.
- Out-of-process plugins get EOF when the server exits.

## Cost with no plugins

- With no plugins, the engine's output read path adds only a single branch that reads one integer (the same goes for session creation, exit and rename). Output bytes are copied only when some plugin wants `output`.
- One turn of the host loop: the registry's `after_tick` (= `pump`) reads three atomic variables and returns, and the sleep-time calculation (`watchdog_ms`) reads one atomic variable. The plugin wakeup event is checked only when there are host events.
- One command: `Embedded::command`, `Polled::command` and the server all first check whether it is a plugin command with a single atomic variable (whether any plugin has added commands). If none has, not even a name comparison happens. The `list-plugins` name comparison happens only for commands the engine does not know (for `Embedded`, on the slow path that hands off to the engine thread; for `Polled`, after the engine has answered with a failure).
- The server loop scans the client list only when a plugin command is awaiting a reply. The polled engine's `timeout_ms()` checks one empty list when no calls were started with `command_start`.
- The registry owns no fd and no thread until a plugin attaches (`Embedded` and `Polled` `attach` to the engine queue on the first `add_plugin` or `listen`).

A table measured alternately against a no-plugin `main` on the same machine (one polled turn, commands, capture-pane, send-keys), and how to measure it again, are in [PLUGINS-INTERNALS.md](PLUGINS-INTERNALS.md#cost-with-no-plugins). Every difference is within the noise of re-running the same build.
