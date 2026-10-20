//! A compact terminal emulator: the screen semantics of the vt100 crate (0.16, MIT, by Jesse Luehrs), ported
//! line for line so captures match it, with storage built for holding a thousand idle screens at once.
//!
//! - Cell is 6 bytes instead of 32: a code point and a palette index for its attributes. Clusters of more
//!   than one code point (emoji sequences, combining marks) go to a process-wide interner, which stays
//!   small because clusters repeat. Halving the cell size halved the CPU under load: with hundreds of
//!   screens, writing cells is cache misses.
//! - Rows hold cells only up to the last one that differs from a blank cell. Unwritten space costs nothing.
//! - Scrollback rows, and whole screens after a quiet spell, are packed: about 2 bytes per ASCII cell,
//!   with runs of identical cells collapsed.
//!
//! tests/diff.rs replays recorded output through this and vt100 and requires identical screens.
//!
//! # Layers
//!
//! Each module depends only on the ones above it in this list; nothing here does I/O except the cold
//! store's backing file.
//!
//! | module    | responsibility |
//! |-----------|----------------|
//! | `text`    | UTF-8 decoding and character widths, the parser's fast paths' view of bytes |
//! | `codec`   | LEB128 and fixed-width little-endian fields (`Out`/`In`) shared by every packed format |
//! | `attrs`   | `Color` and `Attrs`, and their one-word key |
//! | `palette` | a screen's attribute sets, interned into 16-bit ids cells carry |
//! | `cell`    | the 6-byte `Cell`, the cluster interner and the per-thread pool of cell buffers |
//! | `row`     | `Row` and its packed form (the codec scrollback and frozen screens are made of) |
//! | `grid`    | `Grid`: rows as slots into a blob of packed rows, cursor, scroll region, scrollback |
//! | `screen`  | `Screen`: main and alternate grid, pen, modes; the operations escape sequences map to |
//! | `csi`     | `Csi`: CSI parameters, parsed here for complete sequences or taken over from vte |
//! | `vt`      | the dispatcher: `vte::Perform` for `Screen`, control sequences to screen operations |
//! | `frozen`  | `Frozen`: a whole `Term` as bytes, `freeze`/`thaw`, and `export`/`import` across processes |
//! | `cold`    | the process-wide store of compressed frozen screens, and zstd on the Rust allocator |
//!
//! `Term` (this module) ties the byte-stream parser to its screen.
//!
//! # Invariants
//!
//! - A `Term` is in vte's ground state whenever `parser` is `None`: printable ASCII then prints directly.
//! - Between writes a busy screen keeps its rows packed (`Term::pack_live`); only the rows a write
//!   touched are unpacked while it runs.
//! - Ids of combined characters (`Cell` with EXT) are this process's interner's: anything leaving the
//!   process goes through `Term::export`, which carries their text.
//!
//! # Example
//!
//! Feed a program's output to a [`Term`] and read the screen back, cell by cell (the engine does this
//! for every session; `capture-pane` is built on the same calls):
//!
//! ```
//! use zeropty::term::{Pos, Term};
//!
//! let mut t = Term::new(5, 20, 100); // 5 rows, 20 columns, up to 100 scrollback rows
//! t.set_onlcr(true); // "\n" starts the next line at column 0, as a terminal's ONLCR does
//! t.process(b"hello\n\x1b[1mbold\x1b[0m!");
//!
//! let screen = t.screen();
//! let text = |r: u16| screen.row(r).cells().iter().map(|c| c.contents().to_string()).collect::<String>();
//! assert_eq!(text(0), "hello");
//! assert_eq!(text(1), "bold!");
//! assert_eq!(screen.cursor(), Pos { row: 1, col: 5 });
//!
//! let cells = screen.row(1).cells();
//! assert!(screen.attrs_of(&cells[0]).bold()); // SGR 1
//! assert!(!screen.attrs_of(&cells[4]).bold()); // after SGR 0
//! ```

mod attrs;
mod cell;
mod codec;
mod cold;
mod csi;
mod frozen;
mod grid;
mod palette;
mod row;
mod screen;
mod text;
mod vt;

pub use attrs::{Attrs, Color};
pub use cell::{Cell, Text};
pub use frozen::Frozen;
pub use grid::{MAX_SIZE, Pos};
pub use row::Row;
pub use screen::{Screen, TTY_COOKED, TTY_ECHO, TTY_ICANON, TTY_ICRNL, TTY_ISIG};

