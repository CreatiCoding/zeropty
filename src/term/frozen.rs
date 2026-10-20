//! A whole `Term` as bytes: `Frozen` (freeze/thaw, while its session is quiet; deepened into the cold
//! store once quiet for long), and `export`/`import`, the same screen carried to another process.
//!
//! # Formats
//!
//! - The screen (`save_screen`): rows u16, cols u16, scrollback limit (var), modes u8, pen and saved pen
//!   (`Attrs::encode`), the palette as attribute sets, the main grid then the alternate one
//!   (`Grid::save_to`), then the tty modes u8 (absent from an older zeropty's: read as 0).
//! - Deep (in the cold store, see `cold`): `field(screen)`, then the scrollback as a count and a field per
//!   row.
//! - Portable (`export`): PORTABLE, the combined characters the cells use as (id, text) pairs, `field
//!   (screen)`, then the scrollback as for deep. Ids are this process's interner's; `import` maps them to
//!   the importer's by their text.
//!
//! A parser caught mid-sequence stays with a frozen screen (in `FrozenRest`) but is not exported: the
//! rest of that sequence then prints as text on the other side.

use super::Term;
use super::attrs::Attrs;
use super::cell::{CHAR_MASK, Cell, EXT, intern, interned};
use super::codec::{In, Out};
use super::cold;
use super::palette::Palette;
use super::screen::Screen;
use std::collections::{HashMap, VecDeque};

/// Starts Term::export's bytes (and names their version).
const PORTABLE: &[u8] = b"AMS1";

/// A Term packed into bytes while its session is quiet: about 1 KB for a claude screen instead of tens.
///
/// ```
/// use zeropty::term::Term;
///
/// let mut t = Term::new(24, 80, 0);
/// t.process(b"idle");
/// let f = t.freeze(); // a quiet session's screen, while it stays quiet
/// assert!(f.bytes() < 1024);
/// let mut t = Term::thaw(f); // its next output or capture
/// assert_eq!(&*t.screen().row(0).cells()[0].contents(), "i");
/// ```
pub struct Frozen {
    data: Stored,
    /// Only when there is scrollback or a parser caught mid-sequence (most quiet screens have neither).
    rest: Option<Box<FrozenRest>>,
}

enum Stored {
    /// The packed screen.
    Plain(Box<[u8]>),
    /// After `deepen`: an entry in the cold store holding the screen and its scrollback (deep format).
    /// About half the bytes, for screens that stay quiet; unpacking then costs a decompression. Its size
    /// is kept beside it, for the blank screen it becomes if the store cannot give it back.
    Cold { id: u32, rows: u16, cols: u16 },
}

#[derive(Default)]
struct FrozenRest {
    /// Already packed row by row; moved, not copied, so freezing often stays cheap with a long history.
    scrollback: VecDeque<Box<[u8]>>,
    /// Kept only when the parser was caught mid-sequence (it could not be shown to be in its ground state).
    parser: Option<Box<vte::Parser>>,
}

impl Drop for Frozen {
    fn drop(&mut self) {
        if let Stored::Cold { id, .. } = self.data {
            cold::remove(id);
        }
    }
}

/// What a deep screen the cold store could not give back becomes: blank, at its size, no scrollback (the
/// store said why on stderr).
fn lost(rows: u16, cols: u16) -> (Box<[u8]>, VecDeque<Box<[u8]>>) {
    (Term::new(rows, cols, 0).save_screen().into_boxed_slice(), VecDeque::new())
}

/// Writes `rows` as a count and a field per row.
fn put_rows<'a>(o: &mut Out, rows: impl ExactSizeIterator<Item = &'a Box<[u8]>>) {
    o.var(rows.len());
    for row in rows {
        o.field(row);
    }
}

/// Reads rows written by `put_rows`.
fn take_rows(i: &mut In) -> VecDeque<Box<[u8]>> {
    (0..i.var()).map(|_| Box::from(i.field())).collect()
}

/// The screen and scrollback of a deep frame.
fn split_deep(raw: &[u8]) -> (Box<[u8]>, VecDeque<Box<[u8]>>) {
    let mut i = In::new(raw);
    let screen = Box::from(i.field());
    (screen, take_rows(&mut i))
}

