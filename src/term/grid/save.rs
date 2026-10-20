//! A grid as bytes (Frozen, Term::export) and back: cursor, scroll region and rows, each row as
//! [byte length][packed row] so a thaw points at them without decoding any.

use super::{Grid, Pos, Slot};
use crate::term::cell::{CHAR_MASK, Cell, EXT, WIDE, WIDE_CONT};
use crate::term::codec::{In, Out};
use crate::term::row::Row;

impl Grid {
    /// Rewrites the ids of combined characters (cells with EXT) on the grid and in its scrollback, for a
    /// screen from another process (Term::import). An id `f` does not know becomes '?'.
    pub(in crate::term) fn map_ext(&mut self, f: &mut dyn FnMut(u32) -> Option<u32>) {
        let mut fix = |cells: &mut [Cell]| {
            let mut changed = false;
            for c in cells.iter_mut() {
                let ch = c.ch;
                if ch & EXT != 0 {
                    c.ch = match f(ch & CHAR_MASK) {
                        Some(id) => ch & !CHAR_MASK | id,
                        None => ch & (WIDE | WIDE_CONT) | u32::from('?'),
                    };
                    changed = true;
                }
            }
            changed
        };
        self.load_all();
        for r in 0..self.rows.len() {
            let i = usize::from(self.rows[r].live - 1);
            if fix(&mut self.live[i].cells) {
                // Its packed copy is stale: pack_live packs it again.
                self.rows[r].len = 0;
            }
        }
        let mut cells = Vec::new();
        for p in self.scrollback.iter_mut() {
            let (wrapped, _) = Row::decode(p, &mut cells);
            if fix(&mut cells) {
                *p = Row { cells: std::mem::take(&mut cells), cols: self.cols, wrapped }.pack();
            }
        }
    }

    /// Rows go out as [byte length][packed row], so a thaw can point at them without decoding any.
    pub(in crate::term) fn save_to(&self, o: &mut Out) {
        self.pos.encode(o);
        self.saved_pos.encode(o);
        o.u16(self.scroll_top);
        o.u16(self.scroll_bottom);
        o.u8(u8::from(self.origin_mode) | u8::from(self.saved_origin_mode) << 1);
        if self.parked {
            o.var(usize::from(self.rows_n));
            if self.blob.is_empty() {
                // Parked blank: every row empty.
                (0..self.rows_n).for_each(|_| o.var(0));
            } else {
                o.0.extend_from_slice(&self.blob);
            }
            return;
        }
        o.var(self.rows.len());
        let mut scratch = Vec::new();
        for row in &self.rows {
            scratch.clear();
            self.packed(row, &mut scratch);
            o.field(&scratch);
        }
    }

    pub(in crate::term) fn restore(&mut self, i: &mut In) {
        self.pos = Pos::decode(i);
        self.saved_pos = Pos::decode(i);
        self.scroll_top = i.u16();
        self.scroll_bottom = i.u16();
        let f = i.u8();
        self.origin_mode = f & 1 != 0;
        self.saved_origin_mode = f & 2 != 0;
        let n = i.var();
        self.rows = Vec::with_capacity(n);
        self.blob = Vec::new();
        for _ in 0..n {
            let row = i.field();
            self.rows.push(Slot { off: self.blob.len() as u32, len: row.len() as u16, live: 0 });
            self.blob.extend_from_slice(row);
        }
    }
}
