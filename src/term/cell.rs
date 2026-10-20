//! The 6-byte `Cell`, the process-wide interner for multi-code-point clusters, and the per-thread pool
//! of cell buffers rows unpack into.
//!
//! A cell's `ch` is a code point in its low 21 bits (CHAR_MASK), or with EXT an interned cluster id
//! there; WIDE marks the first half of a double-width character, WIDE_CONT its second half (whose code
//! point is 0). No cell has both WIDE and WIDE_CONT: the packed row format uses that combination as a tag.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};
use unicode_width::UnicodeWidthChar as _;

pub(super) const CHAR_MASK: u32 = 0x001F_FFFF;
pub(super) const EXT: u32 = 1 << 29;
pub(super) const WIDE_CONT: u32 = 1 << 30;
pub(super) const WIDE: u32 = 1 << 31;
/// vt100 stops appending combining characters once a cell holds this many bytes.
const MAX_CLUSTER: usize = 18;

struct Interner {
    strs: Vec<&'static str>,
    ids: HashMap<&'static str, u32>,
}

fn interner() -> &'static RwLock<Interner> {
    static I: OnceLock<RwLock<Interner>> = OnceLock::new();
    I.get_or_init(|| RwLock::new(Interner { strs: vec![], ids: HashMap::new() }))
}

/// The id of cluster `s`, interned if new. None once the id space (21 bits) is used up; the append is
/// then dropped.
pub(super) fn intern(s: &str) -> Option<u32> {
    if let Some(&id) = interner().read().unwrap().ids.get(s) {
        return Some(id);
    }
    let mut i = interner().write().unwrap();
    if let Some(&id) = i.ids.get(s) {
        return Some(id);
    }
    let id = u32::try_from(i.strs.len()).ok().filter(|&id| id <= CHAR_MASK)?;
    // Leaked on purpose: the set of distinct clusters is small and lives as long as the process.
    let s: &'static str = Box::leak(s.to_owned().into_boxed_str());
    i.strs.push(s);
    i.ids.insert(s, id);
    Some(id)
}

/// The cluster `id` stands for.
pub(super) fn interned(id: u32) -> &'static str {
    interner().read().unwrap().strs[id as usize]
}

/// 6 bytes: a code point (or, with EXT, an interned cluster id) plus wide flags, and a palette index.
///
/// ```
/// let mut t = zeropty::term::Term::new(1, 10, 0);
/// t.process(b"ok");
/// let cell = t.screen().row(0).cells()[0];
/// assert_eq!(std::mem::size_of_val(&cell), 6);
/// assert_eq!(&*cell.contents(), "o");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C, packed)]
pub struct Cell {
    pub(super) ch: u32,
    pub(super) attr: u16,
}
const _: () = assert!(std::mem::size_of::<Cell>() == 6);

/// An unwritten cell: no text, default attributes. Rows store cells only up to their last non-blank one.
pub(super) const BLANK: Cell = Cell { ch: 0, attr: 0 };

/// The second half of a wide character as `Screen::text` leaves it.
pub(super) const CONT: Cell = Cell { ch: WIDE_CONT, attr: 0 };

/// Cell buffers of released rows, for the next rows unpacked (32: 96 kept ~100 KB more for no CPU gain). Rows are unpacked only while a session's
/// output is being parsed (they are packed again right after), so a few buffers serve every session
/// instead of each one growing and freeing its own.
const POOL_MAX: usize = 32;
thread_local!(static POOL: std::cell::RefCell<Vec<Vec<Cell>>> = const { std::cell::RefCell::new(Vec::new()) });

/// An empty cell buffer, reusing one a released row gave back when there is one.
pub(super) fn take_cells() -> Vec<Cell> {
    POOL.with_borrow_mut(|p| p.pop()).unwrap_or_default()
}

