//! Attached terminals ("viewers"): a terminal attached to a session gets its screen, then everything it
//! prints, and sends it keys and its size. The engine only queues what each viewer is owed
//! (`viewer_events`); the host (crate::server) carries it to the terminal.

use super::backend::Backend;
use super::reply::Reply;
use super::table::SessKey;
use super::{Engine, render};

/// Something for an attached terminal.
///
/// They come from [`Engine::viewer_events`]; [`Engine::attach`] has the whole loop.
///
/// ```
/// use zeropty::engine::ViewerEvent;
///
/// fn pass_on(ev: ViewerEvent, terminal: &mut Vec<u8>) -> bool {
///     match ev {
///         ViewerEvent::Output(bytes) => { terminal.extend_from_slice(&bytes); true }
///         ViewerEvent::Ended => false, // detach the terminal
///     }
/// }
/// let mut t = Vec::new();
/// assert!(pass_on(ViewerEvent::Output(b"hi".to_vec()), &mut t) && t == b"hi");
/// assert!(!pass_on(ViewerEvent::Ended, &mut t));
/// ```
#[derive(Debug, PartialEq, Eq)]
pub enum ViewerEvent {
    /// What its session printed, to write to the terminal as is.
    Output(Vec<u8>),
    /// The session ended; the viewer is gone.
    Ended,
}

