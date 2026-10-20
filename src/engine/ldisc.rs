//! The line discipline a PTY applies to keys before its program reads them, done here since a session's
//! program reads a pipe: pure, keys in and what to do with them out (`Typed`); `Sess::input` carries it
//! out (writes, signals, closing the input, drawing the echo).
//!
//! The modes are crate::term::TTY_* (a new PTY's until the program changes them through the shim):
//! Enter becomes a line feed (ICRNL); C-c, C-\ and C-z signal the program's process group (ISIG); while
//! canonical (ICANON) keys gather into a line the program gets at Enter, BSpace, C-u and C-w edit it,
//! C-v takes the next key as it is and C-d on an empty line ends the input; what is typed shows on the
//! screen (ECHO). Without any of them (0, a program in raw mode such as claude), keys go as they are.

use super::options::LINE_MAX;
use crate::term::{TTY_ECHO, TTY_ICANON, TTY_ICRNL, TTY_ISIG};

/// What keys came to, in order.
#[derive(Default, Debug, PartialEq, Eq)]
pub(super) struct Typed {
    /// Bytes for the program.
    pub(super) out: Vec<u8>,
    /// What the tty echoes, to draw on the screen.
    pub(super) echo: Vec<u8>,
    /// Signals for the program's process group, each after `out[..at]`: (at, signal).
    pub(super) signals: Vec<(usize, i32)>,
    /// C-d on an empty line: the input ends after `out`.
    pub(super) eof: bool,
}

/// The line state a canonical tty keeps between keys.
#[derive(Default, Debug)]
pub(super) struct Line {
    /// The line being typed.
    pub(super) text: Vec<u8>,
    /// The next key goes in as it is (VLNEXT, C-v).
    pub(super) literal: bool,
}

/// Runs `keys` through the discipline of `tty` (not 0: a raw tty's keys go as they are, without this),
/// with `line` the line typed so far, updated.
pub(super) fn feed(tty: u8, line: &mut Line, keys: &[u8]) -> Typed {
    let mut t = Typed::default();
    let echoes = tty & TTY_ECHO != 0;
    let (text, literal) = (&mut line.text, &mut line.literal);
    for &key in keys {
        let b = if tty & TTY_ICRNL != 0 && key == b'\r' && !*literal { b'\n' } else { key };
        if *literal {
            *literal = false;
            if tty & TTY_ICANON != 0 {
                text.push(b)
            } else {
                t.out.push(b)
            }
            if echoes {
                echo_key(&mut t.echo, b)
            }
            continue;
        }
        if tty & TTY_ISIG != 0 {
            let sig = match b {
                0x03 => libc::SIGINT,
                0x1c => libc::SIGQUIT,
                0x1a => libc::SIGTSTP,
                _ => 0,
            };
            if sig != 0 {
                // The line typed so far is dropped, as a tty flushes its input (NOFLSH unset).
                text.clear();
                if echoes {
                    echo_key(&mut t.echo, b)
                }
                t.signals.push((t.out.len(), sig));
                continue;
            }
        }
        if tty & TTY_ICANON == 0 {
            t.out.push(b);
            if echoes {
                echo_key(&mut t.echo, b)
            }
            continue;
        }
        match b {
            // VERASE: the last character, its whole UTF-8 sequence.
            0x7f | 0x08 => {
                if let Some(w) = pop_char(text)
                    && echoes
                {
                    erase(&mut t.echo, w)
                }
            }
            // VKILL: the whole line.
            0x15 => {
                while let Some(w) = pop_char(text) {
                    if echoes {
                        erase(&mut t.echo, w)
                    }
                }
            }
            // VWERASE: back over blanks, then over the word.
            0x17 => {
                for word in [false, true] {
                    while text.last().is_some_and(|&c| (c == b' ' || c == b'\t') != word) {
                        let w = pop_char(text).unwrap_or(0);
                        if echoes {
                            erase(&mut t.echo, w)
                        }
                    }
                }
            }
            0x16 => *literal = true,
            // VEOF: the line so far without a line feed; on an empty line, the end of input.
            0x04 => {
                if text.is_empty() {
                    t.eof = true;
                    break;
                }
                t.out.append(text);
            }
            b'\n' => {
                text.push(b'\n');
                t.out.append(text);
                if echoes {
                    t.echo.push(b'\n')
                }
            }
            // A line longer than LINE_MAX takes no more (a tty's is 1024 bytes on macOS).
            _ if text.len() >= LINE_MAX => {}
            _ => {
                text.push(b);
                if echoes {
                    echo_key(&mut t.echo, b)
                }
            }
        }
    }
    t
}

/// A key as a tty echoes it: control characters as ^X (ECHOCTL), tab and line feed as they are.
fn echo_key(echo: &mut Vec<u8>, b: u8) {
    if b < 0x20 && b != b'\t' && b != b'\n' || b == 0x7f {
        echo.extend_from_slice(&[b'^', b ^ 0x40]);
    } else {
        echo.push(b);
    }
}

