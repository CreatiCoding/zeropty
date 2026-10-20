/* zeropty as a bash builtin: commands go to the server over its socket from inside bash, so none starts
 * a process (as shell/zeropty.zsh does for zsh). Load it once per shell:
 *   enable -f /path/to/zeropty/dist/zeropty_bash.dylib zeropty
 * Works with macOS's /bin/bash (3.2) and newer bash: it uses only the builtin table's layout, which has
 * not changed, and no bash function. What needs the program itself (attach, serve, restart-server, plugin-run, -V, help,
 * any command while no server is listening) runs it: dist/zeropty next to this library, or ZEROPTY_BIN. */
#include <dlfcn.h>
#include <errno.h>
#include <libgen.h>
#include <signal.h>
#include <limits.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <unistd.h>

extern char **environ;

/* bash's own types, as builtins see them (bash 3.2 through 5). */
typedef struct word_desc { char *word; int flags; } WORD_DESC;
typedef struct word_list { struct word_list *next; WORD_DESC *word; } WORD_LIST;
struct builtin {
  char *name;
  int (*function)(WORD_LIST *);
  int flags;
  char *const *long_doc;
  const char *short_doc;
  char *handle;
};
#define BUILTIN_ENABLED 0x01

typedef struct { char *p; size_t len, cap; } buf;

static void put(buf *b, const char *s, size_t n) {
  if (b->len + n + 1 > b->cap) {
    size_t cap = (b->len + n + 1) * 2;
    char *p = realloc(b->p, cap);
    if (!p) { fprintf(stderr, "zeropty: out of memory\n"); abort(); }
    b->p = p; b->cap = cap;
  }
  memcpy(b->p + b->len, s, n);
  b->len += n;
  b->p[b->len] = 0;
}

static void put_json(buf *b, const char *s) {
  put(b, "\"", 1);
  for (; *s; s++) {
    unsigned char c = (unsigned char)*s;
    if (c == '"' || c == '\\') { put(b, "\\", 1); put(b, s, 1); }
    else if (c < 0x20) { char e[8]; snprintf(e, sizeof e, "\\u%04x", c); put(b, e, 6); }
    else put(b, s, 1);
  }
  put(b, "\"", 1);
}

static int write_all(int fd, const char *p, size_t n) {
  while (n > 0) {
    ssize_t w = write(fd, p, n);
    if (w < 0) { if (errno == EINTR) continue; return -1; }
    p += w; n -= (size_t)w;
  }
  return 0;
}

/* The program, for what the server cannot do: ZEROPTY_BIN, else zeropty next to this library. */
static int run_program(char **argv) {
  char path[PATH_MAX];
  const char *bin = getenv("ZEROPTY_BIN");
  if (!bin) {
    Dl_info info;
    if (!dladdr((void *)run_program, &info) || !info.dli_fname) return 127;
    char lib[PATH_MAX];
    snprintf(lib, sizeof lib, "%s", info.dli_fname);
    snprintf(path, sizeof path, "%s/zeropty", dirname(lib));
    bin = path;
  }
  argv[0] = (char *)bin;
  /* bash reaps children in its SIGCHLD handler: held off until this one is waited for. */
  sigset_t chld, old;
  sigemptyset(&chld);
  sigaddset(&chld, SIGCHLD);
  sigprocmask(SIG_BLOCK, &chld, &old);
  pid_t pid;
  int status = 0, r = 127, e = posix_spawn(&pid, bin, NULL, NULL, argv, environ);
  if (e != 0) fprintf(stderr, "zeropty: %s: %s\n", bin, strerror(e));
  else if (waitpid(pid, &status, 0) == pid) r = WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
  sigprocmask(SIG_SETMASK, &old, NULL);
  return r;
}

static int zeropty_builtin(WORD_LIST *list) {
  int argc = 0;
  for (WORD_LIST *l = list; l; l = l->next) argc++;
  char **argv = calloc((size_t)argc + 2, sizeof *argv);
  int i = 1;
  for (WORD_LIST *l = list; l; l = l->next) argv[i++] = l->word->word;
  const char *cmd = argc ? argv[1] : "";
  static const char *program_only[] = { "", "serve", "restart-server", "plugin-run", "attach-session", "attach", "a", "-V", "--version" };
  for (size_t k = 0; k < sizeof program_only / sizeof *program_only; k++)
    if (strcmp(cmd, program_only[k]) == 0) { int r = run_program(argv); free(argv); return r; }
  /* Help is the program's own (the server does not know the word). */
  if (strcmp(cmd, "-h") == 0 || strcmp(cmd, "--help") == 0 || strcmp(cmd, "help") == 0) { int r = run_program(argv); free(argv); return r; }

  struct sockaddr_un addr = { .sun_family = AF_UNIX };
  const char *sock = getenv("ZEROPTY_SOCK");
  char def[PATH_MAX];
  if (!sock) { snprintf(def, sizeof def, "%s/.zeropty/mux.sock", getenv("HOME") ? getenv("HOME") : "/tmp"); sock = def; }
  int fd = -1;
  if (strlen(sock) < sizeof addr.sun_path) {
    strcpy(addr.sun_path, sock);
    fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd >= 0 && connect(fd, (struct sockaddr *)&addr, sizeof addr) != 0) { close(fd); fd = -1; }
  }
  if (fd < 0) { int r = run_program(argv); free(argv); return r; }

  buf req = {0};
  put(&req, "{\"argv\":[", 9);
  for (int k = 1; k <= argc; k++) { if (k > 1) put(&req, ",", 1); put_json(&req, argv[k]); }
  char cwd[PATH_MAX];
  put(&req, "],\"cwd\":", 8);
  put_json(&req, getcwd(cwd, sizeof cwd) ? cwd : "/");
  put(&req, ",\"raw\":true}\n", 13);
  free(argv);
  int bad = write_all(fd, req.p, req.len);
  free(req.p);

  /* "<code> <out bytes> <err bytes>\n", then out and err. `body` is an offset: `in` moves as it grows. */
  buf in = {0};
  char chunk[65536];
  long code = 1, nout = 0, nerr = 0;
  size_t body = 0;
  while (!bad) {
    ssize_t n = read(fd, chunk, sizeof chunk);
    if (n < 0 && errno == EINTR) continue;
    if (n <= 0) break;
    put(&in, chunk, (size_t)n);
    if (!body) {
      char *nl = memchr(in.p, '\n', in.len);
      if (!nl) continue;
      if (sscanf(in.p, "%ld %ld %ld", &code, &nout, &nerr) != 3 || nout < 0 || nerr < 0) break;
      body = (size_t)(nl - in.p) + 1;
    }
    if (in.len - body >= (size_t)(nout + nerr)) break;
  }
  close(fd);
  if (!body || in.len - body < (size_t)(nout + nerr)) {
    free(in.p);
    fprintf(stderr, "zeropty: lost server\n");
    return 1;
  }
  fflush(stdout);
  write_all(1, in.p + body, (size_t)nout);
  write_all(2, in.p + body + nout, (size_t)nerr);
  free(in.p);
  return (int)code;
}

static char *const zeropty_doc[] = { "tmux-style session commands, sent to the zeropty server from inside bash.", NULL };

struct builtin zeropty_struct = { "zeropty", zeropty_builtin, BUILTIN_ENABLED, zeropty_doc, "zeropty <command> [args...]", NULL };
