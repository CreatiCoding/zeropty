//! The grid's editing operations, as vt100's Grid has them: cursor movement within the screen and the
//! scroll region, erasing, inserting and deleting cells and lines, scrolling, and resizing. Every write
//! goes through `row_mut`, which unpacks the row and marks its packed copy stale.

use super::{Grid, Pos, Slot};
use crate::term::cell::BLANK;

impl Grid {
    /// The grid at a new size, as vt100's Grid::set_size: rows cut or added at the bottom (none go to
    /// scrollback), every row cut to the new width and unwrapped, the cursor and saved cursor kept inside.
    /// Scrollback rows keep their width.
    pub(in crate::term) fn resize(&mut self, rows_n: u16, cols: u16) {
        let parked = self.parked;
        self.unpark();
        if !self.rows.is_empty() {
            for r in 0..self.rows.len() {
                let slot = self.rows[r];
                if slot.len > 0 || slot.live > 0 {
                    // Unlike vt100, a wide character cut in half at the new edge is cleared: vt100 keeps the
                    // half and later panics erasing it (Row::clear_wide indexes past the row).
                    self.row_mut(r as u16).unwrap().truncate(cols);
                }
            }
            for row in &mut self.live {
                row.cols = cols;
            }
            while self.rows.len() > usize::from(rows_n) {
                let mut slot = self.rows.pop().unwrap();
                self.release(&mut slot);
            }
            self.rows.resize(usize::from(rows_n), Slot::default());
        }
        if self.scroll_bottom == self.rows_n - 1 {
            self.scroll_bottom = rows_n - 1;
        }
        (self.rows_n, self.cols) = (rows_n, cols);
        if self.scroll_bottom >= rows_n {
            self.scroll_bottom = rows_n - 1;
        }
        if self.scroll_bottom < self.scroll_top {
            self.scroll_top = 0;
        }
        self.row_clamp_bottom(false);
        self.col_clamp();
        self.saved_pos.row = self.saved_pos.row.min(rows_n - 1);
        self.saved_pos.col = self.saved_pos.col.min(cols - 1);
        if parked {
            self.park();
        }
    }

    pub(in crate::term) fn clear(&mut self) {
        self.pos = Pos::default();
        self.saved_pos = Pos::default();
        for r in 0..self.rows.len() {
            self.clear_row(r, 0)
        }
        self.scroll_top = 0;
        self.scroll_bottom = self.rows_n - 1;
        self.origin_mode = false;
        self.saved_origin_mode = false;
    }

    pub(in crate::term) fn set_pos(&mut self, mut pos: Pos) {
        if self.origin_mode {
            pos.row = pos.row.saturating_add(self.scroll_top);
        }
        self.pos = pos;
        self.row_clamp_top(self.origin_mode);
        self.row_clamp_bottom(self.origin_mode);
        self.col_clamp();
    }

    pub(in crate::term) fn save_cursor(&mut self) {
        self.saved_pos = self.pos;
        self.saved_origin_mode = self.origin_mode;
    }

    pub(in crate::term) fn restore_cursor(&mut self) {
        self.pos = self.saved_pos;
        self.origin_mode = self.saved_origin_mode;
    }

    pub(in crate::term) fn erase_all(&mut self, attr: u16) {
        for r in 0..self.rows.len() {
            self.clear_row(r, attr)
        }
    }

    pub(in crate::term) fn erase_all_forward(&mut self, attr: u16) {
        let pos = self.pos;
        for r in usize::from(pos.row) + 1..self.rows.len() {
            self.clear_row(r, attr)
        }
        self.erase_row_forward(attr);
    }

    pub(in crate::term) fn erase_all_backward(&mut self, attr: u16) {
        let pos = self.pos;
        for r in 0..usize::from(pos.row) {
            self.clear_row(r, attr)
        }
        self.erase_row_backward(attr);
    }

    pub(in crate::term) fn erase_row(&mut self, attr: u16) {
        self.clear_row(usize::from(self.pos.row), attr)
    }

    pub(in crate::term) fn erase_row_forward(&mut self, attr: u16) {
        let (cols, pos) = (self.cols, self.pos);
        let row = self.current_row_mut();
        if attr == 0 && pos.col < cols {
            // What erasing each cell to the end would do, in one cut: only a wide character straddling the
            // start needs its first half cleared, and reaching the last column always clears `wrapped`.
            // (claude erases to the end of nearly every line it draws; cell by cell this led the profile.)
            row.clear_wide(pos.col);
            row.cells.truncate(usize::from(pos.col));
            row.wrapped = false;
        } else {
            for col in pos.col..cols {
                row.erase(col, attr)
            }
        }
        row.trim();
    }

