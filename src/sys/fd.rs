//! File descriptors: non-blocking reads and writes as the engine wants them, waiting for several fds
//! (`poll`), the socket pairs sessions print into, and passing fds to another process (SCM_RIGHTS).
//!
//! Every fd here is typed: the engine owns sessions' fds as `OwnedFd` (closed when dropped; an
//! `Option<OwnedFd>` is as small as a `RawFd`, the niche of -1) and lends them to these calls as
//! `BorrowedFd`, so none can be used after its close. Only `set_nonblocking` still takes a `RawFd`, for
//! its deprecated public re-export.

use std::io::{self, ErrorKind};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd as _, AsRawFd as _, BorrowedFd, FromRawFd as _, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::{PoisonError, RwLock};

/// Making fds and starting processes exclude each other. macOS has neither SOCK_CLOEXEC nor pipe2, and
/// std's accept and connect also set FIOCLEX only after the socket exists: an fd is made close-on-exec a
/// moment after it exists, and a process another thread starts in that moment inherits it and holds it
/// open, so its other end never sees the end (a session's output never closing, a pipe-pane command's
/// input staying open, a client's connection outliving the server's close). Every fd the crate makes
/// (pairs, pipes, the listener, accepted and connected sockets) holds this shared, so they never wait for
/// each other; starting a process holds it alone. An uncontended lock per fd made and per spawn, none of
/// them on a hot path (an accept is once per connection, not per command).
static FORKS: RwLock<()> = RwLock::new(());

/// Runs `make` (which creates fds and makes them close-on-exec) while no process can start.
fn making<T>(make: impl FnOnce() -> T) -> T {
    let _making = FORKS.read().unwrap_or_else(PoisonError::into_inner);
    make()
}

/// `UnixStream::pair`, never inherited by a process started meanwhile through [`spawn`].
pub(crate) fn socket_pair() -> io::Result<(UnixStream, UnixStream)> {
    making(UnixStream::pair)
}

/// `UnixListener::bind`, never inherited (as [`socket_pair`]).
pub(crate) fn bind(path: &std::path::Path) -> io::Result<std::os::unix::net::UnixListener> {
    making(|| std::os::unix::net::UnixListener::bind(path))
}

/// One `accept` on `listener`, the connection never inherited (as [`socket_pair`]).
pub(crate) fn accept(listener: &std::os::unix::net::UnixListener) -> io::Result<UnixStream> {
    making(|| listener.accept().map(|(s, _)| s))
}

/// `UnixStream::connect`, never inherited (as [`socket_pair`]).
pub(crate) fn connect(path: &std::path::Path) -> io::Result<UnixStream> {
    making(|| UnixStream::connect(path))
}

