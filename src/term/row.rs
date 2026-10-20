//! `Row`: one unpacked row of cells, and its packed form, the codec scrollback rows, parked screens and
//! frozen screens are made of.
//!
//! # Packed format
//!
//! `[wrapped u8] [cell count, LEB128]`, then runs until the count is reached. A run starts with a tag:
//!
//! - `P_LIT | n-1` (WIDE and WIDE_CONT together, which no cell has): 1-64 cells of code point < 0x100 in
//!   the current attr, a byte each. ASCII text costs a byte a character.
//! - `P_UTF` (WIDE_CONT and CONT_FOLLOWS without WIDE, which no cell packs to): 1-32 characters as UTF-8
//!   in the current attr, a wide one standing for both its halves (`packed_wide`).
//! - otherwise a run of identical cells: the tag carries P_WIDE, P_WIDE_CONT, P_EXT, P_ATTR (a u16 attr
//!   follows and becomes the current one), P_RUN (a byte follows: the run's length less one),
//!   P_CONT_FOLLOWS (a plain second half follows this wide cell) and in its low two bits how many bytes
//!   of code point or cluster id follow (0-3). A row of `─` costs 5 bytes.
//!
//! Trailing blank cells are not packed. Packing then unpacking gives back the same cells.

use super::cell::{BLANK, CHAR_MASK, CONT, Cell, EXT, WIDE, WIDE_CONT, take_cells};
use super::codec::{In, put_var};
use super::text::{decode_utf8, fast_width};
use unicode_width::UnicodeWidthChar as _;

const P_WIDE: u8 = 0x80;
const P_WIDE_CONT: u8 = 0x40;
const P_EXT: u8 = 0x20;
const P_ATTR: u8 = 0x10;
const P_RUN: u8 = 0x08;
const P_CONT_FOLLOWS: u8 = 0x04;
/// The low bits of a run's tag: how many value bytes follow.
const P_VALUE_LEN: u8 = 0x03;

/// Packed-row tag for a literal run of one-byte cells (see Row::pack_into), and the longest such run.
const P_LIT: u8 = 0xc0;
const P_LIT_MAX: usize = 64;
/// Packed-row tag for 1-32 characters as UTF-8 in the current attr, wide ones without their second half
/// (implied by their width): WIDE_CONT and CONT_FOLLOWS without WIDE, which no cell packs to. The length
/// less one is in the remaining bits (0x38 and 0x03).
const P_UTF: u8 = 0x44;
const P_UTF_MAX: usize = 32;

/// Whether a P_UTF character unpacks as wide. Packing uses it too, so a cell goes in a P_UTF run only
/// when this gives back its own WIDE flag.
fn packed_wide(c: char) -> bool {
    fast_width(c).or_else(|| c.width()) == Some(2)
}

/// One unpacked row: its cells (those past `cells.len()` are BLANK) and whether it wrapped.
///
/// ```
/// let mut t = zeropty::term::Term::new(2, 10, 0);
/// t.process(b"row");
/// let row = t.screen().row(0);
/// assert_eq!((row.len(), row.wrapped()), (3, false));
/// ```
#[derive(Clone, Debug)]
pub struct Row {
    pub(super) cells: Vec<Cell>,
    pub(super) cols: u16,
    pub(super) wrapped: bool,
}

impl Row {
    pub(super) fn new(cols: u16) -> Self {
        Self { cells: Vec::new(), cols, wrapped: false }
    }

    /// The cell at `col` (a blank past the stored ones); None past the row's width.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 0);
    /// t.process(b"ab");
    /// let row = t.screen().row(0);
    /// assert_eq!(row.len(), 2); // only the cells written are stored
    /// assert_eq!(&*row.get(1).unwrap().contents(), "b");
    /// assert!(row.get(5).unwrap().is_blank());
    /// assert!(row.get(10).is_none());
    /// ```
    pub fn get(&self, col: u16) -> Option<&Cell> {
        (col < self.cols).then(|| self.cells.get(usize::from(col)).unwrap_or(&BLANK))
    }

