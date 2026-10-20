//! The user's own terminal, for a client attaching to a session (`zeropty attach`): raw mode that is
//! always restored (`RawMode`), its size, its size changes (`Winch`), and waiting on it with other fds
//! (`poll`). Sessions themselves never have a terminal; this is only the viewer's side.
//!
//! Public as `zeropty::tty`, so a front end needs no `libc` of its own.

use std::io;
use std::os::fd::{AsFd, AsRawFd as _, BorrowedFd, OwnedFd};
use std::sync::atomic::{AtomicI32, Ordering};

pub use super::fd::poll;

/// Whether `fd` is a terminal.
///
/// ```
/// use std::os::fd::AsFd as _;
///
/// let null = std::fs::File::open("/dev/null").unwrap();
/// assert!(!zeropty::tty::is_terminal(null.as_fd()));
/// ```
pub fn is_terminal(fd: BorrowedFd<'_>) -> bool {
    // SAFETY: isatty takes an fd and touches no memory.
    unsafe { libc::isatty(fd.as_raw_fd()) == 1 }
}

/// The terminal's size as (rows, cols); None when `fd` is not a terminal or reports no size.
///
/// ```
/// use std::os::fd::AsFd as _;
///
/// let null = std::fs::File::open("/dev/null").unwrap();
/// assert_eq!(zeropty::tty::window_size(null.as_fd()), None);
/// // On a terminal: `zeropty attach` sizes the session to Some((rows, cols)) of std::io::stdin().
/// ```
pub fn window_size(fd: BorrowedFd<'_>) -> Option<(u16, u16)> {
    // SAFETY: winsize is plain data; all-zero is a valid value.
    let mut w: libc::winsize = unsafe { std::mem::zeroed() };
    // SAFETY: TIOCGWINSZ writes one winsize into `w`, alive for the call.
    let r = unsafe { libc::ioctl(fd.as_raw_fd(), libc::TIOCGWINSZ, &mut w) };
    (r == 0 && w.ws_col > 0 && w.ws_row > 0).then_some((w.ws_row, w.ws_col))
}

