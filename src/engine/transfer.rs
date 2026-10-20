//! Sessions leaving and joining an engine, their programs left running (super::handover moves them
//! between processes): a session as a record (super::record) plus its open fds.

use super::Engine;
use super::args::session_name;
use super::backend::{Backend, Clock as _};
use super::record::SessionRecord;
use super::session::{Extra, Sess, Started};
use crate::sys::fd;
use crate::sys::time::unix_now;
use crate::term::Term;
use std::os::fd::{AsFd as _, AsRawFd as _, BorrowedFd, OwnedFd};

impl<B: Backend> Engine<B> {
    /// Every session's name.
    ///
    /// ```
    /// use zeropty::engine::{Engine, Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut e = Engine::new(Options::default());
    /// for name in ["doc-a", "doc-b"] {
    ///     e.command(&argv(&["new-session", "-d", "-s", name, "exec cat"]), "/");
    /// }
    /// let mut names = e.session_names();
    /// names.sort();
    /// assert_eq!(names, ["doc-a", "doc-b"]);
    /// e.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn session_names(&self) -> Vec<String> {
        self.slots.iter().map(|s| s.name.to_string()).collect()
    }

    /// Session `target` as bytes for another engine (import_session), with the fds of its pipes that go
    /// along: its input then its output, those still open, lent for as long as the engine is not used
    /// again. Output pending is read first. The session stays here, as it was, until forget_session.
    ///
    /// # Example
    ///
    /// Moving a session between two engines in one process, its program left running. Across processes the
    /// record and the fds go over a Unix socket instead ([`crate::engine::handover`]); here the fds are
    /// duplicated, as `SCM_RIGHTS` would.
    ///
    /// ```
    /// use zeropty::engine::{Engine, Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let (mut from, mut to) = (Engine::new(Options::default()), Engine::new(Options::default()));
    /// from.command(&argv(&["new-session", "-d", "-s", "doc-move", "-x", "20", "-y", "3", "echo moved; exec cat"]), "/");
    /// // The host's loop, until the line is on screen.
    /// let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    /// while !from.command(&argv(&["capture-pane", "-p", "-t", "=doc-move:"]), "/").out.starts_with("moved") && std::time::Instant::now() < deadline {
    ///     from.try_poll(Some(50), &mut Vec::new()).unwrap();
    ///     from.tick();
    /// }
    ///
    /// let (record, fds) = from.export_session("doc-move").expect("exported");
    /// let fds: Vec<_> = fds.iter().map(|fd| fd.try_clone_to_owned().unwrap()).collect();
    /// assert_eq!(to.import_session(&record, fds).unwrap(), "doc-move");
    /// from.forget_session("doc-move"); // the program goes on, now in `to`
    ///
    /// assert!(from.session_names().is_empty());
    /// assert!(to.command(&argv(&["capture-pane", "-p", "-t", "=doc-move:"]), "/").out.starts_with("moved\n"));
    /// to.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn export_session(&mut self, target: &str) -> Option<(Vec<u8>, Vec<BorrowedFd<'_>>)> {
        let slot = self.names.find(session_name(target), &self.slots)?;
        self.drain_now(slot);
        let size_file = self.size_file_of(slot);
        let s = self.slots.get(slot).unwrap();
        let record = SessionRecord {
            name: s.name.to_string(),
            pid: s.pid,
            rows: s.rows,
            cols: s.cols,
            base: s.base,
            stdin_open: s.stdin.is_some(),
            out_open: s.out.is_some(),
            size_file,
            pending: s.extra.as_ref().map(|x| x.pending.clone()).unwrap_or_default(),
            screen: s.term.read(Term::export),
            line: s.extra.as_ref().map(|x| x.line.text.clone()).unwrap_or_default(),
            created: Some(s.created),
        };
        // Lent from the session, which keeps them open until it is removed (that needs `&mut self`).
        let fds = [&s.stdin, &s.out].into_iter().flatten().map(|fd| fd.as_fd()).collect();
        Some((record.encode(), fds))
    }

    /// Lets go of session `target`, taken by another engine: its program goes on there. Terminals attached
    /// here are told it ended.
    ///
    /// ```
    /// use zeropty::engine::{Engine, Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let (mut from, mut to) = (Engine::new(Options::default()), Engine::new(Options::default()));
    /// from.command(&argv(&["new-session", "-d", "-s", "doc-forget", "exec cat"]), "/");
    /// let (record, fds) = from.export_session("doc-forget").unwrap();
    /// let fds: Vec<_> = fds.iter().map(|fd| fd.try_clone_to_owned().unwrap()).collect();
    /// to.import_session(&record, fds).unwrap();
    /// from.forget_session("doc-forget"); // not killed: cat goes on, in `to`
    /// assert!(from.session_names().is_empty());
    /// assert_eq!(to.command(&argv(&["has-session", "-t", "=doc-forget"]), "/").code, 0);
    /// to.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn forget_session(&mut self, target: &str) {
        if let Some(slot) = self.names.find(session_name(target), &self.slots) {
            self.remove_keeping(slot, true);
        }
    }

    /// Takes in a session from export_session, with its fds (closed when it fails). Returns its name.
    ///
    /// A record that is not one is refused, and nothing is taken in (see
    /// [`export_session`](Self::export_session) for a session moved whole):
    ///
    /// ```
    /// let mut e = zeropty::engine::Engine::new(Default::default());
    /// assert!(e.import_session(b"", Vec::new()).is_err());
    /// assert!(e.session_names().is_empty());
    /// ```
    pub fn import_session(&mut self, record: &[u8], fds: Vec<OwnedFd>) -> Result<String, String> {
        let r = SessionRecord::decode(record)?;
        let term = Term::import(&r.screen).ok_or_else(|| "bad session record\n".to_string())?;
        let wanted = r.fds();
        if fds.len() != wanted {
            return Err(format!("session {}: {} fds for {wanted}\n", r.name, fds.len()));
        }
        if self.names.find(&r.name, &self.slots).is_some() {
            return Err(format!("duplicate session: {}\n", r.name));
        }
        for fd in &fds {
            fd::set_nonblocking(fd.as_raw_fd()).map_err(|e| format!("session {}: {e}\n", r.name))?;
        }
        // The session owns them from here on.
        let mut fds = fds.into_iter();
        let stdin = if r.stdin_open { fds.next() } else { None };
        let out = if r.out_open { fds.next() } else { None };
        let generation = self.next_generation();
        let mut s = Sess::new(
            Started {
                generation,
                name: r.name.as_str().into(),
                pid: r.pid,
                stdin,
                out,
                size: (r.rows, r.cols),
                base: r.base,
                created: r.created.unwrap_or_else(unix_now),
                term,
            },
            self.clock.now_ms(),
        );
        let mut x = Extra { size_file: Some(r.size_file.into_boxed_path()), ..Extra::default() };
        x.line.text = r.line;
        s.extra = Some(Box::new(x));
        let slot = self.insert(s);
        if !r.pending.is_empty() {
            let tag = self.key(slot).udata();
            let s = self.slots.get_mut(slot).expect("just inserted");
            // A program that closed its input gets none of it, as it would not have there.
            let _ = s.write(&self.poller, tag, &r.pending);
        }
        // Output already waiting is reported at once.
        if !self.watch_session(slot) {
            // Ended on the way: the session ends here, as it would have there.
            self.remove(slot);
        }
        Ok(r.name)
    }
}
