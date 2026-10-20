//! The dispatcher: escape and control sequences to screen operations, as vt100's `Perform` maps them.
//! `vte` calls this for whatever the fast paths in `Term::process` leave to it; the fast paths call
//! `Screen::csi` and `execute` directly with the same arguments vte would.

use super::TTY_COOKED;
use super::attrs::{Attrs, Color, MODE_INVERSE, MODE_ITALIC, MODE_UNDERLINE};
use super::csi::{Csi, p1, p2};
use super::grid::Pos;
use super::screen::{CB_GROUND, CB_OTHER, MODE_APPLICATION_CURSOR, MODE_BRACKETED_PASTE, MODE_HIDE_CURSOR, MODE_ONLCR, Screen};

impl vte::Perform for Screen {
    fn print(&mut self, c: char) {
        self.cb = CB_GROUND;
        if !(c == '\u{fffd}' || ('\u{80}'..'\u{a0}').contains(&c)) {
            self.text(c);
        }
    }

    fn execute(&mut self, b: u8) {
        // C0 controls also run in the middle of a CSI sequence, so this says nothing about the state.
        self.cb = CB_OTHER;
        match b {
            8 => self.gm().col_dec(1),
            9 => self.gm().col_tab(),
            10..=12 => {
                self.gm().row_inc_scroll(1);
                if b == 10 && self.mode(MODE_ONLCR) {
                    self.gm().col_set(0);
                }
            }
            13 => self.gm().col_set(0),
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, b: u8) {
        self.cb = CB_GROUND;
        if !intermediates.is_empty() {
            return;
        }
        match b {
            b'7' => self.decsc(),
            b'8' => self.decrc(),
            b'M' => self.gm().row_dec_scroll(1),
            b'c' => {
                let (rows, cols) = self.size();
                let onlcr = self.modes & MODE_ONLCR;
                *self = Self::new(rows, cols, self.grid.scrollback_len);
                self.modes |= onlcr;
            }
            _ => {}
        }
    }

    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {
        self.cb = CB_OTHER
    }
    fn put(&mut self, _byte: u8) {
        self.cb = CB_OTHER
    }
    fn unhook(&mut self) {
        self.cb = CB_OTHER
    }
    // Ends with BEL (back to ground) or with ESC (then an esc_dispatch follows); either way unknown here.
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        self.cb = CB_OTHER;
        // From the shim (shim/shim.c): the program changed OPOST/ONLCR.
        if let [b"zeropty-onlcr", on] = params {
            if *on == b"1" { self.set_mode(MODE_ONLCR) } else { self.clear_mode(MODE_ONLCR) }
        }
        // From the shim: the program changed ICANON, ECHO, ISIG or ICRNL (TTY_* flags, in decimal).
        if let [b"zeropty-tty", flags] = params
            && let Some(f) = std::str::from_utf8(flags).ok().and_then(|f| f.parse::<u8>().ok())
        {
            self.tty = f & TTY_COOKED;
        }
    }

    fn csi_dispatch(&mut self, params: &vte::Params, intermediates: &[u8], _ignore: bool, c: char) {
        self.csi(&Csi::from_vte(params), intermediates, c);
    }
}

impl Screen {
    /// DECSET / DECRST (`CSI ? n h` / `l`).
    fn decset(&mut self, params: &Csi, on: bool) {
        for p in params.iter() {
            match (p, on) {
                ([1], true) => self.set_mode(MODE_APPLICATION_CURSOR),
                ([1], false) => self.clear_mode(MODE_APPLICATION_CURSOR),
                ([6], _) => self.gm().set_origin_mode(on),
                ([25], true) => self.clear_mode(MODE_HIDE_CURSOR),
                ([25], false) => self.set_mode(MODE_HIDE_CURSOR),
                ([47], true) => self.enter_alt(),
                ([47], false) => self.exit_alt(),
                ([1049], true) => {
                    self.decsc();
                    self.alt.clear();
                    self.enter_alt();
                }
                ([1049], false) => {
                    self.exit_alt();
                    self.decrc();
                }
                ([2004], true) => self.set_mode(MODE_BRACKETED_PASTE),
                ([2004], false) => self.clear_mode(MODE_BRACKETED_PASTE),
                _ => {}
            }
        }
    }