    pub(in crate::term) fn erase_row_backward(&mut self, attr: u16) {
        let (cols, pos) = (self.cols, self.pos);
        let row = self.current_row_mut();
        for col in 0..=pos.col.min(cols - 1) {
            row.erase(col, attr)
        }
        row.trim();
    }

    pub(in crate::term) fn insert_cells(&mut self, count: u16) {
        let (cols, pos) = (self.cols, self.pos);
        let wide = pos.col < cols && self.cell(pos).unwrap().is_wide_continuation();
        let row = self.current_row_mut();
        // Past `cols` insertions the row is blank from the cursor on and stays so. Each one moves the
        // whole row: `ESC[65535@` cost 0.25 s of the server's thread, every session waiting.
        for _ in 0..count.min(cols) {
            if wide {
                row.get_mut(pos.col).unwrap().set_wide_continuation(false)
            }
            row.insert(pos.col, BLANK);
            if wide {
                row.get_mut(pos.col).unwrap().set_wide_continuation(true)
            }
        }
        row.truncate(cols);
        row.trim();
    }

    pub(in crate::term) fn delete_cells(&mut self, count: u16) {
        let (cols, pos) = (self.cols, self.pos);
        let row = self.current_row_mut();
        for _ in 0..(count.min(cols - pos.col)) {
            row.remove(pos.col)
        }
        row.wrapped = false;
        row.trim();
    }

    pub(in crate::term) fn erase_cells(&mut self, count: u16, attr: u16) {
        let (cols, pos) = (self.cols, self.pos);
        let row = self.current_row_mut();
        for col in pos.col..((pos.col.saturating_add(count)).min(cols)) {
            row.erase(col, attr)
        }
        row.trim();
    }

    pub(in crate::term) fn insert_lines(&mut self, count: u16) {
        // As insert_cells: past `rows_n` the screen no longer changes (`ESC[65535L` took 18 ms on 4096x4096).
        for _ in 0..count.min(self.rows_n) {
            self.remove_row(usize::from(self.scroll_bottom), false);
            self.insert_empty(usize::from(self.pos.row));
            let b = self.scroll_bottom;
            self.row_mut(b).unwrap().wrap(false);
        }
    }

    pub(in crate::term) fn delete_lines(&mut self, count: u16) {
        for _ in 0..(count.min(self.rows_n - self.pos.row)) {
            self.insert_empty(usize::from(self.scroll_bottom) + 1);
            self.remove_row(usize::from(self.pos.row), false);
        }
    }

    pub(in crate::term) fn scroll_down(&mut self, count: u16) {
        // As insert_lines.
        for _ in 0..count.min(self.rows_n) {
            self.remove_row(usize::from(self.scroll_bottom), false);
            self.insert_empty(usize::from(self.scroll_top));
            let b = self.scroll_bottom;
            self.row_mut(b).unwrap().wrap(false);
        }
    }

    pub(in crate::term) fn set_scroll_region(&mut self, top: u16, bottom: u16) {
        let bottom = bottom.min(self.rows_n - 1);
        if top < bottom {
            self.scroll_top = top;
            self.scroll_bottom = bottom;
        } else {
            self.scroll_top = 0;
            self.scroll_bottom = self.rows_n - 1;
        }
        self.pos.row = self.scroll_top;
        self.pos.col = 0;
    }

    pub(super) fn in_scroll_region(&self) -> bool {
        self.pos.row >= self.scroll_top && self.pos.row <= self.scroll_bottom
    }
    pub(super) fn scroll_region_active(&self) -> bool {
        self.scroll_top != 0 || self.scroll_bottom != self.rows_n - 1
    }

    pub(in crate::term) fn set_origin_mode(&mut self, mode: bool) {
        self.origin_mode = mode;
        self.set_pos(Pos { row: 0, col: 0 });
    }

    pub(in crate::term) fn row_inc_clamp(&mut self, count: u16) {
        let in_region = self.in_scroll_region();
        self.pos.row = self.pos.row.saturating_add(count);
        self.row_clamp_bottom(in_region);
    }

