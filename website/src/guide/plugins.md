---
description: Build a zeropty plugin library in C, attach it to the server, start it with every server, and call its command from the CLI or an embedding host.
---

# Write a plugin

By the end of this guide, you build a small **plugin** as a C library, attach it to the zeropty server, call the command it adds from the CLI and keep it running with every server. The plugin is `tally`: it counts sessions created and ended, and reports how many are live.

## Before you begin

- A Mac with zeropty on your `PATH`.
- The zeropty repository, for `bindings/c/include/zeropty.h` and `examples/plugins/tally.c`. Run the commands from that folder. A plugin library needs only the header; it does not link against `libzeropty_c`.
- `cc` from the Xcode Command Line Tools.

## 1. Choose how to attach

A plugin listens to session events and adds commands. Its commands are called like built-in ones, from the CLI, the socket or an embedded call. There are four ways to attach one, all sharing the same registry, events, access levels and isolation rules:

| Method | Language | Where it runs | Example |
|---|---|---|---|
| Socket (out of process) | Any language (socket and JSON) | A separate process | `examples/plugins/hello.py`, `examples/plugins/notify.mjs` |
| Plugin library (`.dylib`) | C, or anything that exports C functions | A separate process via `zeropty plugin-run`, or inside a host | `examples/plugins/tally.c` |
| C ABI (application binary interface) callback | C, or anything that passes C functions | Host process, on a plugin thread | `examples/plugins/count.c` |
| Rust trait | Rust | Host process, on a plugin thread | `examples/plugin_activity.rs` |

This guide builds a library, because the same `.dylib` runs both beside the server and inside an embedding host. Plugin code never runs on the engine thread, so a slow or crashing plugin does not stop your sessions.

## 2. Write the plugin

A library has one entry point, `zeropty_plugin_init`. It fills in a manifest (name, events, commands) and a callback. The callback gets one JSON message at a time, on the plugin's own thread:

```c [examples/plugins/tally.c]
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "zeropty.h"

static const zeropty_plugin_api *zp; /* from the host: host_command and free */
static atomic_int created, ended;

/* One JSON message at a time, on the plugin's own thread (a real plugin would use a JSON parser). */
static char *on_message(void *user, const char *msg, zeropty_host *host) {
    (void)user;
    if (strstr(msg, "{\"event\":\"session-created\"") == msg) created++;
    if (strstr(msg, "{\"event\":\"session-exited\"") == msg) ended++;
    if (strncmp(msg, "{\"call\"", 7) != 0) return NULL; /* start, events, stop: nothing to answer */
    /* Asks zeropty, from inside the plugin, for the sessions running now: one "x" per session. */
    char *r = zp->host_command(host, "{\"argv\":[\"list-sessions\",\"-F\",\"x\"],\"cwd\":\"/\"}");
    int live = 0;
    const char *out = r ? strstr(r, "\"out\":\"") : NULL;
    for (const char *p = out ? out + 7 : ""; *p && *p != '"'; p++) live += *p == 'x';
    zp->free(r);
    char *reply = malloc(128); /* zeropty frees it */
    snprintf(reply, 128, "{\"code\":0,\"out\":\"created %d, ended %d, live %d\\n\",\"err\":\"\"}", created, ended, live);
    return reply;
}

int zeropty_plugin_init(const zeropty_plugin_api *api, zeropty_plugin_def *def) {
    if (api->api < 1) return 1;
    zp = api;
    def->manifest_json = "{\"name\":\"tally\",\"events\":[\"session-created\",\"session-exited\"],\"commands\":[\"tally\"]}";
    def->callback = on_message;
    def->user = NULL;
    return 0;
}
```

The callback receives `{"start":{"api":1}}` first, then events and calls, and `{"stop":true}` last. For a `call`, return a `malloc`'d reply with `code`, `out` and `err`; zeropty frees it. For anything else, return `NULL`.

`host_command` runs a zeropty command from inside the plugin, within the plugin's **access**. The manifest above declares none, so it gets the default, `read`: `list-sessions` and `capture-pane` work, `send-keys` does not. Add `"access":["input"]` to the manifest to send keys.

## 3. Build it

Build the library with the header alone:

```sh [Terminal]
cc -dynamiclib -O2 -Ibindings/c/include -o /tmp/tally.dylib examples/plugins/tally.c
```

