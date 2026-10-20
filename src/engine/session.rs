//! One session: its program's terminal, its screen (live or frozen) and what reads its output (recording,
//! pipe-pane, attached terminals, wait-panes).
//!
//! Invariants:
//! - `term` is `Moving` only inside `live` and `freeze`; every other reader sees Live or Frozen.
//! - `app_cursor`, `bracketed_paste` and `tty` mirror the screen's after every read (`take`), so keys can
//!   be sent to a frozen screen's program without thawing it.
//! - `extra` is None whenever it would be empty (`trim_extra`): most of a thousand sessions never need it.
//! - `stdin`/`out` are None once closed; the session owns them (`OwnedFd`: closed when dropped, and an
//!   `Option<OwnedFd>` is 4 bytes like a `RawFd`, so a thousand sessions pay nothing for it).
//!
//! It reads and writes its fds itself (crate::sys::fd) but registers interest in them only through the
//! engine's `Poller`, signals its program only through `Programs`, and takes the time from its caller,
//! so the engine's tests drive it with fakes.

use super::backend::{Poller, Programs};
use super::ldisc::{self, Line};
use super::options::{BURST_END, PIPE_PENDING_MAX, SLURP_MAX, SLURP_QUIET_MS, SLURP_STEP, SLURP_WAIT_FROM, freeze_after_ms};
use super::policy::hold_after;
use super::render;
use super::reply::CmdError;
use super::table::{Generational, Named, Slots};
use crate::sys::fd;
use crate::sys::kq::{Change, Interest};
use crate::term::{Frozen, TTY_ICANON, Term};
use std::io::Write as _;
use std::os::fd::{AsFd as _, AsRawFd as _, OwnedFd, RawFd};

/// A session's screen: a Term while busy, bytes while quiet.
pub(super) enum Screenful {
    Live(Box<Term>),
    Frozen(Frozen),
    /// Only while switching between the two.
    Moving,
}

impl Screenful {
    /// Runs `f` on the screen as it is, changing nothing: a frozen one is read from a copy (`Frozen::peek`).
    pub(super) fn read<R>(&self, f: impl FnOnce(&Term) -> R) -> R {
        match self {
            Screenful::Live(t) => f(t),
            Screenful::Frozen(fz) => f(&fz.peek()),
            Screenful::Moving => unreachable!("a screen is read between its moves"),
        }
    }
}

// A closed fd costs nothing over the -1 it replaced: OwnedFd's niche holds the None.
const _: () = assert!(size_of::<Option<OwnedFd>>() == size_of::<RawFd>());

/// What a new session starts with.
pub(super) struct Started {
    pub(super) generation: u32,
    pub(super) name: Box<str>,
    pub(super) pid: i32,
    pub(super) stdin: Option<OwnedFd>,
    pub(super) out: Option<OwnedFd>,
    pub(super) size: (u16, u16),
    pub(super) base: (u16, u16),
    pub(super) created: u32,
    pub(super) term: Term,
}