use csi::Csi;
use screen::{CB_GROUND, CB_NONE, MODE_ONLCR};
use text::decode_utf8;

/// Frees this thread's parsing and packing scratch (row buffers, slabs, packing buffers): for when every
/// screen is frozen, so the allocator can give their pages back (they would pin them for nothing).
///
/// ```
/// let mut t = zeropty::term::Term::new(24, 80, 0);
/// t.process(b"x");
/// let f = t.freeze();
/// zeropty::term::release_scratch(); // every screen on this thread is frozen
/// drop(f);
/// ```
pub fn release_scratch() {
    cell::release_pool();
    grid::release_scratch();
}

thread_local! {
    /// The CSI parameters of the sequence being dispatched; one per thread instead of one per screen.
    static CSI: std::cell::RefCell<Csi> = const { std::cell::RefCell::new(Csi::empty()) };
}

/// Byte-stream parser plus screen, the counterpart of vt100::Parser.
///
/// # Example
///
/// Feed a program's output to a `Term` and read the screen back:
///
/// ```
/// use zeropty::term::Term;
///
/// let mut t = Term::new(3, 20, 100);
/// t.set_onlcr(true);
/// t.process(b"$ make\nok\n$ ");
/// let screen = t.screen();
/// let rows: Vec<String> = (0..3).map(|r| screen.row(r).cells().iter().map(|c| c.contents().to_string()).collect()).collect();
/// assert_eq!(rows, ["$ make", "ok", "$ "]); // the space was written, so it is stored
/// ```
pub struct Term {
    /// Only while vte has state worth keeping (mid-sequence); most bytes never reach it, and 432 bytes per
    /// screen add up over a thousand of them.
    parser: Option<Box<vte::Parser>>,
    screen: Screen,
    /// `parser` is in its ground state (not inside an escape sequence), so printable ASCII prints.
    ground: bool,
    /// ...but holding the first bytes of a UTF-8 character, which the next byte completes or breaks.
    utf8_pending: bool,
}
impl Term {
    /// A screen of `rows` x `cols`, each kept within 1..=MAX_SIZE.
    ///
    /// ```
    /// let t = zeropty::term::Term::new(24, 80, 500); // 500 rows of scrollback at most
    /// assert_eq!(t.view().size(), (24, 80));
    /// ```
    pub fn new(rows: u16, cols: u16, scrollback: usize) -> Self {
        let (rows, cols) = (rows.clamp(1, MAX_SIZE), cols.clamp(1, MAX_SIZE));
        Self { parser: None, screen: Screen::new(rows, cols, scrollback), ground: true, utf8_pending: false }
    }

    /// Runs of printable ASCII in the ground state go straight to the screen; everything else goes through
    /// vte a byte at a time, which shows exactly when it returns to ground. Most output is the former:
    /// this halved the parse cost under load.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 0);
    /// for chunk in [&b"\x1b[1"[..], b"mh", b"\xed\x95", b"\x9c"] { // ESC[1m, "h", "한" in pieces
    ///     t.process(chunk);
    /// }
    /// let screen = t.screen();
    /// let cells = screen.row(0).cells();
    /// assert_eq!((&*cells[0].contents(), &*cells[1].contents()), ("h", "한"));
    /// assert!(screen.attrs_of(&cells[0]).bold());
    /// ```
    pub fn process(&mut self, bytes: &[u8]) {
        // pack_live parked it.
        self.screen.gm().unpark();
        // The CSI buffer is taken once per call: a thread-local access per sequence showed in profiles.
        CSI.with_borrow_mut(|csi| self.process_with(bytes, csi));
        // Readers of the scrollback see it whole (Grid::scrolled).
        self.screen.grid.pack_scrolled();
    }

