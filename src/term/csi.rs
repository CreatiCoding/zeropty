//! `Csi`: the parameters of a CSI sequence, as groups of a value and its ':' subparameters, the way vte
//! hands them over. `Csi::parse` reads a complete sequence straight from the bytes, by vte's rules, so the
//! parser's fast path can dispatch it without vte (over half of what claude writes is CSI sequences).

/// CSI parameters as vte hands them over (groups of a value and its ':' subparameters), but fillable
/// without vte, so complete sequences can skip it.
pub(super) struct Csi {
    vals: [u16; 32],
    /// (start, len) of each group in `vals`
    groups: [(u8, u8); 32],
    n: usize,
}

impl Csi {
    pub(super) const fn empty() -> Self {
        Self { vals: [0; 32], groups: [(0, 0); 32], n: 0 }
    }

    pub(super) fn from_vte(params: &vte::Params) -> Self {
        let mut c = Self::empty();
        let mut used = 0;
        for g in params {
            c.groups[c.n] = (used as u8, g.len() as u8);
            c.vals[used..used + g.len()].copy_from_slice(g);
            used += g.len();
            c.n += 1;
        }
        c
    }

    pub(super) fn is_empty(&self) -> bool {
        self.n == 0
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &[u16]> {
        self.groups[..self.n].iter().map(|&(s, l)| &self.vals[usize::from(s)..usize::from(s) + usize::from(l)])
    }

    /// Parses `ESC [ ... final` the way vte does, for sequences vte would dispatch with nothing ignored:
    /// a private marker (< = > ?) only first, at most 32 values, at most 2 intermediates in all.
    /// Returns (params, intermediates, final byte, length), or None to leave the bytes to vte.
    pub(super) fn parse(c: &mut Self, b: &[u8]) -> Option<([u8; 2], usize, u8, usize)> {
        // Only what gets written is valid; nothing reads past `n` groups.
        c.n = 0;
        let (mut inter, mut ni) = ([0u8; 2], 0);
        let mut i = 2;
        if let Some(&m) = b.get(i).filter(|m| (0x3c..=0x3f).contains(*m)) {
            inter[0] = m;
            ni = 1;
            i += 1;
        }
        let (mut used, mut param, mut group_start) = (0usize, 0u16, 0usize);
        loop {
            let x = *b.get(i)?;
            match x {
                b'0'..=b'9' => param = param.saturating_mul(10).saturating_add(u16::from(x - b'0')),
                b':' | b';' => {
                    if used == 32 {
                        return None;
                    }
                    c.vals[used] = param;
                    used += 1;
                    param = 0;
                    if x == b';' {
                        c.groups[c.n] = (group_start as u8, (used - group_start) as u8);
                        c.n += 1;
                        group_start = used;
                    }
                }
                _ => break,
            }
            i += 1;
        }
        while let Some(&x) = b.get(i).filter(|x| (0x20..=0x2f).contains(*x)) {
            if ni == 2 {
                return None;
            }
            inter[ni] = x;
            ni += 1;
            i += 1;
        }
        let fin = *b.get(i).filter(|x| (0x40..=0x7e).contains(*x))?;
        if used == 32 {
            return None;
        }
        c.vals[used] = param;
        used += 1;
        c.groups[c.n] = (group_start as u8, (used - group_start) as u8);
        c.n += 1;
        Some((inter, ni, fin, i + 1))
    }
}

/// The first value of parameter `k`, or 0. Every group holds at least its main value.
pub(super) fn param(params: &Csi, k: usize) -> u16 {
    if k < params.n { params.vals[usize::from(params.groups[k].0)] } else { 0 }
}

/// Parameter 1, or `default` when absent or 0.
pub(super) fn p1(params: &Csi, default: u16) -> u16 {
    let first = param(params, 0);
    if first == 0 { default } else { first }
}

/// Parameters 1 and 2, each defaulted as `p1` does.
pub(super) fn p2(params: &Csi, d1: u16, d2: u16) -> (u16, u16) {
    let (a, b) = (param(params, 0), param(params, 1));
    (if a == 0 { d1 } else { a }, if b == 0 { d2 } else { b })
}

#[cfg(test)]
mod tests {
    use super::Csi;

    /// What vte dispatches for a CSI sequence: (params, intermediates, final).
    #[derive(Default)]
    struct Seen(Vec<(Vec<Vec<u16>>, Vec<u8>, char)>);

    impl vte::Perform for Seen {
        fn csi_dispatch(&mut self, params: &vte::Params, intermediates: &[u8], ignore: bool, c: char) {
            assert!(!ignore);
            self.0.push((params.iter().map(<[u16]>::to_vec).collect(), intermediates.to_vec(), c));
        }
    }

    /// Csi::parse agrees with vte on every sequence it takes, and leaves to vte those it cannot be sure of.
    #[test]
    fn parse_matches_vte() {
        let taken: &[&[u8]] = &[
            b"\x1b[m",
            b"\x1b[0m",
            b"\x1b[1;31m",
            b"\x1b[38;5;208m",
            b"\x1b[38:2::1:2:3m",
            b"\x1b[?1049h",
            b"\x1b[?25l",
            b"\x1b[2J",
            b"\x1b[10;20H",
            b"\x1b[;5H",
            b"\x1b[>4;1m",
            b"\x1b[ q",
            b"\x1b[99999999m",
            b"\x1b[=5u",
        ];
        for seq in taken {
            let mut vte = vte::Parser::new();
            let mut seen = Seen::default();
            vte.advance(&mut seen, seq);
            let mut c = Csi::empty();
            let (inter, ni, fin, len) = Csi::parse(&mut c, seq).unwrap_or_else(|| panic!("{seq:?} taken"));
            assert_eq!(len, seq.len(), "{seq:?}");
            let ours = (c.iter().map(<[u16]>::to_vec).collect::<Vec<_>>(), inter[..ni].to_vec(), char::from(fin));
            assert_eq!(seen.0, vec![ours], "{seq:?}");
        }
        let left: &[&[u8]] = &[b"\x1b[1;2", b"\x1b[", b"\x1b[1 2 3m", b"\x1b[1\x07m", b"\x1b[?1?2h"];
        for seq in left {
            assert!(Csi::parse(&mut Csi::empty(), seq).is_none(), "{seq:?} goes to vte");
        }
        let many = format!("\x1b[{}m", ["1"; 33].join(";"));
        assert!(Csi::parse(&mut Csi::empty(), many.as_bytes()).is_none(), "past 32 values vte ignores");
    }

    #[test]
    fn defaults() {
        let mut c = Csi::empty();
        Csi::parse(&mut c, b"\x1b[0;7H").unwrap();
        assert_eq!(super::p1(&c, 1), 1, "0 means the default");
        assert_eq!(super::p2(&c, 1, 1), (1, 7));
        assert_eq!(super::param(&c, 5), 0, "absent");
        Csi::parse(&mut c, b"\x1b[m").unwrap();
        assert_eq!(super::p1(&c, 3), 3);
    }
}
