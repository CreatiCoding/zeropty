//! `Grid`: one screen's rows (main or alternate), how they are stored, and its scrollback.
//!
//! Rows live as slots (`Slot`, 8 bytes) into `blob`, a buffer of packed rows, and are unpacked into
//! `live` only while being written or read whole; between writes a busy screen is packed again
//! (`pack_live`) and parked (`park`: the slots dropped too, `blob` holding [length][packed row] per row).
//!
//! Invariants:
//! - A slot with `len > 0` points at an up-to-date packed copy; writing a row (`row_mut`) sets its `len`
//!   to 0 until it is packed again.
//! - While `parked`, `rows`, `live` and `free` are empty and `blob` is the whole screen (empty: blank).
//! - `scrolled` holds rows scrolled off during the current write, oldest first, after `scrollback`;
//!   `Term::process` packs them before it returns, so readers only ever see `scrollback`.
//!
//! Scratch buffers are per thread and reused across grids (`release_scratch` frees them).
//!
//! This module is the storage; `edit` has the editing operations, `read` the reading that changes
//! nothing, `save` the grid as bytes.

mod edit;
mod read;
mod save;

use super::cell::{Cell, give_cells};
use super::codec::{In, Out, put_var};
use super::row::Row;
use std::collections::VecDeque;

/// A position on the screen, 0-based.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Pos {
    /// From the top, 0-based.
    pub row: u16,
    /// From the left, 0-based.
    pub col: u16,
}

impl Pos {
    fn encode(&self, o: &mut Out) {
        o.u16(self.row);
        o.u16(self.col);
    }

    fn decode(i: &mut In) -> Self {
        Pos { row: i.u16(), col: i.u16() }
    }
}

/// The rows of a parked blob ([length][packed row] each, `n` of them): each row's offset in `blob` and
/// its packed bytes (empty for a blank row).
fn parked_rows(blob: &[u8], n: u16) -> impl Iterator<Item = (usize, &[u8])> {
    let mut i = In::new(blob);
    (0..n).map(move |_| {
        let row = i.field();
        (i.at - row.len(), row)
    })
}

/// Where a row's content is. Packed bytes in the grid's blob (`len` > 0) and/or an unpacked Row in
/// `Grid::live` (`live` = index + 1); neither means an empty row. 8 bytes, so 80 rows cost 640 B: every
/// live session keeps its slots. A packed row fits `len` because screens are at most MAX_SIZE wide.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Slot {
    off: u32,
    /// 0 when there is no up-to-date packed copy (the row changed since, or it is empty).
    pub(super) len: u16,
    live: u16,
}
const _: () = assert!(std::mem::size_of::<Slot>() == 8);

impl Slot {
    fn bytes(&self) -> std::ops::Range<usize> {
        self.off as usize..self.off as usize + usize::from(self.len)
    }
}

/// The widest and tallest screen: a packed cell takes at most 7 bytes, so a packed row stays under
/// Slot::len's 64 KB.
pub const MAX_SIZE: u16 = 4096;

#[derive(Clone, Debug)]
pub(super) struct Grid {
    pub(super) rows_n: u16,
    pub(super) cols: u16,
    pub(super) pos: Pos,
    pub(super) saved_pos: Pos,
    pub(super) rows: Vec<Slot>,
    /// Unpacked rows, indexed by Slot::live - 1; `free` lists unused entries.
    pub(super) live: Vec<Row>,
    pub(super) free: Vec<u16>,
    pub(super) scroll_top: u16,
    pub(super) scroll_bottom: u16,
    pub(super) origin_mode: bool,
    pub(super) saved_origin_mode: bool,
    pub(super) scrollback: VecDeque<Box<[u8]>>,
    pub(super) scrollback_len: usize,
    /// Rows scrolled off after `scrollback`, not packed yet: packed when the write that scrolled them
    /// ends (Grid::pack_scrolled), so a burst scrolling far more than the scrollback holds packs only
    /// the rows that stay (5 MB printed at once: a quarter of the parsing was packing rows then dropped).
    pub(super) scrolled: VecDeque<Row>,
    /// Packed rows that slots point into. Rebuilt in place by rebuild and park, so a busy screen reuses
    /// one allocation instead of making a new one four times a second (the churn fragmented the heap:
    /// 8.6 KB per working claude session against 2.6 KB of data).
    pub(super) blob: Vec<u8>,
    /// The screen not shown (main under the alternate one, or the reverse) keeps no slots, only `blob`:
    /// [byte length][packed row] for each of `rows_n` rows.
    pub(super) parked: bool,
}