    fn process_with(&mut self, bytes: &[u8], csi: &mut Csi) {
        let printable = |b: u8| (0x20..0x7f).contains(&b);
        let mut i = 0;
        while i < bytes.len() {
            if self.ground && !self.utf8_pending {
                let b = bytes[i];
                if printable(b) {
                    let start = i;
                    while i < bytes.len() && printable(bytes[i]) {
                        i += 1;
                    }
                    self.screen.print_ascii(&bytes[start..i]);
                    continue;
                }
                // C0 controls in ground: vte only executes them (ESC, CAN and SUB change state; they go below).
                if b < 0x20 && !matches!(b, 0x1b | 0x18 | 0x1a) {
                    vte::Perform::execute(&mut self.screen, b);
                    i += 1;
                    continue;
                }
                // A whole, valid UTF-8 character: what vte would print. Partial or invalid ones go to vte.
                if b >= 0x80 {
                    // A run of them (and the ASCII between) written in one go, as far as it lands on plain
                    // cells: claude's borders and Korean text are mostly this.
                    let used = self.screen.print_run(&bytes[i..]);
                    if used > 0 {
                        i += used;
                        continue;
                    }
                    if let Some((c, len)) = decode_utf8(&bytes[i..]) {
                        vte::Perform::print(&mut self.screen, c);
                        i += len;
                        continue;
                    }
                }
                // A complete CSI sequence: parsed here and dispatched as vte would, ending in ground. Over
                // half the bytes claude writes are CSI sequences.
                if b == 0x1b
                    && bytes.get(i + 1) == Some(&b'[')
                    && let Some((inter, ni, fin, len)) = Csi::parse(csi, &bytes[i..])
                {
                    self.screen.csi(csi, &inter[..ni], char::from(fin));
                    i += len;
                    continue;
                }
            }
            let b = bytes[i];
            i += 1;
            self.screen.cb = CB_NONE;
            self.parser.get_or_insert_with(|| Box::new(vte::Parser::new())).advance(&mut self.screen, &[b]);
            match b {
                // ESC starts a sequence from any state. Checked before the callback: an ESC that breaks a
                // half-received UTF-8 character first prints U+FFFD, which looks like a return to ground.
                0x1b => {
                    self.ground = false;
                    self.utf8_pending = false;
                }
                // CAN and SUB abort any sequence.
                0x18 | 0x1a => {
                    self.ground = true;
                    self.utf8_pending = false;
                }
                _ => {
                    let printed = self.screen.cb == CB_GROUND;
                    // A character printed or a sequence finished.
                    if printed {
                        self.ground = true;
                        self.utf8_pending = false;
                    }
                    // Then, in ground: a lead byte always leaves vte holding a partial character, even right
                    // after it printed U+FFFD for a broken one (or after a BEL-ended OSC we could not see end);
                    // a continuation byte does unless it completed a character. Getting this wrong let the
                    // parser be dropped with a partial character in it.
                    if self.ground && b >= 0x80 && ((0xc2..=0xf4).contains(&b) || !printed) {
                        self.utf8_pending = true;
                    }
                }
            }
        }
        if self.ground && !self.utf8_pending {
            self.parser = None;
        }
    }

    /// The screen as it is, for reading through Screen::visit_rows and Screen::visit_history (nothing
    /// unpacked into it, so it can be read while shared): what captures use.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(2, 10, 0);
    /// t.process(b"x");
    /// let screen: &zeropty::term::Screen = t.view(); // a shared borrow: nothing unpacked
    /// assert_eq!(screen.cursor().col, 1);
    /// ```
    pub fn view(&self) -> &Screen {
        &self.screen
    }

    /// The screen with every row unpacked, for reading rows one by one (Screen::row; tests).
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(2, 10, 0);
    /// t.process(b"x");
    /// t.pack_all();
    /// assert_eq!(&*t.screen().row(0).cells()[0].contents(), "x"); // unpacked again to read
    /// ```
    pub fn screen(&mut self) -> &Screen {
        self.screen.grid.load_all();
        self.screen.alt.load_all();
        &self.screen
    }

    /// Packs every row into a freshly built blob (tests and measurements; the server uses pack_live).
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(24, 80, 0);
    /// t.process(b"packed");
    /// t.pack_all();
    /// assert_eq!(&*t.screen().row(0).cells()[0].contents(), "p");
    /// ```
    pub fn pack_all(&mut self) {
        self.screen.grid.rebuild(true);
        self.screen.alt.rebuild(true);
        if self.screen.alternate() { self.screen.grid.park() } else { self.screen.alt.park() }
    }

