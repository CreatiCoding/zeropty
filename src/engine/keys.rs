//! send-keys' key names as the bytes an xterm sends for them (checked against tmux 3).

/// A tmux key name as the bytes an xterm sends for it: modifiers C-, M- and S- (or ^x for C-x) before a
/// name or a character, names case-insensitive. A modified arrow, Home, End, Insert, Delete, page or
/// F key is CSI 1;m X or CSI n;m ~ with m = 1 + shift + 2 alt + 4 ctrl; M- before anything else is an
/// ESC before it. Anything that is not a key name goes as the text it is (as in tmux). Checked against
/// what tmux 3 sends for the same names (S-Tab is a plain Tab there too; BTab is the back tab).
/// `app_cursor`: the program asked for application cursor keys (arrows as ESC O x).
pub(super) fn key_bytes(key: &str, app_cursor: bool) -> Vec<u8> {
    let (mut ctrl, mut alt, mut shift) = (false, false, false);
    let mut k = key;
    loop {
        let b = k.as_bytes();
        if b.len() > 2 && b[1] == b'-' {
            match b[0] {
                b'C' | b'c' => ctrl = true,
                b'M' | b'm' => alt = true,
                b'S' | b's' => shift = true,
                _ => break,
            }
            k = &k[2..];
        } else if b.len() == 2 && b[0] == b'^' {
            ctrl = true;
            k = &k[1..];
        } else {
            break;
        }
    }
    let m = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(ctrl);
    // CSI 1;m X for a modified key, the plain form otherwise.
    let csi_letter = |x: u8, plain: &[u8]| if m > 1 { format!("\x1b[1;{m}{}", x as char).into_bytes() } else { plain.to_vec() };
    let csi_tilde = |n: u8| if m > 1 { format!("\x1b[{n};{m}~").into_bytes() } else { format!("\x1b[{n}~").into_bytes() };
    let arrow = |x: u8| {
        if m > 1 {
            csi_letter(x, &[])
        } else if app_cursor {
            vec![0x1b, b'O', x]
        } else {
            vec![0x1b, b'[', x]
        }
    };
    let esc = |mut v: Vec<u8>| {
        if alt {
            v.insert(0, 0x1b);
        }
        v
    };
    let name = k.to_ascii_lowercase();
    let named = match name.as_str() {
        "up" => Some(arrow(b'A')),
        "down" => Some(arrow(b'B')),
        "right" => Some(arrow(b'C')),
        "left" => Some(arrow(b'D')),
        "home" => Some(csi_letter(b'H', b"\x1b[H")),
        "end" => Some(csi_letter(b'F', b"\x1b[F")),
        "ic" | "insert" => Some(csi_tilde(2)),
        "dc" | "delete" => Some(csi_tilde(3)),
        "pageup" | "ppage" | "pgup" => Some(csi_tilde(5)),
        "pagedown" | "npage" | "pgdn" => Some(csi_tilde(6)),
        "f1" | "f2" | "f3" | "f4" => {
            let x = b'P' + (name.as_bytes()[1] - b'1');
            Some(if m > 1 { csi_letter(x, &[]) } else { vec![0x1b, b'O', x] })
        }
        "f5" => Some(csi_tilde(15)),
        "f6" => Some(csi_tilde(17)),
        "f7" => Some(csi_tilde(18)),
        "f8" => Some(csi_tilde(19)),
        "f9" => Some(csi_tilde(20)),
        "f10" => Some(csi_tilde(21)),
        "f11" => Some(csi_tilde(23)),
        "f12" => Some(csi_tilde(24)),
        "enter" => Some(esc(b"\r".to_vec())),
        "escape" => Some(esc(b"\x1b".to_vec())),
        "tab" => Some(esc(b"\t".to_vec())),
        "btab" => Some(esc(b"\x1b[Z".to_vec())),
        "bspace" => Some(esc(b"\x7f".to_vec())),
        "space" => Some(esc(if ctrl { vec![0] } else { b" ".to_vec() })),
        _ => None,
    };
    if let Some(v) = named {
        return v;
    }
    // One character, maybe modified; a longer word with no key name is text.
    let mut chars = k.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else { return key.as_bytes().to_vec() };
    if !(ctrl || alt || shift) {
        return k.as_bytes().to_vec();
    }
    let c = if shift { c.to_ascii_uppercase() } else { c };
    let mut v = c.to_string().into_bytes();
    if ctrl && c.is_ascii() {
        let b = c as u8;
        v = vec![match b {
            b'a'..=b'z' => b - b'a' + 1,
            b'A'..=b'Z' => b - b'A' + 1,
            b'@' | b' ' | b'2' => 0,
            b'[' | b'3' => 0x1b,
            b'\\' | b'4' => 0x1c,
            b']' | b'5' => 0x1d,
            b'^' | b'6' => 0x1e,
            b'_' | b'7' | b'/' => 0x1f,
            b'?' | b'8' => 0x7f,
            _ => b,
        }];
    }
    esc(v)
}

#[cfg(test)]
mod tests {
    use super::key_bytes;

    /// What tmux 3 sends for these names.
    #[test]
    fn names_as_tmux_sends_them() {
        let cases: &[(&str, &[u8])] = &[
            ("Enter", b"\r"),
            ("enter", b"\r"),
            ("M-Enter", b"\x1b\r"),
            ("C-c", b"\x03"),
            ("^c", b"\x03"),
            ("C-Space", b"\x00"),
            ("C-[", b"\x1b"),
            ("C-?", b"\x7f"),
            ("M-x", b"\x1bx"),
            ("S-a", b"A"),
            ("Up", b"\x1b[A"),
            ("C-Left", b"\x1b[1;5D"),
            ("S-M-Up", b"\x1b[1;4A"),
            ("Home", b"\x1b[H"),
            ("S-End", b"\x1b[1;2F"),
            ("F1", b"\x1bOP"),
            ("S-F3", b"\x1b[1;2R"),
            ("F5", b"\x1b[15~"),
            ("C-F12", b"\x1b[24;5~"),
            ("PageUp", b"\x1b[5~"),
            ("NPage", b"\x1b[6~"),
            ("DC", b"\x1b[3~"),
            ("BTab", b"\x1b[Z"),
            ("BSpace", b"\x7f"),
            ("Tab", b"\t"),
            ("Escape", b"\x1b"),
            ("hello", b"hello"),
            ("C-hello", b"C-hello"),
            ("한", "한".as_bytes()),
            ("M-한", "\x1b한".as_bytes()),
        ];
        for (name, want) in cases {
            assert_eq!(key_bytes(name, false), *want, "{name}");
        }
    }

    #[test]
    fn arrows_follow_application_cursor_mode() {
        assert_eq!(key_bytes("Down", true), b"\x1bOB");
        assert_eq!(key_bytes("C-Down", true), b"\x1b[1;5B", "a modified arrow is the same in both modes");
    }
}
