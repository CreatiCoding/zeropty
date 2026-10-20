/* An embedded engine listening on a socket, for timing the zeropty CLI and socket clients against it
 * (bench/latency_c.c) next to the zeropty server: a polled engine run by this program's loop, with session
 * "z" replaying claude work. Runs until killed.
 * build: clang -O2 -o /tmp/listen_host bench/listen_host.c -Ldist -lzeropty_c -Wl,-rpath,$PWD/dist
 * usage: listen_host <socket> <repo root> [thread] */
#include <stdio.h>
#include <string.h>
#include <poll.h>
#include "../bindings/c/include/zeropty.h"

int main(int argc, char **argv) {
  int thread = argc > 3 && strcmp(argv[3], "thread") == 0;
  zeropty *a = thread ? zeropty_open("{\"history\": 2000}") : zeropty_open_polled("{\"history\": 2000}");
  if (zeropty_listen(a, argv[1]) != 0) { fprintf(stderr, "cannot listen on %s\n", argv[1]); return 1; }
  static char json[2048];
  snprintf(json, sizeof json, "{\"argv\":[\"new-session\",\"-d\",\"-s\",\"z\",\"-x\",\"220\",\"-y\",\"80\",\"exec perl %s/tests/replay_trace.pl %s/tests/streams/claude2.trace 0.3\"],\"cwd\":\"/\"}", argv[2], argv[2]);
  zeropty_free(zeropty_command(a, json));
  for (;;) {
    if (thread) { poll(NULL, 0, 1000); continue; }
    struct pollfd p = { zeropty_fd(a), POLLIN, 0 };
    poll(&p, 1, (int)zeropty_timeout_ms(a));
    zeropty_process(a);
  }
}
