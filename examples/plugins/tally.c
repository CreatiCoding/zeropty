/* A zeropty plugin library: built with zeropty.h alone, loaded through zeropty_plugin_init.
 * It counts sessions made and ended, and adds the command `tally`, which also asks zeropty for the
 * sessions running now. The same .dylib runs out of process (attached to the server) or in a host's process.
 *
 *   cc -dynamiclib -O2 -Ibindings/c/include -o /tmp/tally.dylib examples/plugins/tally.c
 *   zeropty plugin-run /tmp/tally.dylib &             # a process of its own, attached to the server
 *   zeropty tally                                      # -> created 0, ended 0, live 3
 *   cp /tmp/tally.dylib ~/.zeropty/plugins/            # or: started with every server, restarted if it dies
 *   zeropty_load_plugin(a, "/tmp/tally.dylib");        # or: inside a program that embeds the engine
 */
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
