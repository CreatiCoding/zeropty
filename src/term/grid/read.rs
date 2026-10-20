//! Reading a grid without changing it: its rows for a capture or a search (packed ones decoded into
//! per-thread scratch, so a frozen screen's copy or a shared one can be read), and what its memory holds.

use super::{CELLS, Grid, Slot, parked_rows};
use crate::term::cell::Cell;
use crate::term::row::Row;

impl Grid {
    /// Every row, top to bottom, as (cells, wrapped), without changing the grid: unpacked rows as they are,
    /// packed ones (parked or not) decoded into one reused buffer. What a capture reads, on any thread.
    pub(in crate::term) fn visit_rows(&self, f: &mut dyn FnMut(&[Cell], bool)) {
        CELLS.with_borrow_mut(|buf| {
            if self.parked {
                if self.blob.is_empty() {
                    (0..self.rows_n).for_each(|_| f(&[], false));
                    return;
                }
                for (_, b) in parked_rows(&self.blob, self.rows_n) {
                    if b.is_empty() {
                        f(&[], false);
                    } else {
                        let (wrapped, _) = Row::decode(b, buf);
                        f(buf, wrapped);
                    }
                }
                return;
            }
            if self.rows.is_empty() {
                (0..self.rows_n).for_each(|_| f(&[], false));
                return;
            }
            for slot in &self.rows {
                if slot.live > 0 {
                    let row = &self.live[usize::from(slot.live - 1)];
                    f(&row.cells, row.wrapped);
                } else if slot.len > 0 {
                    let (wrapped, _) = Row::decode(&self.blob[slot.bytes()], buf);
                    f(buf, wrapped);
                } else {
                    f(&[], false);
                }
            }
        })
    }

    /// The rows changed since they were last packed (written since the last pack_live), as visit_rows
    /// gives rows.
    pub(in crate::term) fn visit_changed(&self, f: &mut dyn FnMut(&[Cell], bool)) {
        if self.parked {
            return;
        }
        for slot in &self.rows {
            if slot.live > 0 && slot.len == 0 {
                let row = &self.live[usize::from(slot.live - 1)];
                f(&row.cells, row.wrapped);
            }
        }
    }

    /// The last `n` scrollback rows, oldest first, as visit_rows gives rows.
    pub(in crate::term) fn visit_history(&self, n: usize, f: &mut dyn FnMut(&[Cell], bool)) {
        let n = n.min(self.scrollback.len());
        CELLS.with_borrow_mut(|buf| {
            for p in self.scrollback.iter().skip(self.scrollback.len() - n) {
                let (wrapped, _) = Row::decode(p, buf);
                f(buf, wrapped);
            }
        })
    }

    /// Bytes its rows hold: slots, unpacked rows and the blob (scrollback not included).
    pub(in crate::term) fn approx_rows_bytes(&self) -> usize {
        self.rows.capacity() * std::mem::size_of::<Slot>()
            + self.live.iter().map(|r| r.cells.capacity() * std::mem::size_of::<Cell>() + std::mem::size_of::<Row>()).sum::<usize>()
            + self.blob.capacity()
    }

    /// Where its memory is, one line (the `mem` command).
    pub(in crate::term) fn mem_report(&self, name: &str) -> String {
        format!(
            "{name}: slots {} (cap {}) live rows {} (slab cap {}, cells cap {}) blob {}/{} parked {} scrollback {} rows {} B (deque cap {})",
            self.rows.len(),
            self.rows.capacity(),
            self.rows.iter().filter(|x| x.live > 0).count(),
            self.live.capacity(),
            self.live.iter().map(|r| r.cells.capacity()).sum::<usize>(),
            self.blob.len(),
            self.blob.capacity(),
            self.parked,
            self.scrollback.len(),
            self.scrollback.iter().map(|r| r.len()).sum::<usize>(),
            self.scrollback.capacity(),
        )
    }
}