/// A pipe (read end, write end), both ends close-on-exec and never inherited (as [`socket_pair`]).
pub(crate) fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    making(|| {
        let mut p = [0; 2];
        // SAFETY: pipe writes two fds into `p`, alive for the call.
        if unsafe { libc::pipe(p.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: both fds were just made by pipe() and are owned by nobody else.
        let ends = unsafe { (OwnedFd::from_raw_fd(p[0]), OwnedFd::from_raw_fd(p[1])) };
        for fd in [&ends.0, &ends.1] {
            // SAFETY: F_SETFD sets the flags of an fd held open by `ends`; it touches no memory.
            if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(ends)
    })
}

/// Held (shared) by a test that closes one end of a channel and expects the other end to see it at
/// once. A process being started holds a copy of every fd of the starting process, close-on-exec or
/// not, until it execs: an end closed meanwhile is still open there for that moment. Measured with
/// one thread spawning `/bin/sh` and another dropping a pair's end then reading the other: about 1%
/// of the reads found the end still open (517 of 48617) even with pairs and spawns excluding each
/// other, 0 of 46817 with the drop and the read under this guard. This was the lib tests' last flake
/// (pipe_pane_copies_output_to_its_command, output_reaches_screen_viewers_and_waits: 2 in 2000 runs).
/// Make no fd while holding it: std does not promise a waiting spawn lets a second shared holder in.
#[cfg(test)]
pub(crate) fn no_spawns() -> std::sync::RwLockReadGuard<'static, ()> {
    FORKS.read().unwrap_or_else(PoisonError::into_inner)
}

/// `cmd.spawn()`, never while a [`socket_pair`] is still being made close-on-exec.
pub(crate) fn spawn(cmd: &mut std::process::Command) -> io::Result<std::process::Child> {
    let _alone = FORKS.write().unwrap_or_else(PoisonError::into_inner);
    cmd.spawn()
}

/// Sets O_NONBLOCK on `fd`.
pub fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: fcntl with F_GETFL/F_SETFL reads and writes the flags of `fd` and touches no memory; a bad
    // fd is reported as EBADF.
    let fl = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    // SAFETY: as above.
    if fl < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// One read into `buf`, retried on EINTR: Some(n) for n bytes (0 at the end, and on an error, which ends
/// the stream the same way), None when nothing is waiting (non-blocking fd).
pub(crate) fn read(fd: BorrowedFd<'_>, buf: &mut [u8]) -> Option<usize> {
    // SAFETY: `buf` is valid for writes of `buf.len()` bytes for the duration of the call.
    unsafe { read_raw(fd, buf.as_mut_ptr(), buf.len()) }
}

/// `read` into memory not initialized yet (a Vec's spare capacity): the first `n` bytes of `buf` are
/// initialized when it returns Some(n). No `&mut [u8]` is ever made over the uninitialized bytes.
pub(crate) fn read_uninit(fd: BorrowedFd<'_>, buf: &mut [MaybeUninit<u8>]) -> Option<usize> {
    // SAFETY: `buf` is valid for writes of `buf.len()` bytes for the duration of the call; read(2) only
    // writes through the pointer and never reads the bytes it was given.
    unsafe { read_raw(fd, buf.as_mut_ptr().cast(), buf.len()) }
}

/// read(2) into `len` bytes at `ptr`, as `read` describes.
///
/// # Safety
/// `ptr` must be valid for writes of `len` bytes for the duration of the call (initialized or not).
unsafe fn read_raw(fd: BorrowedFd<'_>, ptr: *mut u8, len: usize) -> Option<usize> {
    loop {
        // SAFETY: the caller guarantees `ptr` is valid for writes of `len` bytes; `fd` is open (borrowed).
        let n = unsafe { libc::read(fd.as_raw_fd(), ptr.cast(), len) };
        if n >= 0 {
            return Some(n as usize);
        }
        match io::Error::last_os_error().kind() {
            ErrorKind::Interrupted => {}
            ErrorKind::WouldBlock => return None,
            _ => return Some(0),
        }
    }
}

/// One write of `bytes` to a non-blocking fd: how many it took (0 when it would block).
pub(crate) fn write(fd: BorrowedFd<'_>, bytes: &[u8]) -> io::Result<usize> {
    loop {
        // SAFETY: `bytes` is valid for reads of `bytes.len()` bytes for the duration of the call.
        let n = unsafe { write_raw(fd.as_raw_fd(), bytes) };
        if n >= 0 {
            return Ok(n as usize);
        }
        let e = io::Error::last_os_error();
        match e.kind() {
            ErrorKind::Interrupted => {}
            ErrorKind::WouldBlock => return Ok(0),
            _ => return Err(e),
        }
    }
}

/// Waits up to `ms` (-1: without end) for any of `fds` to turn readable; returns, per fd, whether it
/// did (or hung up, or failed: a read then says which). A signal interrupting the wait reads as nothing
/// ready.
pub fn poll<const N: usize>(fds: [BorrowedFd<'_>; N], ms: i32) -> io::Result<[bool; N]> {
    let mut p = fds.map(|fd| libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 });
    // SAFETY: `p` is an array of N pollfd, valid for reads and writes for the duration of the call.
    let n = unsafe { libc::poll(p.as_mut_ptr(), N as libc::nfds_t, ms) };
    if n < 0 {
        let e = io::Error::last_os_error();
        return if e.kind() == ErrorKind::Interrupted { Ok([false; N]) } else { Err(e) };
    }
    Ok(p.map(|p| p.revents != 0))
}

/// Waits up to `ms` for `fd` to turn readable; whether it did.
pub(crate) fn poll_readable(fd: BorrowedFd<'_>, ms: i32) -> bool {
    poll([fd], ms).is_ok_and(|[r]| r)
}

/// Sets socket option `opt` (SOL_SOCKET) on `fd` to `value`.
fn set_socket_option(fd: RawFd, opt: libc::c_int, value: libc::c_int) -> io::Result<()> {
    // SAFETY: the option value is a c_int read by the kernel for exactly its size, alive for the call.
    let r = unsafe { libc::setsockopt(fd, libc::SOL_SOCKET, opt, (&value as *const libc::c_int).cast(), std::mem::size_of_val(&value) as _) };
    if r != 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// Writes to `fd` (a socket) fail with EPIPE instead of raising SIGPIPE once the reader is gone.
#[cfg(target_os = "macos")]
pub(crate) fn no_sigpipe(fd: BorrowedFd<'_>) -> io::Result<()> {
    set_socket_option(fd.as_raw_fd(), libc::SO_NOSIGPIPE, 1)
}

/// Linux has no SO_NOSIGPIPE: `write` sends with MSG_NOSIGNAL instead.
#[cfg(target_os = "linux")]
pub(crate) fn no_sigpipe(_: BorrowedFd<'_>) -> io::Result<()> {
    Ok(())
}

/// write(2), as send(2) with MSG_NOSIGNAL on Linux when `fd` is a socket.
///
/// # Safety
/// `fd` must be open for the duration of the call.
#[cfg(target_os = "macos")]
unsafe fn write_raw(fd: RawFd, bytes: &[u8]) -> isize {
    // SAFETY: `bytes` is valid for reads of its length for the call.
    unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) }
}

