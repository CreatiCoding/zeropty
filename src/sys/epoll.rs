//! The engine's queue on Linux: epoll behind kq.rs's types (`Kqueue`, `Change`, `Event`), so nothing
//! above `sys` knows which kernel it runs on. What kqueue does itself is done here:
//!
//! - epoll keeps one registration per fd, kqueue one per fd and filter: an fd's read and write
//!   interests are merged into one mask under one tag (the engine gives both of an fd's the same tag).
//! - A process's exit is a pidfd (Linux 5.3), readable once the process exits.
//! - A user event is an eventfd per ident, written by `trigger`.
//! - A write interest is one-shot: dropped from the mask once reported.
//! - A read's low-water mark is not kept: epoll has none for AF_UNIX sockets or pipes, so every
//!   arrival is reported (the engine then reads a burst in smaller pieces, nothing is lost).
//!
//! Events that did not fit a wait's buffer (an fd both readable and writable counts twice) are kept for
//! the next wait, with an eventfd of their own keeping the queue's fd readable for a host polling it.

use std::collections::HashMap;
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd as _, OwnedFd, RawFd};
use std::sync::{Arc, Mutex};

/// What an event reports.
///
/// ```
/// use zeropty::engine::{Event, Kind};
///
/// let ev = Event::new(Kind::Exited, 4242, 1);
/// assert_eq!((ev.kind(), ev.ident()), (Kind::Exited, 4242)); // ident is the pid
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Its fd has data to read (or reached its end).
    Readable,
    /// Its fd takes writes again.
    Writable,
    /// Its process exited.
    Exited,
    /// A user event (`Kqueue::trigger`), or anything else.
    Other,
}

/// An event the kernel reported: what happened (`kind`), to which fd or pid (`ident`), and the tag it
/// was registered with (`tag`).
///
/// ```
/// use zeropty::engine::{Event, Kind};
///
/// let ev = Event::new(Kind::Readable, 3, 9);
/// assert_eq!((ev.kind(), ev.ident(), ev.tag()), (Kind::Readable, 3, 9));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    kind: Kind,
    ident: RawFd,
    tag: usize,
}

impl Event {
    /// An empty event, to size the buffers `Kqueue::wait` fills.
    pub const EMPTY: Event = Event { kind: Kind::Other, ident: 0, tag: 0 };

    /// An event as the kernel would report it: for pollers that are not the real queue (tests).
    pub fn new(kind: Kind, ident: RawFd, tag: usize) -> Self {
        Event { kind, ident, tag }
    }

    /// The tag it was registered with.
    pub fn tag(&self) -> usize {
        self.tag
    }

    /// What it reports.
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The fd, or the pid for `Kind::Exited`, it is about.
    pub fn ident(&self) -> RawFd {
        self.ident
    }
}

/// What a `Change` asks to be told about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interest {
    /// The fd turned readable, edge-triggered: one event per arrival of new data, not one per wait while
    /// data sits unread. The low-water mark is kept for kqueue's sake; epoll reports any data.
    Read {
        /// Bytes that must be waiting before it is reported (1: any). Not applied on Linux.
        lowat: u32,
    },
    /// The fd takes writes again; reported once.
    WriteOnce,
    /// The process exited; reported once.
    Exit,
    /// A user event, triggered from another thread with `Kqueue::trigger`.
    User,
}

/// An interest to register: applied at once (`Kqueue::apply`) or with the next wait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    ident: RawFd,
    interest: Interest,
    tag: usize,
}

impl Change {
    /// `interest` in `ident` (an fd; a pid for `Interest::Exit`; any number for `Interest::User`),
    /// reported with `tag`.
    pub fn new(ident: RawFd, interest: Interest, tag: usize) -> Self {
        Change { ident, interest, tag }
    }

    /// Reports `fd` readable (any data), tagged `tag`.
    pub fn read(fd: RawFd, tag: usize) -> Self {
        Self::new(fd, Interest::Read { lowat: 1 }, tag)
    }