    pub(super) fn get_mut(&mut self, col: u16) -> Option<&mut Cell> {
        if col >= self.cols {
            return None;
        }
        let i = usize::from(col);
        if self.cells.len() <= i {
            self.cells.resize(i + 1, BLANK);
        }
        Some(&mut self.cells[i])
    }

    /// Drops trailing blank cells. Capacity stays: a busy screen redraws the same rows many times a second,
    /// and freeing and regrowing them was a third of the CPU under load. Quiet screens are packed instead.
    pub(super) fn trim(&mut self) {
        while self.cells.last() == Some(&BLANK) {
            self.cells.pop();
        }
    }

    pub(super) fn clear(&mut self, attr: u16) {
        if attr == 0 {
            self.cells.clear();
        } else {
            self.cells.resize(usize::from(self.cols), BLANK);
            for c in &mut self.cells {
                c.clear(attr)
            }
        }
        self.wrapped = false;
    }

    pub(super) fn insert(&mut self, i: u16, cell: Cell) {
        let i = usize::from(i);
        if self.cells.len() < i {
            self.cells.resize(i, BLANK);
        }
        self.cells.insert(i, cell);
        self.wrapped = false;
    }

    pub(super) fn remove(&mut self, i: u16) {
        self.clear_wide(i);
        if usize::from(i) < self.cells.len() {
            self.cells.remove(usize::from(i));
        }
        self.wrapped = false;
    }

    pub(super) fn erase(&mut self, i: u16, attr: u16) {
        let wide = self.get(i).is_some_and(Cell::is_wide);
        self.clear_wide(i);
        if (usize::from(i) < self.cells.len() || attr != 0)
            && let Some(c) = self.get_mut(i)
        {
            c.clear(attr)
        }
        if i == self.cols - if wide { 2 } else { 1 } {
            self.wrapped = false;
        }
    }

    pub(super) fn truncate(&mut self, len: u16) {
        self.cells.truncate(usize::from(len));
        self.wrapped = false;
        if let Some(last) = self.cells.get_mut(usize::from(len) - 1)
            && last.is_wide()
        {
            let a = last.attr;
            last.clear(a);
        }
    }

