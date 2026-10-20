//! `Screen`: what a terminal shows, the main grid and the alternate one, the pen (current attributes and
//! their palette id), terminal modes and the tty modes the shim reports. Its operations are what control
//! sequences map to (`vt` does the mapping); printing has fast paths for the plain cases that are most of
//! a program's output, each giving exactly the result the general `text` path would.

use super::attrs::Attrs;
use super::cell::{BLANK, Cell, WIDE, WIDE_CONT};
use super::grid::{Grid, Pos};
use super::palette::Palette;
use super::row::Row;
use super::text::{fast_width, narrow_or_wide};

pub(super) const MODE_APPLICATION_CURSOR: u8 = 0b0000_0010;
pub(super) const MODE_HIDE_CURSOR: u8 = 0b0000_0100;
pub(super) const MODE_ALTERNATE_SCREEN: u8 = 0b0000_1000;
pub(super) const MODE_BRACKETED_PASTE: u8 = 0b0001_0000;
/// The tty's ONLCR (a line feed also returns the carriage), which a PTY applies before the terminal sees
/// the bytes. Not a terminal mode: off by default, as in vt100; the server turns it on, and the shim
/// reports a program turning it off or on again (OSC zeropty-onlcr).
pub(super) const MODE_ONLCR: u8 = 0b0010_0000;

/// The tty's input modes (the line discipline a PTY applies to keys before the program reads them, which
/// the engine does here: Sess::input). Reported by the shim as a program changes them (OSC zeropty-tty).
/// 0 is no line discipline: keys go to the program as they are (what zeropty did before it had one, and
/// what a screen from an older zeropty comes back with).
/// Canonical input: keys gather into a line the program gets at Enter.
pub const TTY_ICANON: u8 = 1;
/// Keys typed show on the screen.
pub const TTY_ECHO: u8 = 2;
/// C-c, C-\ and C-z signal the program's process group.
pub const TTY_ISIG: u8 = 4;
/// Enter (CR) reaches the program as a line feed.
pub const TTY_ICRNL: u8 = 8;
/// A new PTY's: a line at a time, echoed, C-c a signal, Enter a line feed.
pub const TTY_COOKED: u8 = TTY_ICANON | TTY_ECHO | TTY_ISIG | TTY_ICRNL;

/// A terminal screen; read it through `Term::view`.
///
/// ```
/// let mut t = zeropty::term::Term::new(24, 80, 0);
/// t.process(b"\x1b[?2004h\x1b[?25l"); // what claude turns on as it starts
/// let screen = t.view();
/// assert!(screen.bracketed_paste() && screen.hide_cursor());
/// assert_eq!(screen.size(), (24, 80));
/// ```
pub struct Screen {
    pub(super) grid: Grid,
    pub(super) alt: Grid,
    pub(super) attrs: Attrs,
    pub(super) saved_attrs: Attrs,
    pub(super) modes: u8,
    /// TTY_* flags.
    pub(super) tty: u8,
    pub(super) palette: Palette,
    /// palette id of `attrs`
    pub(super) cur: u16,
    /// What the parser last called back with; see Term::process.
    pub(super) cb: u8,
}

/// `Screen::cb`: what the parser last called back with, so `Term` can tell when vte is back in its ground
/// state. CB_NONE: nothing yet.
pub(super) const CB_NONE: u8 = 0;
/// print or a finished escape/CSI sequence: the parser is back in its ground state.
pub(super) const CB_GROUND: u8 = 1;
/// Anything else (a C0 control, an OSC, DCS): the state is unknown here.
pub(super) const CB_OTHER: u8 = 2;

impl Screen {
    pub(super) fn new(rows: u16, cols: u16, scrollback: usize) -> Self {
        let mut s = Self::bare(rows, cols, scrollback);
        s.grid.allocate_rows();
        s
    }

