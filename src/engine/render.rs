//! Screens as text: capture-pane's output (plain or with SGR colors), the bytes that draw a screen on an
//! attached terminal, and one row's text for searching. Pure functions of a `Screen`: they read it
//! without changing it (packed rows are decoded into scratch), so they run on frozen screens' copies and
//! from any thread holding the engine.
//!
//! capture-pane's output must stay byte-identical for the same screen (AGENTS.md): it is what callers
//! diff and search.

use super::args::Args;
use super::reply::{Reply, fail};
use crate::term::{Attrs, Cell, Color, Screen};

/// What capture-pane was asked for (its flags).
#[derive(Default)]
pub(super) struct Capture<'a> {
    /// -e: colors and attributes as SGR sequences.
    pub(super) ansi: bool,
    /// -J: wrapped rows joined into one line.
    pub(super) join: bool,
    /// -N: every row's whole width, trailing blanks included.
    pub(super) keep_trailing: bool,
    /// -S and -E: the first and last line (0 is the screen's first row; negative ones the scrollback;
    /// "-" all of it or to the screen's end).
    pub(super) start: Option<&'a str>,
    pub(super) end: Option<&'a str>,
}

impl<'a> Capture<'a> {
    pub(super) fn from_args(a: &'a Args) -> Self {
        Self { ansi: a.has('e'), join: a.has('J'), keep_trailing: a.has('N'), start: a.get('S'), end: a.get('E') }
    }
}

/// capture-pane's output for `screen`: the lines from -S to -E (swapped when backwards), each ending in
/// '\n' unless joined. An -S or -E that is not a number fails.
pub(super) fn capture(screen: &Screen, c: &Capture) -> Result<String, Reply> {
    let (rows, cols) = screen.size();
    // Lines as tmux numbers them: 0 the screen's first row, negative ones the scrollback ("-": all of
    // it, or to the screen's last row), the range from -S to -E (swapped when backwards).
    let hist = screen.history_size() as i64;
    let last = i64::from(rows) - 1;
    let line = |flag: char, v: Option<&str>, all: i64, default: i64| -> Result<i64, Reply> {
        match v {
            None => Ok(default),
            Some("-") => Ok(all),
            Some(v) => v.parse::<i64>().map_err(|_| fail(format!("capture-pane: -{flag} line invalid: {v}\n"))),
        }
    };
    let top = line('S', c.start, -hist, 0)?.clamp(-hist, last);
    let bottom = line('E', c.end, last, last)?.clamp(-hist, last);
    let (top, bottom) = if bottom < top { (bottom, top) } else { (top, bottom) };
    // Room for the screen as text up front: grown a piece at a time it was reallocated a dozen times.
    let mut out = String::with_capacity((bottom - top + 1) as usize * (usize::from(cols) / 2 + 1));
    let mut sgr = Sgr::new();
    let mut emit = |cells: &[Cell], wrapped: bool| {
        row_text(&mut out, screen, cells, cols, c.ansi, c.keep_trailing, &mut sgr);
        if !(c.join && wrapped) {
            out.push('\n');
        }
    };
    if top < 0 {
        // The last -top rows of the scrollback, oldest first, up to `bottom` when it is in there too.
        let mut left = bottom.min(-1) - top + 1;
        screen.visit_history((-top) as usize, &mut |cells, w| {
            if left > 0 {
                left -= 1;
                emit(cells, w)
            }
        });
    }
    if bottom >= 0 {
        let mut r = 0i64;
        screen.visit_rows(&mut |cells, w| {
            if r >= top && r <= bottom {
                emit(cells, w)
            }
            r += 1;
        });
    }
    Ok(out)
}

/// Bytes that draw `screen` on a terminal of its size: every row with its colors, then the cursor and
/// the modes keys depend on (application cursor, bracketed paste).
pub(super) fn redraw(screen: &Screen) -> Vec<u8> {
    let (_, cols) = screen.size();
    let mut out = String::from("\x1b[0m\x1b[?25l\x1b[H\x1b[2J");
    let mut sgr = Sgr::new();
    let mut r = 0;
    screen.visit_rows(&mut |cells, _| {
        r += 1;
        out.push_str(&format!("\x1b[{r};1H"));
        row_text(&mut out, screen, cells, cols, true, false, &mut sgr);
    });
    let p = screen.cursor();
    out.push_str(&format!("\x1b[{};{}H", p.row + 1, p.col + 1));
    out.push_str(if screen.application_cursor() { "\x1b[?1h" } else { "\x1b[?1l" });
    out.push_str(if screen.bracketed_paste() { "\x1b[?2004h" } else { "\x1b[?2004l" });
    if !screen.hide_cursor() {
        out.push_str("\x1b[?25h");
    }
    out.into_bytes()
}