/// What an empty row packs to: not wrapped, no cells.
const EMPTY_ROW: [u8; 2] = [0, 0];

// An emptied Grid::rows, for the next grid to unpark.
thread_local!(static SLOTS: std::cell::RefCell<Vec<Slot>> = const { std::cell::RefCell::new(Vec::new()) });

// Cells decoded for reading (Grid::visit_rows): one buffer, not a row allocated per row read.
thread_local!(static CELLS: std::cell::RefCell<Vec<Cell>> = const { std::cell::RefCell::new(Vec::new()) });

// Scratch buffers of park, pack_live and rebuild.
thread_local!(static PARK: std::cell::RefCell<(Vec<u8>, Vec<u8>)> = const { std::cell::RefCell::new((Vec::new(), Vec::new())) });
thread_local!(static ROWS: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) });
thread_local!(static OUT: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) });
// An emptied `Grid::live` and `free`, for the next grid that unpacks rows.
thread_local!(static SLAB: std::cell::RefCell<(Vec<Row>, Vec<u16>)> = const { std::cell::RefCell::new((Vec::new(), Vec::new())) });

/// Frees this thread's grid scratch (see crate::term::release_scratch).
pub(super) fn release_scratch() {
    CELLS.take();
    SLOTS.take();
    SLAB.take();
    PARK.take();
    ROWS.take();
    OUT.take();
}

impl Grid {
    pub(super) fn new(rows_n: u16, cols: u16, scrollback_len: usize) -> Self {
        Self {
            rows_n,
            cols,
            pos: Pos::default(),
            saved_pos: Pos::default(),
            rows: vec![],
            live: vec![],
            free: vec![],
            scroll_top: 0,
            scroll_bottom: rows_n - 1,
            origin_mode: false,
            saved_origin_mode: false,
            scrollback: VecDeque::new(),
            scrollback_len,
            scrolled: VecDeque::new(),
            blob: Vec::new(),
            parked: false,
        }
    }

    /// Drops the unpacked copy of a slot's row, if any.
    pub(super) fn release(&mut self, slot: &mut Slot) {
        if slot.live > 0 {
            let i = slot.live - 1;
            give_cells(std::mem::replace(&mut self.live[i as usize], Row::new(self.cols)).cells);
            self.free.push(i);
            slot.live = 0;
        }
        if self.free.len() == self.live.len() && !self.live.is_empty() {
            // Handed on to the next screen that unpacks rows, rather than freed and regrown each read.
            let (mut live, mut free) = (std::mem::take(&mut self.live), std::mem::take(&mut self.free));
            live.clear();
            free.clear();
            SLAB.with_borrow_mut(|s| {
                if s.0.capacity() < live.capacity() {
                    *s = (live, free)
                }
            });
        }
    }

    /// The row's packed bytes, appended to `out`.
    pub(super) fn packed(&self, slot: &Slot, out: &mut Vec<u8>) {
        if slot.len > 0 {
            out.extend_from_slice(&self.blob[slot.bytes()]);
        } else if slot.live > 0 {
            self.live[(slot.live - 1) as usize].pack_into(out);
        } else {
            out.extend_from_slice(&EMPTY_ROW);
        }
    }

    /// Keeps the screen as [byte length][packed row] per row in `blob`, with no slots: a busy screen is
    /// parked after every read (Term::pack_live), which also drops the replaced row copies pack_live
    /// leaves in the blob. Nothing at all when every row is blank.
    pub(super) fn park(&mut self) {
        if self.parked || self.rows.is_empty() {
            return;
        }
        let (mut out, mut row) = PARK.take();
        out.clear();
        let mut blank = true;
        for slot in &self.rows {
            let bytes: &[u8] = if slot.len > 0 {
                &self.blob[slot.bytes()]
            } else if slot.live > 0 {
                row.clear();
                self.live[usize::from(slot.live - 1)].pack_into(&mut row);
                &row
            } else {
                &[]
            };
            blank &= bytes.is_empty() || bytes == EMPTY_ROW;
            put_var(&mut out, bytes.len());
            out.extend_from_slice(bytes);
        }
        if blank {
            out.clear();
        }
        self.set_blob(&out);
        PARK.set((out, row));
        let mut rows = std::mem::take(&mut self.rows);
        rows.clear();
        SLOTS.with_borrow_mut(|s| {
            if s.capacity() < rows.capacity() {
                *s = rows
            }
        });
        self.live = Vec::new();
        self.free = Vec::new();
        self.parked = true;
    }