    pub(in crate::term) fn row_inc_scroll(&mut self, count: u16) -> u16 {
        let in_region = self.in_scroll_region();
        self.pos.row = self.pos.row.saturating_add(count);
        let lines = self.row_clamp_bottom(in_region);
        if in_region {
            self.scroll_up(lines);
            lines
        } else {
            0
        }
    }

    pub(in crate::term) fn row_dec_clamp(&mut self, count: u16) {
        let in_region = self.in_scroll_region();
        self.pos.row = self.pos.row.saturating_sub(count);
        self.row_clamp_top(in_region);
    }

    pub(in crate::term) fn row_dec_scroll(&mut self, count: u16) {
        let in_region = self.in_scroll_region();
        let extra = count.saturating_sub(self.pos.row);
        self.pos.row = self.pos.row.saturating_sub(count);
        let lines = self.row_clamp_top(in_region);
        self.scroll_down(lines + extra);
    }

    pub(in crate::term) fn row_set(&mut self, i: u16) {
        self.pos.row = i;
        if self.pos.row > self.rows_n - 1 {
            self.pos.row = self.rows_n - 1
        }
    }

    pub(in crate::term) fn col_inc(&mut self, count: u16) {
        self.pos.col = self.pos.col.saturating_add(count)
    }
    pub(in crate::term) fn col_inc_clamp(&mut self, count: u16) {
        self.pos.col = self.pos.col.saturating_add(count);
        self.col_clamp()
    }
    pub(in crate::term) fn col_dec(&mut self, count: u16) {
        self.pos.col = self.pos.col.saturating_sub(count)
    }
    pub(in crate::term) fn col_tab(&mut self) {
        self.pos.col -= self.pos.col % 8;
        self.pos.col += 8;
        self.col_clamp()
    }
    pub(in crate::term) fn col_set(&mut self, i: u16) {
        self.pos.col = i;
        self.col_clamp()
    }

    pub(in crate::term) fn col_wrap(&mut self, width: u16, wrap: bool) {
        if self.pos.col > self.cols.saturating_sub(width) {
            let prev = self.pos;
            self.pos.col = 0;
            let scrolled = self.row_inc_scroll(1);
            let new = self.pos;
            // On a one-row screen the row just filled has scrolled off: nothing is left to mark (the
            // row number wrapped to 65535 and the server panicked on the unwrap).
            if let Some(prev) = prev.row.checked_sub(scrolled) {
                self.row_mut(prev).unwrap().wrap(wrap && prev + 1 == new.row);
            }
        }
    }

    pub(super) fn row_clamp_top(&mut self, limit: bool) -> u16 {
        if limit && self.pos.row < self.scroll_top {
            let n = self.scroll_top - self.pos.row;
            self.pos.row = self.scroll_top;
            n
        } else {
            0
        }
    }

    pub(super) fn row_clamp_bottom(&mut self, limit: bool) -> u16 {
        let bottom = if limit { self.scroll_bottom } else { self.rows_n - 1 };
        if self.pos.row > bottom {
            let n = self.pos.row - bottom;
            self.pos.row = bottom;
            n
        } else {
            0
        }
    }

    pub(super) fn col_clamp(&mut self) {
        if self.pos.col > self.cols - 1 {
            self.pos.col = self.cols - 1
        }
    }
}

#[cfg(test)]
mod tests {
    //! The grid's editing operations called directly, each checked against vt100 doing the same through
    //! its control sequence.

    use super::super::super::testing::{after, rows, same};

    /// Runs `op` on the current grid of a screen after `setup`, and checks it against vt100 after
    /// `setup` then `seq`; returns the rows.
    fn edit(r: u16, c: u16, setup: &[u8], seq: &[u8], op: impl FnOnce(&mut super::Grid)) -> Vec<String> {
        let mut t = after(r, c, setup);
        op(t.screen.gm());
        same(&mut t, &[setup, seq].concat())
    }

    #[test]
    fn inserting_deleting_and_erasing_cells() {
        let s = b"abcdef\x1b[1;2H";
        assert_eq!(edit(1, 6, s, b"\x1b[2@", |g| g.insert_cells(2)), ["a  bcd"], "pushed off the right");
        assert_eq!(edit(1, 6, s, b"\x1b[2P", |g| g.delete_cells(2)), ["adef"]);
        assert_eq!(edit(1, 6, s, b"\x1b[2X", |g| g.erase_cells(2, 0)), ["a  def"]);
        assert_eq!(edit(1, 6, s, b"\x1b[99P", |g| g.delete_cells(99)), ["a"], "more than there is");
        assert_eq!(edit(1, 6, s, b"\x1b[99@", |g| g.insert_cells(99)), ["a"]);
    }

