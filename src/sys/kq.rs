//! kqueue: one per engine, shared with its host. Events carry a tag in `udata` (an integer, never a
//! pointer): the engine's sessions, the server's connections and the host's own fds tell their events
//! apart by it (see crate::engine::HOST_TAG).
//!
//! The types here are the kernel's `struct kevent`, wrapped so nothing outside this module touches its
//! fields: `Change` is an interest to apply (built by its constructors), `Event` one the kernel reported
//! (read through its accessors). Both are `repr(transparent)`, so a slice of either goes to `kevent(2)`
//! as it is, with no copy.

use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd as _, OwnedFd, RawFd};

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
/// let mut events = vec![Event::EMPTY; 64]; // what Kqueue::wait fills
/// let fake = Event::new(Kind::Readable, 3, 9); // for a poller of a test's own
/// events[0] = fake;
/// assert_eq!((events[0].kind(), events[0].ident(), events[0].tag()), (Kind::Readable, 3, 9));
/// ```
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Event(libc::kevent);

impl Event {
    /// An empty event, to size the buffers `Kqueue::wait` fills.
    pub const EMPTY: Event = Event(libc::kevent { ident: 0, filter: 0, flags: 0, fflags: 0, data: 0, udata: std::ptr::null_mut() });

    /// An event as the kernel would report it: for pollers that are not a kqueue (tests, simulations).
    ///
    /// ```
    /// use zeropty::engine::{Event, Kind};
    ///
    /// assert_eq!(Event::new(Kind::Writable, 3, 1).kind(), Kind::Writable);
    /// ```
    pub fn new(kind: Kind, ident: RawFd, tag: usize) -> Self {
        let filter = match kind {
            Kind::Readable => libc::EVFILT_READ,
            Kind::Writable => libc::EVFILT_WRITE,
            Kind::Exited => libc::EVFILT_PROC,
            Kind::Other => libc::EVFILT_USER,
        };
        Event(libc::kevent { ident: ident as usize, filter, flags: 0, fflags: 0, data: 0, udata: tag as *mut libc::c_void })
    }

    /// The tag it was registered with.
    ///
    /// ```
    /// assert_eq!(zeropty::engine::Event::new(zeropty::engine::Kind::Other, 1, 77).tag(), 77);
    /// ```
    pub fn tag(&self) -> usize {
        self.0.udata as usize
    }

    /// What it reports.
    ///
    /// ```
    /// use zeropty::engine::{Event, Kind};
    ///
    /// assert_eq!(Event::EMPTY.kind(), Kind::Other);
    /// ```
    pub fn kind(&self) -> Kind {
        match self.0.filter {
            libc::EVFILT_READ => Kind::Readable,
            libc::EVFILT_WRITE => Kind::Writable,
            libc::EVFILT_PROC => Kind::Exited,
            _ => Kind::Other,
        }
    }

    /// The fd, or the pid for `Kind::Exited`, it is about.
    ///
    /// ```
    /// assert_eq!(zeropty::engine::Event::new(zeropty::engine::Kind::Readable, 12, 0).ident(), 12);
    /// ```
    pub fn ident(&self) -> RawFd {
        self.0.ident as RawFd
    }
}

// SAFETY: the only pointer in a kevent is `udata`, which here always holds an integer tag and is never
// dereferenced: an Event is plain data, safe to move to and share with any thread.
unsafe impl Send for Event {}
// SAFETY: as for Send.
unsafe impl Sync for Event {}

impl std::fmt::Debug for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Event").field("kind", &self.kind()).field("ident", &self.ident()).field("tag", &self.tag()).finish()
    }
}

/// What a `Change` asks to be told about.
///
/// ```
/// use zeropty::engine::{Change, Interest};
///
/// // Report only once 16 KB are waiting: a burst is read at once (what the engine does while a read is due).
/// let c = Change::new(3, Interest::Read { lowat: 16 << 10 }, 1);
/// assert_eq!(c.interest(), Interest::Read { lowat: 16 << 10 });
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interest {
    /// The fd turned readable, edge-triggered: one event per arrival of new data, not one per wait while
    /// data sits unread; with a low-water mark above 1, only once that many bytes are waiting.
    Read {
        /// Bytes that must be waiting before it is reported (1: any).
        lowat: u32,
    },
    /// The fd takes writes again; reported once.
    WriteOnce,
    /// The process exited; reported once.
    Exit,
    /// A user event (EVFILT_USER), triggered from another thread with `Kqueue::trigger`.
    User,
}