    /// Without rows, for thaw to fill in.
    pub(super) fn bare(rows: u16, cols: u16, scrollback: usize) -> Self {
        let grid = Grid::new(rows, cols, scrollback);
        Self {
            grid,
            alt: Grid::new(rows, cols, 0),
            attrs: Attrs::default(),
            saved_attrs: Attrs::default(),
            modes: 0,
            tty: 0,
            palette: Palette::new(),
            cur: 0,
            cb: CB_NONE,
        }
    }

    /// (rows, cols).
    ///
    /// ```
    /// assert_eq!(zeropty::term::Term::new(24, 80, 0).view().size(), (24, 80));
    /// ```
    pub fn size(&self) -> (u16, u16) {
        (self.grid.rows_n, self.grid.cols)
    }
    /// The program asked for application cursor keys (DECCKM): arrows go as ESC O x.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 0);
    /// assert!(!t.view().application_cursor());
    /// t.process(b"\x1b[?1h"); // DECCKM on, as vim and less turn it
    /// assert!(t.view().application_cursor());
    /// ```
    pub fn application_cursor(&self) -> bool {
        self.mode(MODE_APPLICATION_CURSOR)
    }
    /// The program asked for bracketed paste (mode 2004).
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 0);
    /// t.process(b"\x1b[?2004h"); // claude and zsh turn it on
    /// assert!(t.view().bracketed_paste()); // paste-buffer -p now wraps the text in ESC[200~ ... ESC[201~
    /// ```
    pub fn bracketed_paste(&self) -> bool {
        self.mode(MODE_BRACKETED_PASTE)
    }
    /// The program hid the cursor (DECTCEM off).
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 0);
    /// t.process(b"\x1b[?25l");
    /// assert!(t.view().hide_cursor());
    /// ```
    pub fn hide_cursor(&self) -> bool {
        self.mode(MODE_HIDE_CURSOR)
    }
    /// Where the cursor is.
    ///
    /// ```
    /// use zeropty::term::{Pos, Term};
    ///
    /// let mut t = Term::new(24, 80, 0);
    /// t.process(b"\x1b[5;10Hx"); // CUP to row 5, column 10 (1-based), then one character
    /// assert_eq!(t.view().cursor(), Pos { row: 4, col: 10 });
    /// ```
    pub fn cursor(&self) -> Pos {
        self.g().pos
    }

    /// The rows on screen, then (visit_history) the scrollback, as (cells, wrapped), unpacking nothing into
    /// the screen (see Grid::visit_rows).
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(2, 4, 0);
    /// t.process(b"abcdef"); // wraps after 4 columns
    /// let text = |cells: &[zeropty::term::Cell]| cells.iter().map(|c| c.contents().to_string()).collect::<String>();
    /// let mut rows = Vec::new();
    /// t.view().visit_rows(&mut |cells, wrapped| rows.push((text(cells), wrapped)));
    /// assert_eq!(rows, [("abcd".to_string(), true), ("ef".to_string(), false)]);
    /// ```
    pub fn visit_rows(&self, f: &mut dyn FnMut(&[Cell], bool)) {
        self.g().visit_rows(f)
    }

    /// The last `n` scrollback rows, oldest first, as visit_rows gives rows.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 100);
    /// t.process(b"one\r\ntwo\r\nthree");
    /// let text = |cells: &[zeropty::term::Cell]| cells.iter().map(|c| c.contents().to_string()).collect::<String>();
    /// let mut old = Vec::new();
    /// t.view().visit_history(10, &mut |cells, _| old.push(text(cells)));
    /// assert_eq!(old, ["one", "two"]);
    /// ```
    pub fn visit_history(&self, n: usize, f: &mut dyn FnMut(&[Cell], bool)) {
        self.g().visit_history(n, f)
    }

    /// Rows in the scrollback (none while the alternate screen is up).
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 100);
    /// t.process(b"a\r\nb\r\nc");
    /// assert_eq!(t.view().history_size(), 2);
    /// t.process(b"\x1b[?1049h"); // the alternate screen has no scrollback
    /// assert_eq!(t.view().history_size(), 0);
    /// ```
    pub fn history_size(&self) -> usize {
        self.g().scrollback.len()
    }

    /// The rows on screen written since the last Term::pack_live, as visit_rows gives rows.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(3, 10, 0);
    /// t.process(b"top");
    /// t.pack_live();
    /// t.process(b"\x1b[3Hbottom");
    /// let text = |cells: &[zeropty::term::Cell]| cells.iter().map(|c| c.contents().to_string()).collect::<String>();
    /// let mut changed = Vec::new();
    /// t.view().visit_changed_rows(&mut |cells, _| changed.push(text(cells)));
    /// assert_eq!(changed, ["bottom"]);
    /// ```
    pub fn visit_changed_rows(&self, f: &mut dyn FnMut(&[Cell], bool)) {
        self.g().visit_changed(f)
    }

    /// The alternate screen is up.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(2, 10, 0);
    /// t.process(b"shell\x1b[?1049h"); // a full-screen program starts
    /// assert!(t.view().alternate());
    /// t.process(b"\x1b[?1049l"); // and quits: the shell's screen is back
    /// assert!(!t.view().alternate());
    /// assert_eq!(&*t.screen().row(0).cells()[0].contents(), "s");
    /// ```
    pub fn alternate(&self) -> bool {
        self.mode(MODE_ALTERNATE_SCREEN)
    }

    /// Row `r` on screen; unpacked first, as for `rows`.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(2, 10, 0);
    /// t.process(b"\x1b[2Hsecond");
    /// let screen = t.screen();
    /// assert_eq!(screen.row(0).len(), 0);
    /// assert_eq!(&*screen.row(1).cells()[0].contents(), "s");
    /// ```
    pub fn row(&self, r: u16) -> &Row {
        self.g().loaded(usize::from(r))
    }

    /// The last `n` scrollback rows, oldest first. Empty while the alternate screen is up, as in vt100.
    ///
    /// ```
    /// use zeropty::term::Term;
    ///
    /// let mut t = Term::new(2, 10, 100);
    /// t.set_onlcr(true);
    /// t.process(b"one\ntwo\nthree\nfour");
    /// let row = |r: &zeropty::term::Row| r.cells().iter().map(|c| c.contents().to_string()).collect::<String>();
    /// let screen = t.screen();
    /// assert_eq!(screen.history_size(), 2);
    /// assert_eq!(screen.history(10).iter().map(row).collect::<Vec<_>>(), ["one", "two"]);
    /// assert_eq!(row(screen.row(0)), "three");
    /// ```
    pub fn history(&self, n: usize) -> Vec<Row> {
        let g = self.g();
        let n = n.min(g.scrollback.len());
        g.scrollback.iter().skip(g.scrollback.len() - n).map(|p| Row::unpack(p, g.cols)).collect()
    }

    /// The attributes a cell's palette index stands for.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 0);
    /// t.process(b"\x1b[7mx");
    /// let screen = t.screen();
    /// assert!(screen.attrs_of(&screen.row(0).cells()[0]).inverse());
    /// ```
    pub fn attrs_of(&self, cell: &Cell) -> Attrs {
        self.palette.get(cell.attr)
    }

    /// Bytes held by this screen's cells and scrollback (for measuring, not exact allocator usage).
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(24, 80, 0);
    /// let empty = t.view().approx_bytes();
    /// t.process(&[b'x'; 80 * 24]);
    /// assert!(t.view().approx_bytes() > empty);
    /// ```
    pub fn approx_bytes(&self) -> usize {
        self.grid.approx_rows_bytes() + self.alt.approx_rows_bytes() + self.grid.scrollback.iter().map(|p| p.len() + 16).sum::<usize>()
    }

    pub(super) fn g(&self) -> &Grid {
        if self.mode(MODE_ALTERNATE_SCREEN) { &self.alt } else { &self.grid }
    }
    pub(super) fn gm(&mut self) -> &mut Grid {
        if self.mode(MODE_ALTERNATE_SCREEN) { &mut self.alt } else { &mut self.grid }
    }
    pub(super) fn mode(&self, m: u8) -> bool {
        self.modes & m != 0
    }
    pub(super) fn set_mode(&mut self, m: u8) {
        self.modes |= m
    }
    pub(super) fn clear_mode(&mut self, m: u8) {
        self.modes &= !m
    }

    pub(super) fn enter_alt(&mut self) {
        self.set_mode(MODE_ALTERNATE_SCREEN);
        self.alt.unpark();
        self.alt.allocate_rows();
    }
    pub(super) fn exit_alt(&mut self) {
        self.clear_mode(MODE_ALTERNATE_SCREEN);
        self.grid.unpark();
    }
    pub(super) fn decsc(&mut self) {
        self.gm().save_cursor();
        self.saved_attrs = self.attrs
    }
    pub(super) fn decrc(&mut self) {
        self.gm().restore_cursor();
        self.attrs = self.saved_attrs;
        self.sync_attr();
    }

    pub(super) fn sync_attr(&mut self) {
        self.cur = match self.palette.id(self.attrs) {
            Some(id) => id,
            None => {
                self.compact_palette();
                self.palette.id(self.attrs).unwrap_or(0)
            }
        };
    }

    /// Renumbers the palette down to the attributes still on screen or in scrollback.
    pub(super) fn compact_palette(&mut self) {
        self.grid.pack_scrolled();
        self.grid.load_all();
        self.alt.load_all();
        let mut used = vec![false; self.palette.len()];
        used[0] = true;
        let mut history: Vec<Row> = self.grid.scrollback.iter().map(|p| Row::unpack(p, self.grid.cols)).collect();
        for row in self.grid.live.iter().chain(&self.alt.live).chain(&history) {
            for c in &row.cells {
                used[usize::from(c.attr)] = true
            }
        }
        let mut fresh = Palette::new();
        let map: Vec<u16> =
            (0..self.palette.len()).zip(&used).map(|(i, &u)| if u { fresh.id(Attrs::from_key(self.palette.key(i))).unwrap_or(0) } else { 0 }).collect();
        for row in self.grid.live.iter_mut().chain(self.alt.live.iter_mut()).chain(history.iter_mut()) {
            row.remap(&map);
        }
        // Every row is unpacked now and the old packed copies carry old ids.
        for g in [&mut self.grid, &mut self.alt] {
            for slot in &mut g.rows {
                slot.len = 0
            }
        }
        self.grid.scrollback = history.iter().map(Row::pack).collect();
        self.palette = fresh;
    }

    /// Printable ASCII, as `text` would do it char by char: where the run lands past the row's last cell
    /// and fits before the right margin, it is appended in one go.
    pub(super) fn print_ascii(&mut self, run: &[u8]) {
        let attr = self.cur;
        let g = self.gm();
        let (col, cols) = (g.pos.col, g.cols);
        if col < cols && run.len() <= usize::from(cols - col) {
            let r = g.pos.row;
            let row = g.row_mut(r).unwrap();
            let (start, len) = (usize::from(col), row.cells.len());
            if start >= len {
                row.cells.resize(start, BLANK);
                row.cells.extend(run.iter().map(|&b| Cell { ch: u32::from(b), attr }));
                g.pos.col += run.len() as u16;
                return;
            }
            // Over cells that are not part of a wide character: what text's fast path does a char at a
            // time. Claude redraws its screen over the last one, so most text lands here.
            let over = (start + run.len()).min(len);
            if row.cells[start..over].iter().all(|c| c.ch & (WIDE | WIDE_CONT) == 0) {
                let (now, rest) = run.split_at(over - start);
                for (cell, &b) in row.cells[start..over].iter_mut().zip(now) {
                    *cell = Cell { ch: u32::from(b), attr };
                }
                row.cells.extend(rest.iter().map(|&b| Cell { ch: u32::from(b), attr }));
                g.pos.col += run.len() as u16;
                return;
            }
        }
        for &b in run {
            self.text(char::from(b));
        }
    }

    /// Writes the characters at the start of `b` that print as one or two cells, as text's fast paths
    /// would one after another, while each lands on plain cells inside the row; returns the bytes used
    /// (0: none, the caller goes the usual way).
    pub(super) fn print_run(&mut self, b: &[u8]) -> usize {
        let Some(first) = narrow_or_wide(b) else { return 0 };
        let attr = self.cur;
        let g = self.gm();
        let (cols, r) = (g.cols, g.pos.row);
        let mut col = g.pos.col;
        let fits = |col: u16, w: u8| if w == 1 { col < cols } else { col + 1 < cols };
        if !fits(col, first.1) {
            return 0;
        }
        let row = g.row_mut(r).unwrap();
        let plain = |cells: &[Cell], i: usize| cells.get(i).is_none_or(|c| c.ch & (WIDE | WIDE_CONT) == 0);
        let mut used = 0;
        let mut next = Some(first);
        while let Some((v, w, len)) = next {
            let i = usize::from(col);
            if !fits(col, w) || !plain(&row.cells, i) || (w == 2 && !plain(&row.cells, i + 1)) {
                break;
            }
            if row.cells.len() < i + usize::from(w) {
                row.cells.resize(i + usize::from(w), BLANK);
            }
            if w == 1 {
                row.cells[i] = Cell { ch: v, attr };
            } else {
                row.cells[i] = Cell { ch: v | WIDE, attr };
                row.cells[i + 1] = Cell { ch: WIDE_CONT, attr: 0 };
            }
            col += u16::from(w);
            used += len;
            next = narrow_or_wide(&b[used..]);
        }
        g.pos.col = col;
        used
    }

    pub(super) fn text(&mut self, c: char) {
        // Fast path for printable ASCII landing on a plain cell or past the row's last one: the same result
        // as below (no wrap pending, no wide neighbour to fix up, nothing left to trim) in one lookup.
        if (' '..='~').contains(&c) {
            let attr = self.cur;
            let g = self.gm();
            if g.pos.col < g.cols {
                let i = usize::from(g.pos.col);
                let r = g.pos.row;
                let row = g.row_mut(r).unwrap();
                let cell = Cell { ch: u32::from(c), attr };
                if i >= row.cells.len() {
                    row.cells.resize(i, BLANK);
                    row.cells.push(cell);
                    g.pos.col += 1;
                    return;
                }
                if row.cells[i].ch & (WIDE | WIDE_CONT) == 0 {
                    row.cells[i] = cell;
                    g.pos.col += 1;
                    return;
                }
            }
        }
        let pos = self.g().pos;
        let cols = self.g().cols;
        let attrs = self.cur;
        let width = fast_width(c);
        // A single-width character on a plain cell: the general path below reduces to one store.
        if width == Some(1) && pos.col < cols {
            let row = self.gm().row_mut(pos.row).unwrap();
            let i = usize::from(pos.col);
            if row.cells.get(i).is_none_or(|c| c.ch & (WIDE | WIDE_CONT) == 0) {
                if row.cells.len() <= i {
                    row.cells.resize(i + 1, BLANK);
                }
                row.cells[i] = Cell { ch: u32::from(c), attr: attrs };
                self.gm().pos.col += 1;
                return;
            }
        }
        // A double-width character on two plain cells inside the row: two stores. Korean text is all this.
        if width == Some(2) && pos.col + 1 < cols {
            let row = self.gm().row_mut(pos.row).unwrap();
            let i = usize::from(pos.col);
            let plain = |c: Option<&Cell>| c.is_none_or(|c| c.ch & (WIDE | WIDE_CONT) == 0);
            if plain(row.cells.get(i)) && plain(row.cells.get(i + 1)) {
                if row.cells.len() < i + 2 {
                    row.cells.resize(i + 2, BLANK);
                }
                row.cells[i] = Cell { ch: u32::from(c) | WIDE, attr: attrs };
                row.cells[i + 1] = Cell { ch: WIDE_CONT, attr: 0 };
                self.gm().pos.col += 2;
                return;
            }
        }
        if width.is_none() && u32::from(c) < 256 {
            return;
        }
        let width = width.unwrap_or(1) as u16;

        let mut wrap = false;
        if pos.col > cols - width {
            let last = self.gm().cell(Pos { row: pos.row, col: cols - 1 }).unwrap();
            if last.has_contents() || last.is_wide_continuation() {
                wrap = true;
            }
        }
        self.gm().col_wrap(width, wrap);
        let pos = self.g().pos;

        if width == 0 {
            if pos.col > 0 {
                let mut p = Pos { row: pos.row, col: pos.col - 1 };
                if self.gm().cell(p).unwrap().is_wide_continuation() {
                    p.col -= 1;
                }
                self.gm().cell_mut(p).unwrap().append(c);
            } else if pos.row > 0 && self.gm().row(pos.row - 1).unwrap().wrapped() {
                let mut p = Pos { row: pos.row - 1, col: cols - 1 };
                if self.gm().cell(p).unwrap().is_wide_continuation() {
                    p.col = cols - 2;
                }
                self.gm().cell_mut(p).unwrap().append(c);
            }
            return;
        }

        if self.gm().cell(pos).unwrap().is_wide_continuation() {
            self.gm().cell_mut(Pos { row: pos.row, col: pos.col - 1 }).unwrap().clear(attrs);
        }
        if self.gm().cell(pos).unwrap().is_wide() {
            self.gm().cell_mut(Pos { row: pos.row, col: pos.col + 1 }).unwrap().set(' ', attrs);
        }
        self.gm().cell_mut(pos).unwrap().set(c, attrs);
        self.gm().col_inc(1);
        if width > 1 {
            let pos = self.g().pos;
            if self.gm().cell(pos).unwrap().is_wide() {
                let nn = Pos { row: pos.row, col: pos.col + 1 };
                self.gm().cell_mut(nn).unwrap().clear(attrs);
                if nn.col == cols - 1 {
                    self.gm().row_mut(pos.row).unwrap().wrap(false);
                }
            }
            let next = self.gm().cell_mut(pos).unwrap();
            next.clear(0);
            next.set_wide_continuation(true);
            self.gm().col_inc(1);
        }
        let r = self.g().pos.row;
        self.gm().row_mut(r).unwrap().trim();
    }

    pub(super) fn ed(&mut self, mode: u16) {
        let a = self.cur;
        match mode {
            0 => self.gm().erase_all_forward(a),
            1 => self.gm().erase_all_backward(a),
            2 => self.gm().erase_all(a),
            _ => {}
        }
    }

    pub(super) fn el(&mut self, mode: u16) {
        let a = self.cur;
        match mode {
            0 => self.gm().erase_row_forward(a),
            1 => self.gm().erase_row_backward(a),
            2 => self.gm().erase_row(a),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    //! Printing, wrapping, the alternate screen and erasing, on `Screen` directly where it has the
    //! operation, each checked against vt100 for the same bytes.

    use super::super::testing::{after, check, rows, same};

    #[test]
    fn printing_wraps_at_the_margin() {
        assert_eq!(check(3, 5, b"abcdefg"), ["abcde", "fg", ""]);
        let t = after(3, 5, b"abcdefg");
        assert!(t.view().row(0).wrapped() && !t.view().row(1).wrapped(), "a soft wrap is marked");
        // Exactly to the margin: the wrap waits for the next character.
        assert_eq!(check(3, 5, b"abcde"), ["abcde", "", ""]);
        assert_eq!(check(3, 5, b"abcde\r\nx"), ["abcde", "x", ""]);
        // A wide character that does not fit goes to the next row whole.
        assert_eq!(check(3, 5, "abcd한".as_bytes()), ["abcd", "한", ""]);
        // Past the bottom: rows scroll into the history.
        let t = after(2, 5, b"1\r\n2\r\n3");
        assert_eq!(rows(t.view()), ["2", "3"]);
        assert_eq!(t.view().history_size(), 1);
        check(2, 5, b"1\r\n2\r\n3");
    }

    #[test]
    fn wide_and_combining_characters() {
        assert_eq!(check(2, 8, "e\u{301}x한y".as_bytes()), ["e\u{301}x한y", ""], "a combining mark joins the cell before");
        let t = super::super::testing::after(2, 8, "e\u{301}x".as_bytes());
        assert_eq!(&*t.view().row(0).get(0).unwrap().contents(), "e\u{301}", "one cell");
        // Over either half of a wide character, both halves go.
        check(2, 8, "한글\x1b[1Gx".as_bytes());
        check(2, 8, "한글\x1b[2Gx".as_bytes());
        check(2, 8, "ab\x1b[1G한".as_bytes());
        // The text path and the fast paths agree.
        let mut t = after(2, 8, b"");
        for ch in "a한\u{301}b".chars() {
            t.screen.text(ch);
        }
        assert_eq!(rows(t.view()), ["a한\u{301}b", ""]);
    }

    #[test]
    fn the_alternate_screen_keeps_the_main_one() {
        let mut t = after(3, 10, b"main\r\nline");
        assert!(!t.view().alternate());
        t.process(b"\x1b[?1049h");
        assert!(t.view().alternate());
        assert_eq!(rows(t.view()), ["", "", ""], "cleared");
        t.process(b"alt\r\n\r\n\r\n");
        assert_eq!(t.view().history_size(), 0, "nothing scrolls into history from it");
        t.process(b"\x1b[?1049l");
        assert!(!t.view().alternate());
        let all = b"main\r\nline\x1b[?1049halt\r\n\r\n\r\n\x1b[?1049l";
        assert_eq!(same(&mut t, all), ["main", "line", ""]);
        let c = t.view().cursor();
        assert_eq!((c.row, c.col), (1, 4), "1049 restores the cursor");
    }

    #[test]
    fn erasing_in_display_and_line() {
        let setup = b"aaaaaa\r\nbbbbbb\r\ncccccc\x1b[2;3H";
        for (mode, seq) in [(0u16, &b"\x1b[K"[..]), (1, b"\x1b[1K"), (2, b"\x1b[2K")] {
            let mut t = after(3, 6, setup);
            t.screen.el(mode);
            let bytes = [&setup[..], seq].concat();
            same(&mut t, &bytes);
        }
        for (mode, seq) in [(0u16, &b"\x1b[J"[..]), (1, b"\x1b[1J"), (2, b"\x1b[2J")] {
            let mut t = after(3, 6, setup);
            t.screen.ed(mode);
            let bytes = [&setup[..], seq].concat();
            same(&mut t, &bytes);
        }
        let mut t = after(3, 6, setup);
        t.screen.el(0);
        assert_eq!(rows(t.view()), ["aaaaaa", "bb", "cccccc"]);
        t.screen.ed(1);
        assert_eq!(rows(t.view()), ["", "", "cccccc"]);
    }

    #[test]
    fn attributes_go_into_the_palette_and_compact() {
        let mut t = after(2, 40, b"");
        // Many pens, each overwritten: compacting keeps only the ones cells still use.
        for i in 0..300u32 {
            t.process(format!("\x1b[38;5;{}m{}\r", i % 256, i % 10).as_bytes());
        }
        let cell = *t.view().row(0).get(0).unwrap();
        assert_eq!(t.view().attrs_of(&cell).fg, crate::term::Color::Idx((299 % 256) as u8));
        assert!(t.screen.palette.len() > 200);
        t.screen.compact_palette();
        t.screen.sync_attr();
        assert!(t.screen.palette.len() <= 3, "{}", t.screen.palette.len());
        let cell = *t.view().row(0).get(0).unwrap();
        assert_eq!(t.view().attrs_of(&cell).fg, crate::term::Color::Idx((299 % 256) as u8), "ids renumbered with the cells");
        t.process(b"x");
        let cell = *t.view().row(0).get(0).unwrap();
        assert_eq!(&*cell.contents(), "x");
        assert_eq!(t.view().attrs_of(&cell).fg, crate::term::Color::Idx((299 % 256) as u8), "the pen too");
    }
}