pub(super) struct Sess {
    /// Distinguishes this session from an earlier one in the same slot, for events queued before a reuse.
    pub(super) generation: u32,
    pub(super) name: Box<str>,
    pub(super) pid: i32,
    /// Non-blocking; None once closed (C-d). Bytes the socket did not take yet wait in Extra::pending,
    /// flushed when it turns writable.
    pub(super) stdin: Option<OwnedFd>,
    /// Non-blocking; None once the program closed its output.
    pub(super) out: Option<OwnedFd>,
    pub(super) last_output_ms: u64,
    /// When a capture last read it: a screen being read is not compressed into the cold store.
    pub(super) last_read_ms: u64,
    /// Quiet this long (ms), the screen is frozen. Starts at freeze_after_ms; a session that prints again
    /// soon after being frozen gets its gap plus that, up to HOLD_CAP_MS, so one read every 300 ms does
    /// not freeze and thaw it between reads.
    pub(super) hold: u16,
    /// A coalesced read of `out` is scheduled.
    pub(super) due: bool,
    /// The last read filled the buffer: more is coming fast, so the next read is not delayed.
    pub(super) burst: bool,
    /// application cursor and bracketed paste, readable without unpacking a frozen screen
    pub(super) app_cursor: bool,
    pub(super) bracketed_paste: bool,
    /// The tty's input modes (crate::term::TTY_*), readable without unpacking a frozen screen.
    pub(super) tty: u8,
    /// When it was made, in seconds since 1970 (#{session_created}).
    pub(super) created: u32,
    pub(super) rows: u16,
    pub(super) cols: u16,
    /// The size new-session gave it, restored when the last terminal attached to it leaves.
    pub(super) base: (u16, u16),
    /// What most sessions never need, boxed so they do not carry it (a thousand of them are held at once).
    pub(super) extra: Option<Box<Extra>>,
    pub(super) term: Screenful,
    /// ZEROPTY_RECORD=<dir> keeps each session's output, for tests/diff.rs and benchmarks.
    pub(super) record: Option<Box<Recorder>>,
}

#[derive(Default)]
pub(super) struct Extra {
    /// Input the program's socket did not take yet.
    pub(super) pending: Vec<u8>,
    /// Ids of the terminals attached to it (Engine::attach): what it prints is copied to them.
    pub(super) viewers: Vec<u32>,
    /// Output read for the viewers and not yet handed to them.
    pub(super) forward: Vec<u8>,
    /// For a session from another engine (import_session): the size file its program was started with.
    pub(super) size_file: Option<Box<std::path::Path>>,
    /// The line being typed while the tty is canonical (ICANON), which the program gets at a line feed,
    /// and whether the next key is taken literally.
    pub(super) line: Line,
    /// C-d on an empty line came while input was still queued: the input closes once it is written.
    pub(super) eof: bool,
    /// The echo of keys sent, drawn with the next read (Sess::drain), ahead of what the program printed
    /// after them: send-keys returns without drawing (thawing a frozen screen for it made it 0.06-0.07 ms
    /// embedded, against 0.04-0.05), as a PTY's echo reaches tmux after send-keys returned.
    pub(super) echo: Vec<u8>,
    /// pipe-pane: the command its output is copied to.
    pub(super) pipe: Option<Box<PanePipe>>,
    /// wait-panes on it: its output is read as it comes, like a viewed session's, and the rows each read
    /// changes are searched for their text.
    pub(super) waits: Vec<Watch>,
}

impl Extra {
    fn is_empty(&self) -> bool {
        self.pending.is_empty()
            && self.line.text.is_empty()
            && !self.line.literal
            && !self.eof
            && self.echo.is_empty()
            && self.pipe.is_none()
            && self.viewers.is_empty()
            && self.forward.is_empty()
            && self.size_file.is_none()
            && self.waits.is_empty()
    }
}

/// One wait-pane's part held with its session (see Sess::take).
pub(super) struct Watch {
    pub(super) id: u32,
    pub(super) text: Option<Box<str>>,
    /// `text` was on the screen at some point since the wait began.
    pub(super) seen: bool,
}

/// A pipe-pane command: what the session prints is written to its input, a socket (SO_NOSIGPIPE: the
/// command ending must not signal the host). What it does not take at once waits, up to PIPE_PENDING_MAX
/// (then the rest is dropped).
pub(super) struct PanePipe {
    /// Closed when the pipe is dropped: the command gets the end of its input (it is reaped when it
    /// exits, Engine::event).
    pub(super) fd: OwnedFd,
    pub(super) pid: i32,
    /// Its session's event tag, for the write interest when the command falls behind.
    pub(super) udata: usize,
    pub(super) pending: Vec<u8>,
}

