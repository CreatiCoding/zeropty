//! The primitives every packed format here is written with (frozen screens, `Term::export`, a grid's
//! parked rows): single bytes, little-endian u16, and LEB128 lengths, plus length-prefixed byte strings.
//!
//! `In` trusts its input: it indexes and panics past the end. Bytes from this process are always
//! complete; bytes from another (`Term::import`) are decoded under `catch_unwind`.

/// Appends `v` as LEB128: 7 bits a byte, low first, the high bit set on every byte but the last.
#[inline]
pub(super) fn put_var(out: &mut Vec<u8>, mut v: usize) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

/// A growing byte buffer to write fields into.
pub(super) struct Out(pub(super) Vec<u8>);

impl Out {
    pub(super) fn u8(&mut self, v: u8) {
        self.0.push(v)
    }

    pub(super) fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes())
    }

    /// LEB128 (see `put_var`).
    pub(super) fn var(&mut self, v: usize) {
        put_var(&mut self.0, v)
    }

    /// `bytes` after their length (LEB128).
    pub(super) fn field(&mut self, bytes: &[u8]) {
        self.var(bytes.len());
        self.0.extend_from_slice(bytes);
    }
}

/// A cursor over bytes written with `Out`: `at` is the next byte to read.
pub(super) struct In<'a> {
    pub(super) buf: &'a [u8],
    pub(super) at: usize,
}

impl<'a> In<'a> {
    pub(super) fn new(buf: &'a [u8]) -> Self {
        Self { buf, at: 0 }
    }

    pub(super) fn u8(&mut self) -> u8 {
        self.at += 1;
        self.buf[self.at - 1]
    }

    pub(super) fn u16(&mut self) -> u16 {
        u16::from_le_bytes([self.u8(), self.u8()])
    }

    pub(super) fn var(&mut self) -> usize {
        let (mut v, mut shift) = (0usize, 0);
        loop {
            let b = self.u8();
            v |= usize::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return v;
            }
            shift += 7;
        }
    }

    /// The next `n` bytes.
    pub(super) fn take(&mut self, n: usize) -> &'a [u8] {
        let b = &self.buf[self.at..self.at + n];
        self.at += n;
        b
    }

    /// A field written by `Out::field`.
    pub(super) fn field(&mut self) -> &'a [u8] {
        let n = self.var();
        self.take(n)
    }

    /// Whether every byte was read.
    pub(super) fn at_end(&self) -> bool {
        self.at >= self.buf.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{In, Out};

    #[test]
    fn var_is_leb128() {
        for (v, bytes) in [(0usize, &[0u8][..]), (127, &[0x7f]), (128, &[0x80, 1]), (300, &[0xac, 2]), (1 << 21, &[0x80, 0x80, 0x80, 1])] {
            let mut o = Out(Vec::new());
            o.var(v);
            assert_eq!(o.0, bytes, "{v}");
            assert_eq!(In::new(bytes).var(), v);
        }
    }

    #[test]
    fn fields_round_trip() {
        let mut o = Out(Vec::new());
        o.u8(7);
        o.u16(0xbeef);
        o.field(b"hello");
        o.field(&[]);
        o.var(usize::MAX);
        let mut i = In::new(&o.0);
        assert_eq!(i.u8(), 7);
        assert_eq!(i.u16(), 0xbeef);
        assert_eq!(i.field(), b"hello");
        assert_eq!(i.field(), b"");
        assert!(!i.at_end());
        assert_eq!(i.var(), usize::MAX);
        assert!(i.at_end());
    }

    #[test]
    #[should_panic]
    fn reading_past_the_end_panics() {
        In::new(&[3, b'a']).field();
    }
}
