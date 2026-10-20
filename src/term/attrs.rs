//! What a cell looks like apart from its text: colors and the SGR modes (bold, dim, italic, underline,
//! inverse). `Attrs::key` packs one set into a u64, the form palettes store and compare.

use super::codec::{In, Out};

/// A foreground or background color as SGR sets it.
///
/// ```
/// use zeropty::term::{Color, Term};
///
/// let mut t = Term::new(1, 10, 0);
/// t.process(b"\x1b[31ma\x1b[38;5;200mb\x1b[38;2;1;2;3mc\x1b[39md");
/// let screen = t.screen();
/// let fg: Vec<Color> = screen.row(0).cells().iter().map(|c| screen.attrs_of(c).fg).collect();
/// assert_eq!(fg, [Color::Idx(1), Color::Idx(200), Color::Rgb(1, 2, 3), Color::Default]);
/// ```
#[derive(Eq, PartialEq, Debug, Copy, Clone, Default, Hash)]
pub enum Color {
    /// The terminal's own.
    #[default]
    Default,
    /// One of the 256 indexed colors (0-15: the ANSI ones).
    Idx(u8),
    /// A 24-bit color.
    Rgb(u8, u8, u8),
}

const MODE_INTENSITY: u8 = 0b0000_0011;
const MODE_BOLD: u8 = 0b0000_0001;
const MODE_DIM: u8 = 0b0000_0010;
pub(super) const MODE_ITALIC: u8 = 0b0000_0100;
pub(super) const MODE_UNDERLINE: u8 = 0b0000_1000;
pub(super) const MODE_INVERSE: u8 = 0b0001_0000;

/// A cell's attributes: its colors and SGR modes (read through `bold()` and the like).
///
/// ```
/// use zeropty::term::{Color, Term};
///
/// let mut t = Term::new(1, 20, 0);
/// t.process(b"\x1b[3;4;31;48;2;1;2;3mx");
/// let screen = t.screen();
/// let a = screen.attrs_of(&screen.row(0).cells()[0]);
/// assert_eq!((a.fg, a.bg), (Color::Idx(1), Color::Rgb(1, 2, 3)));
/// assert!(a.italic() && a.underline() && !a.bold());
/// ```
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Attrs {
    /// Foreground.
    pub fg: Color,
    /// Background.
    pub bg: Color,
    /// MODE_* bits; bold and dim exclude each other, as in vt100.
    mode: u8,
}

impl Attrs {
    /// All of it in one integer: each color as a 2-bit kind and 24 bits, then the mode byte. Palettes hold
    /// these instead of Attrs, so lookups compare one word and claude's 80-odd colors take 8 bytes each.
    pub(super) fn key(&self) -> u64 {
        let c = |c: Color| -> u64 {
            match c {
                Color::Default => 0,
                Color::Idx(i) => 1 << 24 | u64::from(i),
                Color::Rgb(r, g, b) => 2 << 24 | u64::from(r) << 16 | u64::from(g) << 8 | u64::from(b),
            }
        };
        c(self.fg) | c(self.bg) << 26 | u64::from(self.mode) << 52
    }