/// One read from `fd` (blocking or not, as the fd is), retried on EINTR; 0 at its end. Unbuffered, so
/// what `poll` reports is what the next read gets (std's stdin buffers ahead of it).
///
/// ```
/// use std::io::Write as _;
/// use std::os::fd::AsFd as _;
///
/// let (mut tx, rx) = std::os::unix::net::UnixStream::pair()?;
/// tx.write_all(b"keys")?;
/// let mut buf = [0u8; 16];
/// let n = zeropty::tty::read(rx.as_fd(), &mut buf)?;
/// assert_eq!(&buf[..n], b"keys");
/// # Ok::<(), std::io::Error>(())
/// ```
pub fn read(fd: BorrowedFd<'_>, buf: &mut [u8]) -> io::Result<usize> {
    loop {
        // SAFETY: `buf` is valid for writes of `buf.len()` bytes for the duration of the call.
        let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if n >= 0 {
            return Ok(n as usize);
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// A terminal in raw mode (keys as typed, no echo, no signals from keys) until this is dropped, which
/// puts back the modes it had, on a panic too.
///
/// ```no_run
/// use std::os::fd::AsFd as _;
///
/// let stdin = std::io::stdin();
/// let raw = zeropty::tty::RawMode::enter(stdin.as_fd())?; // keys as typed, until `raw` is dropped
/// // ... read keys with zeropty::tty::read, send them to a session ...
/// drop(raw); // the terminal's modes are back, on a panic too
/// # Ok::<(), std::io::Error>(())
/// ```
pub struct RawMode {
    fd: OwnedFd,
    saved: libc::termios,
}

impl RawMode {
    /// Switches the terminal `fd` to raw mode.
    ///
    /// Fails on anything that is not a terminal:
    ///
    /// ```
    /// use std::os::fd::AsFd as _;
    ///
    /// let null = std::fs::File::open("/dev/null").unwrap();
    /// assert!(zeropty::tty::RawMode::enter(null.as_fd()).is_err());
    /// ```
    pub fn enter(fd: BorrowedFd<'_>) -> io::Result<Self> {
        // A handle of its own, so the guard stays valid whatever the caller does with `fd`.
        let fd = fd.try_clone_to_owned()?;
        // SAFETY: termios is plain data; all-zero is a valid value, overwritten by tcgetattr.
        let mut saved: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: tcgetattr writes one termios into `saved`, alive for the call.
        if unsafe { libc::tcgetattr(fd.as_raw_fd(), &mut saved) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut raw = saved;
        // SAFETY: cfmakeraw only edits the termios it is given.
        unsafe { libc::cfmakeraw(&mut raw) };
        // SAFETY: tcsetattr reads one termios from `raw`, alive for the call.
        if unsafe { libc::tcsetattr(fd.as_raw_fd(), libc::TCSANOW, &raw) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { fd, saved })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        // SAFETY: tcsetattr reads one termios from `self.saved`. Nothing is left to do if it fails (the
        // terminal is going away).
        unsafe { libc::tcsetattr(self.fd.as_raw_fd(), libc::TCSANOW, &self.saved) };
    }
}

/// The write end of the SIGWINCH pipe, for the handler (-1: none installed).
static WINCH_FD: AtomicI32 = AtomicI32::new(-1);

extern "C" fn on_winch(_: libc::c_int) {
    // Only async-signal-safe work here: one byte down the pipe the client polls. A full pipe already
    // says the size changed.
    let fd = WINCH_FD.load(Ordering::Relaxed);
    if fd >= 0 {
        // SAFETY: write(2) is async-signal-safe; the buffer is a static byte.
        unsafe { libc::write(fd, b"w".as_ptr().cast(), 1) };
    }
}

/// Size changes of this process's terminal (SIGWINCH) as an fd that turns readable: poll it with the
/// others, then `clear` it and read the new `window_size`. One at a time per process; dropping it puts
/// the default handling back.
///
/// ```
/// use std::os::fd::AsFd as _;
/// use zeropty::tty::{poll, Winch};
///
/// let winch = Winch::install()?;
/// assert!(Winch::install().is_err()); // one at a time
/// let [resized] = poll([winch.as_fd()], 0)?; // poll it with the terminal and the socket
/// assert!(!resized);
/// winch.clear();
/// # Ok::<(), std::io::Error>(())
/// ```
pub struct Winch {
    read: OwnedFd,
    /// Held open for the handler, which writes to it through WINCH_FD.
    _write: OwnedFd,
}

impl Winch {
    /// Installs the SIGWINCH handler. Fails if one is installed already.
    ///
    /// ```
    /// let w = zeropty::tty::Winch::install()?;
    /// drop(w); // SIGWINCH is handled as before again
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn install() -> io::Result<Self> {
        // Close-on-exec: a program the client starts (a plugin, a pipe-pane command) must not hold it.
        let (read, write) = super::fd::pipe()?;
        for fd in [&read, &write] {
            super::fd::set_nonblocking(fd.as_raw_fd())?;
        }
        if WINCH_FD.compare_exchange(-1, write.as_raw_fd(), Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "a SIGWINCH handler is installed already"));
        }
        // SAFETY: sigaction is plain data; all-zero is an empty mask and no flags.
        let mut sa: libc::sigaction = unsafe { std::mem::zeroed() };
        sa.sa_sigaction = on_winch as *const () as usize;
        sa.sa_flags = libc::SA_RESTART;
        // SAFETY: installs `on_winch`, which only does async-signal-safe work, for SIGWINCH.
        if unsafe { libc::sigaction(libc::SIGWINCH, &sa, std::ptr::null_mut()) } != 0 {
            let e = io::Error::last_os_error();
            WINCH_FD.store(-1, Ordering::SeqCst);
            return Err(e);
        }
        Ok(Self { read, _write: write })
    }

    /// Takes the pending notices out of the pipe, so it reports the next change only.
    ///
    /// ```
    /// let w = zeropty::tty::Winch::install()?;
    /// unsafe { libc::raise(libc::SIGWINCH) }; // as a terminal resized
    /// w.clear(); // the notice is taken; the next one is news
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn clear(&self) {
        let mut b = [0u8; 64];
        while super::fd::read(self.read.as_fd(), &mut b).is_some_and(|n| n == b.len()) {}
    }
}

impl AsFd for Winch {
    /// Readable once the size changed.
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.read.as_fd()
    }
}

impl Drop for Winch {
    fn drop(&mut self) {
        // SAFETY: puts back the default action for SIGWINCH (ignored); no memory is passed but a null old
        // action. Done before the pipe closes, so the handler never writes to a reused fd.
        unsafe { libc::signal(libc::SIGWINCH, libc::SIG_DFL) };
        WINCH_FD.store(-1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use std::os::fd::AsFd as _;

    #[test]
    fn winch_turns_readable() {
        let w = super::Winch::install().unwrap();
        assert!(super::Winch::install().is_err(), "one at a time");
        assert_eq!(super::poll([w.as_fd()], 0).unwrap(), [false]);
        // SAFETY: raise sends SIGWINCH to this thread; the handler installed above only writes a byte.
        unsafe { libc::raise(libc::SIGWINCH) };
        assert_eq!(super::poll([w.as_fd()], 1000).unwrap(), [true]);
        w.clear();
        assert_eq!(super::poll([w.as_fd()], 0).unwrap(), [false], "cleared");
        drop(w);
        assert!(super::Winch::install().is_ok(), "free again once dropped");
    }

    #[test]
    fn not_a_terminal() {
        let (a, _b) = crate::sys::fd::socket_pair().unwrap();
        assert!(!super::is_terminal(a.as_fd()));
        assert_eq!(super::window_size(a.as_fd()), None);
        assert!(super::RawMode::enter(a.as_fd()).is_err(), "tcgetattr refuses a socket");
    }
}