/// Gives a row's cell buffer back for the next row unpacked on this thread.
pub(super) fn give_cells(mut v: Vec<Cell>) {
    if v.capacity() == 0 {
        return;
    }
    v.clear();
    POOL.with_borrow_mut(|p| {
        if p.len() < POOL_MAX {
            p.push(v)
        }
    });
}

/// Frees this thread's pooled cell buffers (see crate::term::release_scratch).
pub(super) fn release_pool() {
    POOL.take();
}

/// A cell's text: one char inline, or an interned cluster.
///
/// ```
/// use zeropty::term::{Term, Text};
///
/// let mut t = Term::new(1, 10, 0);
/// t.process("ae\u{301}".as_bytes()); // e and a combining acute accent
/// let cells = t.screen().row(0).cells();
/// assert!(matches!(cells[0].contents(), Text::Char(..)));
/// assert!(matches!(cells[1].contents(), Text::Ext("e\u{301}"))); // two code points, interned
/// ```
pub enum Text {
    /// One character as UTF-8: its bytes and how many of them are used.
    Char([u8; 4], u8),
    /// A cluster of several code points, from the interner.
    Ext(&'static str),
}

impl std::ops::Deref for Text {
    type Target = str;
    fn deref(&self) -> &str {
        match self {
            // SAFETY: contents() wrote these bytes with char::encode_utf8. Checking them again was 7% of a
            // capture.
            Text::Char(b, n) => unsafe { std::str::from_utf8_unchecked(&b[..usize::from(*n)]) },
            Text::Ext(s) => s,
        }
    }
}

impl Cell {
    /// Writes `c` alone in palette entry `attr`, WIDE when it takes two columns.
    pub(super) fn set(&mut self, c: char, attr: u16) {
        self.ch = u32::from(c) | if c.width().unwrap_or(1) > 1 { WIDE } else { 0 };
        self.attr = attr;
    }

    /// Adds a combining character to the cell's cluster (vt100's append).
    pub(super) fn append(&mut self, c: char) {
        let mut s = String::from(&*self.contents());
        if s.len() >= MAX_CLUSTER {
            return;
        }
        if s.is_empty() {
            s.push(' ');
        }
        s.push(c);
        if let Some(id) = intern(&s) {
            self.ch = (self.ch & (WIDE | WIDE_CONT)) | EXT | id;
        }
    }

    pub(super) fn clear(&mut self, attr: u16) {
        // vt100 keeps the length and the wide flags in one byte and zeroes it, so the flags go too.
        self.ch = 0;
        self.attr = attr;
    }