    /// The fd (or pid) it names.
    pub fn ident(&self) -> RawFd {
        self.ident
    }

    /// The tag it reports with.
    pub fn tag(&self) -> usize {
        self.tag
    }

    /// What it asks for.
    pub fn interest(&self) -> Interest {
        self.interest
    }
}

// What an epoll entry's data names, in its top 32 bits; the low 32 are the fd, pid or user ident.
const SRC_FD: u64 = 0;
const SRC_PROC: u64 = 1;
const SRC_USER: u64 = 2;
const SRC_PENDING: u64 = 3;

/// An fd's interests, merged.
#[derive(Debug)]
struct Watched {
    read: bool,
    write: bool,
    tag: usize,
}

#[derive(Debug, Default)]
struct State {
    fds: HashMap<RawFd, Watched>,
    /// pid -> (its pidfd, tag).
    procs: HashMap<i32, (OwnedFd, usize)>,
    /// user ident -> (its eventfd, tag).
    users: HashMap<usize, (OwnedFd, usize)>,
    /// Events a wait had no room for.
    pending: Vec<Event>,
}

#[derive(Debug)]
struct Shared {
    state: Mutex<State>,
    /// Readable while `pending` holds events.
    kick: OwnedFd,
}

/// The engine's event queue (an epoll on Linux), closed on drop of the last handle.
///
/// ```
/// use std::io::Write as _;
/// use zeropty::engine::{Change, Event, Kind, Kqueue};
///
/// let q = Kqueue::new()?;
/// let (mut tx, rx) = std::os::unix::net::UnixStream::pair()?;
/// q.apply(&[Change::read(std::os::fd::AsRawFd::as_raw_fd(&rx), 7)])?;
/// tx.write_all(b"x")?;
/// let mut events = [Event::EMPTY; 4];
/// assert_eq!(q.wait(&[], &mut events, 1000)?, 1);
/// assert_eq!((events[0].kind(), events[0].tag()), (Kind::Readable, 7));
/// assert_eq!(q.wait(&[], &mut events, 0)?, 0); // edge-triggered: the unread byte is not news again
/// # Ok::<(), std::io::Error>(())
/// ```
#[derive(Debug)]
pub struct Kqueue {
    ep: OwnedFd,
    shared: Arc<Shared>,
}

fn cvt(r: libc::c_int) -> io::Result<libc::c_int> {
    if r < 0 { Err(io::Error::last_os_error()) } else { Ok(r) }
}

fn eventfd() -> io::Result<OwnedFd> {
    // SAFETY: eventfd takes plain integers; a non-negative result is a new fd nothing else owns.
    let fd = cvt(unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) })?;
    // SAFETY: just returned by eventfd.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Adds 1 to an eventfd: it turns readable.
