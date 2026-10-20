/* zeropty embedded in a process: tmux's session commands without PTYs. Link with -lzeropty_c.
 *
 *   zeropty *a = zeropty_open(NULL);
 *   char *r = zeropty_command(a, "{\"argv\":[\"new-session\",\"-d\",\"-s\",\"x\",\"exec cat\"],\"cwd\":\"/\"}");
 *   ... r is {"code":0,"out":"","err":""} ...
 *   zeropty_free(r);
 *   zeropty_close(a);
 *
 * Commands may come from any thread; each runs on the caller's thread. */
#ifndef ZEROPTY_H
#define ZEROPTY_H
#ifdef __cplusplus
extern "C" {
#endif

/* The version this header belongs to (Semantic Versioning, docs/VERSIONING.md), set by scripts/release.mjs.
 * Compare it at run time with zeropty_version(): a library of another MAJOR (another MINOR before 1.0) may
 * not match these declarations. */
#define ZEROPTY_VERSION_MAJOR 0
#define ZEROPTY_VERSION_MINOR 1
#define ZEROPTY_VERSION_PATCH 2
/* The pre-release part ("rc.1" in 0.2.0-rc.1), "" for a release. */
#define ZEROPTY_VERSION_PRERELEASE ""
#define ZEROPTY_VERSION_STRING "0.1.2"
/* One number to compare with #if: MAJOR * 1000000 + MINOR * 1000 + PATCH. It leaves the pre-release out, so
 * 0.2.0-rc.1 and 0.2.0 have the same number: tell them apart with ZEROPTY_VERSION_PRERELEASE (or
 * ZEROPTY_VERSION_IS_PRERELEASE, which #if can test). */
#define ZEROPTY_VERSION_NUMBER (ZEROPTY_VERSION_MAJOR * 1000000 + ZEROPTY_VERSION_MINOR * 1000 + ZEROPTY_VERSION_PATCH)
/* 1 for a pre-release (ZEROPTY_VERSION_PRERELEASE is not ""), 0 for a release. */
#define ZEROPTY_VERSION_IS_PRERELEASE 0

typedef struct zeropty zeropty;

/* The linked library's version, as ZEROPTY_VERSION_STRING: a static string, never freed. */
const char *zeropty_version(void);

/* options_json: NULL, or {"history": rows of scrollback, "size_dir": "where resized sessions' sizes go"}. */
zeropty *zeropty_open(const char *options_json);
/* request_json: {"argv": ["capture-pane", "-p", "-t", "=x:"], "cwd": "/"}. Returns {"code","out","err"}. */
char *zeropty_command(zeropty *a, const char *request_json);
/* Frees a reply of zeropty_command: each exactly once, never with free(). NULL is a no-op. */
void zeropty_free(char *reply);
/* Answers commands on a Unix socket at path too, as the zeropty server does: `ZEROPTY_SOCK=path zeropty send-keys ...`,
 * attach and socket clients reach this engine's sessions with no server process. 0, or -1 on failure. */
int zeropty_listen(zeropty *a, const char *path);
/* Stops the engine; sessions keep running until their programs end (send "kill-server" first to end them).
 * The handle is invalid afterwards: no call on it may run during or after zeropty_close. */
void zeropty_close(zeropty *a);

/* No thread of its own: the host runs the engine from its event loop. Call zeropty_process when
 * zeropty_fd(a) is readable or zeropty_timeout_ms(a) milliseconds passed. Every call from that one thread.
 *   zeropty *a = zeropty_open_polled(NULL);
 *   for (;;) { poll({zeropty_fd(a), POLLIN}, zeropty_timeout_ms(a)); zeropty_process(a); } */
zeropty *zeropty_open_polled(const char *options_json);
int zeropty_fd(zeropty *a);
long long zeropty_timeout_ms(zeropty *a);
void zeropty_process(zeropty *a);

/* zeropty_command without waiting for a plugin: a plugin's command comes back in flight, anything else
 * answered already. The host's thread is never held while a plugin works:
 *   zeropty_call *c = zeropty_command_start(a, "{\"argv\":[\"my-plugin-cmd\"],\"cwd\":\"/\"}");
 *   for (;;) { poll({zeropty_fd(a), POLLIN}, zeropty_timeout_ms(a)); zeropty_process(a);
 *              char *r = zeropty_call_poll(c); if (r) { ...; zeropty_free(r); break; } }
 *   zeropty_call_free(c);
 * The plugin's reply turns zeropty_fd readable and zeropty_timeout_ms counts the call's deadline, so the
 * loop never spins. zeropty_call_poll: NULL while pending, else the reply JSON once (free with zeropty_free).
 * Freed before its reply, a call the plugin did not start on is never run. Either kind of engine. */
typedef struct zeropty_call zeropty_call;
zeropty_call *zeropty_command_start(zeropty *a, const char *request_json);
char *zeropty_call_poll(zeropty_call *c);
long long zeropty_call_due_ms(zeropty_call *c);
void zeropty_call_free(zeropty_call *c);

/* Plugins (docs/PLUGINS.md): a C function hears this engine's sessions and answers commands it adds.
 * It is called on the plugin's own thread, never the engine's, with one JSON message each time:
 *   {"start":{"api":1}}, then events such as {"event":"session-exited","session":"w","reason":"killed"},
 *   calls of its commands {"call":1,"argv":["hello","x"],"cwd":"/"}, and last {"stop":true}.
 * For a call it returns the reply {"code":0,"out":"...","err":""} in memory from malloc (zeropty frees
 * it), NULL otherwise. `host` runs commands from inside the callback (zeropty_host_command); it is valid
 * only during the call. A callback that hangs past the deadline (10 s) is cut off, one that is slow
 * loses events: the engine is never held up by it. */
typedef struct zeropty_host zeropty_host;
typedef char *(*zeropty_plugin_fn)(void *user, const char *message_json, zeropty_host *host);
/* manifest_json: {"name":"hello","events":["session-created","output"],"commands":["hello"],"access":["input"],"api":1}.
 * "access" (what zeropty_host_command may run: "read" by default, "input", "manage", "call", "all") may be left out.
 * NULL when added; otherwise why not (free with zeropty_free). */
char *zeropty_add_plugin(zeropty *a, const char *manifest_json, zeropty_plugin_fn callback, void *user);
/* request_json as for zeropty_command; returns the reply JSON (free with zeropty_free). */
char *zeropty_host_command(zeropty_host *host, const char *request_json);

/* Plugin libraries: a .dylib built with this header alone (no -lzeropty_c), with one entry point,
 *   int zeropty_plugin_init(const zeropty_plugin_api *api, zeropty_plugin_def *def);
 * which fills def and returns 0 (anything else: not loaded), and keeps api: api->host_command is
 * zeropty_host_command for it, and api->free frees what that returns. The same library runs
 *   - in a process of its own, attached to the server: `zeropty plugin-run plugin.dylib`; or put in the
 *     plugin directory, which only a server started with ZEROPTY_PLUGIN_DIR set has (none by default),
 *     and which starts it with the server and again if it crashes;
 *   - in this process: zeropty_load_plugin (a crash there is this process's). */
#define ZEROPTY_PLUGIN_API 1
typedef struct zeropty_plugin_api {
    unsigned int api;  /* the host's plugin API version */
    unsigned int size; /* sizeof what the host passes: later versions only add fields at the end */
    char *(*host_command)(zeropty_host *host, const char *request_json);
    void (*free)(char *s);
} zeropty_plugin_api;
typedef struct zeropty_plugin_def {
    const char *manifest_json; /* as for zeropty_add_plugin, valid while the library is loaded */
    zeropty_plugin_fn callback;
    void *user;
} zeropty_plugin_def;
typedef int (*zeropty_plugin_init_fn)(const zeropty_plugin_api *api, zeropty_plugin_def *def);
/* Loads a plugin library into this engine's process. NULL when added; otherwise why not (free with zeropty_free). */
char *zeropty_load_plugin(zeropty *a, const char *path);

#ifdef __cplusplus
}
#endif
#endif