impl Frozen {
    /// The packed screen (tests and measurements); empty once deep.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(24, 80, 0);
    /// t.process(b"x");
    /// let mut f = t.freeze();
    /// assert!(!f.data().is_empty());
    /// f.deepen();
    /// assert!(f.data().is_empty()); // compressed into the cold store
    /// ```
    pub fn data(&self) -> &[u8] {
        match &self.data {
            Stored::Plain(b) => b,
            Stored::Cold { .. } => &[],
        }
    }

    /// Bytes it holds: its packed screen (compressed, once deep), scrollback and kept parser.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(24, 80, 0);
    /// t.process(b"a quiet screen");
    /// let f = t.freeze();
    /// assert!(f.bytes() > 0 && f.bytes() < 1024, "{}", f.bytes());
    /// ```
    pub fn bytes(&self) -> usize {
        let data = match &self.data {
            Stored::Plain(b) => b.len(),
            Stored::Cold { id, .. } => cold::len(*id),
        };
        data + self
            .rest
            .as_ref()
            .map_or(0, |r| r.scrollback.iter().map(|x| x.len() + 16).sum::<usize>() + r.parser.as_ref().map_or(0, |_| std::mem::size_of::<vte::Parser>()))
    }

    /// Whether `deepen` moved it to the cold store.
    ///
    /// ```
    /// let f = zeropty::term::Term::new(24, 80, 0).freeze();
    /// assert!(!f.is_deep()); // until deepen
    /// ```
    pub fn is_deep(&self) -> bool {
        matches!(self.data, Stored::Cold { .. })
    }

    /// Compresses the screen and its scrollback, into the process's cold store. Left as it was if
    /// compression fails.
    ///
    /// ```
    /// use zeropty::term::Term;
    ///
    /// let mut t = Term::new(24, 80, 100);
    /// t.process(b"a quiet screen");
    /// let mut f = t.freeze();
    /// f.deepen();
    /// assert!(f.is_deep());
    /// let mut copy = f.peek(); // a copy to read; `f` stays compressed
    /// assert_eq!(&*copy.screen().row(0).get(0).unwrap().contents(), "a");
    /// ```
    pub fn deepen(&mut self) {
        let Stored::Plain(screen) = &self.data else { return };
        let scrollback = self.rest.as_mut().map(|r| std::mem::take(&mut r.scrollback)).unwrap_or_default();
        let mut o = Out(Vec::with_capacity(screen.len() + 64));
        o.field(screen);
        put_rows(&mut o, scrollback.iter());
        // The screen's format starts with rows and cols (save_screen).
        let (rows, cols) = (u16::from_le_bytes([screen[0], screen[1]]), u16::from_le_bytes([screen[2], screen[3]]));
        let Some(id) = cold::stash(&o.0) else {
            if let Some(r) = self.rest.as_mut() {
                r.scrollback = scrollback
            }
            return;
        };
        self.data = Stored::Cold { id, rows, cols };
        if self.rest.as_ref().is_some_and(|r| r.parser.is_none()) {
            self.rest = None;
        }
    }

    /// A copy of the frozen screen to read, leaving this one as it is (frozen and, if deep, compressed).
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(2, 10, 0);
    /// t.process(b"frozen");
    /// let f = t.freeze();
    /// let mut copy = f.peek();
    /// assert_eq!(&*copy.screen().row(0).cells()[0].contents(), "f");
    /// assert!(!f.data().is_empty()); // still frozen
    /// ```
    pub fn peek(&self) -> Term {
        let copy = match &self.data {
            Stored::Plain(b) => {
                let rest = self.rest.as_ref().map(|r| Box::new(FrozenRest { scrollback: r.scrollback.clone(), parser: None }));
                Frozen { data: Stored::Plain(b.clone()), rest }
            }
            &Stored::Cold { id, rows, cols } => {
                let (screen, scrollback) = cold::fetch(id).map_or_else(|| lost(rows, cols), |raw| split_deep(&raw));
                let rest = (!scrollback.is_empty()).then(|| Box::new(FrozenRest { scrollback, parser: None }));
                Frozen { data: Stored::Plain(screen), rest }
            }
        };
        Term::thaw(copy)
    }

    /// Back out of the cold store: the screen plain again, the scrollback in `rest`.
    fn undeepen(&mut self) {
        let Stored::Cold { id, rows, cols } = self.data else { return };
        let (screen, scrollback) = cold::fetch_and_free(id).map_or_else(|| lost(rows, cols), |raw| split_deep(&raw));
        self.data = Stored::Plain(screen);
        if !scrollback.is_empty() {
            self.rest.get_or_insert_with(Default::default).scrollback = scrollback;
        }
    }
}