fn bump(fd: RawFd) -> io::Result<()> {
    let one = 1u64;
    // SAFETY: writes the 8 bytes of `one`, alive for the call.
    let n = unsafe { libc::write(fd, (&one as *const u64).cast(), 8) };
    // EAGAIN: the counter is at its maximum, readable all the same.
    if n < 0 && io::Error::last_os_error().kind() != io::ErrorKind::WouldBlock { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// Empties an eventfd: not readable until the next `bump`.
fn drain(fd: RawFd) {
    let mut v = 0u64;
    // SAFETY: reads at most 8 bytes into `v`, alive for the call. Nothing to read (EAGAIN) is fine.
    let _ = unsafe { libc::read(fd, (&mut v as *mut u64).cast(), 8) };
}

impl Kqueue {
    /// A new queue (close-on-exec).
    pub fn new() -> io::Result<Self> {
        // SAFETY: epoll_create1 takes a flag; a non-negative result is a new fd that nothing else owns.
        let fd = cvt(unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) })?;
        // SAFETY: just returned by epoll_create1.
        let ep = unsafe { OwnedFd::from_raw_fd(fd) };
        let kick = eventfd()?;
        let q = Kqueue { ep, shared: Arc::new(Shared { state: Mutex::new(State::default()), kick }) };
        q.ctl(libc::EPOLL_CTL_ADD, q.shared.kick.as_raw_fd(), libc::EPOLLIN as u32, SRC_PENDING << 32)?;
        Ok(q)
    }

    /// Another handle on the same queue: for a thread that waits on it while another holds the first.
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Kqueue { ep: self.ep.try_clone()?, shared: self.shared.clone() })
    }

    fn ctl(&self, op: libc::c_int, fd: RawFd, events: u32, data: u64) -> io::Result<()> {
        let mut ev = libc::epoll_event { events, u64: data };
        // SAFETY: `ev` is a valid epoll_event alive for the call; epoll_ctl only reads it.
        cvt(unsafe { libc::epoll_ctl(self.ep.as_raw_fd(), op, fd, &mut ev) }).map(drop)
    }

    /// Registers `w`'s mask for `fd`: modified when known, added when not (or when the kernel forgot it:
    /// the fd was closed, and its number may be a new file's).
    fn register(&self, fd: RawFd, w: &Watched, known: bool) -> io::Result<bool> {
        let mut mask = libc::EPOLLET as u32 | libc::EPOLLRDHUP as u32;
        if w.read {
            mask |= libc::EPOLLIN as u32;
        }
        if w.write {
            mask |= libc::EPOLLOUT as u32;
        }
        let data = (SRC_FD << 32) | fd as u32 as u64;
        if known {
            match self.ctl(libc::EPOLL_CTL_MOD, fd, mask, data) {
                Err(e) if e.raw_os_error() == Some(libc::ENOENT) => {}
                r => return r.map(|()| true),
            }
        }
        match self.ctl(libc::EPOLL_CTL_ADD, fd, mask, data) {
            Err(e) if e.raw_os_error() == Some(libc::EEXIST) => self.ctl(libc::EPOLL_CTL_MOD, fd, mask, data).map(|()| true),
            r => r.map(|()| false),
        }
    }

    fn apply_one(&self, st: &mut State, c: &Change) -> io::Result<()> {
        match c.interest {
            Interest::Read { .. } | Interest::WriteOnce => {
                if c.ident < 0 {
                    return Err(io::Error::from_raw_os_error(libc::EBADF));
                }
                let read = matches!(c.interest, Interest::Read { .. });
                let old = st.fds.get(&c.ident);
                let mut w = match old {
                    Some(o) => Watched { read: o.read, write: o.write, tag: c.tag },
                    None => Watched { read: false, write: false, tag: c.tag },
                };
                if read {
                    w.read = true
                } else {
                    w.write = true
                }
                let still = self.register(c.ident, &w, old.is_some())?;
                if !still {
                    // A new registration: what was remembered for this number was another file's.
                    w = Watched { read, write: !read, tag: c.tag };
                    if old.is_some() {
                        self.register(c.ident, &w, true)?;
                    }
                }
                st.fds.insert(c.ident, w);
                Ok(())
            }
            Interest::Exit => {
                if let Some(p) = st.procs.get_mut(&c.ident) {
                    p.1 = c.tag;
                    return Ok(());
                }
                // SAFETY: pidfd_open takes a pid and flags; a non-negative result is a new fd.
                let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, c.ident, 0) };
                if fd < 0 {
                    return Err(io::Error::last_os_error());
                }
                // SAFETY: just returned by pidfd_open.
                let pidfd = unsafe { OwnedFd::from_raw_fd(fd as RawFd) };
                self.ctl(libc::EPOLL_CTL_ADD, pidfd.as_raw_fd(), libc::EPOLLIN as u32, (SRC_PROC << 32) | c.ident as u32 as u64)?;
                st.procs.insert(c.ident, (pidfd, c.tag));
                Ok(())
            }
            Interest::User => {
                let ident = c.ident as usize;
                if let Some(u) = st.users.get_mut(&ident) {
                    u.1 = c.tag;
                    return Ok(());
                }
                let efd = eventfd()?;
                self.ctl(libc::EPOLL_CTL_ADD, efd.as_raw_fd(), libc::EPOLLIN as u32, (SRC_USER << 32) | ident as u32 as u64)?;
                st.users.insert(ident, (efd, c.tag));
                Ok(())
            }
        }
    }

    /// Applies `changes` now. Fails with the first change the kernel refused (ESRCH for an exit
    /// interest in a process already gone); the ones before it are applied.
    pub fn apply(&self, changes: &[Change]) -> io::Result<()> {
        if changes.is_empty() {
            return Ok(());
        }
        let mut st = self.shared.state.lock().unwrap();
        changes.iter().try_for_each(|c| self.apply_one(&mut st, c))
    }

    /// Applies `changes`, then waits up to `timeout_ms` for events into `events`; returns how many came
    /// (0 on timeout, or when a signal interrupted the wait).
    ///
    /// As with kqueue: a refused change (its fd closed, its process gone) is dropped and the wait
    /// returns 0 at once, the changes after it applied; with no room for events (`events` empty), the
    /// refusal is the error and the changes after it are not applied.
    pub fn wait(&self, changes: &[Change], events: &mut [Event], timeout_ms: u64) -> io::Result<usize> {
        if events.is_empty() {
            return self.apply(changes).map(|()| 0);
        }
        {
            let mut st = self.shared.state.lock().unwrap();
            let mut refused = false;
            for c in changes {
                refused |= self.apply_one(&mut st, c).is_err();
            }
            if refused {
                return Ok(0);
            }
            if !st.pending.is_empty() {
                return Ok(self.take_pending(&mut st, events));
            }
        }
        let mut raw = vec![libc::epoll_event { events: 0, u64: 0 }; events.len()];
        let timeout = timeout_ms.min(i32::MAX as u64) as i32;
        // SAFETY: `raw` holds `raw.len()` epoll_events, writable for the call. The lock is not held:
        // another thread's `trigger` must get it while this one waits.
        let n = unsafe { libc::epoll_wait(self.ep.as_raw_fd(), raw.as_mut_ptr(), raw.len() as i32, timeout) };
        if n < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::Interrupted { Ok(0) } else { Err(e) };
        }
        let mut st = self.shared.state.lock().unwrap();
        for ev in &raw[..n as usize] {
            let (bits, data) = (ev.events, ev.u64);
            let id = data as u32;
            match data >> 32 {
                SRC_FD => {
                    let fd = id as RawFd;
                    let Some(w) = st.fds.get_mut(&fd) else { continue };
                    let tag = w.tag;
                    let ended = bits & (libc::EPOLLHUP | libc::EPOLLERR) as u32 != 0;
                    let readable = w.read && (ended || bits & (libc::EPOLLIN | libc::EPOLLRDHUP) as u32 != 0);
                    let writable = w.write && (ended || bits & libc::EPOLLOUT as u32 != 0);
                    if writable {
                        w.write = false;
                        let w = Watched { read: w.read, write: false, tag };
                        let _ = self.register(fd, &w, true);
                    }
                    if readable {
                        st.pending.push(Event::new(Kind::Readable, fd, tag));
                    }
                    if writable {
                        st.pending.push(Event::new(Kind::Writable, fd, tag));
                    }
                }
                SRC_PROC => {
                    let pid = id as i32;
                    // Dropping the pidfd takes it off the queue: one-shot.
                    if let Some((_fd, tag)) = st.procs.remove(&pid) {
                        st.pending.push(Event::new(Kind::Exited, pid, tag));
                    }
                }
                SRC_USER => {
                    if let Some((efd, tag)) = st.users.get(&(id as usize)) {
                        drain(efd.as_raw_fd());
                        let tag = *tag;
                        st.pending.push(Event::new(Kind::Other, id as RawFd, tag));
                    }
                }
                _ => drain(self.shared.kick.as_raw_fd()),
            }
        }
        Ok(self.take_pending(&mut st, events))
    }

    /// Moves what fits of `pending` into `events`; what is left keeps the queue readable.
    fn take_pending(&self, st: &mut State, events: &mut [Event]) -> usize {
        let n = st.pending.len().min(events.len());
        for (slot, ev) in events.iter_mut().zip(st.pending.drain(..n)) {
            *slot = ev;
        }
        if st.pending.is_empty() {
            drain(self.shared.kick.as_raw_fd());
        } else {
            let _ = bump(self.shared.kick.as_raw_fd());
        }
        n
    }

    /// Triggers user event `ident` (registered with `Interest::User`): wakes a thread waiting here.
    ///
    /// ```
    /// use zeropty::engine::{Change, Event, Interest, Kind, Kqueue};
    ///
    /// let q = Kqueue::new()?;
    /// q.apply(&[Change::new(1, Interest::User, 42)])?;
    /// let waker = q.try_clone()?;
    /// let t = std::thread::spawn(move || waker.trigger(1, 42));
    /// let mut events = [Event::EMPTY; 1];
    /// assert_eq!(q.wait(&[], &mut events, 5000)?, 1);
    /// assert_eq!((events[0].kind(), events[0].tag()), (Kind::Other, 42));
    /// t.join().unwrap()?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn trigger(&self, ident: usize, _tag: usize) -> io::Result<()> {
        let st = self.shared.state.lock().unwrap();
        match st.users.get(&ident) {
            Some((efd, _)) => bump(efd.as_raw_fd()),
            None => Err(io::Error::from_raw_os_error(libc::ENOENT)),
        }
    }
}