/// Calls `f` with the text of `screen`'s rows (only those written since they were last packed, unless
/// `all`), as capture-pane prints them without colors.
pub(super) fn visit_text(screen: &Screen, all: bool, f: &mut dyn FnMut(&str)) {
    let (_, cols) = screen.size();
    let mut line = String::new();
    let mut sgr = Sgr::new();
    let mut look = |cells: &[Cell], _: bool| {
        line.clear();
        row_text(&mut line, screen, cells, cols, false, false, &mut sgr);
        f(&line);
    };
    if all { screen.visit_rows(&mut look) } else { screen.visit_changed_rows(&mut look) }
}

/// Appends the SGR sequence for `a` to `out`: "\x1b[0;...m", a reset then each attribute.
fn sgr_into(out: &mut String, a: &Attrs) {
    use std::fmt::Write as _;
    out.push_str("\x1b[0");
    for (on, p) in [(a.bold(), ";1"), (a.dim(), ";2"), (a.italic(), ";3"), (a.underline(), ";4"), (a.inverse(), ";7")] {
        if on {
            out.push_str(p);
        }
    }
    for (c, base) in [(a.fg, 30u8), (a.bg, 40)] {
        let _ = match c {
            Color::Default => Ok(()),
            Color::Idx(i) if i < 8 => write!(out, ";{}", base + i),
            Color::Idx(i) if i < 16 => write!(out, ";{}", base + 60 + i - 8),
            Color::Idx(i) => write!(out, ";{};5;{i}", base + 8),
            Color::Rgb(r, g, b) => write!(out, ";{};2;{r};{g};{b}", base + 8),
        };
    }
    out.push('m');
}

const RESET: &str = "\x1b[0m";

/// What row_text carries across rows: the last cell's palette id and its SGR, and the SGR in effect on
/// the output. Kept as reused strings: building and copying one per change of attributes was most of a
/// busy screen's capture after the cells themselves.
struct Sgr {
    attr: u16,
    cur: String,
    /// `cur` is a reset.
    reset: bool,
    last: String,
}

impl Sgr {
    fn new() -> Self {
        Self { attr: 0, cur: RESET.into(), reset: true, last: String::new() }
    }
}

/// One row as capture-pane prints it, appended to `out` (newline not included). `sg` carries the last
/// cell's palette id and its SGR across rows: building an SGR string per cell made capturing a screen with
/// 200 rows of history take milliseconds. Printable ASCII, most of a screen, goes straight in, and SGR
/// strings are compared only when a cell's attributes differ from the last one's.
fn row_text(out: &mut String, screen: &Screen, cells: &[Cell], cols: u16, ansi: bool, keep_trailing: bool, sg: &mut Sgr) {
    // The SGR in effect on the output: a reset at the start of each row.
    sg.last.clear();
    sg.last.push_str(RESET);
    let mut same = sg.reset;
    let mut pending_spaces = 0usize;
    // Cells past the row's end are default blanks: they would only be trailing blanks, which are dropped;
    // so are blanks in the default attributes at its end (claude's rows are mostly those), skipped here
    // without the work below. `sg` caches by palette id, so skipping them changes nothing it carries.
    let mut end = cells.len().min(usize::from(cols));
    while !keep_trailing && end > 0 && cells[end - 1].is_blank() && cells[end - 1].attr() == 0 {
        end -= 1;
    }
    for cell in &cells[..end] {
        if cell.is_wide_continuation() {
            continue;
        }
        if ansi && cell.attr() != sg.attr {
            sg.attr = cell.attr();
            sg.cur.clear();
            sgr_into(&mut sg.cur, &screen.attrs_of(cell));
            sg.reset = sg.cur == RESET;
            same = sg.cur == sg.last;
        }
        let ascii = cell.ascii();
        let blank = cell.is_blank();
        let text = if ascii.is_none() && !blank { Some(cell.contents()) } else { None };
        // Trailing blanks are dropped (tmux does the same); blanks with a background still count.
        if blank && (!ansi || sg.reset) {
            pending_spaces += 1;
            continue;
        }
        if pending_spaces > 0 {
            if ansi && sg.last != RESET {
                out.push_str(RESET);
                sg.last.clear();
                sg.last.push_str(RESET);
                same = sg.reset;
            }
            out.extend(std::iter::repeat_n(' ', pending_spaces));
            pending_spaces = 0;
        }
        if ansi && !same {
            out.push_str(&sg.cur);
            sg.last.clear();
            sg.last.push_str(&sg.cur);
            same = true;
        }
        match (ascii, &text) {
            (Some(b), _) => out.push(char::from(b)),
            (None, _) if blank => out.push(' '),
            (None, Some(t)) => out.push_str(t),
            (None, None) => unreachable!(),
        }
    }
    if keep_trailing {
        // capture-pane -N: the row's whole width, its trailing blanks and the cells past its end.
        if ansi && sg.last != RESET {
            out.push_str(RESET);
            sg.last.clear();
            sg.last.push_str(RESET);
        }
        out.extend(std::iter::repeat_n(' ', pending_spaces + usize::from(cols) - end));
    }
    if ansi && sg.last != RESET {
        out.push_str(RESET);
    }
}