    pub(super) fn unpark(&mut self) {
        if !self.parked {
            return;
        }
        // The slots of the screen parked last, reused: a busy screen is parked and unparked every read.
        let mut rows = SLOTS.take();
        rows.reserve_exact(usize::from(self.rows_n));
        if self.blob.is_empty() {
            rows.resize(usize::from(self.rows_n), Slot::default());
        } else {
            rows.extend(parked_rows(&self.blob, self.rows_n).map(|(off, b)| Slot { off: off as u32, len: b.len() as u16, live: 0 }));
        }
        self.rows = rows;
        self.parked = false;
    }

    /// Replaces the blob's contents, keeping its allocation when it is big enough and not far too big.
    pub(super) fn set_blob(&mut self, bytes: &[u8]) {
        self.blob.clear();
        // Exact growth (not Vec's doubling) and a shrink past 25% slack: the blob is most of a busy
        // screen's memory, and doubling left it up to twice the size it needed. (Rounding up to powers of
        // two, to fill allocator pages better, used 30% more heap for the same footprint.)
        if self.blob.capacity() > bytes.len() + bytes.len() / 4 + 64 {
            self.blob.shrink_to(bytes.len());
        }
        self.blob.reserve_exact(bytes.len());
        self.blob.extend_from_slice(bytes);
    }

    /// Packs every unpacked row and drops the unpacked copies, after a read. Changed rows are appended to
    /// the blob; the copies they replace stay there as garbage until it is half the blob, then the blob
    /// is rebuilt. Cost follows the rows written, not the screen.
    pub(super) fn pack_live(&mut self) {
        if self.parked || self.live.is_empty() {
            return;
        }
        // The changed rows are packed together first, then added to the blob in one exact growth: growing
        // it row by row copied the whole blob each time, and Vec's doubling would leave half of it unused.
        let mut added = ROWS.take();
        added.clear();
        let base = self.blob.len();
        let mut used = 0usize;
        for i in 0..self.rows.len() {
            let mut slot = self.rows[i];
            if slot.live > 0 {
                if slot.len == 0 {
                    let row = &self.live[(slot.live - 1) as usize];
                    if !row.cells.is_empty() || row.wrapped {
                        let off = added.len();
                        row.pack_into(&mut added);
                        slot.off = (base + off) as u32;
                        slot.len = (added.len() - off) as u16;
                    }
                }
                self.release(&mut slot);
                self.rows[i] = slot;
            }
            used += slot.len as usize;
        }
        self.blob.reserve_exact(added.len());
        self.blob.extend_from_slice(&added);
        ROWS.set(added);
        if self.blob.len() > 2 * used + 256 {
            self.rebuild(false);
        }
    }

    /// Rebuilds the blob from the rows' current packed bytes, dropping replaced copies. With `release`,
    /// unpacked rows are packed into it too and their unpacked copies dropped.
    pub(super) fn rebuild(&mut self, release: bool) {
        if self.parked {
            return;
        }
        let mut out = OUT.take();
        out.clear();
        for i in 0..self.rows.len() {
            let mut slot = self.rows[i];
            if slot.len > 0 || (release && slot.live > 0) {
                let off = out.len();
                self.packed(&slot, &mut out);
                slot.off = off as u32;
                slot.len = (out.len() - off) as u16;
                if release {
                    self.release(&mut slot);
                }
                self.rows[i] = slot;
            }
        }
        self.set_blob(&out);
        OUT.set(out);
    }

    /// Index into `live` of row `r`, unpacking it first if needed.
    pub(super) fn load(&mut self, r: usize) -> usize {
        let slot = self.rows[r];
        if slot.live > 0 {
            return (slot.live - 1) as usize;
        }
        let row = if slot.len > 0 { Row::unpack_from(&self.blob[slot.bytes()], self.cols).0 } else { Row::pooled(self.cols) };
        if self.live.capacity() == 0 {
            (self.live, self.free) = SLAB.take();
        }
        let i = match self.free.pop() {
            Some(i) => {
                self.live[i as usize] = row;
                i as usize
            }
            None => {
                self.live.push(row);
                self.live.len() - 1
            }
        };
        self.rows[r].live = i as u16 + 1;
        i
    }