impl PanePipe {
    /// false once the command is gone.
    fn send(&mut self, poller: &impl Poller, bytes: &[u8]) -> bool {
        if !self.pending.is_empty() {
            let room = PIPE_PENDING_MAX.saturating_sub(self.pending.len());
            self.pending.extend_from_slice(&bytes[..bytes.len().min(room)]);
            return true;
        }
        let Ok(done) = fd::write(self.fd.as_fd(), bytes) else { return false };
        if done < bytes.len() {
            self.pending.extend_from_slice(&bytes[done..bytes.len().min(done + PIPE_PENDING_MAX)]);
            // Without the interest the pending bytes would never go: as good as gone.
            return poller.apply(&[Change::new(self.fd.as_raw_fd(), Interest::WriteOnce, self.udata)]).is_ok();
        }
        true
    }

    /// Writes what waited (the command's input turned writable); false once the command is gone.
    pub(super) fn flush(&mut self, poller: &impl Poller) -> bool {
        let pending = std::mem::take(&mut self.pending);
        self.send(poller, &pending)
    }
}

/// <name>.raw: the bytes as read. <name>.trace: the same reads as [u64 ms since start][u32 len][bytes],
/// so a benchmark can replay them at their original pace.
pub(super) struct Recorder {
    raw: std::fs::File,
    trace: std::fs::File,
    start_ms: u64,
}

impl Recorder {
    pub(super) fn open(dir: &std::path::Path, name: &str) -> Option<Box<Self>> {
        // A name may hold "/" (tmux allows it): the files stay in `dir`.
        let name = name.replace('/', "_");
        let open = |ext: &str| std::fs::OpenOptions::new().create(true).append(true).open(dir.join(format!("{name}.{ext}"))).ok();
        Some(Box::new(Self { raw: open("raw")?, trace: open("trace")?, start_ms: crate::sys::time::now_ms() }))
    }

    /// Appends one read. A recording is for tests and benchmarks: a failed write loses that read from
    /// it, never the session.
    fn write(&mut self, bytes: &[u8]) {
        let _ = self.raw.write_all(bytes);
        let mut head = [0u8; 12];
        // The wall clock's pace, not the engine's: a replay needs the real gaps.
        head[..8].copy_from_slice(&(crate::sys::time::now_ms() - self.start_ms).to_le_bytes());
        head[8..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        let _ = self.trace.write_all(&head);
        let _ = self.trace.write_all(bytes);
    }
}

/// The engine's counters (`stats`), and what reading needs from its options.
#[derive(Default)]
pub(super) struct Stats {
    pub(super) drains: u64,
    pub(super) reads: u64,
    pub(super) read_bytes: u64,
    pub(super) loops: u64,
    pub(super) events: u64,
    /// Options::slurp_max, for Sess::slurp (0: SLURP_MAX).
    pub(super) slurp_max: usize,
    /// Output is tapped (crate::engine::tap, kind OUTPUT): what drain_into reads is copied to `tapped`
    /// too (tap::copy), in pieces of at most tap::CHUNK bytes and TAP_BYTES in all. False when nobody
    /// asked, and the copy is skipped.
    pub(super) tap: bool,
    pub(super) tapped: Vec<Vec<u8>>,
    /// Bytes read since the last event that did not fit in `tapped` (counted as one dropped event).
    pub(super) tap_cut: bool,
}

// The read buffer, one for all sessions, never zeroed again (zeroing 16 KB per read showed in profiles);
// freed while every screen is frozen.
thread_local!(static BUF: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) });

/// Frees the read buffer (every screen is frozen and quiet).
pub(super) fn release_read_buffer() {
    BUF.take();
}

impl Generational for Sess {
    fn generation(&self) -> u32 {
        self.generation
    }
}

impl Named for Slots<Sess> {
    fn name_at(&self, slot: usize) -> Option<&str> {
        self.get(slot).map(|s| &*s.name)
    }
    fn slots(&self) -> usize {
        self.capacity()
    }
}

