//! Bytes as the parser's fast paths see them: one UTF-8 character at a time and how many cells it takes.
//! Pure functions, checked against `std` and `unicode-width` in the tests below.

use unicode_width::UnicodeWidthChar as _;

/// The character at the start of `b` and its byte length, if it prints as one or two cells: printable
/// ASCII, or whole UTF-8 that vte would print (not U+FFFD or C1) and is not a combining mark or control.
pub(super) fn narrow_or_wide(b: &[u8]) -> Option<(u32, u8, usize)> {
    let x = *b.first()?;
    let (c, len) = if (0x20..0x7f).contains(&x) {
        return Some((u32::from(x), 1, 1));
    } else if x >= 0x80 {
        decode_utf8(b)?
    } else {
        return None;
    };
    if c == '\u{fffd}' || ('\u{80}'..'\u{a0}').contains(&c) {
        return None;
    }
    match fast_width(c) {
        Some(w @ 1..=2) => Some((u32::from(c), w as u8, len)),
        _ => None,
    }
}

/// unicode-width's answer, without its table walk for the blocks terminal output is full of. Each range
/// is checked against unicode-width in tests::fast_width_matches.
pub(super) fn fast_width(c: char) -> Option<usize> {
    match u32::from(c) {
        0xac00..=0xd7a3 => Some(2),                   // Hangul syllables
        0x4e00..=0x9fff => Some(2),                   // CJK unified ideographs
        0x3041..=0x3096 | 0x30a1..=0x30fa => Some(2), // hiragana, katakana
        0x2500..=0x257f => Some(1),                   // box drawing
        _ => c.width(),
    }
}

/// One complete, valid UTF-8 character at the start of `b`, by the same rules as `str::from_utf8` (no
/// overlong forms, surrogates or code points past U+10FFFF). None when incomplete or invalid.
pub(super) fn decode_utf8(b: &[u8]) -> Option<(char, usize)> {
    let cont = |i: usize, lo: u8, hi: u8| b.get(i).filter(|&&x| (lo..=hi).contains(&x)).map(|&x| u32::from(x & 0x3f));
    let lead = u32::from(b[0]);
    let (cp, len) = match b[0] {
        0xc2..=0xdf => ((lead & 0x1f) << 6 | cont(1, 0x80, 0xbf)?, 2),
        0xe0..=0xef => {
            let (lo, hi) = match b[0] {
                0xe0 => (0xa0, 0xbf),
                0xed => (0x80, 0x9f),
                _ => (0x80, 0xbf),
            };
            ((lead & 0x0f) << 12 | cont(1, lo, hi)? << 6 | cont(2, 0x80, 0xbf)?, 3)
        }
        0xf0..=0xf4 => {
            let (lo, hi) = match b[0] {
                0xf0 => (0x90, 0xbf),
                0xf4 => (0x80, 0x8f),
                _ => (0x80, 0xbf),
            };
            ((lead & 0x07) << 18 | cont(1, lo, hi)? << 12 | cont(2, 0x80, 0xbf)? << 6 | cont(3, 0x80, 0xbf)?, 4)
        }
        _ => return None,
    };
    Some((char::from_u32(cp)?, len))
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthChar as _;

    #[test]
    fn fast_width_matches() {
        for cp in [0xac00..=0xd7a3u32, 0x4e00..=0x9fff, 0x3041..=0x3096, 0x30a1..=0x30fa, 0x2500..=0x257f].into_iter().flatten() {
            let c = char::from_u32(cp).unwrap();
            assert_eq!(super::fast_width(c), c.width(), "U+{cp:04X}");
        }
    }

    /// decode_utf8 accepts exactly what std does: every 2- and 3-byte sequence, and a sample of 4-byte ones.
    #[test]
    fn decode_utf8_matches_std() {
        let check = |b: &[u8]| {
            let ours = super::decode_utf8(b);
            // The first character, if some prefix of `b` is exactly one valid character.
            let std = (1..=b.len()).find_map(|k| std::str::from_utf8(&b[..k]).ok().filter(|s| s.chars().count() == 1).map(|s| (s.chars().next().unwrap(), k)));
            assert_eq!(ours, std, "{b:02x?}");
        };
        for a in 0x80..=0xffu8 {
            for b in 0..=0xffu8 {
                check(&[a, b]);
                for c in 0..=0xffu8 {
                    check(&[a, b, c]);
                }
            }
        }
        let mut x: u32 = 1;
        for _ in 0..1_000_000 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let b = x.to_le_bytes();
            check(&[0xf0 | (b[0] & 0x07), b[1], b[2], b[3]]);
        }
    }
    /// The fast path's view of a character: printable ASCII and whole narrow or wide characters only.
    #[test]
    fn narrow_or_wide_takes_only_what_prints() {
        assert_eq!(super::narrow_or_wide(b"a"), Some((u32::from('a'), 1, 1)));
        assert_eq!(super::narrow_or_wide("한".as_bytes()), Some((u32::from('한'), 2, 3)));
        assert_eq!(super::narrow_or_wide("─x".as_bytes()), Some((u32::from('─'), 1, 3)));
        assert_eq!(super::narrow_or_wide(b"\x1b["), None, "a control");
        assert_eq!(super::narrow_or_wide(b"\xea\xb0"), None, "an incomplete character");
        assert_eq!(super::narrow_or_wide("\u{301}".as_bytes()), None, "a combining mark has no width");
        assert_eq!(super::narrow_or_wide("\u{85}".as_bytes()), None, "C1 controls are vte's");
        assert_eq!(super::narrow_or_wide("\u{fffd}".as_bytes()), None, "U+FFFD is vte's");
        assert_eq!(super::narrow_or_wide(b""), None);
    }
}