    pub(super) fn from_key(k: u64) -> Self {
        let c = |v: u64| match v >> 24 & 3 {
            1 => Color::Idx(v as u8),
            2 => Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8),
            _ => Color::Default,
        };
        Self { fg: c(k & 0x3ff_ffff), bg: c(k >> 26 & 0x3ff_ffff), mode: (k >> 52) as u8 }
    }

    /// SGR 1.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 0);
    /// t.process(b"\x1b[1mx");
    /// let screen = t.screen();
    /// assert!(screen.attrs_of(&screen.row(0).cells()[0]).bold());
    /// ```
    pub fn bold(&self) -> bool {
        self.mode & MODE_BOLD != 0
    }
    /// SGR 2.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 0);
    /// t.process(b"\x1b[2mx");
    /// let screen = t.screen();
    /// assert!(screen.attrs_of(&screen.row(0).cells()[0]).dim());
    /// ```
    pub fn dim(&self) -> bool {
        self.mode & MODE_DIM != 0
    }
    /// SGR 3.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 0);
    /// t.process(b"\x1b[3mx");
    /// let screen = t.screen();
    /// assert!(screen.attrs_of(&screen.row(0).cells()[0]).italic());
    /// ```
    pub fn italic(&self) -> bool {
        self.mode & MODE_ITALIC != 0
    }
    /// SGR 4.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 0);
    /// t.process(b"\x1b[4mx");
    /// let screen = t.screen();
    /// assert!(screen.attrs_of(&screen.row(0).cells()[0]).underline());
    /// ```
    pub fn underline(&self) -> bool {
        self.mode & MODE_UNDERLINE != 0
    }
    /// SGR 7.
    ///
    /// ```
    /// let mut t = zeropty::term::Term::new(1, 10, 0);
    /// t.process(b"\x1b[7mx");
    /// let screen = t.screen();
    /// assert!(screen.attrs_of(&screen.row(0).cells()[0]).inverse());
    /// ```
    pub fn inverse(&self) -> bool {
        self.mode & MODE_INVERSE != 0
    }
    pub(super) fn set_bold(&mut self) {
        self.mode = (self.mode & !MODE_INTENSITY) | MODE_BOLD
    }
    pub(super) fn set_dim(&mut self) {
        self.mode = (self.mode & !MODE_INTENSITY) | MODE_DIM
    }
    pub(super) fn set_normal_intensity(&mut self) {
        self.mode &= !MODE_INTENSITY
    }
    pub(super) fn flag(&mut self, bit: u8, on: bool) {
        if on { self.mode |= bit } else { self.mode &= !bit }
    }

    /// Written as each color (a kind byte, then its bytes) and the mode byte.
    pub(super) fn encode(&self, o: &mut Out) {
        for c in [self.fg, self.bg] {
            match c {
                Color::Default => o.u8(0),
                Color::Idx(i) => {
                    o.u8(1);
                    o.u8(i)
                }
                Color::Rgb(r, g, b) => {
                    o.u8(2);
                    o.u8(r);
                    o.u8(g);
                    o.u8(b)
                }
            }
        }
        o.u8(self.mode);
    }

    pub(super) fn decode(i: &mut In) -> Self {
        let mut color = || match i.u8() {
            1 => Color::Idx(i.u8()),
            2 => Color::Rgb(i.u8(), i.u8(), i.u8()),
            _ => Color::Default,
        };
        let (fg, bg) = (color(), color());
        Self { fg, bg, mode: i.u8() }
    }
}

#[cfg(test)]
mod tests {
    use super::{Attrs, Color, MODE_INVERSE, MODE_ITALIC};
    use crate::term::codec::{In, Out};

    fn samples() -> Vec<Attrs> {
        let mut v = vec![Attrs::default()];
        for (fg, bg) in [(Color::Idx(1), Color::Default), (Color::Rgb(1, 2, 3), Color::Idx(255)), (Color::Default, Color::Rgb(255, 0, 128))] {
            let mut a = Attrs { fg, bg, mode: 0 };
            a.set_bold();
            a.flag(MODE_ITALIC, true);
            v.push(a);
            a.set_dim();
            a.flag(MODE_INVERSE, true);
            v.push(a);
        }
        v
    }

    #[test]
    fn key_round_trips_and_tells_sets_apart() {
        let all = samples();
        for a in &all {
            assert_eq!(Attrs::from_key(a.key()), *a);
            assert!(a.key() >> 63 == 0, "bit 63 is free (palette::NO_KEY)");
        }
        let keys: std::collections::HashSet<u64> = all.iter().map(Attrs::key).collect();
        assert_eq!(keys.len(), all.len());
        assert_eq!(Attrs::default().key(), 0, "the default set is key 0 (palette id 0)");
    }

    #[test]
    fn encode_round_trips() {
        let mut o = Out(Vec::new());
        samples().iter().for_each(|a| a.encode(&mut o));
        let mut i = In::new(&o.0);
        for a in samples() {
            assert_eq!(Attrs::decode(&mut i), a);
        }
        assert!(i.at_end());
    }

    #[test]
    fn bold_and_dim_exclude_each_other() {
        let mut a = Attrs::default();
        a.set_bold();
        a.set_dim();
        assert!(a.dim() && !a.bold());
        a.set_normal_intensity();
        assert!(!a.dim() && !a.bold());
    }
}