    pub(super) fn wrap(&mut self, wrap: bool) {
        self.wrapped = wrap
    }
    /// The row continues on the next one (its last column was written past).
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(2, 4, 0);
    /// t.process(b"abcdef"); // 4 columns: "ef" goes on in the next row
    /// let screen = t.screen();
    /// assert!(screen.row(0).wrapped() && !screen.row(1).wrapped());
    /// ```
    pub fn wrapped(&self) -> bool {
        self.wrapped
    }
    /// Cells stored; those past it are default blanks.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 80, 0);
    /// t.process(b"\x1b[5Gx"); // column 5, then one character
    /// assert_eq!(t.screen().row(0).len(), 5); // four blanks before it are stored too
    /// ```
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// No cells stored: the whole row is blank.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(2, 10, 0);
    /// t.process(b"x");
    /// let screen = t.screen();
    /// assert!(!screen.row(0).is_empty() && screen.row(1).is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub(super) fn clear_wide(&mut self, col: u16) {
        let Some(cell) = self.get(col) else { return };
        let other = if cell.is_wide() {
            col + 1
        } else if cell.is_wide_continuation() {
            col - 1
        } else {
            return;
        };
        if let Some(o) = self.get_mut(other) {
            let a = o.attr;
            o.clear(a);
        }
    }

    // -------- packing, for scrollback and frozen screens (format: see the module's docs)

    /// The packed row, in an allocation of its own.
    pub(super) fn pack(&self) -> Box<[u8]> {
        let mut out = Vec::with_capacity(4 + self.cells.len() * 2);
        self.pack_into(&mut out);
        out.into_boxed_slice()
    }

    /// Appends the packed row to `out`: [wrapped] [cell count, LEB128] then the runs.
    pub(super) fn pack_into(&self, out: &mut Vec<u8>) {
        let mut cells = &self.cells[..];
        while cells.last() == Some(&BLANK) {
            cells = &cells[..cells.len() - 1];
        }
        out.push(u8::from(self.wrapped));
        put_var(out, cells.len());
        // CONT, the second half of a wide character as text() leaves it, is implied by P_CONT_FOLLOWS on
        // the first.
        let mut prev_attr = 0u16;
        let mut i = 0;
        while i < cells.len() {
            let c = cells[i];
            // Plain one-byte cells in the current attr go as a literal: one tag, then a byte each. Text is
            // mostly this; a tag per cell made packing a busy screen cost half as much as parsing it.
            let lit = |c: &Cell| c.ch < 0x100 && c.attr == prev_attr;
            if lit(&c) {
                let start = i;
                let end = cells.len().min(i + P_LIT_MAX);
                // Up to where four equal cells start: from there a RUN is shorter.
                while i < end && lit(&cells[i]) && !(i + 3 < cells.len() && cells[i + 1..i + 4].iter().all(|x| *x == cells[i])) {
                    i += 1;
                }
                if i > start {
                    out.push(P_LIT | (i - start - 1) as u8);
                    out.extend(cells[start..i].iter().map(|c| c.ch as u8));
                    continue;
                }
            }
            // Text that is not all one-byte (Korean, for one) as a UTF-8 run: unpacking a tag per character
            // led the profile when claude redrew an answer (it rewrites rows in place).
            if c.ch >= 0x100 {
                // Cells the run takes for the character at j: 1, 2 for a wide one with its second half, or
                // 0 when it cannot go in a run.
                let utf = |j: usize| -> usize {
                    let c = cells[j];
                    if c.attr != prev_attr || c.ch & (EXT | WIDE_CONT) != 0 {
                        return 0;
                    }
                    let (v, wide) = (c.ch & CHAR_MASK, c.ch & WIDE != 0);
                    let Some(ch) = char::from_u32(v) else { return 0 };
                    if (v >= 0x100 && packed_wide(ch)) != wide {
                        0
                    } else if wide {
                        if cells.get(j + 1) == Some(&CONT) { 2 } else { 0 }
                    } else {
                        1
                    }
                };
                let (tag_at, start) = (out.len(), i);
                out.push(0);
                let mut n = 0;
                while i < cells.len() && n < P_UTF_MAX {
                    let k = utf(i);
                    if k == 0 || (k == 1 && i + 3 < cells.len() && cells[i + 1..i + 4].iter().all(|x| *x == cells[i])) {
                        break;
                    }
                    let mut b = [0u8; 4];
                    out.extend_from_slice(char::from_u32(cells[i].ch & CHAR_MASK).unwrap().encode_utf8(&mut b).as_bytes());
                    i += k;
                    n += 1;
                }
                if n > 0 {
                    let l = (n - 1) as u8;
                    out[tag_at] = P_UTF | (l & 0x1c) << 1 | (l & 0x03);
                    continue;
                }
                out.truncate(tag_at);
                i = start;
            }
            let mut run = 1;
            while run < 256 && i + run < cells.len() && cells[i + run] == c {
                run += 1;
            }
            let (ch, attr) = (c.ch, c.attr);
            // P_LIT relies on no cell being both halves of a wide character.
            debug_assert!(ch & (WIDE | WIDE_CONT) != WIDE | WIDE_CONT);
            let value = ch & CHAR_MASK;
            let n: u8 = if value == 0 {
                0
            } else if value < 0x100 {
                1
            } else if value < 0x1_0000 {
                2
            } else {
                3
            };
            let mut tag = n;
            let cont_follows = run == 1 && ch & WIDE != 0 && cells.get(i + 1) == Some(&CONT);
            if cont_follows {
                tag |= P_CONT_FOLLOWS
            }
            if ch & WIDE != 0 {
                tag |= P_WIDE
            }
            if ch & WIDE_CONT != 0 {
                tag |= P_WIDE_CONT
            }
            if ch & EXT != 0 {
                tag |= P_EXT
            }
            if attr != prev_attr {
                tag |= P_ATTR
            }
            if run > 1 {
                tag |= P_RUN
            }
            out.push(tag);
            if attr != prev_attr {
                out.extend_from_slice(&attr.to_le_bytes());
                prev_attr = attr;
            }
            if run > 1 {
                out.push((run - 1) as u8);
            }
            out.extend_from_slice(&value.to_le_bytes()[..usize::from(n)]);
            i += run + usize::from(cont_follows);
        }
    }

    /// The row packed in `data`, `cols` wide.
    pub(super) fn unpack(data: &[u8], cols: u16) -> Self {
        Self::unpack_from(data, cols).0
    }

    /// An empty row whose cells reuse a buffer a released row gave back.
    pub(super) fn pooled(cols: u16) -> Self {
        Self { cells: take_cells(), cols, wrapped: false }
    }

    /// Reads one packed row from the start of `data`; returns it and the bytes it took.
    pub(super) fn unpack_from(data: &[u8], cols: u16) -> (Self, usize) {
        // A pooled buffer, or one allocation of the exact size; pushing cell by cell regrew each row several times.
        let mut cells = take_cells();
        let (wrapped, used) = Self::decode(data, &mut cells);
        (Row { cells, cols, wrapped }, used)
    }

    /// Decodes one packed row into `cells` (cleared first); returns whether it wrapped and the bytes it took.
    pub(super) fn decode(data: &[u8], cells: &mut Vec<Cell>) -> (bool, usize) {
        cells.clear();
        let wrapped = data[0] == 1;
        let mut head = In { buf: data, at: 1 };
        let count = head.var();
        let mut i = head.at;
        cells.reserve(count);
        let mut attr = 0u16;
        while cells.len() < count {
            let tag = data[i];
            i += 1;
            if tag & P_LIT == P_LIT {
                let n = usize::from(tag & !P_LIT) + 1;
                cells.extend(data[i..i + n].iter().map(|&b| Cell { ch: u32::from(b), attr }));
                i += n;
                continue;
            }
            if tag & (P_WIDE | P_WIDE_CONT | P_CONT_FOLLOWS) == P_UTF {
                let n = usize::from((tag >> 1) & 0x1c | (tag & 0x03)) + 1;
                for _ in 0..n {
                    let b = data[i];
                    if b < 0x80 {
                        cells.push(Cell { ch: u32::from(b), attr });
                        i += 1;
                        continue;
                    }
                    let (c, len) = decode_utf8(&data[i..]).expect("UTF-8 this process packed");
                    i += len;
                    let v = u32::from(c);
                    if v >= 0x100 && packed_wide(c) {
                        cells.push(Cell { ch: v | WIDE, attr });
                        cells.push(CONT);
                    } else {
                        cells.push(Cell { ch: v, attr });
                    }
                }
                continue;
            }
            if tag & P_ATTR != 0 {
                attr = u16::from_le_bytes([data[i], data[i + 1]]);
                i += 2;
            }
            let mut run = 1;
            if tag & P_RUN != 0 {
                run += usize::from(data[i]);
                i += 1;
            }
            let n = usize::from(tag & P_VALUE_LEN);
            let mut v = [0u8; 4];
            v[..n].copy_from_slice(&data[i..i + n]);
            i += n;
            let mut ch = u32::from_le_bytes(v);
            if tag & P_WIDE != 0 {
                ch |= WIDE
            }
            if tag & P_WIDE_CONT != 0 {
                ch |= WIDE_CONT
            }
            if tag & P_EXT != 0 {
                ch |= EXT
            }
            cells.extend(std::iter::repeat_n(Cell { ch, attr }, run));
            if tag & P_CONT_FOLLOWS != 0 {
                cells.push(CONT);
            }
        }
        (wrapped, i)
    }

    /// The cells stored; those past them (up to the width) are blank.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 80, 0);
    /// t.process(b"hi");
    /// let row = t.screen().row(0);
    /// assert_eq!(row.cells().len(), 2); // not 80: the blank rest is not stored
    /// assert_eq!(&*row.cells()[1].contents(), "i");
    /// ```
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// Rewrites palette indexes (palette compaction).
    pub(super) fn remap(&mut self, map: &[u16]) {
        for c in &mut self.cells {
            c.attr = map[usize::from(c.attr)]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Row;
    use crate::term::cell::{BLANK, CONT, Cell, EXT, WIDE, intern};

    fn cell(c: char, attr: u16) -> Cell {
        let mut x = BLANK;
        x.set(c, attr);
        x
    }

    /// Packs and unpacks `cells`: the same cells come back (less trailing blanks), in as many bytes.
    fn round_trip(cells: Vec<Cell>, wrapped: bool) -> usize {
        let row = Row { cells: cells.clone(), cols: 4096, wrapped };
        let packed = row.pack();
        let (back, used) = Row::unpack_from(&packed, 4096);
        let mut want = cells;
        while want.last() == Some(&BLANK) {
            want.pop();
        }
        assert_eq!(back.cells, want);
        assert_eq!(back.wrapped, wrapped);
        assert_eq!(used, packed.len(), "reads exactly its own bytes");
        packed.len()
    }

    #[test]
    fn packs_every_kind_of_cell() {
        let ascii: Vec<Cell> = "hello, world".chars().map(|c| cell(c, 0)).collect();
        assert_eq!(round_trip(ascii.clone(), false), 2 + 1 + 12, "a byte a character, one tag");
        let mut korean = Vec::new();
        for c in "한글 텍스트".chars() {
            korean.push(cell(c, 3));
            if c != ' ' {
                korean.push(CONT);
            }
        }
        round_trip(korean, true);
        let line = vec![cell('─', 0); 80];
        assert!(round_trip(line, false) <= 6, "a run");
        let id = intern("e\u{301}").unwrap();
        let ext = Cell { ch: EXT | id, attr: 7 };
        let wide_ext = Cell { ch: EXT | WIDE | intern("👍🏽").unwrap(), attr: 0 };
        round_trip(vec![ext, cell('x', 1), wide_ext, CONT, BLANK, BLANK, cell('y', 2), BLANK], false);
        round_trip(vec![cell(' ', 5); 10], false);
        round_trip(Vec::new(), true);
    }

    /// Long and mixed rows: runs past a tag's limits, attributes changing every few cells.
    #[test]
    fn packs_long_mixed_rows() {
        let mut x: u32 = 7;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        for _ in 0..200 {
            let mut cells = Vec::new();
            while cells.len() < 600 {
                let r = next();
                let attr = (r >> 8 & 3) as u16;
                let n = 1 + (r >> 12) as usize % 300;
                match r % 5 {
                    0 => cells.extend(std::iter::repeat_n(cell('a', attr), n)),
                    1 => cells.extend((0..n % 70).map(|i| cell(char::from(b'!' + (i % 90) as u8), attr))),
                    2 => (0..n % 40).for_each(|_| cells.extend([cell('가', attr), CONT])),
                    3 => cells.extend(std::iter::repeat_n(BLANK, n % 9)),
                    _ => cells.push(cell('é', attr)),
                }
            }
            round_trip(cells, next() % 2 == 0);
        }
    }

    #[test]
    fn editing_keeps_wide_characters_whole() {
        let mut r = Row::new(6);
        *r.get_mut(0).unwrap() = cell('한', 0);
        *r.get_mut(1).unwrap() = CONT;
        *r.get_mut(2).unwrap() = cell('x', 0);
        r.erase(1, 0);
        assert!(!r.get(0).unwrap().is_wide(), "erasing the second half clears the first");
        assert!(r.get(9).is_none(), "past the width");
        r.truncate(1);
        assert_eq!(r.len(), 1);
        r.wrap(true);
        r.clear(0);
        assert!(r.is_empty() && !r.wrapped());
    }
}
