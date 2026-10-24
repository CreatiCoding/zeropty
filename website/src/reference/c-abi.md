---
description: Every function, type and macro of zeropty's C ABI (libzeropty_c.dylib and zeropty.h), with arguments, return values and who frees what.
---

# C ABI

Every function, type and macro in `zeropty.h`, the header of `libzeropty_c.dylib`. For building, linking and a first program, see [Embed zeropty in C or Rust](/guide/embed).

The build checks this list: `api/c-abi.txt` is compared with the library and the header, and the site's build fails if a symbol in it is missing here. The source is [`bindings/c/include/zeropty.h`](https://github.com/CreatiCoding/zeropty/blob/main/bindings/c/include/zeropty.h).

## Rules for every call

| Rule | Detail |
|---|---|
| Requests and replies are JSON | A request is `{"argv": [...], "cwd": "/"}`, the same argv as the CLI. A reply is `{"code": 0, "out": "...", "err": ""}`. |
| Who frees a reply | Every `char *` the library returns is freed exactly once with `zeropty_free`, never with `free()`. `zeropty_version` is the exception: a static string. |
| Threads | With `zeropty_open`, commands may come from any thread and each runs on the caller's thread. With `zeropty_open_polled`, every call comes from the one thread that runs the loop. |
| After close | The handle is invalid after `zeropty_close`: no call on it may run during or after it. |

## Version

### `ZEROPTY_VERSION_*`

```c
#define ZEROPTY_VERSION_MAJOR 0
#define ZEROPTY_VERSION_MINOR 1
#define ZEROPTY_VERSION_PATCH 5
#define ZEROPTY_VERSION_PRERELEASE ""
#define ZEROPTY_VERSION_STRING "0.1.5"
#define ZEROPTY_VERSION_NUMBER (ZEROPTY_VERSION_MAJOR * 1000000 + ZEROPTY_VERSION_MINOR * 1000 + ZEROPTY_VERSION_PATCH)
#define ZEROPTY_VERSION_IS_PRERELEASE 0
```

The version this header belongs to, following [Versioning](/guide/versioning). The values above are those of the current release.

| Macro | Meaning |
|---|---|
| `ZEROPTY_VERSION_MAJOR`, `ZEROPTY_VERSION_MINOR`, `ZEROPTY_VERSION_PATCH` | The three numbers. |
| `ZEROPTY_VERSION_PRERELEASE` | The pre-release part (`"rc.1"` in 0.2.0-rc.1), `""` for a release. |
| `ZEROPTY_VERSION_STRING` | The whole version as a string. |
| `ZEROPTY_VERSION_NUMBER` | One number for `#if`: MAJOR × 1000000 + MINOR × 1000 + PATCH. It leaves the pre-release out, so 0.2.0-rc.1 and 0.2.0 have the same number. |
| `ZEROPTY_VERSION_IS_PRERELEASE` | `1` for a pre-release, `0` for a release. `#if` can test it. |

Compare the header with the library at run time: a library of another MAJOR (another MINOR before 1.0) may not match these declarations.

### `zeropty_version`

```c
const char *zeropty_version(void);
```

The linked library's version, in the same form as `ZEROPTY_VERSION_STRING`. A static string: never free it.

## Engine with its own threads

### `zeropty`

```c
typedef struct zeropty zeropty;
```

An engine handle. Opaque: use it only through the functions on this page.

### `zeropty_open`

```c
zeropty *zeropty_open(const char *options_json);
```

Starts an engine in this process.

| Argument | Meaning |
|---|---|
| `options_json` | `NULL`, or `{"history": ROWS, "size_dir": "DIR"}`: rows of scrollback per session, and where resized sessions' window sizes are written for their programs. |

Returns the handle, or `NULL` on failure. Close it with `zeropty_close`.

```c
zeropty *zp = zeropty_open("{\"history\": 2000}");
if (!zp) return 1;
```

### `zeropty_command`

```c
char *zeropty_command(zeropty *a, const char *request_json);
```

Runs one command, as the CLI would.

| Argument | Meaning |
|---|---|
| `a` | The engine. |
| `request_json` | `{"argv": ["capture-pane", "-p", "-t", "=x:"], "cwd": "/"}`. |

