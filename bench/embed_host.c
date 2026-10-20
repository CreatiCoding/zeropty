/* What the embedded engine costs its host, measured in a host that does nothing else (Node's own heap
 * moves by megabytes on its own): the process's footprint before the engine, with it, and with N sessions.
 * build: clang -O2 -o /tmp/embed_host bench/embed_host.c -Ldist -lzeropty_c -Wl,-rpath,$PWD/dist
 * usage: embed_host <N> <idle|trace> <repo root> [polled|thread] [socket]
 *   polled: the engine has no thread; this program's loop runs it (zeropty_open_polled).
 *   socket: it also listens there (zeropty_listen), as the zeropty server would. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <poll.h>
#include <time.h>
#include "../bindings/c/include/zeropty.h"

static long footprint_kb(void) {
  char cmd[64]; snprintf(cmd, sizeof cmd, "footprint %d", getpid());
  FILE *p = popen(cmd, "r"); char line[512]; double v = 0; char unit[8] = "";
  while (p && fgets(line, sizeof line, p))
    if (strstr(line, "phys_footprint:") && sscanf(strstr(line, ":") + 1, "%lf %7s", &v, unit) == 2) break;
  if (p) pclose(p);
  return (long)(strcmp(unit, "MB") == 0 ? v * 1024 : strcmp(unit, "GB") == 0 ? v * 1048576 : v);
}

static zeropty *polled;

/* Sleeps `s` seconds; with a polled engine, runs it meanwhile. */
static void wait_s(int s) {
  if (!polled) { sleep(s); return; }
  struct timespec now; clock_gettime(CLOCK_MONOTONIC, &now);
  double end = now.tv_sec + now.tv_nsec / 1e9 + s;
  for (;;) {
    clock_gettime(CLOCK_MONOTONIC, &now);
    double left = end - (now.tv_sec + now.tv_nsec / 1e9);
    if (left <= 0) return;
    long long t = zeropty_timeout_ms(polled); int ms = (int)(left * 1000) + 1;
    struct pollfd p = { zeropty_fd(polled), POLLIN, 0 };
    poll(&p, 1, t < ms ? (int)t : ms);
    zeropty_process(polled);
  }
}

static long avg_kb(void) { long s = 0; for (int i = 0; i < 10; i++) { wait_s(2); s += footprint_kb(); } return s / 10; }

static void run(zeropty *a, const char *json) { char *r = zeropty_command(a, json); zeropty_free(r); }

int main(int argc, char **argv) {
  int n = atoi(argv[1]); const char *mode = argv[2], *root = argv[3];
  long none = avg_kb();
  int use_polled = argc > 4 && strcmp(argv[4], "polled") == 0;
  zeropty *a = use_polled ? zeropty_open_polled("{\"history\": 2000}") : zeropty_open("{\"history\": 2000}");
  if (use_polled) polled = a;
  const char *sock = argc > 5 ? argv[5] : NULL;
  if (sock && zeropty_listen(a, sock) != 0) { fprintf(stderr, "cannot listen on %s\n", sock); return 1; }
  long engine = avg_kb();
  static char json[4096];
  for (int i = 1; i <= n; i++) {
    char cmd[1024];
    if (strcmp(mode, "trace") == 0) { double x = i * 0.6180339887; x -= (long)x; snprintf(cmd, sizeof cmd, "exec perl %s/tests/replay_trace.pl %s/tests/streams/claude2.trace %.6f", root, root, x); }
    else snprintf(cmd, sizeof cmd, "cat %s/tests/streams/claude1.raw; exec sleep 100000", root);
    snprintf(json, sizeof json, "{\"argv\":[\"new-session\",\"-d\",\"-s\",\"s%d\",\"-x\",\"220\",\"-y\",\"80\",\"%s\"],\"cwd\":\"/\"}", i, cmd);
    run(a, json);
  }
  wait_s(15);
  long full = avg_kb();
  printf("embedded(C host%s%s) N=%-5d %-6s host alone %ld KB, engine started %+ld KB, %d sessions %+ld KB more -> zeropty adds %ld KB\n",
         use_polled ? ", polled" : "", sock ? ", listening" : "", n, mode, none, engine - none, n, full - engine, full - none);
  run(a, "{\"argv\":[\"kill-server\"],\"cwd\":\"/\"}");
  zeropty_close(a);
  return 0;
}