    /// The screen at a new size (each kept within 1..=MAX_SIZE), as vt100's set_size does it.
    ///
    /// ```
    /// use zeropty::term::Term;
    ///
    /// let mut t = Term::new(24, 80, 0);
    /// t.resize(10, 40);
    /// assert_eq!(t.screen().size(), (10, 40));
    /// t.resize(0, 10_000); // kept within 1..=MAX_SIZE
    /// assert_eq!(t.screen().size(), (1, zeropty::term::MAX_SIZE));
    /// ```
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.clamp(1, MAX_SIZE), cols.clamp(1, MAX_SIZE));
        self.screen.grid.resize(rows, cols);
        self.screen.alt.resize(rows, cols);
    }

    /// Line feeds also return the carriage, as through a tty with ONLCR (the server's sessions start so).
    ///
    /// ```
    /// use zeropty::term::{Pos, Term};
    ///
    /// let mut raw = Term::new(2, 10, 0);
    /// raw.process(b"ab\ncd"); // a bare line feed keeps the column
    /// assert_eq!(raw.view().cursor(), Pos { row: 1, col: 4 });
    /// let mut tty = Term::new(2, 10, 0);
    /// tty.set_onlcr(true);
    /// tty.process(b"ab\ncd");
    /// assert_eq!(tty.view().cursor(), Pos { row: 1, col: 2 });
    /// ```
    pub fn set_onlcr(&mut self, on: bool) {
        if on { self.screen.set_mode(MODE_ONLCR) } else { self.screen.clear_mode(MODE_ONLCR) }
    }

    /// What the server does to a busy screen now and then: packs rows left unpacked (by a capture) and
    /// parks the screen not shown.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(24, 80, 0);
    /// t.process(b"busy");
    /// t.settle();
    /// assert_eq!(&*t.screen().row(0).cells()[0].contents(), "b");
    /// ```
    pub fn settle(&mut self) {
        self.pack_live();
        if self.screen.alternate() { self.screen.grid.park() } else { self.screen.alt.park() }
    }

    /// Packs every row the last write unpacked (see Grid::pack_live) and parks the screen: between reads
    /// a busy screen keeps only its packed rows, not their 8-byte slots or the copies pack_live replaced
    /// (400 busy claude sessions: about 0.9 KB each).
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(24, 80, 0);
    /// t.process(b"written");
    /// t.pack_live(); // after each read of output, as the engine does
    /// assert_eq!(&*t.screen().row(0).cells()[0].contents(), "w");
    /// ```
    pub fn pack_live(&mut self) {
        self.screen.grid.pack_live();
        self.screen.alt.pack_live();
        self.screen.grid.park();
        self.screen.alt.park();
    }

    /// Application cursor keys and bracketed paste, without unpacking anything.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 0);
    /// t.process(b"\x1b[?2004h");
    /// assert_eq!(t.modes(), (false, true)); // (application cursor, bracketed paste)
    /// ```
    pub fn modes(&self) -> (bool, bool) {
        (self.screen.application_cursor(), self.screen.bracketed_paste())
    }

    /// The tty's input modes (TTY_* flags).
    ///
    /// ```
    /// use zeropty::term::{Term, TTY_COOKED};
    ///
    /// let mut t = Term::new(1, 10, 0);
    /// t.set_tty(TTY_COOKED); // what zeropty-tty reports for a program in cooked mode
    /// assert_eq!(t.tty(), TTY_COOKED);
    /// ```
    pub fn tty(&self) -> u8 {
        self.screen.tty
    }

    /// Sets the tty's input modes (a new session's: TTY_COOKED).
    ///
    /// ```
    /// use zeropty::term::{Term, TTY_COOKED};
    ///
    /// let mut t = Term::new(1, 10, 0);
    /// t.set_tty(TTY_COOKED);
    /// t.set_tty(0); // raw, as a full-screen program sets it
    /// assert_eq!(t.tty(), 0);
    /// ```
    pub fn set_tty(&mut self, flags: u8) {
        self.screen.tty = flags
    }

    /// Bytes held for cells and scrollback; not exact allocator usage.
    ///
    /// ```
    /// let t = zeropty::term::Term::new(24, 80, 0);
    /// assert_eq!(t.approx_bytes(), t.view().approx_bytes());
    /// ```
    pub fn approx_bytes(&self) -> usize {
        self.screen.approx_bytes()
    }

    /// Where this screen's memory is, for diagnosing (the server's `mem` command).
    ///
    /// ```
    /// let t = zeropty::term::Term::new(24, 80, 0);
    /// assert!(t.mem_report().contains("scrollback"));
    /// ```
    pub fn mem_report(&self) -> String {
        let s = &self.screen;
        format!(
            "{}\n{}\npalette {} (cap {}) alt shown {} parser {}\n",
            s.grid.mem_report("main"),
            s.alt.mem_report("alt"),
            s.palette.len(),
            s.palette.capacity(),
            s.alternate(),
            self.parser.is_some()
        )
    }
}