Returns the reply `{"code", "out", "err"}`, never `NULL` for a valid engine. Free it with `zeropty_free`. A plugin's command waits for the plugin: to wait without holding the thread, use [`zeropty_command_start`](#zeropty_command_start).

### `zeropty_free`

```c
void zeropty_free(char *reply);
```

Frees a reply of this library: each exactly once, never with `free()`. `NULL` is a no-op.

### `zeropty_listen`

```c
int zeropty_listen(zeropty *a, const char *path);
```

Answers commands on a Unix socket at `path` too, as the zeropty server does. `ZEROPTY_SOCK=PATH zeropty send-keys ...`, `attach` and socket clients then reach this engine's sessions with no server process.

Returns `0`, or `-1` if it could not listen there (a server already answers on it, the path is too long, ...). A polled engine serves the socket in `zeropty_process`.

### `zeropty_close`

```c
void zeropty_close(zeropty *a);
```

Stops the engine. Sessions keep running until their programs end: send `kill-server` first to end them. The handle is invalid afterwards, and no call on it may run during or after `zeropty_close`.

## Engine run by your event loop

These four functions give an engine with no thread of its own: your loop runs it. Every call comes from that one thread.

```c
zeropty *a = zeropty_open_polled(NULL);
for (;;) { poll({zeropty_fd(a), POLLIN}, zeropty_timeout_ms(a)); zeropty_process(a); }
```

### `zeropty_open_polled`

```c
zeropty *zeropty_open_polled(const char *options_json);
```

