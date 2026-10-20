//! wait-pane: waits that last, which a host answers when they are over (crate::server holds the client
//! meanwhile). The engine keeps each wait's text with its session (`Watch`) and marks it seen as rows
//! with it are written, so waiting polls nothing.

use super::Engine;
use super::args::parse;
use super::backend::{Backend, Clock as _};
use super::render::Capture;
use super::reply::{CmdError, Reply, fail, ok};
use super::session::Watch;
use super::table::SessKey;

/// A wait-pane in progress (Engine::wait_start).
///
/// One comes from [`Engine::wait_start`] and goes back with [`Engine::wait_end`]:
///
/// ```
/// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
/// let mut e = zeropty::engine::Engine::new(Default::default());
/// e.command(&argv(&["new-session", "-d", "-s", "doc-wait-struct", "exec cat"]), "/");
/// let w = e.wait_start(&argv(&["-t", "=doc-wait-struct", "-q", "60000"])).map_err(|r| r.err).unwrap();
/// assert!(e.wait_check(&w).is_none()); // not quiet for a minute yet
/// e.wait_end(&w);
/// e.command(&argv(&["kill-server"]), "/");
/// ```
#[derive(Debug)]
pub struct Wait {
    key: SessKey,
    id: u32,
    quiet: Option<u64>,
    deadline: Option<u64>,
}