impl Term {
    /// Frozen's data: size, modes, attributes, the palette as attribute sets, then both grids.
    fn save_screen(&self) -> Vec<u8> {
        let s = &self.screen;
        let mut o = Out(Vec::with_capacity(1024));
        o.u16(s.grid.rows_n);
        o.u16(s.grid.cols);
        o.var(s.grid.scrollback_len);
        o.u8(s.modes);
        s.attrs.encode(&mut o);
        s.saved_attrs.encode(&mut o);
        o.var(s.palette.len());
        for i in 0..s.palette.len() {
            Attrs::from_key(s.palette.key(i)).encode(&mut o);
        }
        s.grid.save_to(&mut o);
        s.alt.save_to(&mut o);
        // Last, so a screen from an zeropty before it reads as none (Term::thaw).
        o.u8(s.tty);
        o.0
    }

    /// The screen packed into bytes; `thaw` gives it back as it was, parser state included.
    ///
    /// # Example
    ///
    /// What the engine does to a session that went quiet, and to it again when it prints:
    ///
    /// ```
    /// use zeropty::term::{Screen, Term};
    ///
    /// let text = |s: &Screen, r: u16| s.row(r).cells().iter().map(|c| c.contents().to_string()).collect::<String>();
    /// let mut t = Term::new(24, 80, 100);
    /// t.process(b"\x1b[1mbuild\x1b[0m done");
    /// let frozen = t.freeze();
    /// assert!(frozen.bytes() < 1024);
    ///
    /// let mut t = Term::thaw(frozen); // exactly as it was, parser state included
    /// t.process(b"!");
    /// let screen = t.screen();
    /// assert_eq!(text(screen, 0), "build done!");
    /// assert!(screen.attrs_of(&screen.row(0).cells()[0]).bold());
    /// ```
    pub fn freeze(mut self) -> Frozen {
        let scrollback = std::mem::take(&mut self.screen.grid.scrollback);
        let data = self.save_screen();
        let fresh = self.ground && !self.utf8_pending;
        let parser = if fresh { None } else { self.parser };
        let rest = (!scrollback.is_empty() || parser.is_some()).then(|| Box::new(FrozenRest { scrollback, parser }));
        Frozen { data: Stored::Plain(data.into_boxed_slice()), rest }
    }

    /// The screen as bytes another process can take in (Term::import): what freezing keeps, the
    /// scrollback, and the text of the combined characters its cells hold, as their ids are this
    /// process's own. A parser caught mid-sequence is not kept: the rest of that sequence prints as text.
    ///
    /// # Example
    ///
    /// ```
    /// use zeropty::term::{Screen, Term};
    ///
    /// let text = |s: &Screen, r: u16| s.row(r).cells().iter().map(|c| c.contents().to_string()).collect::<String>();
    /// let mut t = Term::new(3, 20, 100);
    /// t.process("hello 👋🏽".as_bytes());
    /// let bytes = t.export(); // what goes over the socket in a handover
    ///
    /// let mut copy = Term::import(&bytes).expect("a screen");
    /// assert_eq!(text(copy.screen(), 0), "hello 👋🏽");
    /// assert!(Term::import(b"not a screen").is_none()); // bad bytes fail the call, not the process
    /// ```
    pub fn export(&self) -> Vec<u8> {
        let mut ids = std::collections::BTreeSet::new();
        let mut note = |cells: &[Cell], _: bool| {
            for c in cells {
                let ch = c.ch;
                if ch & EXT != 0 {
                    ids.insert(ch & CHAR_MASK);
                }
            }
        };
        self.screen.grid.visit_rows(&mut note);
        self.screen.alt.visit_rows(&mut note);
        self.screen.grid.visit_history(usize::MAX, &mut note);
        let mut o = Out(PORTABLE.to_vec());
        o.var(ids.len());
        for id in ids {
            o.var(id as usize);
            o.field(interned(id).as_bytes());
        }
        o.field(&self.save_screen());
        put_rows(&mut o, self.screen.grid.scrollback.iter());
        o.0
    }