Like [`zeropty_open`](#zeropty_open), with the same `options_json`, but the engine has no thread. The other calls on the handle (`zeropty_command`, `zeropty_listen`, `zeropty_close`, ...) work as before, from the loop's thread.

### `zeropty_fd`

```c
int zeropty_fd(zeropty *a);
```

A file descriptor to wait on for reading. When it is readable, call `zeropty_process`. `-1` for an engine with its own thread.

### `zeropty_timeout_ms`

```c
long long zeropty_timeout_ms(zeropty *a);
```

Milliseconds until `zeropty_process` is due even with `zeropty_fd` not readable.

### `zeropty_process`

```c
void zeropty_process(zeropty *a);
```

Handles what is waiting, without blocking. Call it when `zeropty_fd` is readable or `zeropty_timeout_ms` milliseconds have passed.

## Commands that do not wait for a plugin

A plugin's command can take time. These calls start a command and let your loop go on until the reply comes. They work with either kind of engine.

```c
zeropty_call *c = zeropty_command_start(a, "{\"argv\":[\"my-plugin-cmd\"],\"cwd\":\"/\"}");
for (;;) { poll({zeropty_fd(a), POLLIN}, zeropty_timeout_ms(a)); zeropty_process(a);
           char *r = zeropty_call_poll(c); if (r) { ...; zeropty_free(r); break; } }
zeropty_call_free(c);
```

The plugin's reply makes `zeropty_fd` readable, and `zeropty_timeout_ms` counts the call's deadline, so the loop never spins.

### `zeropty_call`

```c
typedef struct zeropty_call zeropty_call;
```

A command in flight. Opaque.

### `zeropty_command_start`

```c
zeropty_call *zeropty_command_start(zeropty *a, const char *request_json);
```

Starts a command with the same `request_json` as [`zeropty_command`](#zeropty_command). A plugin's command comes back in flight; any other command (and a bad request) is answered already. Free the call with `zeropty_call_free`.

### `zeropty_call_poll`

```c
char *zeropty_call_poll(zeropty_call *c);
```

`NULL` while the call is pending. Otherwise the reply JSON, once: the plugin's reply, or the failure once the plugin stopped or the deadline passed. Free it with `zeropty_free`.

### `zeropty_call_due_ms`

```c
long long zeropty_call_due_ms(zeropty_call *c);
```

Milliseconds until the call's deadline, when `zeropty_call_poll` gives up on the plugin. `-1` for `NULL`.

### `zeropty_call_free`

```c
void zeropty_call_free(zeropty_call *c);
```

Frees the call. Freed before its reply, a call the plugin did not start on is never run.

## Plugins in this process

A C function can hear this engine's sessions and answer commands it adds. For writing one, see [Write a plugin](/guide/plugins). For the messages and access rules, see [Plugin API](/reference/plugin-api).

### `zeropty_host`

```c
typedef struct zeropty_host zeropty_host;
```

What the plugin's callback uses to run commands. Valid only during the call it was passed to.

### `zeropty_plugin_fn`

```c
typedef char *(*zeropty_plugin_fn)(void *user, const char *message_json, zeropty_host *host);
```

The plugin's callback. It runs on the plugin's own thread, never the engine's, with one JSON message each time:

| Message | When |
|---|---|
| `{"start":{"api":1}}` | First. |
| `{"event":"session-exited","session":"w","reason":"killed"}` and other events | For the events the manifest asks for. |
| `{"call":1,"argv":["hello","x"],"cwd":"/"}` | A call of one of its commands. |
| `{"stop":true}` | Last. |

For a call it returns the reply `{"code":0,"out":"...","err":""}` in memory from `malloc` (zeropty frees it), and `NULL` otherwise. A callback that hangs past the deadline (10 s) is cut off, and one that is slow loses events: the engine is never held up by it.

### `zeropty_add_plugin`

```c
char *zeropty_add_plugin(zeropty *a, const char *manifest_json, zeropty_plugin_fn callback, void *user);
```

Adds a plugin to this engine.

| Argument | Meaning |
|---|---|
| `manifest_json` | `{"name":"hello","events":["session-created","output"],"commands":["hello"],"access":["input"],"api":1}`. `events` may be a list or a comma-separated string (`"session-created,output"`). `access` is what `zeropty_host_command` may run: `"read"` by default, `"input"`, `"manage"`, `"call"` or `"all"`. It may be left out. |
| `callback` | The [`zeropty_plugin_fn`](#zeropty_plugin_fn). |
| `user` | Passed back to every call of `callback`. |

Returns `NULL` when added; otherwise why not, freed with `zeropty_free`.

### `zeropty_host_command`

```c
char *zeropty_host_command(zeropty_host *host, const char *request_json);
```

Runs a command from inside the callback, with the same `request_json` as [`zeropty_command`](#zeropty_command). Safe with either kind of engine: it goes through the engine's loop. Returns the reply JSON, freed with `zeropty_free`.

## Plugin libraries

A plugin library is a `.dylib` built with this header alone (no `-lzeropty_c`), with one entry point:

```c
int zeropty_plugin_init(const zeropty_plugin_api *api, zeropty_plugin_def *def);
```

It fills `def` and returns `0` (anything else: not loaded), and keeps `api`. The same library runs in a process of its own attached to the server (`zeropty plugin-run plugin.dylib`, or from the plugin directory of a server started with `ZEROPTY_PLUGIN_DIR`), or in this process through [`zeropty_load_plugin`](#zeropty_load_plugin).

### `ZEROPTY_PLUGIN_API`

```c
#define ZEROPTY_PLUGIN_API 1
```

The plugin API version this header describes.

### `zeropty_plugin_api`

```c
typedef struct zeropty_plugin_api {
    unsigned int api;
    unsigned int size;
    char *(*host_command)(zeropty_host *host, const char *request_json);
    void (*free)(char *s);
} zeropty_plugin_api;
```

| Field | Meaning |
|---|---|
| `api` | The host's plugin API version. |
| `size` | `sizeof` what the host passes. Later versions only add fields at the end. |
| `host_command` | [`zeropty_host_command`](#zeropty_host_command) for this plugin. |
| `free` | Frees what `host_command` returns. |

### `zeropty_plugin_def`

```c
typedef struct zeropty_plugin_def {
    const char *manifest_json;
    zeropty_plugin_fn callback;
    void *user;
} zeropty_plugin_def;
```

| Field | Meaning |
|---|---|
| `manifest_json` | As for [`zeropty_add_plugin`](#zeropty_add_plugin). Valid while the library is loaded. |
| `callback` | The plugin's [`zeropty_plugin_fn`](#zeropty_plugin_fn). |
| `user` | Passed back to every call of `callback`. |

### `zeropty_plugin_init_fn`

```c
typedef int (*zeropty_plugin_init_fn)(const zeropty_plugin_api *api, zeropty_plugin_def *def);
```

The type of the entry point `zeropty_plugin_init`.

### `zeropty_load_plugin`

```c
char *zeropty_load_plugin(zeropty *a, const char *path);
```

Loads a plugin library into this engine's process. A crash in the library is this process's crash. Returns `NULL` when added; otherwise why not, freed with `zeropty_free`.