impl<B: Backend> Engine<B> {
    /// `wait-pane -t target [-m text] [-q ms] [-T ms]` (argv without the command): waits until `text` has
    /// been on the screen (since the wait began, or already there) and/or the session printed nothing for
    /// `ms`, both when both are given; -T gives up. A host that waits this way polls nothing: the session's
    /// output is read as it comes and only the rows each read changes are searched.
    ///
    /// # Example
    ///
    /// A host loop on [`crate::embed::Polled`] waiting for a line, with no polling of the screen:
    ///
    /// ```
    /// use zeropty::{embed::Polled, engine::Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut zp = Polled::start(Options::default());
    /// // The session stays alive after its line (exec cat), so the line is seen before the session ends.
    /// let r = zp.command(&argv(&["new-session", "-d", "-s", "doc-wait", "sh -c 'sleep 0.2; echo ready; exec cat'"]), "/");
    /// assert_eq!(r.code, 0, "{}", r.err);
    ///
    /// // wait-pane -t =doc-wait -m ready -T 5000, without the command's name.
    /// let wait = zp.engine().wait_start(&argv(&["-t", "=doc-wait", "-m", "ready", "-T", "5000"])).map_err(|r| r.err).unwrap();
    /// let reply = loop {
    ///     if let Some(reply) = zp.engine().wait_check(&wait) {
    ///         break reply;
    ///     }
    ///     let mut p = libc::pollfd { fd: zp.fd(), events: libc::POLLIN, revents: 0 };
    ///     unsafe { libc::poll(&mut p, 1, zp.timeout_ms().min(100) as i32) };
    ///     zp.process();
    /// };
    /// zp.engine().wait_end(&wait);
    /// assert_eq!((reply.code, reply.err.as_str()), (0, ""));
    ///
    /// // A wait needs -m or -q; the Err is the reply the CLI would print.
    /// let err = zp.engine().wait_start(&argv(&["-t", "=doc-wait"])).err().unwrap();
    /// assert_eq!(err.err, "wait-pane: wait for what? (-m text, -q ms)\n");
    /// zp.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn wait_start(&mut self, argv: &[String]) -> Result<Wait, Reply> {
        let a = parse("wait-pane", argv, "t:q:m:T:")?;
        let slot = self.find(a.get('t'))?;
        let ms = |c: char| a.get(c).map(|v| v.parse::<u64>().map_err(|_| CmdError::new(format!("wait-pane: -{c} takes milliseconds\n")))).transpose();
        let (quiet, timeout) = (ms('q')?, ms('T')?);
        let text: Option<Box<str>> = a.get('m').map(Box::from);
        if quiet.is_none() && text.is_none() {
            return Err(CmdError::new("wait-pane: wait for what? (-m text, -q ms)\n").into());
        }
        // Output already printed counts.
        self.drain_now(slot);
        let seen = text.as_ref().is_some_and(|t| self.render(slot, &Capture::default()).unwrap_or_default().contains(&**t));
        let id = self.next_wait;
        self.next_wait = self.next_wait.wrapping_add(1);
        let key = self.key(slot);
        self.sess(slot).extra().waits.push(Watch { id, text, seen });
        Ok(Wait { key, id, quiet, deadline: timeout.map(|t| self.clock.now_ms() + t) })
    }

    /// A wait from wait_start is over (answered, or its client left).
    ///
    /// # Example
    ///
    /// Giving up on a wait nobody needs any more (its client hung up):
    ///
    /// ```
    /// use zeropty::{embed::Polled, engine::Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut zp = Polled::start(Options::default());
    /// zp.command(&argv(&["new-session", "-d", "-s", "doc-wait-end", "exec cat"]), "/");
    /// let wait = zp.engine().wait_start(&argv(&["-t", "=doc-wait-end", "-m", "never printed"])).map_err(|r| r.err).unwrap();
    /// assert!(zp.engine().wait_check(&wait).is_none()); // still waiting, and no -T: it never times out
    /// zp.engine().wait_end(&wait);
    /// zp.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn wait_end(&mut self, w: &Wait) {
        if let Some(s) = self.slots.find_mut(w.key) {
            if let Some(x) = s.extra.as_mut() {
                x.waits.retain(|x| x.id != w.id);
            }
            s.trim_extra();
        }
    }

    /// wait-pane's reply once its wait is over; None while it goes on.
    ///
    /// # Example
    ///
    /// A wait that times out (`-T`), checked without the engine reading anything:
    ///
    /// ```
    /// use zeropty::{embed::Polled, engine::Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut zp = Polled::start(Options::default());
    /// zp.command(&argv(&["new-session", "-d", "-s", "doc-wait-check", "exec cat"]), "/");
    /// let wait = zp.engine().wait_start(&argv(&["-t", "=doc-wait-check", "-m", "never printed", "-T", "50"])).map_err(|r| r.err).unwrap();
    /// assert!(zp.engine().wait_check(&wait).is_none());
    /// std::thread::sleep(std::time::Duration::from_millis(80));
    /// let reply = zp.engine().wait_check(&wait).unwrap();
    /// assert_eq!((reply.code, reply.err.as_str()), (1, "wait-pane: timed out\n"));
    /// zp.engine().wait_end(&wait);
    /// zp.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn wait_check(&self, w: &Wait) -> Option<Reply> {
        let now = self.clock.now_ms();
        let Some(s) = self.slots.find(w.key) else {
            return Some(fail("wait-pane: session ended\n"));
        };
        let seen = s.extra.as_ref().and_then(|x| x.waits.iter().find(|x| x.id == w.id)).is_none_or(|x| x.text.is_none() || x.seen);
        // Output waiting for its read is output: not quiet.
        let quiet = w.quiet.is_none_or(|q| !s.due && now >= s.last_output_ms + q);
        if seen && quiet {
            return Some(ok(""));
        }
        if w.deadline.is_some_and(|d| now >= d) {
            return Some(fail("wait-pane: timed out\n"));
        }
        None
    }

    /// Milliseconds until `w` may be over without the session printing (quiet long enough, or timed out).
    ///
    /// ```
    /// use zeropty::{embed::Polled, engine::Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut zp = Polled::start(Options::default());
    /// zp.command(&argv(&["new-session", "-d", "-s", "doc-due", "exec cat"]), "/");
    /// let timed = zp.engine().wait_start(&argv(&["-t", "=doc-due", "-m", "x", "-T", "60000"])).map_err(|r| r.err).unwrap();
    /// assert!(zp.engine().wait_due_ms(&timed).is_some_and(|ms| ms > 50_000 && ms <= 60_000));
    /// let untimed = zp.engine().wait_start(&argv(&["-t", "=doc-due", "-m", "x"])).map_err(|r| r.err).unwrap();
    /// assert_eq!(zp.engine().wait_due_ms(&untimed), None);
    /// zp.engine().wait_end(&timed);
    /// zp.engine().wait_end(&untimed);
    /// zp.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn wait_due_ms(&self, w: &Wait) -> Option<u64> {
        let s = self.slots.get(w.key.slot)?;
        let quiet = w.quiet.map(|q| s.last_output_ms + q);
        [quiet, w.deadline].into_iter().flatten().min().map(|at| at.saturating_sub(self.clock.now_ms()))
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{engine, readable, session};
    use crate::engine::{Clock as _, fail, ok};
    use std::io::Write as _;

    fn argv(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn waits_end_on_text_on_quiet_or_at_their_deadline() {
        let mut e = engine();
        let (key, mut prog) = session(&mut e, "a");
        assert_eq!(e.wait_start(&argv(&["-t", "a"])).unwrap_err(), fail("wait-pane: wait for what? (-m text, -q ms)\n"));
        assert_eq!(e.wait_start(&argv(&["-t", "a", "-q", "soon"])).unwrap_err(), fail("wait-pane: -q takes milliseconds\n"));
        assert_eq!(e.wait_start(&argv(&["-t", "b", "-q", "5"])).unwrap_err(), fail("can't find session: b\n"));

        // Text: the session is read as it prints (no coalescing while waited on).
        let w = e.wait_start(&argv(&["-t", "a", "-m", "ready", "-T", "1000"])).unwrap();
        assert_eq!(e.wait_check(&w), None);
        assert_eq!(e.wait_due_ms(&w), Some(1000), "only the deadline: text is not a timer");
        prog.write_all(b"ready\r\n").unwrap();
        e.handle(&[readable(&e, key)], &mut Vec::new());
        assert_eq!(e.next_timer_ms(), 1, "read at the next millisecond");
        e.clock.advance(1);
        e.tick();
        assert_eq!(e.wait_check(&w), Some(ok("")));
        e.wait_end(&w);
        assert!(e.slots.get(key.slot).unwrap().extra.is_none(), "nothing kept once it is over");

        // Text already on the screen counts.
        let w = e.wait_start(&argv(&["-t", "a", "-m", "ready"])).unwrap();
        assert_eq!(e.wait_check(&w), Some(ok("")));
        e.wait_end(&w);

        // Quiet: no output for the time given, output waiting for its read included.
        let w = e.wait_start(&argv(&["-t", "a", "-q", "500"])).unwrap();
        assert_eq!(e.wait_check(&w), None);
        assert_eq!(e.wait_due_ms(&w), Some(500), "the last output was just now");
        e.clock.advance(499);
        assert_eq!(e.wait_check(&w), None);
        e.clock.advance(1);
        assert_eq!(e.wait_check(&w), Some(ok("")));
        e.wait_end(&w);

        // A deadline, then the session ending.
        let w = e.wait_start(&argv(&["-t", "a", "-m", "never", "-T", "100"])).unwrap();
        e.clock.advance(100);
        assert_eq!(e.wait_check(&w), Some(fail("wait-pane: timed out\n")));
        e.wait_end(&w);
        let w = e.wait_start(&argv(&["-t", "a", "-m", "never"])).unwrap();
        e.remove(key.slot);
        assert_eq!(e.wait_check(&w), Some(fail("wait-pane: session ended\n")));
        let _ = e.clock.now_ms();
    }
}