/// write(2), as send(2) with MSG_NOSIGNAL on Linux when `fd` is a socket.
///
/// # Safety
/// `fd` must be open for the duration of the call.
#[cfg(target_os = "linux")]
unsafe fn write_raw(fd: RawFd, bytes: &[u8]) -> isize {
    // SAFETY: `bytes` is valid for reads of its length for the call.
    let n = unsafe { libc::send(fd, bytes.as_ptr().cast(), bytes.len(), libc::MSG_NOSIGNAL) };
    if n < 0 && io::Error::last_os_error().raw_os_error() == Some(libc::ENOTSOCK) {
        // SAFETY: as above.
        return unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
    }
    n
}

/// What a session's program writes into, before it waits for zeropty to read: a full channel holds the
/// program up until the next read (the engine's BURST_LOWAT has that read come early).
const OUTPUT_BUF: libc::c_int = 64 << 10;

/// What zeropty writes ahead of the program reading its keys (a pipe's 16 KB, which input had before).
const INPUT_BUF: libc::c_int = 16 << 10;

/// A session's terminal (zeropty's end, the program's end): one socket pair both ways, as a PTY is one
/// file, so /dev/tty opened read-write (fzf) both reads keys and draws. Not a pipe: macOS gives pipes
/// their buffers from one 16 MB pool for the whole system: past ~250 pipes holding 64 KB, every other
/// pipe gets 512 bytes, which no read threshold can tell apart (fstat and kevent still say 16 KB). Such
/// a session's program then printed 512 bytes per coalesced read (300 ms): 1000 sessions replaying real
/// claude work had 59% of their output through, the rest held up in the programs. A socket's buffers are
/// its own, set here.
pub(crate) fn terminal_channel() -> io::Result<(OwnedFd, OwnedFd)> {
    let (ours, theirs) = socket_pair()?;
    set_socket_option(theirs.as_raw_fd(), libc::SO_SNDBUF, OUTPUT_BUF)?;
    set_socket_option(ours.as_raw_fd(), libc::SO_RCVBUF, OUTPUT_BUF)?;
    set_socket_option(ours.as_raw_fd(), libc::SO_SNDBUF, INPUT_BUF)?;
    set_socket_option(theirs.as_raw_fd(), libc::SO_RCVBUF, INPUT_BUF)?;
    // Keys sent after the program is gone fail with EPIPE, not SIGPIPE, also in a process embedding the
    // engine that does not ignore SIGPIPE.
    no_sigpipe(ours.as_fd())?;
    Ok((ours.into(), theirs.into()))
}

/// End of input on a session's terminal (C-d): the program's reads see EOF while its output still
/// flows, as a pipe's write end closed did. An fd that is not a socket (the input pipe of a session from
/// an older server, kept across restart-server) has nothing to shut: closing it is its EOF.
pub(crate) fn shut_input(fd: BorrowedFd<'_>) {
    // SAFETY: shutdown takes an fd held open by `fd` and touches no memory.
    let _ = unsafe { libc::shutdown(fd.as_raw_fd(), libc::SHUT_WR) };
}

/// Room for the two fds a session sends, aligned for cmsghdr.
type Control = [u64; 8];