    /// A screen from Term::export, made at home here; None when the bytes are not one. They come from
    /// another process: bytes that do not parse fail this call, not the process.
    ///
    /// ```
    /// use zeropty::term::Term;
    ///
    /// let bytes = Term::new(2, 10, 0).export();
    /// assert_eq!(Term::import(&bytes).unwrap().view().size(), (2, 10));
    /// assert!(Term::import(&bytes[..3]).is_none()); // cut short
    /// ```
    pub fn import(b: &[u8]) -> Option<Self> {
        std::panic::catch_unwind(|| Self::import_unchecked(b)).ok().flatten()
    }

    fn import_unchecked(b: &[u8]) -> Option<Self> {
        let mut i = In::new(b.strip_prefix(PORTABLE)?);
        let mut map = HashMap::new();
        for _ in 0..i.var() {
            let id = i.var() as u32;
            let text = std::str::from_utf8(i.field()).ok()?;
            map.insert(id, intern(text)?);
        }
        let data = Box::from(i.field());
        let scrollback = take_rows(&mut i);
        let rest = (!scrollback.is_empty()).then(|| Box::new(FrozenRest { scrollback, parser: None }));
        let mut t = Term::thaw(Frozen { data: Stored::Plain(data), rest });
        if !map.is_empty() {
            let mut f = |id: u32| map.get(&id).copied();
            t.screen.grid.map_ext(&mut f);
            t.screen.alt.map_ext(&mut f);
        }
        t.pack_live();
        Some(t)
    }

    /// The screen `f` froze, unpacked enough to take output again (rows stay packed until written).
    ///
    /// ```
    /// use zeropty::term::Term;
    ///
    /// let mut t = Term::new(2, 10, 100);
    /// t.process(b"kept");
    /// let mut t = Term::thaw(t.freeze());
    /// assert_eq!(&*t.screen().row(0).cells()[0].contents(), "k");
    /// t.process(b" going"); // and it goes on where it was
    /// assert_eq!(t.view().cursor().col, 10);
    /// ```
    pub fn thaw(mut f: Frozen) -> Self {
        f.undeepen();
        let Stored::Plain(data) = std::mem::replace(&mut f.data, Stored::Plain(Box::default())) else { unreachable!() };
        let mut i = In::new(&data);
        let (rows, cols) = (i.u16(), i.u16());
        let mut s = Screen::bare(rows, cols, i.var());
        s.modes = i.u8();
        s.attrs = Attrs::decode(&mut i);
        s.saved_attrs = Attrs::decode(&mut i);
        let n = i.var();
        s.palette = Palette::from_keys((0..n).map(|_| Attrs::decode(&mut i).key()).collect());
        s.cur = s.palette.id(s.attrs).unwrap_or(0);
        s.grid.restore(&mut i);
        s.alt.restore(&mut i);
        s.tty = if i.at_end() { 0 } else { i.u8() };
        let rest = f.rest.take().map(|r| *r).unwrap_or_default();
        s.grid.scrollback = rest.scrollback;
        // A kept parser was mid-sequence or mid-character; it goes byte by byte until it is back in ground.
        let ground = rest.parser.is_none();
        Self { parser: rest.parser, screen: s, ground, utf8_pending: false }
    }
}

#[cfg(test)]
mod tests {
    use super::super::codec::In;
    use super::super::{Cell, Term};

    /// Everything a reader sees: scrollback and screen rows (text, attributes, wrap), cursor, modes.
    fn dump(t: &Term) -> String {
        let s = t.view();
        let mut out = String::new();
        let mut row = |cells: &[Cell], wrapped: bool| {
            for c in cells {
                out.push_str(&format!("{}{:?}|", &*c.contents(), s.attrs_of(c)));
            }
            out.push_str(if wrapped { "+\n" } else { "\n" });
        };
        s.visit_history(usize::MAX, &mut row);
        s.visit_rows(&mut row);
        format!("{out}{:?} {} {} {}", s.cursor(), s.application_cursor(), s.bracketed_paste(), s.hide_cursor())
    }

    /// A screen with a bit of everything: colors, wide and combined characters, scrollback, wrapped rows.
    fn busy() -> Term {
        let mut t = Term::new(8, 30, 50);
        t.set_onlcr(true);
        let lines = "\x1b[31mred\x1b[0m e\u{301}\u{302} 한글 x\u{20dd} \x1b[38;5;208;48;2;1;2;3mpen\x1b[0m end of a long line that wraps\n".repeat(12);
        t.process(lines.as_bytes());
        t
    }