impl Sess {
    /// A session for a program just started (or taken over) at `now`: its screen live, cooked or as
    /// `term` says.
    pub(super) fn new(s: Started, now: u64) -> Self {
        let (app_cursor, bracketed_paste) = s.term.modes();
        Sess {
            generation: s.generation,
            name: s.name,
            pid: s.pid,
            stdin: s.stdin,
            out: s.out,
            last_output_ms: now,
            last_read_ms: 0,
            hold: freeze_after_ms() as u16,
            due: false,
            burst: false,
            app_cursor,
            bracketed_paste,
            tty: s.term.tty(),
            created: s.created,
            rows: s.size.0,
            cols: s.size.1,
            base: s.base,
            extra: None,
            term: Screenful::Live(Box::new(s.term)),
            record: None,
        }
    }

    /// Its output's fd, while open: what its read interest names.
    #[inline]
    pub(super) fn out_fd(&self) -> Option<RawFd> {
        self.out.as_ref().map(|f| f.as_raw_fd())
    }

    /// Whether `ident` (an event's) is its input's fd.
    pub(super) fn is_stdin(&self, ident: RawFd) -> bool {
        self.stdin.as_ref().is_some_and(|f| f.as_raw_fd() == ident)
    }

    /// The screen as a Term, thawed if it was frozen.
    pub(super) fn live(&mut self) -> &mut Term {
        if let Screenful::Frozen(_) = self.term {
            let Screenful::Frozen(f) = std::mem::replace(&mut self.term, Screenful::Moving) else { unreachable!() };
            self.term = Screenful::Live(Box::new(Term::thaw(f)));
        }
        match &mut self.term {
            Screenful::Live(t) => t,
            _ => unreachable!(),
        }
    }

    pub(super) fn freeze(&mut self) {
        if let Screenful::Live(_) = self.term {
            let Screenful::Live(t) = std::mem::replace(&mut self.term, Screenful::Moving) else { unreachable!() };
            self.term = Screenful::Frozen(t.freeze());
        }
    }

    pub(super) fn frozen(&self) -> bool {
        matches!(self.term, Screenful::Frozen(_))
    }

    /// The extra state, made if it was not there.
    pub(super) fn extra(&mut self) -> &mut Extra {
        self.extra.get_or_insert_with(Default::default)
    }

    /// Reads everything waiting in the output pipe into the screen, at `now`.
    pub(super) fn drain(&mut self, stats: &mut Stats, now: u64, poller: &impl Poller) {
        if let Some(echo) = self.extra.as_mut().map(|x| std::mem::take(&mut x.echo)).filter(|e| !e.is_empty()) {
            self.take(&echo, now, poller);
            self.trim_extra();
        }
        if self.out.is_none() {
            return;
        }
        self.due = false;
        BUF.with_borrow_mut(|buf| {
            if buf.is_empty() {
                buf.resize(16 * 1024, 0);
            }
            self.drain_into(buf, stats, now, poller)
        })
    }

    fn drain_into(&mut self, buf: &mut [u8], stats: &mut Stats, now: u64, poller: &impl Poller) {
        stats.drains += 1;
        let n = match self.read_out(buf, stats) {
            Some(0) => return self.close_out(),
            Some(n) => n,
            None => return,
        };
        if n < buf.len() {
            // Less than asked for: the pipe is empty now. Edge-triggered kqueue will report the next write.
            // A burst lasts until a read finds little: ending it at the first read short of the buffer
            // let a fast writer fill the pipe and wait a whole coalesce interval, again and again.
            if n < BURST_END {
                self.burst = false;
            }
            if stats.tap {
                super::tap::copy(stats, &buf[..n]);
            }
            return self.take(&buf[..n], now, poller);
        }
        // The pipe was full: a program printing a lot. All it has for us is read before any of it is
        // parsed, so it is not held back by the parsing (cat of 5 MB: done in ~2 ms instead of 17).
        // Freed right after: it is as big as the burst.
        self.burst = true;
        let mut big = buf.to_vec();
        let ended = self.slurp(&mut big, stats);
        if stats.tap {
            super::tap::copy(stats, &big);
        }
        self.take(&big, now, poller);
        if ended {
            self.close_out();
        }
    }