#[cfg(test)]
mod tests {
    use super::{Capture, capture, redraw, visit_text};
    use crate::term::Term;

    fn term(bytes: &[u8]) -> Term {
        let mut t = Term::new(4, 20, 10);
        t.set_onlcr(true);
        t.process(bytes);
        t
    }

    #[test]
    fn plain_and_colored() {
        let t = term(b"plain  \n\x1b[1;31mred\x1b[0m and \x1b[44m  \x1b[0m\n\x1b[38;5;208mx\x1b[38;2;1;2;3my");
        let plain = capture(t.view(), &Capture::default()).unwrap();
        assert_eq!(plain, "plain\nred and\nxy\n\n", "trailing blanks dropped, blanks with a background too");
        let ansi = capture(t.view(), &Capture { ansi: true, ..Capture::default() }).unwrap();
        assert_eq!(ansi, "plain\n\x1b[0;1;31mred\x1b[0m and \x1b[0;44m  \x1b[0m\n\x1b[0;38;5;208mx\x1b[0;38;2;1;2;3my\x1b[0m\n\n",);
        let wide = capture(t.view(), &Capture { keep_trailing: true, ..Capture::default() }).unwrap();
        assert!(wide.lines().all(|l| l.len() == 20), "{wide:?}");
    }

    #[test]
    fn ranges_and_joined_rows() {
        // History: 0 1 2. Screen: 3, 4, then one line wrapped over two rows.
        let t = term(b"0\n1\n2\n3\n4\n5 a row long enough_to wrap");
        let c = |s: Option<&str>, e: Option<&str>, join: bool| capture(t.view(), &Capture { start: s, end: e, join, ..Capture::default() }).unwrap();
        assert_eq!(c(None, None, false), "3\n4\n5 a row long enough_\nto wrap\n");
        assert_eq!(c(Some("-"), Some("-1"), false), "0\n1\n2\n");
        assert_eq!(c(Some("-2"), Some("0"), false), "1\n2\n3\n");
        assert_eq!(c(Some("1"), Some("-1"), false), "2\n3\n4\n", "swapped when backwards");
        assert_eq!(c(Some("2"), Some("3"), true), "5 a row long enough_to wrap\n");
        assert_eq!(c(Some("-99"), Some("99"), false).lines().count(), 7, "clamped to what there is");
        let bad = capture(t.view(), &Capture { end: Some("x"), ..Capture::default() }).unwrap_err();
        assert_eq!(bad.err, "capture-pane: -E line invalid: x\n");
    }

    #[test]
    fn wide_and_combined() {
        let t = term("한글 e\u{301}!".as_bytes());
        assert_eq!(capture(t.view(), &Capture::default()).unwrap(), "한글 e\u{301}!\n\n\n\n");
    }

    #[test]
    fn redraw_draws_every_row_and_the_modes() {
        let t = term(b"\x1b[?1h\x1b[?2004hhi\n\x1b[32mthere");
        let r = String::from_utf8(redraw(t.view())).unwrap();
        assert!(r.starts_with("\x1b[0m\x1b[?25l\x1b[H\x1b[2J\x1b[1;1Hhi\x1b[2;1H\x1b[0;32mthere\x1b[0m\x1b[3;1H\x1b[4;1H"), "{r:?}");
        assert!(r.ends_with("\x1b[2;6H\x1b[?1h\x1b[?2004h\x1b[?25h"), "{r:?}");
    }

    #[test]
    fn text_of_changed_rows() {
        let mut t = term(b"one\ntwo");
        t.pack_live();
        t.process(b"\x1b[1;1Hnew");
        let mut seen = vec![];
        visit_text(t.view(), false, &mut |l| seen.push(l.to_string()));
        assert_eq!(seen, ["new"], "only the row written since");
        seen.clear();
        visit_text(t.view(), true, &mut |l| seen.push(l.to_string()));
        assert_eq!(seen, ["new", "two", "", ""]);
    }
}