    pub(super) fn load_all(&mut self) {
        self.unpark();
        for r in 0..self.rows.len() {
            self.load(r);
        }
    }

    /// Row `r` once every row is unpacked (after load_all).
    pub(super) fn loaded(&self, r: usize) -> &Row {
        &self.live[(self.rows[r].live - 1) as usize]
    }

    pub(super) fn allocate_rows(&mut self) {
        if self.rows.is_empty() && !self.parked {
            self.rows = vec![Slot::default(); usize::from(self.rows_n)];
        }
    }

    /// Row `r` set to blanks with `attr`; an empty slot when attr is the default.
    pub(super) fn clear_row(&mut self, r: usize, attr: u16) {
        if attr == 0 {
            let mut slot = self.rows[r];
            self.release(&mut slot);
            self.rows[r] = Slot::default();
        } else {
            self.row_mut(r as u16).unwrap().clear(attr);
        }
    }

    /// Takes row `r` out of the grid as packed bytes (for scrollback), or just drops it.
    pub(super) fn remove_row(&mut self, r: usize, keep: bool) -> Option<Box<[u8]>> {
        let mut slot = self.rows.remove(r);
        let bytes = keep.then(|| {
            let mut b = Vec::new();
            self.packed(&slot, &mut b);
            b.into_boxed_slice()
        });
        self.release(&mut slot);
        bytes
    }

    pub(super) fn insert_empty(&mut self, r: usize) {
        self.rows.insert(r, Slot::default());
    }

    /// Row `r` for reading, unpacked if needed.
    pub(super) fn row(&mut self, r: u16) -> Option<&Row> {
        let r = usize::from(r);
        if r >= self.rows.len() {
            return None;
        }
        let i = self.load(r);
        Some(&self.live[i])
    }

    /// Row `r` for writing: unpacked, and its packed copy no longer current.
    pub(super) fn row_mut(&mut self, r: u16) -> Option<&mut Row> {
        let r = usize::from(r);
        if r >= self.rows.len() {
            return None;
        }
        let i = self.load(r);
        let slot = &mut self.rows[r];
        slot.len = 0;
        Some(&mut self.live[i])
    }

    pub(super) fn cell(&mut self, p: Pos) -> Option<&Cell> {
        self.row(p.row).and_then(|r| r.get(p.col))
    }
    pub(super) fn cell_mut(&mut self, p: Pos) -> Option<&mut Cell> {
        self.row_mut(p.row).and_then(|r| r.get_mut(p.col))
    }
    pub(super) fn current_row_mut(&mut self) -> &mut Row {
        let r = self.pos.row;
        self.row_mut(r).unwrap()
    }
    pub(super) fn scroll_up(&mut self, count: u16) {
        for _ in 0..(count.min(self.rows_n - self.scroll_top)) {
            self.insert_empty(usize::from(self.scroll_bottom) + 1);
            let r = usize::from(self.scroll_top);
            if self.scrollback_len == 0 || self.scroll_region_active() {
                self.remove_row(r, false);
                continue;
            }
            // A row changed since it was last packed goes to `scrolled` as it is.
            let mut slot = self.rows[r];
            if slot.len == 0 && slot.live > 0 {
                self.rows.remove(r);
                let i = slot.live - 1;
                let row = std::mem::replace(&mut self.live[usize::from(i)], Row::new(self.cols));
                self.free.push(i);
                slot.live = 0;
                self.release(&mut slot);
                self.scrolled.push_back(row);
            } else {
                let bytes = self.remove_row(r, true).expect("kept");
                self.pack_scrolled();
                self.scrollback.push_back(bytes);
            }
            // The oldest go first, packed or not.
            while self.scrollback.len() + self.scrolled.len() > self.scrollback_len {
                if self.scrollback.pop_front().is_none() {
                    give_cells(self.scrolled.pop_front().expect("over the limit").cells);
                }
            }
        }
    }

    /// Packs the rows in `scrolled` into the scrollback.
    pub(super) fn pack_scrolled(&mut self) {
        if self.scrolled.is_empty() {
            return;
        }
        let mut out = ROWS.take();
        for row in std::mem::take(&mut self.scrolled) {
            out.clear();
            row.pack_into(&mut out);
            self.scrollback.push_back(Box::from(&out[..]));
            give_cells(row.cells);
        }
        ROWS.set(out);
    }
}