    /// One read of the output pipe into `buf`: Some(0) at its end, None when nothing is waiting.
    fn read_out(&mut self, buf: &mut [u8], stats: &mut Stats) -> Option<usize> {
        let n = fd::read(self.out.as_ref()?.as_fd(), buf);
        count(stats, n)
    }

    /// Reads into `big` until the program stops printing (SLURP_QUIET_MS) or `big` holds SLURP_MAX;
    /// true once the program closed its output.
    fn slurp(&mut self, big: &mut Vec<u8>, stats: &mut Stats) -> bool {
        let max = if stats.slurp_max == 0 { SLURP_MAX } else { stats.slurp_max };
        while big.len() < max {
            big.reserve(SLURP_STEP);
            let room = max - big.len();
            let spare = big.spare_capacity_mut();
            let want = spare.len().min(room);
            let Some(out) = self.out.as_ref() else { return true };
            // Into the spare capacity as it is, never zeroed: read(2) writes the bytes before they count.
            let n = count(stats, fd::read_uninit(out.as_fd(), &mut spare[..want]));
            match n {
                Some(0) => return true,
                // SAFETY: read_uninit initialized the first `n` (<= `want`) bytes of the spare capacity,
                // which begins at `big.len()`; `n` <= `want` <= capacity - len, so the new length fits.
                Some(n) => unsafe { big.set_len(big.len() + n) },
                // Empty for now: a program printing this much is likely between two writes. A short
                // wait on this one pipe, only past SLURP_WAIT_FROM (claude's redraws never get there).
                None if big.len() >= SLURP_WAIT_FROM => {
                    if !self.out.as_ref().is_some_and(|out| fd::poll_readable(out.as_fd(), SLURP_QUIET_MS)) {
                        return false;
                    }
                }
                None => return false,
            }
        }
        false
    }

    /// Output read from the program at `now`: into the screen, and to whoever records or watches it.
    fn take(&mut self, bytes: &[u8], now: u64, poller: &impl Poller) {
        if let Some(r) = self.record.as_mut() {
            r.write(bytes);
        }
        if let Some(x) = self.extra.as_mut().filter(|x| x.pipe.is_some())
            && !x.pipe.as_mut().unwrap().send(poller, bytes)
        {
            x.pipe = None;
        }
        if let Some(x) = self.extra.as_mut().filter(|x| !x.viewers.is_empty()) {
            x.forward.extend_from_slice(bytes);
        }
        if self.frozen() {
            self.hold = hold_after(now - self.last_output_ms, freeze_after_ms());
        }
        self.last_output_ms = now;
        let t = self.live();
        let alternate = t.view().alternate();
        t.process(bytes);
        if let (Some(x), Screenful::Live(t)) = (self.extra.as_mut(), &self.term)
            && x.waits.iter().any(|w| w.text.is_some() && !w.seen)
        {
            // Only the rows this read wrote: the rest were searched when they were written. A switch
            // of screens shows rows written before, so the whole screen then.
            let all = t.view().alternate() != alternate;
            search(t.view(), all, &mut x.waits);
        }
        let t = self.live();
        // Caught up: what this read unpacked is packed again, so a busy screen holds unpacked only the
        // rows being parsed (it held tens of KB while claude redrew a long answer).
        t.pack_live();
        let ((a, b), tty) = (t.modes(), t.tty());
        self.app_cursor = a;
        self.bracketed_paste = b;
        self.tty = tty;
    }

    fn close_out(&mut self) {
        self.out = None
    }