    /// The cell's text ("" when unwritten).
    ///
    /// ```
    /// use zeropty::term::Term;
    ///
    /// let mut t = Term::new(2, 10, 0);
    /// t.process("a한e\u{301}".as_bytes()); // e and a combining acute accent
    /// let cells = t.screen().row(0).cells();
    /// assert_eq!(&*cells[0].contents(), "a");
    /// assert_eq!(&*cells[1].contents(), "한"); // double width: the next cell is its second half
    /// assert!(cells[1].is_wide() && cells[2].is_wide_continuation());
    /// assert_eq!(&*cells[3].contents(), "e\u{301}"); // a cluster of two code points, in one cell
    /// ```
    pub fn contents(&self) -> Text {
        let ch = self.ch;
        if ch & EXT != 0 {
            return Text::Ext(interned(ch & CHAR_MASK));
        }
        let mut b = [0u8; 4];
        let n = char::from_u32(ch & CHAR_MASK).filter(|&c| c != '\0').map_or(0, |c| c.encode_utf8(&mut b).len());
        Text::Char(b, n as u8)
    }
    /// The character, when it is printable ASCII other than space (most of a screen): no flags, one byte.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 0);
    /// t.process("a 한".as_bytes());
    /// let cells = t.screen().row(0).cells();
    /// assert_eq!(cells[0].ascii(), Some(b'a'));
    /// assert_eq!(cells[1].ascii(), None); // a space
    /// assert_eq!(cells[2].ascii(), None); // not ASCII
    /// ```
    pub fn ascii(&self) -> Option<u8> {
        let ch = self.ch;
        (0x21..0x7f).contains(&ch).then_some(ch as u8)
    }
    /// Whether anything was written to it (a space counts).
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 0);
    /// t.process(b" x");
    /// let row = t.screen().row(0);
    /// let (space, never) = (row.get(0).unwrap(), row.get(5).unwrap());
    /// assert!(space.has_contents() && space.is_blank());
    /// assert!(!never.has_contents() && never.is_blank());
    /// ```
    pub fn has_contents(&self) -> bool {
        self.ch & (CHAR_MASK | EXT) != 0
    }
    /// Empty or a space: what a capture prints as a blank.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 0);
    /// t.process(b"a b");
    /// let blanks: Vec<bool> = t.screen().row(0).cells().iter().map(|c| c.is_blank()).collect();
    /// assert_eq!(blanks, [false, true, false]);
    /// ```
    pub fn is_blank(&self) -> bool {
        self.ch == 0 || self.ch == 0x20
    }
    /// The first half of a double-width character.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 0);
    /// t.process("한a".as_bytes());
    /// let cells = t.screen().row(0).cells();
    /// assert!(cells[0].is_wide() && !cells[2].is_wide());
    /// assert_eq!(t.view().cursor().col, 3); // two columns, then one
    /// ```
    pub fn is_wide(&self) -> bool {
        self.ch & WIDE != 0
    }
    /// The second half of a double-width character (no text of its own).
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 0);
    /// t.process("漢".as_bytes());
    /// let cells = t.screen().row(0).cells();
    /// assert!(cells[1].is_wide_continuation());
    /// assert_eq!(&*cells[1].contents(), ""); // its text is in cells[0]
    /// ```
    pub fn is_wide_continuation(&self) -> bool {
        self.ch & WIDE_CONT != 0
    }
    pub(super) fn set_wide_continuation(&mut self, on: bool) {
        if on { self.ch |= WIDE_CONT } else { self.ch &= !WIDE_CONT }
    }
    /// Index into the screen's palette; see `Screen::attrs_of`.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 0);
    /// t.process(b"a\x1b[1mb");
    /// let screen = t.screen();
    /// let cells = screen.row(0).cells();
    /// assert_ne!(cells[0].attr(), cells[1].attr()); // two attribute sets, two palette entries
    /// assert!(screen.attrs_of(&cells[1]).bold());
    /// ```
    pub fn attr(&self) -> u16 {
        self.attr
    }
}

#[cfg(test)]
mod tests {
    use super::{BLANK, CONT};

    #[test]
    fn text_flags_and_clusters() {
        let mut c = BLANK;
        assert!(c.is_blank() && !c.has_contents() && c.ascii().is_none());
        assert_eq!(&*c.contents(), "");
        c.set('x', 3);
        assert_eq!((c.ascii(), c.attr(), c.is_wide()), (Some(b'x'), 3, false));
        c.set('한', 0);
        assert!(c.is_wide() && c.ascii().is_none());
        assert_eq!(&*c.contents(), "한");
        c.append('\u{302}');
        assert!(c.is_wide(), "a cluster keeps its width");
        assert_eq!(&*c.contents(), "한\u{302}");
        let mut e = BLANK;
        e.append('\u{301}');
        assert_eq!(&*e.contents(), " \u{301}", "on an empty cell, as vt100: after a space");
        assert!(CONT.is_wide_continuation() && !CONT.has_contents());
        c.clear(5);
        assert_eq!((c.is_wide(), c.has_contents(), c.attr()), (false, false, 5), "clear drops the flags too");
    }

    #[test]
    fn clusters_stop_growing() {
        let mut c = BLANK;
        c.set('e', 0);
        for _ in 0..20 {
            c.append('\u{301}');
        }
        assert!(c.contents().len() <= super::MAX_CLUSTER + 2, "{}", c.contents().len());
        let _copy = c;
    }
}