/// The last character of `line` (all of a UTF-8 sequence), and the columns its echo took.
fn pop_char(line: &mut Vec<u8>) -> Option<usize> {
    use unicode_width::UnicodeWidthChar as _;
    let mut start = line.len().checked_sub(1)?;
    while start > 0 && line[start] & 0xc0 == 0x80 {
        start -= 1;
    }
    let width = match std::str::from_utf8(&line[start..]).ok().and_then(|s| s.chars().next()) {
        Some(c) if (c as u32) < 0x20 || c as u32 == 0x7f => {
            if c == '\t' {
                1
            } else {
                2
            }
        }
        Some(c) => c.width().unwrap_or(1),
        None => 1,
    };
    line.truncate(start);
    Some(width)
}

/// Rubs out a character's echo `w` columns wide (ECHOE).
fn erase(echo: &mut Vec<u8>, w: usize) {
    for _ in 0..w {
        echo.extend_from_slice(b"\x08 \x08");
    }
}

#[cfg(test)]
mod tests {
    use super::{Line, Typed, feed};
    use crate::term::{TTY_COOKED, TTY_ECHO, TTY_ICANON, TTY_ISIG};

    fn typed(tty: u8, line: &mut Line, keys: &[u8]) -> Typed {
        feed(tty, line, keys)
    }

    #[test]
    fn cooked_lines_go_at_enter() {
        let mut l = Line::default();
        let t = typed(TTY_COOKED, &mut l, b"hel");
        assert_eq!((t.out.as_slice(), t.echo.as_slice()), (&b""[..], &b"hel"[..]));
        let t = typed(TTY_COOKED, &mut l, b"lo\r");
        assert_eq!(t.out, b"hello\n", "Enter is a line feed (ICRNL)");
        assert_eq!(t.echo, b"lo\n");
        assert!(l.text.is_empty());
    }

    #[test]
    fn editing_keys() {
        let mut l = Line::default();
        let t = typed(TTY_COOKED, &mut l, "ab 한\x7f\x7fc".as_bytes());
        assert_eq!(l.text, b"abc");
        assert_eq!(t.echo, "ab 한\x08 \x08\x08 \x08\x08 \x08c".as_bytes(), "a wide character rubs out two columns");
        typed(TTY_COOKED, &mut l, b" two  \x17");
        assert_eq!(l.text, b"abc ", "C-w: blanks, then the word");
        typed(TTY_COOKED, &mut l, b"\x15");
        assert!(l.text.is_empty(), "C-u: the whole line");
        let t = typed(TTY_COOKED, &mut l, b"\x16\x03x\r");
        assert_eq!(t.out, b"\x03x\n", "C-v: the next key as it is");
        assert!(t.signals.is_empty());
        assert_eq!(t.echo, b"^Cx\n");
    }

    #[test]
    fn signals_come_in_order() {
        let mut l = Line::default();
        let t = typed(TTY_ISIG, &mut l, b"ab\x03cd\x1a");
        assert_eq!(t.out, b"abcd");
        assert_eq!(t.signals, [(2, libc::SIGINT), (4, libc::SIGTSTP)]);
        let t = typed(TTY_COOKED, &mut l, b"typed\x1c");
        assert!(l.text.is_empty(), "a signal drops the line");
        assert_eq!(t.signals, [(0, libc::SIGQUIT)]);
    }

    #[test]
    fn end_of_input() {
        let mut l = Line::default();
        let t = typed(TTY_COOKED, &mut l, b"part\x04");
        assert_eq!((t.out.as_slice(), t.eof), (&b"part"[..], false), "C-d hands the line over without a line feed");
        let t = typed(TTY_COOKED, &mut l, b"\x04more");
        assert!(t.eof, "C-d on an empty line ends the input");
        assert!(t.out.is_empty() && l.text.is_empty(), "nothing after it");
    }

    #[test]
    fn modes_one_at_a_time() {
        let mut l = Line::default();
        let t = typed(TTY_ECHO, &mut l, b"a\x01\r");
        assert_eq!((t.out.as_slice(), t.echo.as_slice()), (&b"a\x01\r"[..], &b"a^A^M"[..]), "not canonical: keys go at once");
        let t = typed(TTY_ICANON, &mut l, b"x\r\n");
        assert_eq!(t.out, b"x\r\n", "no ICRNL: CR stays in the line");
        assert!(t.echo.is_empty(), "no ECHO");
        let t = typed(TTY_ICANON, &mut l, &vec![b'y'; super::LINE_MAX + 10]);
        assert!(t.out.is_empty());
        assert_eq!(l.text.len(), super::LINE_MAX, "a line stops growing");
    }
}