/// An interest to register: applied at once (`Kqueue::apply`) or with the next wait.
///
/// ```
/// use zeropty::engine::{Change, Event, Kqueue};
///
/// let q = Kqueue::new()?;
/// let (_tx, rx) = std::os::unix::net::UnixStream::pair()?;
/// let change = Change::read(std::os::fd::AsRawFd::as_raw_fd(&rx), 5);
/// let mut events = [Event::EMPTY; 4];
/// assert_eq!(q.wait(&[change], &mut events, 0)?, 0); // applied with the wait; nothing to read yet
/// # Ok::<(), std::io::Error>(())
/// ```
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Change(libc::kevent);

impl Change {
    /// `interest` in `ident` (an fd; a pid for `Interest::Exit`; any number for `Interest::User`),
    /// reported with `tag`.
    ///
    /// ```
    /// use zeropty::engine::{Change, Interest};
    ///
    /// let c = Change::new(std::process::id() as i32, Interest::Exit, 2); // a pid for Interest::Exit
    /// assert_eq!((c.interest(), c.tag()), (Interest::Exit, 2));
    /// ```
    pub fn new(ident: RawFd, interest: Interest, tag: usize) -> Self {
        let (filter, flags, fflags, data) = match interest {
            Interest::Read { lowat } if lowat > 1 => (libc::EVFILT_READ, libc::EV_ADD | libc::EV_CLEAR, libc::NOTE_LOWAT, lowat as isize),
            Interest::Read { .. } => (libc::EVFILT_READ, libc::EV_ADD | libc::EV_CLEAR, 0, 0),
            Interest::WriteOnce => (libc::EVFILT_WRITE, libc::EV_ADD | libc::EV_ONESHOT, 0, 0),
            Interest::Exit => (libc::EVFILT_PROC, libc::EV_ADD | libc::EV_ONESHOT, libc::NOTE_EXIT, 0),
            Interest::User => (libc::EVFILT_USER, libc::EV_ADD | libc::EV_CLEAR, 0, 0),
        };
        Change(libc::kevent { ident: ident as usize, filter, flags, fflags, data, udata: tag as *mut libc::c_void })
    }

    /// Reports `fd` readable (any data), tagged `tag`.
    ///
    /// ```
    /// use zeropty::engine::{Change, Interest};
    ///
    /// assert_eq!(Change::read(3, 1), Change::new(3, Interest::Read { lowat: 1 }, 1));
    /// ```
    pub fn read(fd: RawFd, tag: usize) -> Self {
        Self::new(fd, Interest::Read { lowat: 1 }, tag)
    }

    /// The fd (or pid) it names.
    ///
    /// ```
    /// assert_eq!(zeropty::engine::Change::read(7, 0).ident(), 7);
    /// ```
    pub fn ident(&self) -> RawFd {
        self.0.ident as RawFd
    }

    /// The tag it reports with.
    ///
    /// ```
    /// assert_eq!(zeropty::engine::Change::read(7, 99).tag(), 99);
    /// ```
    pub fn tag(&self) -> usize {
        self.0.udata as usize
    }

    /// What it asks for.
    ///
    /// ```
    /// use zeropty::engine::{Change, Interest};
    ///
    /// assert_eq!(Change::new(1, Interest::WriteOnce, 0).interest(), Interest::WriteOnce);
    /// ```
    pub fn interest(&self) -> Interest {
        match self.0.filter {
            libc::EVFILT_READ => Interest::Read { lowat: if self.0.fflags & libc::NOTE_LOWAT != 0 { self.0.data as u32 } else { 1 } },
            libc::EVFILT_WRITE => Interest::WriteOnce,
            libc::EVFILT_PROC => Interest::Exit,
            _ => Interest::User,
        }
    }
}

// SAFETY: as for Event: `udata` is an integer tag, never dereferenced.
unsafe impl Send for Change {}
// SAFETY: as for Send.
unsafe impl Sync for Change {}

impl std::fmt::Debug for Change {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Change").field("ident", &self.ident()).field("interest", &self.interest()).field("tag", &self.tag()).finish()
    }
}

impl PartialEq for Change {
    fn eq(&self, o: &Self) -> bool {
        (self.ident(), self.interest(), self.tag()) == (o.ident(), o.interest(), o.tag())
    }
}

