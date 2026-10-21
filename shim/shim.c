// Makes the session's socket look like a terminal, so a TUI runs without a PTY.
#include <ctype.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pwd.h>
#include <readpassphrase.h>
#include <signal.h>
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
#include <sys/socket.h>
#include <sys/stat.h>

#define INTERPOSE(n, o) \
  __attribute__((used)) static struct { const void *r; const void *o; } _i_##o \
  __attribute__((section("__DATA,__interpose"))) = { (const void *)&n, (const void *)&o }

static struct termios fake;
static int fake_init;

// The session's terminal is one socket, read and written both ways as a PTY is. The server names it in
// ZEROPTY_TTY_ID ("dev:ino"): `tty_out`. Only an fd open on it is a terminal: one redirected to a file,
// a pipe or /dev/null is not, whatever its number, and neither is anything in a process started outside
// a session (DYLD_INSERT_LIBRARIES is inherited by every child).
// A session from an older server, kept across restart-server, has two files: an input pipe (`tty_in`,
// read only) and the output socket, named "dev:ino,dev:ino", or not at all by a server older still.
// An inode number of a pipe or socket is a kernel address, reused once the file is freed: a shimmed
// process that outlives its session (nohup) could take a new one for the old terminal. Within the
// session the server holds the files open, so no number is reused.
struct id { dev_t dev; ino_t ino; int set; };
static struct id tty_in, tty_out;
// The session is from an older server: its input is `tty_in` (a pipe), or nothing when fd 0 was not it.
static int legacy;

// The file `fd` is open on, when it is of type `type` (S_IFIFO, S_IFSOCK).
static struct id file_of(int fd, mode_t type) {
  struct stat st;
  if (fstat(fd, &st) != 0 || (st.st_mode & S_IFMT) != type) return (struct id){ 0, 0, 0 };
  return (struct id){ st.st_dev, st.st_ino, 1 };
}

__attribute__((constructor)) static void read_tty_id(void) {
  const char *v = getenv("ZEROPTY_TTY_ID");
  int id, od; unsigned long long ii, oi;
  int n = v ? sscanf(v, "%d:%llu,%d:%llu", &id, &ii, &od, &oi) : 0;
  if (n == 4) {
    tty_in = (struct id){ id, ii, 1 }; tty_out = (struct id){ od, oi, 1 }; legacy = 1;
  } else if (n == 2) {
    tty_out = (struct id){ id, ii, 1 };
  } else if (!v && getenv("ZEROPTY_SESSION")) {
    legacy = 1;
    // A session started by a server from before ZEROPTY_TTY_ID (kept across restart-server): such a
    // server made the input a pipe and the output a socket, so take them from this process's fds as
    // it starts. A redirect to a file or a pipe on is then still not a terminal.
    tty_in = file_of(0, S_IFIFO);
    tty_out = file_of(1, S_IFSOCK);
    if (!tty_out.set) tty_out = file_of(2, S_IFSOCK);
  }
}

static int is(int fd, const struct id *f) {
  struct stat st;
  return f->set && fd >= 0 && fstat(fd, &st) == 0 && st.st_dev == f->dev && st.st_ino == f->ino;
}

// A dup of either (a /dev/tty stand-in from my_open included) is the same file, so it counts too.
static int ours(int fd) { return is(fd, &tty_in) || is(fd, &tty_out); }

// Where mode changes go: the session's socket, as this process has it (stdout, else stderr, else stdin:
// one file, so the same stream whichever), or nowhere.
static int out_fd(void) { return is(1, &tty_out) ? 1 : is(2, &tty_out) ? 2 : is(0, &tty_out) ? 0 : -1; }

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
// from it (less, more, fzf) then quit. They get the session's socket instead, when this process still
// has it: a dup, so a terminal too, read and written whatever the mode asked. In a session from an
// older server reading goes to its input pipe, which cannot be written.
static int my_open(const char *path, int flags, ...) {
  int mode = 0;
  if (flags & O_CREAT) { va_list ap; va_start(ap, flags); mode = va_arg(ap, int); va_end(ap); }
  int fd = open(path, flags, mode);
  if (fd < 0 && path && strcmp(path, "/dev/tty") == 0) {
    int e = errno, from = out_fd();
    if ((flags & O_ACCMODE) != O_WRONLY && legacy) from = is(0, &tty_in) ? 0 : -1;
    if (from < 0) { errno = e; return fd; }
    fd = fcntl(from, (flags & O_CLOEXEC) ? F_DUPFD_CLOEXEC : F_DUPFD, 3);
  }
  return fd;
}

// libc's readpassphrase (ssh, ssh-keygen, ssh-add) and getpass turn echo off with libc's own tcgetattr and
// tcsetattr: interposing reaches only calls from other images, so the passphrase was echoed onto the
// session's screen (and its scrollback). On the session's terminal they are done here, with the calls
// above; anywhere else libc's own run as before.