impl AsFd for Kqueue {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.ep.as_fd()
    }
}

impl AsRawFd for Kqueue {
    fn as_raw_fd(&self) -> RawFd {
        self.ep.as_raw_fd()
    }
}

#[cfg(test)]
mod tests {
    use super::{Change, Event, Interest, Kind, Kqueue};
    use std::io::Write as _;
    use std::os::fd::AsRawFd as _;

    #[test]
    fn user_events_wake_a_wait() {
        let kq = Kqueue::new().unwrap();
        kq.apply(&[Change::new(7, Interest::User, 42)]).unwrap();
        let mut events = [Event::EMPTY; 4];
        assert_eq!(kq.wait(&[], &mut events, 0).unwrap(), 0, "nothing yet");
        kq.trigger(7, 42).unwrap();
        let other = kq.try_clone().unwrap();
        assert_eq!(other.wait(&[], &mut events, 1000).unwrap(), 1);
        assert_eq!((events[0].tag(), events[0].kind()), (42, Kind::Other));
        assert_eq!(kq.wait(&[], &mut events, 0).unwrap(), 0, "reported once");
    }

    #[test]
    fn reads_are_edge_triggered() {
        let (a, b) = crate::sys::fd::socket_pair().unwrap();
        let kq = Kqueue::new().unwrap();
        kq.apply(&[Change::read(a.as_raw_fd(), 5)]).unwrap();
        (&b).write_all(b"x").unwrap();
        let mut events = [Event::EMPTY; 4];
        assert_eq!(kq.wait(&[], &mut events, 1000).unwrap(), 1);
        assert_eq!((events[0].kind(), events[0].ident(), events[0].tag()), (Kind::Readable, a.as_raw_fd(), 5));
        assert_eq!(kq.wait(&[], &mut events, 0).unwrap(), 0, "unread data is not reported again");
        (&b).write_all(b"y").unwrap();
        assert_eq!(kq.wait(&[], &mut events, 1000).unwrap(), 1, "new data is");
        // Re-registering while data waits reports it, as kqueue's EV_ADD does.
        assert_eq!(kq.wait(&[Change::read(a.as_raw_fd(), 5)], &mut events, 1000).unwrap(), 1);
    }

