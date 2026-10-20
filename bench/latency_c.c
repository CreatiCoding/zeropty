// From a C program: tmux -C against the zeropty socket, alternating, send-keys and capture-pane on a session
// replaying claude work (no Node in between). usage (after both servers have a session "z"):
//   clang -O2 -o /tmp/latency_c bench/latency_c.c && /tmp/latency_c <zeropty socket> <tmux -L name> 400 [gap us]
// gap: an idle pause before each request (0: back to back), as a program asking now and then does.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <spawn.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <mach/mach_time.h>
extern char **environ;
static double now_us(void) { static mach_timebase_info_data_t tb; if (!tb.denom) mach_timebase_info(&tb); return (double)mach_absolute_time() * tb.numer / tb.denom / 1000.0; }
static char buf[1 << 20];
static void zeropty_ask(int fd, const char *req) {
  write(fd, req, strlen(req)); size_t got = 0, body = 0; long c, o = -1, e = 0;
  for (;;) { ssize_t n = read(fd, buf + got, sizeof buf - got - 1); if (n <= 0) exit(1); got += n; buf[got] = 0;
    if (!body) { char *nl = memchr(buf, '\n', got); if (!nl) continue; sscanf(buf, "%ld %ld %ld", &c, &o, &e); body = nl - buf + 1; }
    if (got - body >= (size_t)(o + e)) return; }
}
// tmux -C: one command line in, "%begin ...\n...%end ...\n" back (notifications in between are skipped).
static int tin, tout; static char tb[1 << 20]; static size_t tlen;
static void tmux_ask(const char *cmd) {
  write(tin, cmd, strlen(cmd));
  for (;;) {
    char *end;
    while ((end = memmem(tb, tlen, "\n%end ", 6)) == NULL && (end = memmem(tb, tlen, "\n%error ", 8)) == NULL) {
      ssize_t n = read(tout, tb + tlen, sizeof tb - tlen); if (n <= 0) exit(2); tlen += n;
    }
    char *nl = memchr(end + 1, '\n', tlen - (end + 1 - tb));
    if (!nl) { ssize_t n = read(tout, tb + tlen, sizeof tb - tlen); if (n <= 0) exit(2); tlen += n; continue; }
    size_t used = nl + 1 - tb; memmove(tb, tb + used, tlen - used); tlen -= used; return;
  }
}
static int cmp(const void *a, const void *b) { double x = *(double *)a, y = *(double *)b; return (x > y) - (x < y); }
static void rep(const char *l, double *t, int n) { qsort(t, n, 8, cmp); printf("  %-28s p50 %7.1f us  p95 %7.1f us\n", l, t[n / 2], t[n * 95 / 100]); }
int main(int argc, char **argv) {
  int n = atoi(argv[3]), gap = argc > 4 ? atoi(argv[4]) : 0;
  int s = socket(AF_UNIX, SOCK_STREAM, 0); struct sockaddr_un a = {.sun_family = AF_UNIX}; strcpy(a.sun_path, argv[1]);
  if (connect(s, (void *)&a, sizeof a)) { perror("connect"); return 1; }
  int pin[2], pout[2]; pipe(pin); pipe(pout);
  posix_spawn_file_actions_t fa; posix_spawn_file_actions_init(&fa);
  posix_spawn_file_actions_adddup2(&fa, pin[0], 0); posix_spawn_file_actions_adddup2(&fa, pout[1], 1);
  char *targv[] = {"tmux", "-L", argv[2], "-C", "attach", "-t", "z", NULL}; pid_t pid;
  if (posix_spawnp(&pid, "tmux", &fa, NULL, targv, environ)) { perror("spawn"); return 1; }
  close(pin[0]); close(pout[1]); tin = pin[1]; tout = pout[0];
  // tmux -C answers the attach itself first (an empty %begin/%end) and says the session changed: all of
  // that is read and dropped, so each command below is matched with its own reply.
  usleep(300000);
  { int fl = fcntl(tout, F_GETFL); fcntl(tout, F_SETFL, fl | O_NONBLOCK); while (read(tout, tb, sizeof tb) > 0) {} fcntl(tout, F_SETFL, fl); tlen = 0; }
  tmux_ask("display-message -p x\n");
  double *t[4]; for (int k = 0; k < 4; k++) t[k] = calloc(n, 8);
  for (int round = 0; round < 2; round++) {
    for (int i = 0; i < n; i++) {
      if (gap) usleep(gap);
      double t0 = now_us(); tmux_ask("send-keys -t z -l x\n"); t[0][i] = now_us() - t0;
      if (gap) usleep(gap);
      t0 = now_us(); zeropty_ask(s, "{\"argv\":[\"send-keys\",\"-t\",\"=z:\",\"-l\",\"x\"],\"cwd\":\"/\",\"raw\":true}\n"); t[1][i] = now_us() - t0;
      if (gap) usleep(gap);
      t0 = now_us(); tmux_ask("capture-pane -p -t z\n"); t[2][i] = now_us() - t0;
      if (gap) usleep(gap);
      t0 = now_us(); zeropty_ask(s, "{\"argv\":[\"capture-pane\",\"-p\",\"-t\",\"=z:\"],\"cwd\":\"/\",\"raw\":true}\n"); t[3][i] = now_us() - t0;
    }
    printf("round %d (C client, %d each, gap %d us):\n", round + 1, n, gap);
    rep("send-keys: tmux -C", t[0], n); rep("send-keys: zeropty socket", t[1], n); rep("capture-pane: tmux -C", t[2], n); rep("capture-pane: zeropty socket", t[3], n);
  }
  return 0;
}
