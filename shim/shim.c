// Makes the session's pipe and socket look like a terminal, so a TUI runs without a PTY.
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <termios.h>
#include <unistd.h>
#define TTYDEFCHARS
#include <sys/ttydefaults.h>
#include <unistd.h>
#include <sys/ioctl.h>
#include <sys/stat.h>

#define INTERPOSE(n, o) \
  __attribute__((used)) static struct { const void *r; const void *o; } _i_##o \
  __attribute__((section("__DATA,__interpose"))) = { (const void *)&n, (const void *)&o }

static struct termios fake;
static int fake_init;

// The session's terminal is two files: its input pipe and its output socket. The server names them in
// ZEROPTY_TTY_ID ("dev:ino,dev:ino", input then output). Only an fd open on one of them is a terminal:
// one redirected to a file, a pipe or /dev/null is not, whatever its number, and neither is anything in
// a process started outside a session (DYLD_INSERT_LIBRARIES is inherited by every child).
struct id { dev_t dev; ino_t ino; int set; };
static struct id tty_in, tty_out;

__attribute__((constructor)) static void read_tty_id(void) {
  const char *v = getenv("ZEROPTY_TTY_ID");
  int id, od; unsigned long long ii, oi;
  if (v && sscanf(v, "%d:%llu,%d:%llu", &id, &ii, &od, &oi) == 4) {
    tty_in = (struct id){ id, ii, 1 }; tty_out = (struct id){ od, oi, 1 };
  }
}

static int is(int fd, const struct id *f) {
  struct stat st;
  return f->set && fd >= 0 && fstat(fd, &st) == 0 && st.st_dev == f->dev && st.st_ino == f->ino;
}

// A dup of either (a /dev/tty stand-in from my_open included) is the same file, so it counts too.
static int ours(int fd) { return is(fd, &tty_in) || is(fd, &tty_out); }

// Where mode changes go: the session's output, as this process has it (stdout, else stderr), or nowhere.
static int out_fd(void) { return is(1, &tty_out) ? 1 : is(2, &tty_out) ? 2 : -1; }

static int env_int(const char *k, int d) { const char *v = getenv(k); return v ? atoi(v) : d; }

// The window size: from ZEROPTY_SIZE_FILE ("cols rows") when the server gave one, read at every ask, as
// the server rewrites it when an attached terminal resizes the session (then sends SIGWINCH); else
// the size the session started with.
static void window_size(int *cols, int *rows) {
  *cols = env_int("ZEROPTY_COLS", 220); *rows = env_int("ZEROPTY_ROWS", 80);
  const char *path = getenv("ZEROPTY_SIZE_FILE");
  if (!path) return;
  int fd = open(path, O_RDONLY | O_CLOEXEC);
  if (fd < 0) return;
  char b[32]; ssize_t n = read(fd, b, sizeof b - 1); close(fd);
  int c, r;
  if (n > 0 && (b[n] = 0, sscanf(b, "%d %d", &c, &r) == 2) && c > 0 && r > 0) { *cols = c; *rows = r; }
}

static int my_isatty(int fd) { return ours(fd) ? 1 : isatty(fd); }

static int my_tcgetattr(int fd, struct termios *t) {
  if (!ours(fd)) return tcgetattr(fd, t);
  if (!fake_init) {
    memset(&fake, 0, sizeof fake);
    fake.c_iflag = ICRNL | IXON; fake.c_oflag = OPOST | ONLCR;
    fake.c_cflag = CS8 | CREAD; fake.c_lflag = ISIG | ICANON | ECHO | ECHOE | ECHOK | IEXTEN;
    // A new tty's control characters (C-c, C-d, BSpace ...): libedit binds keys from them.
    memcpy(fake.c_cc, ttydefchars, sizeof fake.c_cc);
    fake.c_ispeed = fake.c_ospeed = B38400; fake_init = 1;
  }
  *t = fake; return 0;
}