/// A kqueue, closed on drop.
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
pub struct Kqueue(OwnedFd);

impl Kqueue {
    /// A new kqueue (close-on-exec, as kqueues always are: they are not inherited across fork).
    ///
    /// ```
    /// let q = zeropty::engine::Kqueue::new()?; // closed when dropped
    /// # drop(q);
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn new() -> io::Result<Self> {
        // SAFETY: kqueue() takes no arguments; a non-negative result is a new fd that nothing else owns.
        let fd = unsafe { libc::kqueue() };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` was just returned by kqueue() and is owned by nobody else.
        Ok(Kqueue(unsafe { OwnedFd::from_raw_fd(fd) }))
    }

    /// Another handle on the same kqueue (dup): for a thread that waits on it while another holds the
    /// first.
    ///
    /// ```
    /// use zeropty::engine::{Change, Event, Interest, Kqueue};
    ///
    /// let q = Kqueue::new()?;
    /// let other = q.try_clone()?; // the same queue: what one registers, the other reports
    /// q.apply(&[Change::new(1, Interest::User, 3)])?;
    /// q.trigger(1, 3)?;
    /// let mut events = [Event::EMPTY; 2];
    /// assert_eq!(other.wait(&[], &mut events, 1000)?, 1);
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn try_clone(&self) -> io::Result<Self> {
        self.0.try_clone().map(Kqueue)
    }

    /// One `kevent(2)` call, retried on EINTR when it only applies changes.
    fn kevent(&self, changes: &[Change], events: &mut [Event], timeout_ms: Option<u64>) -> io::Result<usize> {
        let ts = timeout_ms.map(|ms| libc::timespec { tv_sec: (ms / 1000) as _, tv_nsec: ((ms % 1000) * 1_000_000) as _ });
        let tsp = ts.as_ref().map_or(std::ptr::null(), |t| t as *const libc::timespec);
        loop {
            // SAFETY: `Change` and `Event` are repr(transparent) over libc::kevent, so both slices are
            // valid arrays of kevent of the lengths passed; `tsp` is null or points at `ts`, alive here.
            let n = unsafe {
                libc::kevent(self.0.as_raw_fd(), changes.as_ptr().cast(), changes.len() as i32, events.as_mut_ptr().cast(), events.len() as i32, tsp)
            };
            if n >= 0 {
                return Ok(n as usize);
            }
            let e = io::Error::last_os_error();
            // A wait interrupted is a wait that saw nothing; changes alone are applied again.
            if e.kind() != io::ErrorKind::Interrupted || !events.is_empty() {
                return Err(e);
            }
        }
    }

    /// Applies `changes` now. Fails with the first change the kernel refused (ESRCH for an exit
    /// interest in a process already gone); the ones before it are applied.
    ///
    /// ```
    /// use zeropty::engine::{Change, Interest, Kqueue};
    ///
    /// let q = Kqueue::new()?;
    /// q.apply(&[])?; // nothing to do
    /// // A process that is not there any more (pid 0 is never a child to watch): refused.
    /// assert!(q.apply(&[Change::new(i32::MAX, Interest::Exit, 1)]).is_err());
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn apply(&self, changes: &[Change]) -> io::Result<()> {
        if changes.is_empty() {
            return Ok(());
        }
        self.kevent(changes, &mut [], Some(0)).map(drop)
    }

    /// Applies `changes`, then waits up to `timeout_ms` for events into `events`; returns how many came
    /// (0 on timeout, or when a signal interrupted the wait).
    ///
    /// A change the kernel refuses (its fd closed, its process gone) comes back from kevent(2) as an
    /// EV_ERROR entry among the events while there is room for it, and the call then returns at once
    /// with those entries only: they are dropped here, so `events` only ever holds what happened (such a
    /// wait returns 0 early; the changes after the refused one were applied). With no room left, kevent(2) fails with the refused change's errno
    /// instead, and the changes after it are not applied: an `Err` is either that or the queue itself
    /// failing (see crate::engine::Engine::try_poll, which tells them apart).
    ///
    /// ```
    /// use zeropty::engine::{Event, Kqueue};
    ///
    /// let q = Kqueue::new()?;
    /// let mut events = [Event::EMPTY; 4];
    /// let start = std::time::Instant::now();
    /// assert_eq!(q.wait(&[], &mut events, 20)?, 0); // nothing registered: times out
    /// assert!(start.elapsed() >= std::time::Duration::from_millis(15));
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn wait(&self, changes: &[Change], events: &mut [Event], timeout_ms: u64) -> io::Result<usize> {
        let n = match self.kevent(changes, events, Some(timeout_ms)) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => return Ok(0),
            r => r?,
        };
        if changes.is_empty() {
            // Only a change can come back as an error.
            return Ok(n);
        }
        let mut kept = 0;
        for i in 0..n {
            if events[i].0.flags & libc::EV_ERROR == 0 {
                events[kept] = events[i];
                kept += 1;
            }
        }
        Ok(kept)
    }