/// Writes `bytes` to `s` with `fds` attached to the first of them.
pub(crate) fn send_with_fds(s: &UnixStream, bytes: &[u8], fds: &[BorrowedFd<'_>]) -> io::Result<()> {
    use std::io::Write as _;
    let raw: Vec<RawFd> = fds.iter().map(|fd| fd.as_raw_fd()).collect();
    let mut control: Control = [0; 8];
    let mut iov = libc::iovec { iov_base: bytes.as_ptr() as *mut libc::c_void, iov_len: bytes.len() };
    // SAFETY: msghdr is plain data; all-zero is its empty value.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    if !raw.is_empty() {
        let len = std::mem::size_of_val(&raw[..]) as u32;
        // SAFETY: CMSG_SPACE is arithmetic on its argument.
        let space = unsafe { libc::CMSG_SPACE(len) } as usize;
        if space > std::mem::size_of::<Control>() {
            return Err(io::Error::new(ErrorKind::InvalidInput, "too many fds"));
        }
        msg.msg_control = control.as_mut_ptr().cast();
        msg.msg_controllen = space as _;
        // SAFETY: msg_control points at `control`, aligned for cmsghdr and at least CMSG_SPACE(len) bytes
        // (checked above), so the first header and its data fit; `raw` holds `raw.len()` fds to copy.
        unsafe {
            let c = libc::CMSG_FIRSTHDR(&msg);
            (*c).cmsg_level = libc::SOL_SOCKET;
            (*c).cmsg_type = libc::SCM_RIGHTS;
            (*c).cmsg_len = libc::CMSG_LEN(len) as _;
            std::ptr::copy_nonoverlapping(raw.as_ptr(), libc::CMSG_DATA(c).cast::<RawFd>(), raw.len());
        }
    }
    loop {
        // SAFETY: `msg` points at `iov` (into `bytes`) and `control`, all alive for the call.
        let n = unsafe { libc::sendmsg(s.as_raw_fd(), &msg, 0) };
        if n >= 0 {
            return (&*s).write_all(&bytes[n as usize..]);
        }
        let e = io::Error::last_os_error();
        if e.kind() != ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// Fills `buf` from `s` and returns the fds that came with it, close-on-exec (they would otherwise leak
/// into every program started after, holding other sessions' pipes open). On an error, fds received so
/// far are closed (dropped).
pub(crate) fn recv_with_fds(s: &UnixStream, buf: &mut [u8]) -> io::Result<Vec<OwnedFd>> {
    let mut fds = Vec::new();
    let mut got = 0;
    while got < buf.len() {
        let mut control: Control = [0; 8];
        let mut iov = libc::iovec { iov_base: buf[got..].as_mut_ptr().cast(), iov_len: buf.len() - got };
        // SAFETY: msghdr is plain data; all-zero is its empty value.
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = control.as_mut_ptr().cast();
        msg.msg_controllen = std::mem::size_of::<Control>() as _;
        // SAFETY: `msg` points at `iov` (into the unfilled part of `buf`) and `control`, alive for the call.
        let n = unsafe { libc::recvmsg(s.as_raw_fd(), &mut msg, 0) };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        // SAFETY: the kernel filled `control` with msg_controllen bytes of cmsg headers; CMSG_FIRSTHDR and
        // CMSG_NXTHDR stay within it, and each SCM_RIGHTS header holds `count` fds the kernel just
        // installed in this process, owned by nobody else until wrapped here.
        unsafe {
            let mut c = libc::CMSG_FIRSTHDR(&msg);
            while !c.is_null() {
                if (*c).cmsg_level == libc::SOL_SOCKET && (*c).cmsg_type == libc::SCM_RIGHTS {
                    let count = ((*c).cmsg_len as usize - libc::CMSG_LEN(0) as usize) / std::mem::size_of::<RawFd>();
                    let data = libc::CMSG_DATA(c).cast::<RawFd>();
                    for k in 0..count {
                        let fd = data.add(k).read_unaligned();
                        libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
                        fds.push(OwnedFd::from_raw_fd(fd));
                    }
                }
                c = libc::CMSG_NXTHDR(&msg, c);
            }
        }
        if msg.msg_flags & libc::MSG_CTRUNC != 0 {
            return Err(io::Error::other("fds cut off"));
        }
        if n == 0 {
            return Err(ErrorKind::UnexpectedEof.into());
        }
        got += n as usize;
    }
    Ok(fds)
}

#[cfg(test)]
mod tests {
    use std::os::fd::{AsFd as _, AsRawFd as _};

    #[test]
    fn nonblocking_reads_and_writes() {
        let (r, w) = super::terminal_channel().unwrap();
        super::set_nonblocking(r.as_raw_fd()).unwrap();
        super::set_nonblocking(w.as_raw_fd()).unwrap();
        assert!(super::set_nonblocking(-1).is_err());
        let (r, w) = (r.as_fd(), w.as_fd());
        let mut buf = [0u8; 8];
        assert_eq!(super::read(r, &mut buf), None, "nothing waiting");
        assert!(!super::poll_readable(r, 0));
        assert_eq!(super::write(w, b"hi").unwrap(), 2);
        assert!(super::poll_readable(r, 100));
        assert_eq!(super::read(r, &mut buf), Some(2));
        // The channel holds about OUTPUT_BUF: a writer that fills it is told 0, not an error.
        let big = vec![0u8; 1 << 20];
        let mut sent = 0;
        loop {
            match super::write(w, &big).unwrap() {
                0 => break,
                n => sent += n,
            }
        }
        assert!((16 << 10..=1 << 20).contains(&sent), "{sent}");
    }

    #[test]
    fn reads_into_uninitialized_memory() {
        let (r, w) = super::terminal_channel().unwrap();
        super::set_nonblocking(r.as_raw_fd()).unwrap();
        let mut v: Vec<u8> = Vec::with_capacity(16);
        assert_eq!(super::read_uninit(r.as_fd(), v.spare_capacity_mut()), None, "nothing waiting");
        assert_eq!(super::write(w.as_fd(), b"abc").unwrap(), 3);
        let n = super::read_uninit(r.as_fd(), v.spare_capacity_mut()).unwrap();
        // SAFETY: read_uninit initialized the first `n` bytes of the spare capacity.
        unsafe { v.set_len(n) };
        assert_eq!(v, b"abc");
        // Shut, not only dropped: a process a parallel test is starting holds a copy of `w` until it
        // execs (no_spawns), and the end then came late.
        let _ = std::os::unix::net::UnixStream::from(w).shutdown(std::net::Shutdown::Write);
        assert_eq!(super::read_uninit(r.as_fd(), v.spare_capacity_mut()), Some(0), "the end");
    }

    /// Every fd the crate makes is close-on-exec: pipes, the listener, accepted and connected sockets.
    #[test]
    fn made_fds_are_close_on_exec() {
        let cloexec = |fd: i32| {
            // SAFETY: fcntl F_GETFD only reads the flags of an fd the caller holds open.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            flags >= 0 && flags & libc::FD_CLOEXEC != 0
        };
        let (r, w) = super::pipe().unwrap();
        assert!(cloexec(r.as_raw_fd()) && cloexec(w.as_raw_fd()), "pipe");
        let dir = std::env::temp_dir().join(format!("zeropty-fd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s");
        let _ = std::fs::remove_file(&path);
        let l = super::bind(&path).unwrap();
        let c = super::connect(&path).unwrap();
        let a = super::accept(&l).unwrap();
        assert!(cloexec(l.as_raw_fd()) && cloexec(c.as_raw_fd()) && cloexec(a.as_raw_fd()), "sockets");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn poll_says_which() {
        let (a, b) = crate::sys::fd::socket_pair().unwrap();
        let (c, _d) = crate::sys::fd::socket_pair().unwrap();
        assert_eq!(super::poll([a.as_fd(), c.as_fd()], 0).unwrap(), [false, false]);
        use std::io::Write as _;
        (&b).write_all(b"x").unwrap();
        assert_eq!(super::poll([a.as_fd(), c.as_fd()], 1000).unwrap(), [true, false]);
    }

    #[test]
    fn fds_go_along() {
        let (a, b) = crate::sys::fd::socket_pair().unwrap();
        let (r, w) = super::terminal_channel().unwrap();
        super::send_with_fds(&a, b"head", &[w.as_fd()]).unwrap();
        let mut buf = [0u8; 4];
        let fds = super::recv_with_fds(&b, &mut buf).unwrap();
        assert_eq!((&buf, fds.len()), (b"head", 1));
        // The fd received is the same channel: what is written to it comes out of `r`.
        assert_eq!(super::write(fds[0].as_fd(), b"x").unwrap(), 1);
        let mut one = [0u8; 1];
        assert_eq!(super::read(r.as_fd(), &mut one), Some(1));
        // SAFETY: fcntl F_GETFD only reads the flags of an fd held open by `fds`.
        let flags = unsafe { libc::fcntl(fds[0].as_raw_fd(), libc::F_GETFD) };
        assert!(flags & libc::FD_CLOEXEC != 0, "close-on-exec");
    }
}