    #[test]
    fn reads_and_one_shot_writes_on_one_fd() {
        let (a, b) = crate::sys::fd::socket_pair().unwrap();
        let kq = Kqueue::new().unwrap();
        kq.apply(&[Change::read(a.as_raw_fd(), 3), Change::new(a.as_raw_fd(), Interest::WriteOnce, 3)]).unwrap();
        (&b).write_all(b"x").unwrap();
        // Room for one: the other comes with the next wait.
        let mut one = [Event::EMPTY; 1];
        assert_eq!(kq.wait(&[], &mut one, 1000).unwrap(), 1);
        let first = one[0].kind();
        assert_eq!(kq.wait(&[], &mut one, 0).unwrap(), 1);
        let mut kinds = [first, one[0].kind()];
        kinds.sort_by_key(|k| *k as u8);
        assert_eq!(kinds, [Kind::Readable, Kind::Writable]);
        (&b).write_all(b"y").unwrap();
        let mut events = [Event::EMPTY; 4];
        assert_eq!(kq.wait(&[], &mut events, 1000).unwrap(), 1, "the write was one-shot");
        assert_eq!(events[0].kind(), Kind::Readable);
    }

    #[test]
    fn exits_are_reported_once() {
        let mut child = std::process::Command::new("/bin/sh").args(["-c", "exit 0"]).spawn().unwrap();
        let pid = child.id() as i32;
        let kq = Kqueue::new().unwrap();
        kq.apply(&[Change::new(pid, Interest::Exit, 8)]).unwrap();
        let mut events = [Event::EMPTY; 4];
        assert_eq!(kq.wait(&[], &mut events, 5000).unwrap(), 1);
        assert_eq!((events[0].kind(), events[0].ident(), events[0].tag()), (Kind::Exited, pid, 8));
        child.wait().unwrap();
        assert_eq!(kq.wait(&[], &mut events, 0).unwrap(), 0);
    }