impl<B: Backend> Engine<B> {
    /// Attaches a terminal to session `target`: returns its viewer id and what to write to the terminal
    /// to show the screen as it is. With `size` (rows, cols), the session takes that size (its program
    /// gets SIGWINCH), as tmux sizes a window to its latest client; it gets its own back when the last
    /// terminal detaches.
    ///
    /// # Example
    ///
    /// A terminal of the host's own (here a byte buffer): the screen as it is, then the keys it types
    /// and what the session prints, through [`viewer_input`](Self::viewer_input) and
    /// [`viewer_events`](Self::viewer_events).
    ///
    /// ```
    /// use zeropty::engine::{Engine, Options, ViewerEvent};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut e = Engine::new(Options::default());
    /// e.command(&argv(&["new-session", "-d", "-s", "doc-attach", "exec cat"]), "/");
    ///
    /// let (id, redraw) = e.attach("=doc-attach", Some((10, 40))).map_err(|r| r.err).unwrap();
    /// let mut terminal = redraw; // bytes to write to the terminal as they are
    /// e.viewer_input(id, b"hi\r").unwrap(); // typed keys: cat prints the line back
    ///
    /// let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    /// while terminal.windows(2).filter(|w| w == b"hi").count() < 2 && std::time::Instant::now() < deadline {
    ///     e.try_poll(Some(50), &mut Vec::new()).unwrap();
    ///     e.tick();
    ///     for (viewer, ev) in e.viewer_events() {
    ///         if let (true, ViewerEvent::Output(bytes)) = (viewer == id, ev) {
    ///             terminal.extend_from_slice(&bytes);
    ///         }
    ///     }
    /// }
    /// assert_eq!(terminal.windows(2).filter(|w| w == b"hi").count(), 2, "{:?}", String::from_utf8_lossy(&terminal));
    ///
    /// e.detach(id); // the session gets its own size back
    /// let r = e.command(&argv(&["display-message", "-p", "-t", "=doc-attach:", "#{window_width}"]), "/");
    /// assert_eq!(r.out, "80\n");
    /// e.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn attach(&mut self, target: &str, size: Option<(u16, u16)>) -> Result<(u32, Vec<u8>), Reply> {
        let slot = self.find(Some(target))?;
        self.drain_now(slot);
        if let Some((rows, cols)) = size {
            self.resize_session(slot, rows, cols);
        }
        let id = self.next_viewer;
        self.next_viewer = self.next_viewer.wrapping_add(1).max(1);
        let key = self.key(slot);
        self.sess(slot).extra().viewers.push(id);
        self.viewers.insert(id, key);
        Ok((id, self.redraw(slot)))
    }

    /// Detaches a terminal; the last one to leave gives the session its own size back.
    ///
    /// ```
    /// use zeropty::engine::{Engine, Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut e = Engine::new(Options::default());
    /// e.command(&argv(&["new-session", "-d", "-s", "doc-detach", "-x", "80", "-y", "24", "exec cat"]), "/");
    /// let (id, _) = e.attach("=doc-detach", Some((10, 40))).map_err(|r| r.err).unwrap();
    /// let size = |e: &mut Engine| e.command(&argv(&["display-message", "-p", "-t", "=doc-detach:", "#{window_width}x#{window_height}"]), "/").out;
    /// assert_eq!(size(&mut e), "40x10\n");
    /// e.detach(id);
    /// assert_eq!(size(&mut e), "80x24\n");
    /// e.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn detach(&mut self, id: u32) {
        let Some(slot) = self.viewer_slot(id) else { return };
        self.viewers.remove(&id);
        let s = self.sess(slot);
        if let Some(x) = s.extra.as_mut() {
            x.viewers.retain(|&v| v != id);
            if x.viewers.is_empty() {
                x.forward = Vec::new();
            }
        }
        if !s.viewed() {
            s.trim_extra();
            let (rows, cols) = s.base;
            self.resize_session(slot, rows, cols);
        }
    }

    /// Keys from an attached terminal, for its session's program.
    ///
    /// ```
    /// use zeropty::engine::{Engine, Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut e = Engine::new(Options::default());
    /// e.command(&argv(&["new-session", "-d", "-s", "doc-input", "exec cat"]), "/");
    /// let (id, _) = e.attach("=doc-input", None).map_err(|r| r.err).unwrap();
    /// assert_eq!(e.viewer_input(id, b"typed\r"), Ok(()));
    /// e.detach(id);
    /// assert_eq!(e.viewer_input(id, b"late"), Err("detached\n".to_string()));
    /// e.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn viewer_input(&mut self, id: u32, bytes: &[u8]) -> Result<(), String> {
        let slot = self.viewer_slot(id).ok_or("detached\n")?;
        self.modes_now(slot);
        self.type_keys(slot, bytes).map_err(String::from)
    }

    /// An attached terminal changed size: so does its session.
    ///
    /// ```
    /// use zeropty::engine::{Engine, Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut e = Engine::new(Options::default());
    /// e.command(&argv(&["new-session", "-d", "-s", "doc-resize", "exec cat"]), "/");
    /// let (id, _) = e.attach("=doc-resize", Some((24, 80))).map_err(|r| r.err).unwrap();
    /// e.viewer_resize(id, 30, 100); // the terminal window was made bigger
    /// let r = e.command(&argv(&["display-message", "-p", "-t", "=doc-resize:", "#{window_width}x#{window_height}"]), "/");
    /// assert_eq!(r.out, "100x30\n");
    /// e.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn viewer_resize(&mut self, id: u32, rows: u16, cols: u16) {
        if let Some(slot) = self.viewer_slot(id) {
            self.resize_session(slot, rows, cols);
        }
    }

    /// The whole screen again, for a terminal that fell too far behind to be sent the backlog.
    ///
    /// ```
    /// use zeropty::engine::{Engine, Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut e = Engine::new(Options::default());
    /// e.command(&argv(&["new-session", "-d", "-s", "doc-redraw", "exec cat"]), "/");
    /// let (id, first) = e.attach("=doc-redraw", None).map_err(|r| r.err).unwrap();
    /// assert_eq!(e.viewer_redraw(id), Some(first)); // nothing printed since: the same screen
    /// e.detach(id);
    /// assert_eq!(e.viewer_redraw(id), None);
    /// e.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn viewer_redraw(&mut self, id: u32) -> Option<Vec<u8>> {
        let slot = self.viewer_slot(id)?;
        Some(self.redraw(slot))
    }

    /// What to pass on to attached terminals since the last call, in order.
    ///
    /// ```
    /// use zeropty::engine::{Engine, Options, ViewerEvent};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut e = Engine::new(Options::default());
    /// e.command(&argv(&["new-session", "-d", "-s", "doc-events", "exec cat"]), "/");
    /// let (id, _) = e.attach("=doc-events", None).map_err(|r| r.err).unwrap();
    /// e.command(&argv(&["kill-session", "-t", "=doc-events"]), "/");
    /// let ended = e.viewer_events().into_iter().any(|(v, ev)| v == id && matches!(ev, ViewerEvent::Ended));
    /// assert!(ended);
    /// ```
    pub fn viewer_events(&mut self) -> Vec<(u32, ViewerEvent)> {
        std::mem::take(&mut self.viewer_events)
    }

    /// Whether output or an ending waits for an attached terminal (viewer_events).
    ///
    /// ```
    /// let e = zeropty::engine::Engine::new(Default::default());
    /// assert!(!e.has_viewer_events()); // nobody attached
    /// ```
    pub fn has_viewer_events(&self) -> bool {
        !self.viewer_events.is_empty()
    }

    /// The slot of viewer `id`'s session, while it lasts.
    fn viewer_slot(&self, id: u32) -> Option<usize> {
        let &key: &SessKey = self.viewers.get(&id)?;
        self.slots.find(key).map(|_| key.slot)
    }

    /// Hands output read for a session's viewers to them.
    pub(super) fn pass_on(&mut self, slot: usize) {
        let Some(s) = self.slots.get_mut(slot) else { return };
        let Some(x) = s.extra.as_mut().filter(|x| !x.forward.is_empty()) else { return };
        let bytes = std::mem::take(&mut x.forward);
        let (last, others) = x.viewers.split_last().expect("forward is only filled while viewed");
        for id in others {
            self.viewer_events.push((*id, ViewerEvent::Output(bytes.clone())));
        }
        self.viewer_events.push((*last, ViewerEvent::Output(bytes)));
    }

    /// Bytes that draw a session's screen on a terminal of its size (see render::redraw).
    fn redraw(&self, slot: usize) -> Vec<u8> {
        self.slots.get(slot).expect("a viewed session").term.read(|t| render::redraw(t.view()))
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{engine, readable, session};
    use super::ViewerEvent;
    use crate::engine::Clock as _;
    use std::io::Write as _;

    #[test]
    fn viewers_get_the_screen_then_the_output_then_the_end() {
        let mut e = engine();
        let (key, mut prog) = session(&mut e, "a");
        prog.write_all(b"before").unwrap();
        let (id, screen) = e.attach("a", Some((6, 30))).unwrap();
        assert!(String::from_utf8_lossy(&screen).contains("before"), "output pending is read first");
        assert_eq!((e.slots.get(key.slot).unwrap().rows, e.slots.get(key.slot).unwrap().cols), (6, 30), "takes the terminal's size");
        assert!(e.attach("b", None).is_err());
        // Output read goes to the viewer as it is, at once (no coalescing while viewed).
        prog.write_all(b"after").unwrap();
        e.handle(&[readable(&e, key)], &mut Vec::new());
        e.clock.advance(1);
        e.tick();
        assert!(e.has_viewer_events());
        match e.viewer_events().as_slice() {
            [(v, ViewerEvent::Output(bytes))] => assert_eq!((*v, bytes.as_slice()), (id, &b"after"[..])),
            other => panic!("{} events", other.len()),
        }
        // A second viewer; the first leaves, the size stays until the last one does.
        let (id2, _) = e.attach("a", Some((10, 40))).unwrap();
        e.detach(id);
        assert_eq!(e.slots.get(key.slot).unwrap().rows, 10);
        e.detach(id2);
        assert_eq!((e.slots.get(key.slot).unwrap().rows, e.slots.get(key.slot).unwrap().cols), (4, 20), "its own size back");
        assert!(e.slots.get(key.slot).unwrap().extra.is_none());
        // The session ends with a viewer attached: it is told.
        let (id3, _) = e.attach("a", None).unwrap();
        e.remove(key.slot);
        assert!(matches!(e.viewer_events().as_slice(), [(v, ViewerEvent::Ended)] if *v == id3));
        assert_eq!(e.viewer_input(id3, b"x"), Err("detached\n".into()));
        assert_eq!(e.viewer_redraw(id3), None);
        let _ = e.clock.now_ms();
    }
}