## 4. Attach it to the server

Start a server if none is running, run the library with `plugin-run`, then call its command:

```sh [Terminal]
zeropty new-session -d -s a sh            # starts the server if none is running
zeropty plugin-run /tmp/tally.dylib &
zeropty tally
```

`plugin-run` loads the library into a process of its own and attaches it to the server over the socket. If the library crashes, only that process dies. When the server exits, `plugin-run` exits too.

## 5. Start it with every server

Put the library in a plugin directory and point `ZEROPTY_PLUGIN_DIR` at it. The server starts the plugins in it and restarts them if they exit:

```sh [Terminal]
export ZEROPTY_PLUGIN_DIR=~/.zeropty/plugins               # put this in ~/.zshrc to keep it always on
mkdir -p ~/.zeropty/plugins
cp /tmp/tally.dylib ~/.zeropty/plugins/                  # a .dylib is started with zeropty plugin-run
zeropty restart-server                                     # takes effect from the next server
```

::: warning Off by default
Without `ZEROPTY_PLUGIN_DIR`, the server runs nothing, even if `~/.zeropty/plugins` exists. Setting it is your consent to run the code in that directory as your user. Directories and files that other users can write to are skipped.
:::

- The server takes `ZEROPTY_PLUGIN_DIR` from the shell that starts it: the `new-session` that starts the server, or the `restart-server`.
- Executables and `.dylib` files start in name order. Each plugin's output goes to `NAME.log` in the directory.
- A plugin that exits quickly is restarted after 0.5 seconds, doubling up to 30 seconds.
- Plugins survive `restart-server`: the old server's `plugin-run` exits with it, and the new server starts its own after it takes over the socket.

## 6. Call it from a program

Plugin commands are called like any command. From a host that embeds the engine, you can choose whether the calling thread waits:

| Host | Waits for the reply | Returns at once |
|---|---|---|
| C | `zeropty_command` | `zeropty_command_start` → `zeropty_call_poll` → `zeropty_call_free` |
| Rust `Embedded`, `Polled` | `command(argv, cwd)` | `command_start(argv, cwd) -> Call` |
| Node (`client/embedded.mjs`) | `zeroptySync(argv)` | `await zeropty(argv)` |

An embedding host can also load the same library into its own process with `zeropty_load_plugin(a, "/tmp/tally.dylib")`. Then a crash in the library is a crash of that process.

## Verify

Start two sessions and end one, then call the command and list the plugins:

```sh [Terminal]
zeropty new-session -d -s b sh
zeropty new-session -d -s c sh
zeropty kill-session -t '=c:'
zeropty tally
zeropty list-plugins
```

You should see the counts since the plugin started. `live` counts every running session, so with only `a` and `b` left it reads:

```text
created 2, ended 1, live 2
```

`list-plugins` prints one line per plugin, in the form `name: state (api N) events=... commands=... access=... dropped=N`. Look for `tally: running`.

## Troubleshooting

| Symptom | Fix |
|---|---|
| `unknown command` for `tally` | The plugin is not attached. Check `zeropty list-plugins`. Right after `restart-server`, plugins reattach within tens of ms. |
| `failed (stuck in a hook ...)` | A callback ran longer than the deadline (10 seconds, `ZEROPTY_PLUGIN_TIMEOUT_MS`). Reply "started" first and let the result be fetched with another command. |
| `plugin tally lacks access input` | Add the access to the manifest, for example `"access":["input"]`. |
| `bad manifest: ...` | The JSON manifest allows only `name`, `events`, `commands`, `access` and `api`. Check for typos such as `"event"`. |
| `dropped=` keeps growing | The plugin reads events too slowly; its queue holds 1024 events or 8MB of output. Prefer `activity` over `output`. |
| The plugin directory does nothing | `ZEROPTY_PLUGIN_DIR` was not set in the shell that started the server. Set it and run `zeropty restart-server`. |

## Next steps

- [Plugin API](/reference/plugin-api#events) for every event, and [access levels](/reference/plugin-api#access).
- [Socket protocol for plugins](/reference/plugin-api#1-from-another-process-the-socket-protocol-any-language) to write a plugin in Python or Node.js.
- [Embed zeropty in C or Rust](/guide/embed) to host plugins in your own process.
