/* A zeropty plugin in C, in the host's own process (zeropty_add_plugin): it counts sessions made and
 * ended, and adds the command `count`.
 *
 *   ./build.sh && cc -O2 -o /tmp/count examples/plugins/count.c -Idist -Ldist -lzeropty_c -Wl,-rpath,"$PWD/dist"
 *   /tmp/count
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdatomic.h>
#include <unistd.h>
#include "zeropty.h"

static atomic_int created, ended;

/* Called on the plugin's own thread with one JSON message (a real plugin would use a JSON parser). */
static char *on_message(void *user, const char *msg, zeropty_host *host) {
    (void)user;
    if (strstr(msg, "\"event\":\"session-created\"")) created++;
    if (strstr(msg, "\"event\":\"session-exited\"")) ended++;
    if (strncmp(msg, "{\"call\"", 7) == 0) {
        /* Asks the engine, from inside the plugin, how many sessions there are now. */
        char *r = zeropty_host_command(host, "{\"argv\":[\"list-sessions\",\"-F\",\"x\"],\"cwd\":\"/\"}");
        int live = 0;
        for (const char *p = strstr(r, "\"out\":\""); p && *p; p++) live += *p == 'x';
        zeropty_free(r);
        char *reply = malloc(128);
        snprintf(reply, 128, "{\"code\":0,\"out\":\"created %d, ended %d, live %d\\n\",\"err\":\"\"}", created, ended, live);
        return reply; /* zeropty frees it */
    }
    return NULL;
}

int main(void) {
    zeropty *a = zeropty_open(NULL);
    char *err = zeropty_add_plugin(a, "{\"name\":\"count\",\"events\":[\"session-created\",\"session-exited\"],\"commands\":[\"count\"]}", on_message, NULL);
    if (err) { fprintf(stderr, "%s", err); zeropty_free(err); return 1; }
    zeropty_free(zeropty_command(a, "{\"argv\":[\"new-session\",\"-d\",\"-s\",\"one\",\"exec cat\"],\"cwd\":\"/\"}"));
    zeropty_free(zeropty_command(a, "{\"argv\":[\"new-session\",\"-d\",\"-s\",\"two\",\"true\"],\"cwd\":\"/\"}"));
    /* Events reach the plugin on its own thread, soon but not at once: wait (5 s at most) for `true` to end. */
    for (int i = 0; i < 500 && ended < 1; i++) usleep(10 * 1000);
    char *r = zeropty_command(a, "{\"argv\":[\"count\"],\"cwd\":\"/\"}");
    printf("%s\n", r); /* {"code":0,"out":"created 2, ended 1, live 1\n","err":""} */
    zeropty_free(r);
    zeropty_free(zeropty_command(a, "{\"argv\":[\"kill-server\"],\"cwd\":\"/\"}"));
    zeropty_close(a);
    return 0;
}