    /// Writes to the program's input, queueing what the socket cannot take now (it is written when the
    /// socket turns writable: `flush`).
    pub(super) fn write(&mut self, poller: &impl Poller, udata: usize, bytes: &[u8]) -> Result<(), CmdError> {
        let Some(stdin) = self.stdin.as_ref() else { return Err(CmdError::new("write: input closed\n")) };
        if let Some(x) = self.extra.as_mut().filter(|x| !x.pending.is_empty()) {
            x.pending.extend_from_slice(bytes);
            return Ok(());
        }
        let raw = stdin.as_raw_fd();
        let done = fd::write(stdin.as_fd(), bytes).map_err(|e| CmdError::new(format!("write: {e}\n")))?;
        if done < bytes.len() {
            self.extra().pending.extend_from_slice(&bytes[done..]);
            poller.apply(&[Change::new(raw, Interest::WriteOnce, udata)]).map_err(|e| CmdError::new(format!("write: {e}\n")))?;
        }
        Ok(())
    }

    /// Writes what waited for the program's input (it turned writable); closes the input after it when
    /// C-d asked for that.
    pub(super) fn flush(&mut self, poller: &impl Poller, udata: usize) {
        let pending = self.extra.as_mut().map(|x| std::mem::take(&mut x.pending)).unwrap_or_default();
        // Fails only once the program closed its input: what waited has nowhere to go.
        let _ = self.write(poller, udata, &pending);
        if self.extra.as_ref().is_some_and(|x| x.eof && x.pending.is_empty()) {
            self.extra().eof = false;
            self.close_input();
        }
        self.trim_extra();
    }

    /// End of input for the program (C-d on an empty line): shutting the socket's input is for good,
    /// where a tty's EOF is one read's. Programs that stop at it (cat, a shell) are alike. Its output
    /// still flows: `out` is the same socket.
    fn close_input(&mut self) {
        if self.extra.as_ref().is_some_and(|x| !x.pending.is_empty()) {
            self.extra().eof = true;
            return;
        }
        if let Some(stdin) = self.stdin.take() {
            fd::shut_input(stdin.as_fd());
        }
    }

    /// Keys for the program, through the line discipline a PTY would apply (see super::ldisc); true when
    /// there is echo to draw, with a read.
    pub(super) fn input(&mut self, poller: &impl Poller, programs: &impl Programs, udata: usize, keys: &[u8]) -> Result<bool, CmdError> {
        let tty = self.tty;
        // A line typed while canonical goes to the program once it is not (raw mode), as a tty hands its
        // line over when the mode changes; here with the next keys.
        if tty & TTY_ICANON == 0 {
            self.hand_line_over(poller, udata)?;
        }
        if tty == 0 {
            return self.write(poller, udata, keys).map(|()| false);
        }
        let typed = ldisc::feed(tty, &mut self.extra().line, keys);
        // Keys before a signal reach the program before it; those after, after.
        let mut from = 0;
        for (at, sig) in typed.signals {
            self.write(poller, udata, &typed.out[from..at])?;
            from = at;
            programs.signal_group(self.pid, sig);
        }
        self.write(poller, udata, &typed.out[from..])?;
        if typed.eof {
            self.close_input();
        }
        let echoed = !typed.echo.is_empty();
        if echoed {
            self.extra().echo.extend_from_slice(&typed.echo);
        }
        self.trim_extra();
        Ok(echoed)
    }

    /// Writes the line typed while the tty was canonical, if any, to the program.
    pub(super) fn hand_line_over(&mut self, poller: &impl Poller, udata: usize) -> Result<(), CmdError> {
        match self.extra.as_mut().map(|x| std::mem::take(&mut x.line.text)).filter(|l| !l.is_empty()) {
            Some(line) => self.write(poller, udata, &line),
            None => Ok(()),
        }
    }

    /// Terminals are attached to it.
    pub(super) fn viewed(&self) -> bool {
        self.extra.as_ref().is_some_and(|x| !x.viewers.is_empty())
    }