    #[test]
    fn refused_changes_are_errors() {
        let kq = Kqueue::new().unwrap();
        let e = kq.apply(&[Change::new(i32::MAX, Interest::Exit, 1)]).unwrap_err();
        assert_eq!(e.raw_os_error(), Some(libc::ESRCH));
        assert!(kq.apply(&[Change::read(-1, 1)]).is_err());
    }

    #[test]
    fn refused_changes_in_a_wait_are_not_events() {
        let kq = Kqueue::new().unwrap();
        let (a, b) = crate::sys::fd::socket_pair().unwrap();
        (&b).write_all(b"x").unwrap();
        let closed = 999_999;
        let mut events = [Event::EMPTY; 4];
        assert_eq!(kq.wait(&[Change::read(closed, 1), Change::read(a.as_raw_fd(), 2)], &mut events, 1000).unwrap(), 0);
        assert_eq!(kq.wait(&[], &mut events, 1000).unwrap(), 1);
        assert_eq!((events[0].kind(), events[0].tag()), (Kind::Readable, 2));
        assert!(kq.wait(&[Change::read(closed, 1)], &mut [], 0).is_err());
    }

    #[test]
    fn a_reused_fd_number_starts_clean() {
        let kq = Kqueue::new().unwrap();
        let (a, _b) = crate::sys::fd::socket_pair().unwrap();
        let n = a.as_raw_fd();
        kq.apply(&[Change::new(n, Interest::WriteOnce, 1), Change::read(n, 1)]).unwrap();
        let mut events = [Event::EMPTY; 4];
        assert_eq!(kq.wait(&[], &mut events, 1000).unwrap(), 1, "writable");
        drop(a);
        drop(_b);
        // A new file under the same number: only what is asked of it now counts.
        let (c, d) = crate::sys::fd::socket_pair().unwrap();
        assert!(c.as_raw_fd() == n || d.as_raw_fd() == n);
        let mine = if c.as_raw_fd() == n { &c } else { &d };
        kq.apply(&[Change::new(mine.as_raw_fd(), Interest::WriteOnce, 2)]).unwrap();
        assert_eq!(kq.wait(&[], &mut events, 1000).unwrap(), 1);
        assert_eq!((events[0].kind(), events[0].tag()), (Kind::Writable, 2));
    }
}