// The session's terminal for a passphrase (a /dev/tty stand-in), or -1. With RPP_STDIN libc leaves echo
// alone, so its own readpassphrase already does what it would on a terminal.
static int passphrase_tty(int flags) {
  if (flags & RPP_STDIN) return -1;
  int fd = my_open("/dev/tty", O_RDWR | O_CLOEXEC);
  if (fd >= 0 && !ours(fd)) { close(fd); fd = -1; }
  return fd;
}

static volatile sig_atomic_t caught[NSIG];

static void catch_signal(int s) { caught[s] = 1; }

static char *read_passphrase(int fd, const char *prompt, char *buf, size_t size, int flags) {
  // As libc's: a signal while echo is off restores the terminal first, then is delivered; after a stop
  // (C-z, then fg) the prompt starts over.
  static const int sigs[] = { SIGALRM, SIGHUP, SIGINT, SIGPIPE, SIGQUIT, SIGTERM, SIGTSTP, SIGTTIN, SIGTTOU };
  enum { NS = sizeof sigs / sizeof *sigs };
  for (;;) {
    struct sigaction sa = { 0 }, old_sa[NS];
    sa.sa_handler = catch_signal;
    sigemptyset(&sa.sa_mask);
    for (int i = 0; i < NS; i++) { caught[sigs[i]] = 0; sigaction(sigs[i], &sa, &old_sa[i]); }
    struct termios old, t;
    my_tcgetattr(fd, &old);
    t = old;
    if (!(flags & RPP_ECHO_ON)) t.c_lflag &= ~(ECHO | ECHONL);
    my_tcsetattr(fd, TCSAFLUSH, &t);
    // TCSAFLUSH: keys typed ahead are dropped, not taken as the passphrase.
    char drop[256];
    while (recv(fd, drop, sizeof drop, MSG_DONTWAIT) > 0) {}
    // Written to the session's socket: in a session from an older server, fd is the input pipe.
    int w = out_fd();
    if (w < 0) w = fd;
    (void)!write(w, prompt, strlen(prompt));
    size_t i = 0;
    ssize_t n;
    char c;
    // The program may have made the terminal non-blocking (the socket's flags are shared): wait for keys.
    while ((n = read(fd, &c, 1)) == 1 || (n < 0 && errno == EAGAIN && poll(&(struct pollfd){ fd, POLLIN, 0 }, 1, -1) >= 0)) {
      if (n < 0) continue;
      if (c == '\n' || c == '\r') break;
      if (i + 1 >= size) continue;
      if (flags & RPP_SEVENBIT) c &= 0x7f;
      if (isalpha((unsigned char)c) && (flags & RPP_FORCELOWER)) c = (char)tolower((unsigned char)c);
      if (isalpha((unsigned char)c) && (flags & RPP_FORCEUPPER)) c = (char)toupper((unsigned char)c);
      buf[i++] = c;
    }
    buf[i] = 0;
    int e = errno;
    if (!(t.c_lflag & ECHO)) (void)!write(w, "\n", 1);
    my_tcsetattr(fd, TCSAFLUSH, &old);
    int again = 0;
    for (int k = 0; k < NS; k++) sigaction(sigs[k], &old_sa[k], NULL);
    for (int k = 0; k < NS; k++) {
      if (!caught[sigs[k]]) continue;
      kill(getpid(), sigs[k]);
      again |= sigs[k] == SIGTSTP || sigs[k] == SIGTTIN || sigs[k] == SIGTTOU;
    }
    if (again) continue;
    if (n < 0) {
      memset(buf, 0, size);
      errno = e;
      return NULL;
    }
    return buf;
  }
}

static char *my_readpassphrase(const char *prompt, char *buf, size_t size, int flags) {
  if (size == 0) { errno = EINVAL; return NULL; }
  int fd = passphrase_tty(flags);
  if (fd < 0) return readpassphrase(prompt, buf, size, flags);
  char *r = read_passphrase(fd, prompt, buf, size, flags);
  int e = errno;
  close(fd);
  errno = e;
  return r;
}

// As libc's getpass: a failed read gives "", not NULL.
static char *my_getpass(const char *prompt) {
  static char buf[_PASSWORD_LEN + 1];
  int fd = passphrase_tty(0);
  if (fd < 0) return getpass(prompt);
  if (!read_passphrase(fd, prompt, buf, sizeof buf, RPP_ECHO_OFF)) buf[0] = 0;
  int e = errno;
  close(fd);
  errno = e;
  return buf;
}

INTERPOSE(my_open, open);
INTERPOSE(my_readpassphrase, readpassphrase);
INTERPOSE(my_getpass, getpass);
INTERPOSE(my_isatty, isatty);
INTERPOSE(my_tcgetattr, tcgetattr);
INTERPOSE(my_tcsetattr, tcsetattr);
INTERPOSE(my_ioctl, ioctl);