    /// wait-panes wait on it.
    pub(super) fn waited(&self) -> bool {
        self.extra.as_ref().is_some_and(|x| !x.waits.is_empty())
    }

    /// Drops the box once it holds nothing.
    pub(super) fn trim_extra(&mut self) {
        if self.extra.as_ref().is_some_and(|x| x.is_empty()) {
            self.extra = None;
        }
    }

    /// Closes its terminal (the program sees EOF on its input, EPIPE or SIGPIPE on its output).
    pub(super) fn close(&mut self) {
        self.stdin = None;
        self.out = None;
    }
}

/// Counts one read of `n` bytes (None: nothing was waiting) in `stats`.
#[inline]
fn count(stats: &mut Stats, n: Option<usize>) -> Option<usize> {
    stats.reads += 1;
    if let Some(n) = n {
        stats.read_bytes += n as u64;
    }
    n
}

/// Marks the watches whose text is on `screen`'s changed rows (every row with `all`) as seen.
fn search(screen: &crate::term::Screen, all: bool, watches: &mut [Watch]) {
    render::visit_text(screen, all, &mut |line| {
        for w in watches.iter_mut() {
            if let Some(t) = &w.text {
                w.seen |= line.contains(&**t);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::backend::fake::FakePoller;
    use super::{Sess, Started, Watch};
    use crate::term::Term;
    use std::os::fd::{AsFd as _, AsRawFd as _, OwnedFd};

    fn session(out: Option<OwnedFd>) -> Sess {
        Sess::new(
            Started { generation: 1, name: "t".into(), pid: 0, stdin: None, out, size: (4, 20), base: (4, 20), created: 0, term: Term::new(4, 20, 10) },
            0,
        )
    }

    /// Output read goes to the screen and is forwarded to viewers; wait-panes see their text.
    #[test]
    fn output_reaches_screen_viewers_and_waits() {
        let (r, w) = crate::sys::fd::terminal_channel().unwrap();
        crate::sys::fd::set_nonblocking(r.as_raw_fd()).unwrap();
        let mut s = session(Some(r));
        let x = s.extra();
        x.viewers.push(9);
        x.waits.push(Watch { id: 1, text: Some("needle".into()), seen: false });
        assert_eq!(crate::sys::fd::write(w.as_fd(), b"hay\r\nneedle\r\n\x1b[?2004h").unwrap(), 21);
        let mut stats = super::Stats::default();
        let poller = FakePoller::default();
        s.drain(&mut stats, 0, &poller);
        let x = s.extra.as_ref().unwrap();
        assert_eq!(x.forward, b"hay\r\nneedle\r\n\x1b[?2004h");
        assert!(x.waits[0].seen);
        assert!(s.bracketed_paste, "modes mirrored");
        assert_eq!((stats.drains, stats.read_bytes), (1, 21));
        {
            // No process starts meanwhile: one would hold `w` open until it execs (fd::no_spawns).
            let _no_spawns = crate::sys::fd::no_spawns();
            drop(w);
            s.drain(&mut stats, 0, &poller);
        }
        assert!(s.out.is_none(), "closed at the end of output");
        s.freeze();
        assert!(s.frozen());
        let text = s.term.read(|t| super::super::render::capture(t.view(), &Default::default()).unwrap());
        assert_eq!(text, "hay\nneedle\n\n\n");
        assert!(s.frozen(), "read without thawing");
    }

    #[test]
    fn extra_is_dropped_once_empty() {
        let mut s = session(None);
        s.extra().viewers.push(1);
        s.trim_extra();
        assert!(s.extra.is_some());
        s.extra().viewers.clear();
        s.trim_extra();
        assert!(s.extra.is_none());
        assert_eq!(s.write(&FakePoller::default(), 0, b"x").unwrap_err().message(), "write: input closed\n");
    }
}