    /// Triggers user event `ident` (registered with `Interest::User`): wakes a thread waiting here.
    ///
    /// Waking a thread blocked in [`wait`](Self::wait), as [`crate::embed::Embedded`] wakes its engine's
    /// thread when a command comes:
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
    pub fn trigger(&self, ident: usize, tag: usize) -> io::Result<()> {
        let ev = libc::kevent { ident, filter: libc::EVFILT_USER, flags: 0, fflags: libc::NOTE_TRIGGER, data: 0, udata: tag as *mut libc::c_void };
        self.apply(&[Change(ev)])
    }
}

impl AsFd for Kqueue {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl AsRawFd for Kqueue {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
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
        // Another handle on the same queue sees it.
        let other = kq.try_clone().unwrap();
        assert_eq!(other.wait(&[], &mut events, 1000).unwrap(), 1);
        assert_eq!((events[0].tag(), events[0].kind()), (42, Kind::Other));
        assert_eq!(kq.wait(&[], &mut events, 0).unwrap(), 0, "EV_CLEAR: reported once");
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
        // Only 2 bytes waiting: a low-water mark of 4 holds the report back.
        let lowat = Change::new(a.as_raw_fd(), Interest::Read { lowat: 4 }, 5);
        assert_eq!(lowat.interest(), Interest::Read { lowat: 4 });
        assert_eq!(kq.wait(&[lowat], &mut events, 0).unwrap(), 0);
        (&b).write_all(b"yz").unwrap();
        assert_eq!(kq.wait(&[], &mut events, 0).unwrap(), 0);
        (&b).write_all(b"w").unwrap();
        assert_eq!(kq.wait(&[], &mut events, 1000).unwrap(), 1);
    }

    #[test]
    fn refused_changes_are_errors() {
        let kq = Kqueue::new().unwrap();
        // No such process: ESRCH, which the engine reads as "already gone".
        let e = kq.apply(&[Change::new(i32::MAX, Interest::Exit, 1)]).unwrap_err();
        assert_eq!(e.raw_os_error(), Some(libc::ESRCH));
        // A closed fd: EBADF.
        assert!(kq.apply(&[Change::read(-1, 1)]).is_err());
    }

    #[test]
    fn refused_changes_in_a_wait_are_not_events() {
        let kq = Kqueue::new().unwrap();
        let (a, b) = crate::sys::fd::socket_pair().unwrap();
        (&b).write_all(b"x").unwrap();
        // An fd not open (far above any this process has, so no other test's) among the changes: the
        // kernel reports it as an EV_ERROR entry, which is dropped; the change after it still applies.
        let closed = 999_999;
        let mut events = [Event::EMPTY; 4];
        // The kernel returns at once with the refusal alone, which is no event.
        assert_eq!(kq.wait(&[Change::read(closed, 1), Change::read(a.as_raw_fd(), 2)], &mut events, 1000).unwrap(), 0);
        assert_eq!(kq.wait(&[], &mut events, 1000).unwrap(), 1);
        assert_eq!((events[0].kind(), events[0].tag()), (Kind::Readable, 2));
        // No room to report it: the refusal is the wait's error.
        assert!(kq.wait(&[Change::read(closed, 1)], &mut [], 0).is_err());
    }

    #[test]
    fn changes_and_events_read_back() {
        for i in [Interest::Read { lowat: 1 }, Interest::Read { lowat: 9 }, Interest::WriteOnce, Interest::Exit, Interest::User] {
            let c = Change::new(3, i, 77);
            assert_eq!((c.ident(), c.interest(), c.tag()), (3, i, 77));
        }
        for k in [Kind::Readable, Kind::Writable, Kind::Exited, Kind::Other] {
            let e = Event::new(k, 9, 1 << 40);
            assert_eq!((e.kind(), e.ident(), e.tag()), (k, 9, 1 << 40));
        }
    }
}