    /// SGR (`CSI ... m`): the pen's attributes; the caller syncs its palette id.
    fn sgr(&mut self, params: &Csi) {
        if params.is_empty() {
            self.attrs = Attrs::default();
            return;
        }
        let mut iter = params.iter();
        macro_rules! next {
            () => {
                match iter.next() {
                    Some(n) => n,
                    None => return,
                }
            };
        }
        macro_rules! u8_ {
            ($n:expr) => {
                match u8::try_from($n) {
                    Ok(n) => n,
                    Err(_) => return,
                }
            };
        }
        macro_rules! next_u8 {
            () => {
                if let &[n] = next!() { u8_!(n) } else { return }
            };
        }
        loop {
            match next!() {
                [0] => self.attrs = Attrs::default(),
                [1] => self.attrs.set_bold(),
                [2] => self.attrs.set_dim(),
                [3] => self.attrs.flag(MODE_ITALIC, true),
                [4] => self.attrs.flag(MODE_UNDERLINE, true),
                [7] => self.attrs.flag(MODE_INVERSE, true),
                [22] => self.attrs.set_normal_intensity(),
                [23] => self.attrs.flag(MODE_ITALIC, false),
                [24] => self.attrs.flag(MODE_UNDERLINE, false),
                [27] => self.attrs.flag(MODE_INVERSE, false),
                [n] if (30..=37).contains(n) => self.attrs.fg = Color::Idx(u8_!(*n) - 30),
                [38, 2, r, g, b] => self.attrs.fg = Color::Rgb(u8_!(*r), u8_!(*g), u8_!(*b)),
                [38, 5, i] => self.attrs.fg = Color::Idx(u8_!(*i)),
                [38] => match next!() {
                    [2] => {
                        let (r, g, b) = (next_u8!(), next_u8!(), next_u8!());
                        self.attrs.fg = Color::Rgb(r, g, b)
                    }
                    [5] => self.attrs.fg = Color::Idx(next_u8!()),
                    _ => return,
                },
                [39] => self.attrs.fg = Color::Default,
                [n] if (40..=47).contains(n) => self.attrs.bg = Color::Idx(u8_!(*n) - 40),
                [48, 2, r, g, b] => self.attrs.bg = Color::Rgb(u8_!(*r), u8_!(*g), u8_!(*b)),
                [48, 5, i] => self.attrs.bg = Color::Idx(u8_!(*i)),
                [48] => match next!() {
                    [2] => {
                        let (r, g, b) = (next_u8!(), next_u8!(), next_u8!());
                        self.attrs.bg = Color::Rgb(r, g, b)
                    }
                    [5] => self.attrs.bg = Color::Idx(next_u8!()),
                    _ => return,
                },
                [49] => self.attrs.bg = Color::Default,
                [n] if (90..=97).contains(n) => self.attrs.fg = Color::Idx(u8_!(*n) - 82),
                [n] if (100..=107).contains(n) => self.attrs.bg = Color::Idx(u8_!(*n) - 92),
                _ => {}
            }
        }
    }