static int onlcr(const struct termios *t) { return (t->c_oflag & (OPOST | ONLCR)) == (OPOST | ONLCR); }

// The input modes zeropty's line discipline applies (src/engine/ldisc.rs), as crate::term::TTY_* flags.
static int tty_flags(const struct termios *t) {
  return (t->c_lflag & ICANON ? 1 : 0) | (t->c_lflag & ECHO ? 2 : 0) | (t->c_lflag & ISIG ? 4 : 0) | (t->c_iflag & ICRNL ? 8 : 0);
}

// A PTY turns "\n" into "\r\n" until the program clears OPOST or ONLCR; there is no PTY here, so the
// server's screen does it instead. A change is reported in the output stream itself, so it applies from
// exactly this point (an OSC the screen otherwise ignores).
static int my_tcsetattr(int fd, int act, const struct termios *t) {
  if (!ours(fd)) return tcsetattr(fd, act, t);
  struct termios old; my_tcgetattr(fd, &old);
  fake = *t;
  // Input modes too (raw mode clears them all): zeropty applies them to the keys it is sent, so they go out
  // in the output stream, ahead of anything printed after them.
  int out = out_fd();
  if (out < 0) return 0;
  if (tty_flags(&old) != tty_flags(t)) {
    char m[32]; int n = snprintf(m, sizeof m, "\033]zeropty-tty;%d\007", tty_flags(t));
    (void)!write(out, m, n);
  }
  if (onlcr(&old) != onlcr(t)) {
    const char *m = onlcr(t) ? "\033]zeropty-onlcr;1\007" : "\033]zeropty-onlcr;0\007";
    (void)!write(out, m, strlen(m));
  }
  return 0;
}

static int my_ioctl(int fd, unsigned long req, ...) {
  va_list ap; va_start(ap, req); void *arg = va_arg(ap, void *); va_end(ap);
  if (ours(fd)) {
    if (req == TIOCGWINSZ) {
      struct winsize *w = arg;
      int c, r; window_size(&c, &r);
      w->ws_col = c; w->ws_row = r;
      w->ws_xpixel = w->ws_ypixel = 0; return 0;
    }
    // The program is the terminal's foreground process group: libedit (python's REPL, sftp ...) turns
    // line editing off, prompt and all, when tcgetpgrp fails.
    if (req == TIOCGPGRP) { *(pid_t *)arg = getpgrp(); return 0; }
    if (req == TIOCSPGRP) return 0;
    if (req == TIOCGETA) return my_tcgetattr(fd, arg);
    if (req == TIOCSETA || req == TIOCSETAW || req == TIOCSETAF) return my_tcsetattr(fd, 0, arg);
  }
  return ioctl(fd, req, arg);
}

// A session has no controlling terminal, so opening /dev/tty fails (ENXIO); programs that read keys
// from it (less, more, fzf) then quit. They get the session's input instead (its output for write-only),
// when this process still has it: a dup, so a terminal too.
static int my_open(const char *path, int flags, ...) {
  int mode = 0;
  if (flags & O_CREAT) { va_list ap; va_start(ap, flags); mode = va_arg(ap, int); va_end(ap); }
  int fd = open(path, flags, mode);
  if (fd < 0 && path && strcmp(path, "/dev/tty") == 0) {
    int e = errno, from = (flags & O_ACCMODE) == O_WRONLY ? out_fd() : is(0, &tty_in) ? 0 : -1;
    if (from < 0) { errno = e; return fd; }
    fd = fcntl(from, (flags & O_CLOEXEC) ? F_DUPFD_CLOEXEC : F_DUPFD, 3);
  }
  return fd;
}

INTERPOSE(my_open, open);
INTERPOSE(my_isatty, isatty);
INTERPOSE(my_tcgetattr, tcgetattr);
INTERPOSE(my_tcsetattr, tcsetattr);
INTERPOSE(my_ioctl, ioctl);