    /// Freezing, deepening and thawing give back the same screen, which goes on the same.
    #[test]
    fn freeze_deepen_thaw_round_trip() {
        let mut t = busy();
        let before = dump(&t);
        let mut f = Term::thaw(t.freeze()).freeze();
        assert_eq!(dump(&f.peek()), before, "peek of a plain frozen screen");
        f.deepen();
        assert!(f.is_deep() && f.data().is_empty());
        assert_eq!(dump(&f.peek()), before, "peek of a deep one leaves it deep");
        assert!(f.is_deep());
        t = Term::thaw(f);
        assert_eq!(dump(&t), before);
        let mut fresh = busy();
        t.process(b"more\n\x1b[1mbold");
        fresh.process(b"more\n\x1b[1mbold");
        assert_eq!(dump(&t), dump(&fresh));
    }

    /// A parser caught mid-sequence is kept frozen: the sequence finishes after the thaw.
    #[test]
    fn freeze_keeps_a_parser_mid_sequence() {
        let mut t = Term::new(3, 10, 0);
        t.process(b"\x1b]0;tit");
        let mut t = Term::thaw(t.freeze());
        t.process(b"le\x07ok");
        let mut s = String::new();
        t.view().visit_rows(&mut |cells, _| s.extend(cells.iter().map(|c| c.contents().to_string())));
        assert_eq!(s, "ok", "the OSC did not print");
    }

    /// What goes through export and import reads the same and goes on the same: colors, wide and combined
    /// characters, scrollback, the alternate screen, scroll region, saved cursor and the current pen.
    #[test]
    fn export_import_round_trip() {
        let mut t = busy();
        t.process(b"\x1b[?1h\x1b[?2004h\x1b[2;6r\x1b[3;4H\x1b7\x1b[1;4m");
        let mut copy = Term::import(&t.export()).expect("imports");
        assert_eq!(dump(&copy), dump(&t));
        let more = "after\n\x1b8saved \u{1100}\u{1161} a\u{301}\n\x1b[?1049halt\x1b[?1049lback".as_bytes();
        t.process(more);
        copy.process(more);
        assert_eq!(dump(&copy), dump(&t), "goes on the same");
        // And from the alternate screen.
        t.process(b"\x1b[?1049h\x1b[32mon alt e\xcc\x81");
        let copy = Term::import(&t.export()).expect("imports");
        assert_eq!(dump(&copy), dump(&t), "on the alternate screen");
        assert!(Term::import(b"not a screen").is_none());
        assert!(Term::import(&t.export()[..40]).is_none(), "cut short");
    }

    /// Combined characters are carried by their text: a cell shows what the record's table says its id
    /// is, not what the id means in this process.
    #[test]
    fn import_maps_combined_characters_by_text() {
        let mut t = Term::new(3, 10, 10);
        t.process("a\u{301} b\u{302}\nplain".as_bytes());
        let mut bytes = t.export();
        // The table: [count] then ([id][len][text])*; swap the texts of the first two entries.
        let mut i = In::new(&bytes[4..]);
        assert_eq!(i.var(), 2);
        let mut entries = vec![];
        for _ in 0..2 {
            let _id = i.var();
            let len = i.var();
            entries.push((i.at, len));
            i.at += len;
        }
        let texts: Vec<Vec<u8>> = entries.iter().map(|&(at, len)| bytes[4 + at..4 + at + len].to_vec()).collect();
        assert_eq!(texts[0].len(), texts[1].len(), "same length, so they swap in place");
        let (at0, at1) = (4 + entries[0].0, 4 + entries[1].0);
        bytes[at0..at0 + texts[0].len()].copy_from_slice(&texts[1]);
        bytes[at1..at1 + texts[1].len()].copy_from_slice(&texts[0]);
        let copy = Term::import(&bytes).expect("imports");
        let first = |t: &Term| {
            let mut v = vec![];
            t.view().visit_rows(&mut |cells, _| v.push(cells.iter().map(|c| c.contents().to_string()).collect::<Vec<_>>()));
            v
        };
        let (was, now) = (first(&t), first(&copy));
        assert_eq!((now[0][0].as_str(), now[0][2].as_str()), (was[0][2].as_str(), was[0][0].as_str()), "swapped: {now:?}");
        assert_eq!(now[1], was[1], "untouched cells stay");
    }
}