    /// A complete CSI sequence; from vte or from `Term::process`'s fast path.
    pub(super) fn csi(&mut self, params: &Csi, intermediates: &[u8], c: char) {
        self.cb = CB_GROUND;
        match intermediates.first() {
            None => match c {
                '@' => self.gm().insert_cells(p1(params, 1)),
                'A' => self.gm().row_dec_clamp(p1(params, 1)),
                'B' => self.gm().row_inc_clamp(p1(params, 1)),
                'C' => self.gm().col_inc_clamp(p1(params, 1)),
                'D' => self.gm().col_dec(p1(params, 1)),
                'E' => {
                    self.gm().col_set(0);
                    self.gm().row_inc_clamp(p1(params, 1))
                }
                'F' => {
                    self.gm().col_set(0);
                    self.gm().row_dec_clamp(p1(params, 1))
                }
                'G' => self.gm().col_set(p1(params, 1) - 1),
                'H' => {
                    let (r, c) = p2(params, 1, 1);
                    self.gm().set_pos(Pos { row: r - 1, col: c - 1 })
                }
                'J' => self.ed(p1(params, 0)),
                'K' => self.el(p1(params, 0)),
                'L' => self.gm().insert_lines(p1(params, 1)),
                'M' => self.gm().delete_lines(p1(params, 1)),
                'P' => self.gm().delete_cells(p1(params, 1)),
                'S' => self.gm().scroll_up(p1(params, 1)),
                'T' => self.gm().scroll_down(p1(params, 1)),
                'X' => {
                    let a = self.cur;
                    self.gm().erase_cells(p1(params, 1), a)
                }
                'd' => self.gm().row_set(p1(params, 1) - 1),
                'm' => {
                    self.sgr(params);
                    self.sync_attr();
                }
                'r' => {
                    let rows = self.size().0;
                    let (t, b) = p2(params, 1, rows);
                    self.gm().set_scroll_region(t - 1, b - 1);
                }
                _ => {}
            },
            Some(b'?') => match c {
                'J' => self.ed(p1(params, 0)),
                'K' => self.el(p1(params, 0)),
                'h' => self.decset(params, true),
                'l' => self.decset(params, false),
                _ => {}
            },
            Some(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    //! The dispatcher: each sequence checked for what it sets, and against vt100 for the same bytes.

    use super::super::testing::{after, check, oracle};
    use super::super::{Color, TTY_COOKED};

    fn color(c: vt100::Color) -> Color {
        match c {
            vt100::Color::Default => Color::Default,
            vt100::Color::Idx(i) => Color::Idx(i),
            vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
        }
    }

    #[test]
    fn sgr_sets_the_pen() {
        let bytes = b"\x1b[1;31;42mA\x1b[0mB\x1b[38;5;200;48;2;1;2;3mC\x1b[22;2;4;7mD\x1b[39;49;24;27mE\x1b[95;104mF\x1b[38;5;300mG\x1b[38;2;1mH\x1b[mI";
        let t = after(1, 20, bytes);
        let p = oracle(1, 20, bytes);
        let v = t.view();
        for col in 0..9u16 {
            let a = v.attrs_of(v.row(0).get(col).unwrap());
            let o = p.screen().cell(0, col).unwrap();
            assert_eq!((a.fg, a.bg), (color(o.fgcolor()), color(o.bgcolor())), "colors of column {col}");
            assert_eq!((a.bold(), a.dim(), a.underline(), a.inverse()), (o.bold(), o.dim(), o.underline(), o.inverse()), "modes of column {col}");
        }
        let a = |col| v.attrs_of(v.row(0).get(col).unwrap());
        assert!(a(0).bold() && a(0).fg == Color::Idx(1) && a(0).bg == Color::Idx(2));
        assert_eq!(a(1), Default::default(), "0 resets");
        assert_eq!((a(2).fg, a(2).bg), (Color::Idx(200), Color::Rgb(1, 2, 3)));
        assert!(!a(3).bold() && a(3).dim() && a(3).underline() && a(3).inverse(), "22 then 2: dim");
        assert_eq!((a(5).fg, a(5).bg), (Color::Idx(13), Color::Idx(12)), "bright colors");
        assert_eq!(a(8), Default::default(), "an empty SGR resets");
    }

    #[test]
    fn cursor_positions_are_one_based_and_clamped() {
        for (seq, at) in [
            (&b"\x1b[H"[..], (0, 0)),
            (b"\x1b[2;3H", (1, 2)),
            (b"\x1b[99;99H", (3, 9)),
            (b"\x1b[0;0H", (0, 0)),
            (b"\x1b[;5H", (0, 4)),
            (b"\x1b[3d", (2, 0)),
            (b"\x1b[7G", (0, 6)),
        ] {
            let t = after(4, 10, seq);
            let c = t.view().cursor();
            assert_eq!((c.row, c.col), at, "{seq:?}");
            check(4, 10, seq);
        }
        // Relative moves stop at the edges.
        check(4, 10, b"\x1b[2;2H\x1b[9A\x1b[9D\x1b[3B\x1b[99C\x1b[E\x1b[F");
    }

    #[test]
    fn origin_mode_counts_from_the_scroll_region() {
        let bytes = b"\x1b[2;4r\x1b[?6h\x1b[1;1Hx\x1b[9;1Hy\x1b[?6l\x1b[1;1Hz";
        assert_eq!(check(5, 3, bytes), ["z", "x", "", "y", ""]);
    }

    #[test]
    fn modes_turn_on_and_off() {
        let mut t = after(2, 10, b"\x1b[?1h\x1b[?2004h\x1b[?25l");
        assert_eq!(t.modes(), (true, true));
        assert!(t.view().hide_cursor());
        t.process(b"\x1b[?1l\x1b[?2004l\x1b[?25h");
        assert_eq!(t.modes(), (false, false));
        assert!(!t.view().hide_cursor());
        // Several in one sequence.
        t.process(b"\x1b[?1;2004h");
        assert_eq!(t.modes(), (true, true));
    }

    #[test]
    fn the_shims_osc_sets_the_tty_modes() {
        let mut t = after(1, 10, b"");
        t.set_tty(TTY_COOKED);
        t.process(b"\x1b]zeropty-tty;0\x07");
        assert_eq!(t.tty(), 0, "raw");
        t.process(b"\x1b]zeropty-tty;255\x07");
        assert_eq!(t.tty(), TTY_COOKED, "unknown bits dropped");
        t.process(b"\x1b]zeropty-tty;x\x07\x1b]other;1\x07");
        assert_eq!(t.tty(), TTY_COOKED, "not a number, or not ours: unchanged");
        assert_eq!(super::super::testing::rows(t.view()), [""], "nothing printed");
    }

    #[test]
    fn escapes_save_restore_and_reset() {
        check(3, 10, b"ab\x1b7\x1b[3;5Hcd\x1b8ef");
        check(3, 10, b"\x1b[3;1Hx\x1bMy\x1bDz\x1bEw");
        assert_eq!(check(3, 10, b"text\x1b[1mbold\x1bcafter"), ["after", "", ""], "RIS clears everything");
    }
}