#[cfg(test)]
mod tests {
    fn cursor_after(t: &mut super::Term, bytes: &[u8]) -> (u16, u16) {
        t.process(bytes);
        let p = t.screen().cursor();
        (p.row, p.col)
    }

    /// ONLCR: on, a line feed returns the carriage; the shim's OSC turns it off and on; a reset and a
    /// freeze keep it.
    #[test]
    fn onlcr() {
        let mut t = super::Term::new(10, 20, 0);
        assert_eq!(cursor_after(&mut t, b"ab\n"), (1, 2), "off by default, as in vt100");
        t.set_onlcr(true);
        assert_eq!(cursor_after(&mut t, b"ab\n"), (2, 0));
        assert_eq!(cursor_after(&mut t, b"\x1b]zeropty-onlcr;0\x07ab\n"), (3, 2));
        assert_eq!(cursor_after(&mut t, b"\x1b]zeropty-onlcr;1\x07\n"), (4, 0));
        assert_eq!(cursor_after(&mut t, b"\x1bcab\n"), (1, 0), "kept by a reset");
        let mut t = super::Term::thaw(t.freeze());
        assert_eq!(cursor_after(&mut t, b"ab\n"), (2, 0), "kept by a freeze");
        assert_eq!(cursor_after(&mut t, b"ab\x0b"), (3, 2), "VT is not translated");
    }
}

#[cfg(test)]
pub(super) mod testing {
    //! Reading a screen as text, and the `vt100` crate (the emulator zeropty must match) as the oracle
    //! for the same bytes, for the unit tests of `screen`, `grid::edit` and `vt`.

    use super::{Screen, Term};

    /// The rows on screen as text, trailing blanks trimmed, a wide character once.
    pub(in crate::term) fn rows(s: &Screen) -> Vec<String> {
        let mut out = Vec::new();
        s.visit_rows(&mut |cells, _| {
            let mut line = String::new();
            for c in cells.iter().filter(|c| !c.is_wide_continuation()) {
                if c.has_contents() { line.push_str(&c.contents()) } else { line.push(' ') }
            }
            out.push(line.trim_end().to_owned());
        });
        out
    }

    /// A screen of `r` x `c` (100 rows of scrollback) after `bytes`.
    pub(in crate::term) fn after(r: u16, c: u16, bytes: &[u8]) -> Term {
        let mut t = Term::new(r, c, 100);
        t.process(bytes);
        t
    }

    /// vt100 after the same bytes.
    pub(in crate::term) fn oracle(r: u16, c: u16, bytes: &[u8]) -> vt100::Parser {
        let mut p = vt100::Parser::new(r, c, 100);
        p.process(bytes);
        p
    }

    /// The rows as `rows` gives them, from vt100.
    pub(in crate::term) fn oracle_rows(p: &vt100::Parser) -> Vec<String> {
        let (_, cols) = p.screen().size();
        p.screen().rows(0, cols).map(|r| r.trim_end().to_owned()).collect()
    }

    /// `t`'s rows and cursor, checked against vt100's for `bytes` on a screen of the same size; returns
    /// the rows, for the test to state what they are.
    pub(in crate::term) fn same(t: &mut Term, bytes: &[u8]) -> Vec<String> {
        let (r, c) = t.view().size();
        let p = oracle(r, c, bytes);
        let got = rows(t.view());
        assert_eq!(got, oracle_rows(&p), "rows after {:?}", String::from_utf8_lossy(bytes));
        let cur = t.view().cursor();
        assert_eq!((cur.row, cur.col), p.screen().cursor_position(), "cursor after {:?}", String::from_utf8_lossy(bytes));
        got
    }

    /// `same` for a fresh `r` x `c` screen after `bytes`.
    pub(in crate::term) fn check(r: u16, c: u16, bytes: &[u8]) -> Vec<String> {
        same(&mut after(r, c, bytes), bytes)
    }
}

#[cfg(test)]
mod size_report {
    #[test]
    fn print_sizes() {
        use std::mem::size_of;
        println!(
            "Term {} Screen {} Grid {} Palette {} Slot {} Row {} Frozen {}",
            size_of::<super::Term>(),
            size_of::<super::Screen>(),
            size_of::<super::grid::Grid>(),
            size_of::<super::palette::Palette>(),
            size_of::<super::grid::Slot>(),
            size_of::<super::Row>(),
            size_of::<super::Frozen>()
        );
        assert_eq!(size_of::<super::Frozen>(), 24);
    }
}