    #[test]
    fn inserting_and_deleting_lines_in_the_scroll_region() {
        let s = b"1\r\n2\r\n3\r\n4\r\n5\x1b[2;4r\x1b[2;1H";
        assert_eq!(edit(5, 3, s, b"\x1b[L", |g| g.insert_lines(1)), ["1", "", "2", "3", "5"], "the region's last line goes");
        assert_eq!(edit(5, 3, s, b"\x1b[M", |g| g.delete_lines(1)), ["1", "3", "4", "", "5"]);
        assert_eq!(edit(5, 3, s, b"\x1b[9L", |g| g.insert_lines(9)), ["1", "", "", "", "5"]);
        // Below the region, as vt100 has it: the lines from the cursor down move, the region's bottom goes.
        let s = b"1\r\n2\r\n3\r\n4\r\n5\x1b[2;4r\x1b[5;1H";
        assert_eq!(edit(5, 3, s, b"\x1b[L", |g| g.insert_lines(1)), ["1", "2", "3", "5", ""]);
    }

    #[test]
    fn scrolling_within_a_region_keeps_the_history() {
        // A region below the top: what scrolls off it is gone, not in the history.
        let mut t = after(4, 3, b"1\r\n2\r\n3\r\n4\x1b[2;3r\x1b[3;1H\n");
        let bytes = b"1\r\n2\r\n3\r\n4\x1b[2;3r\x1b[3;1H\n";
        assert_eq!(same(&mut t, bytes), ["1", "3", "", "4"]);
        assert_eq!(t.view().history_size(), 0);
        // A region from the top: it goes to the history, as a full screen's would.
        let bytes = b"1\r\n2\r\n3\r\n4\x1b[1;3r\x1b[3;1H\n";
        let mut t = after(4, 3, bytes);
        assert_eq!(same(&mut t, bytes), ["2", "3", "", "4"]);
        let p = super::super::super::testing::oracle(4, 3, bytes);
        let mut p = p;
        p.screen_mut().set_scrollback(10);
        assert_eq!(t.view().history_size(), p.screen().scrollback(), "as vt100 keeps it");
        // Scrolling down (reverse index at the top) inserts at the region's top.
        assert_eq!(edit(4, 3, b"1\r\n2\r\n3\r\n4\x1b[2;3r\x1b[2;1H", b"\x1bM", |g| g.row_dec_scroll(1)), ["1", "", "2", "4"]);
    }

    #[test]
    fn scroll_regions_that_make_no_sense_are_the_whole_screen() {
        for seq in [&b"\x1b[3;2r"[..], b"\x1b[2;2r", b"\x1b[r", b"\x1b[0;99r"] {
            let mut t = after(4, 3, b"x\x1b[3;3H");
            let bytes = [&b"x\x1b[3;3H"[..], seq].concat();
            t.process(seq);
            same(&mut t, &bytes);
            let g = t.screen.g();
            assert_eq!((g.scroll_top, g.scroll_bottom), (0, 3), "{seq:?}");
            assert_eq!((g.pos.row, g.pos.col), (0, 0), "the cursor goes home");
        }
    }

    #[test]
    fn cursor_movement_stays_on_the_screen() {
        let mut t = after(3, 4, b"");
        let g = t.screen.gm();
        g.col_inc_clamp(99);
        g.row_inc_clamp(99);
        assert_eq!((g.pos.row, g.pos.col), (2, 3));
        g.row_dec_clamp(99);
        g.col_dec(99);
        assert_eq!((g.pos.row, g.pos.col), (0, 0));
        g.col_tab();
        assert_eq!(g.pos.col, 3, "a tab stops at the margin");
    }

    #[test]
    fn resizing_keeps_the_top_left() {
        let mut t = after(3, 5, b"abcde\r\nfghij\r\nklm");
        t.resize(2, 3);
        assert_eq!(rows(t.view()), ["abc", "fgh"]);
        let mut p = super::super::super::testing::oracle(3, 5, b"abcde\r\nfghij\r\nklm");
        p.screen_mut().set_size(2, 3);
        assert_eq!(rows(t.view()), super::super::super::testing::oracle_rows(&p));
        t.resize(4, 6);
        assert_eq!(rows(t.view()), ["abc", "fgh", "", ""]);
    }
}
