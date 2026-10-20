//! zeropty::term must produce the same screen as the vt100 crate it was ported from: every cell's text, width
//! flags and attributes, row wrap flags, cursor, modes, and scrollback. Checked after every chunk of
//! recorded claude output (tests/streams/*.raw) and of random escape-heavy input.

use rand::{Rng, SeedableRng, rngs::StdRng};
use zeropty::term::{Color, Term};

fn color(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Default,
        vt100::Color::Idx(i) => Color::Idx(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn compare(ours: &mut Term, theirs: &mut vt100::Parser, ctx: &str) {
    let a = ours.screen();
    let (rows, cols) = a.size();
    let b = theirs.screen();
    assert_eq!((a.cursor().row, a.cursor().col), b.cursor_position(), "cursor {ctx}");
    assert_eq!(a.application_cursor(), b.application_cursor(), "app cursor {ctx}");
    assert_eq!(a.bracketed_paste(), b.bracketed_paste(), "bracketed paste {ctx}");
    assert_eq!(a.hide_cursor(), b.hide_cursor(), "hide cursor {ctx}");
    for r in 0..rows {
        let row = a.row(r);
        assert_eq!(row.wrapped(), b.row_wrapped(r), "wrapped row {r} {ctx}");
        for c in 0..cols {
            let x = row.get(c).unwrap();
            let y = b.cell(r, c).unwrap();
            let at = format!("cell {r},{c} {ctx}");
            assert_eq!(&*x.contents(), y.contents(), "contents {at}");
            assert_eq!(x.is_wide(), y.is_wide(), "wide {at}");
            assert_eq!(x.is_wide_continuation(), y.is_wide_continuation(), "wide cont {at}");
            let (xa, ya) = (&a.attrs_of(x), y);
            assert_eq!(xa.fg, color(ya.fgcolor()), "fg {at}");
            assert_eq!(xa.bg, color(ya.bgcolor()), "bg {at}");
            assert_eq!(
                (xa.bold(), xa.dim(), xa.italic(), xa.underline(), xa.inverse()),
                (ya.bold(), ya.dim(), ya.italic(), ya.underline(), ya.inverse()),
                "mode {at}"
            );
        }
    }
}

/// Scrollback, oldest first, read from vt100 one screenful at a time.
fn compare_history(ours: &mut Term, theirs: &mut vt100::Parser, ctx: &str) {
    let (rows, cols) = ours.screen().size();
    theirs.screen_mut().set_scrollback(usize::MAX);
    let total = theirs.screen().scrollback();
    let hist = ours.screen().history(usize::MAX);
    assert_eq!(hist.len(), total, "scrollback length {ctx}");
    let mut off = total;
    let mut i = 0;
    while off > 0 {
        theirs.screen_mut().set_scrollback(off);
        let n = off.min(usize::from(rows));
        for r in 0..n as u16 {
            let row = &hist[i];
            assert_eq!(row.wrapped(), theirs.screen().row_wrapped(r), "history wrapped {i} {ctx}");
            for c in 0..cols {
                let x = row.get(c).unwrap();
                // vt100 keeps a scrollback row at the width it had: past it, a resized screen has none.
                let Some(y) = theirs.screen().cell(r, c) else {
                    assert_eq!(&*x.contents(), "", "history contents past the row {i},{c} {ctx}");
                    assert!(!x.is_wide_continuation(), "history wc past the row {i},{c} {ctx}");
                    continue;
                };
                let xa = ours.screen().attrs_of(x);
                assert_eq!(&*x.contents(), y.contents(), "history contents {i},{c} {ctx}");
                assert_eq!(xa.fg, color(y.fgcolor()), "history fg {i},{c} {ctx}");
                assert_eq!(xa.bg, color(y.bgcolor()), "history bg {i},{c} {ctx}");
                assert_eq!(x.is_wide_continuation(), y.is_wide_continuation(), "history wc {i},{c} {ctx}");
            }
            i += 1;
        }
        off -= n;
    }
    theirs.screen_mut().set_scrollback(0);
}

/// Odd seeds read up to 4 KB at a time, like the server's 16 KB reads, so long sequences arrive whole;
/// even seeds read up to 64 bytes, so sequences and characters split across reads.
fn replay(data: &[u8], rows: u16, cols: u16, history: usize, seed: u64, name: &str) {
    let max_read = if seed % 2 == 1 { 4096 } else { 64 };
    let mut ours = Term::new(rows, cols, history);
    let mut theirs = vt100::Parser::new(rows, cols, history);
    let mut rng = StdRng::seed_from_u64(seed);
    // DIFF_OFF=resize,freeze,refreeze,pack_all,pack_live turns those steps off (the random draws stay,
    // so the reads are the same): which one a failure needs.
    let off = std::env::var("DIFF_OFF").unwrap_or_default();
    let on = |step: &str| !off.split(',').any(|o| o == step);
    let mut i = 0;
    while i < data.len() {
        // Odd chunk sizes split escape sequences and UTF-8 the way pipe reads do.
        let n = rng.random_range(1..=max_read).min(data.len() - i);
        // A resize now and then (an attached terminal of another size), on both, between reads.
        if rng.random_range(0..40) == 0 && on("resize") {
            let (r, mut c) = (rng.random_range(2..=rows + 10), rng.random_range(2..=cols + 20));
            // Not a width that cuts a wide character on screen: vt100 keeps the half and panics later
            // (zeropty clears it; see Grid::resize).
            let (now_rows, now_cols) = theirs.screen().size();
            if c < now_cols && (0..now_rows).any(|y| theirs.screen().cell(y, c - 1).is_some_and(|x| x.is_wide())) {
                c = now_cols;
            }
            // The same for the main screen under the alternate one, which vt100 does not show: no
            // narrowing then.
            if c < now_cols && theirs.screen().alternate_screen() {
                c = now_cols;
            }
            ours.resize(r, c);
            theirs.screen_mut().set_size(r, c);
        }
        // Freezing between reads must not change anything, whether or not the parser is mid-sequence.
        if rng.random_range(0..3) == 0 && on("freeze") {
            let mut f = ours.freeze();
            if rng.random_range(0..2) == 0 {
                f.deepen();
            }
            ours = Term::thaw(f);
        }
        ours.process(&data[i..i + n]);
        // Refreezing with rows still packed from the last thaw (their bytes are copied, not repacked).
        if rng.random_range(0..4) == 0 && on("refreeze") {
            ours = Term::thaw(Term::thaw(ours.freeze()).freeze());
        }
        // Packing quiet rows of a live screen: all of them (age 0), then carrying on with them packed.
        if rng.random_range(0..4) == 0 && on("pack_all") {
            ours.pack_all();
        }
        // What the server does after every read: changed rows appended to the blob, the blob rebuilt
        // once garbage is half of it.
        if rng.random_range(0..2) == 0 && on("pack_live") {
            ours.pack_live();
        }
        // vt100 gets one byte at a time: vte 0.15.0 drops a character when a read ends inside a UTF-8
        // character and the next read continues with another character (advance_partial_utf8 returns
        // the valid prefix length, not the first character's). Byte by byte it never looks ahead.
        for b in &data[i..i + n] {
            theirs.process(std::slice::from_ref(b));
        }
        compare(&mut ours, &mut theirs, &format!("{name} @{}", i + n));
        i += n;
    }
    let mut ours = Term::thaw(ours.freeze());
    compare(&mut ours, &mut theirs, &format!("{name} after freeze"));
    compare_history(&mut ours, &mut theirs, name);
}

#[test]
fn recorded_streams() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/streams");
    let mut count = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "raw") {
            let data = std::fs::read(&path).unwrap();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            // The recorded size, then smaller screens so the same output wraps and scrolls a lot.
            for (rows, cols) in [(40, 120), (12, 50), (5, 17)] {
                replay(&data, rows, cols, 300, 7, &format!("{name} {rows}x{cols}"));
            }
            count += 1;
        }
    }
    assert!(count > 0, "no recordings in {}", dir.display());
}

/// Random mixes of text (ASCII, Hangul, emoji, combining marks) and the control and escape sequences
/// both emulators handle, plus some neither does.
fn random_stream(rng: &mut StdRng, len: usize) -> Vec<u8> {
    const TEXT: &[&str] = &[
        "a",
        "Z",
        " ",
        "hello",
        "안녕",
        "한",
        "🎉",
        "⚠️",
        "👍🏽",
        "e\u{301}",
        "\u{301}",
        "\u{200d}",
        "中文",
        "─",
        "❯",
        "\t",
        "x\u{fe0f}",
        "👨‍👩‍👧",
        "\u{7f}",
        "\u{1b}",
        // Enough combining marks to reach vt100's 18-byte cell limit.
        "a\u{301}\u{302}\u{303}\u{304}\u{305}\u{306}\u{307}\u{308}\u{309}",
        // CAN and SUB, alone and aborting a sequence midway.
        "\u{18}",
        "\u{1a}",
        "\u{1b}[3\u{18}x",
        "\u{1b}]0;ti\u{1a}tle",
    ];
    let mut out = vec![];
    while out.len() < len {
        let n = |rng: &mut StdRng, hi: u32| rng.random_range(0..=hi).to_string();
        let piece: String = match rng.random_range(0..40) {
            0..=14 => TEXT[rng.random_range(0..TEXT.len())].to_string(),
            15 => "\r".into(),
            16 => "\n".into(),
            17 => "\x08".into(),
            18 => format!("\x1b[{};{}H", n(rng, 45), n(rng, 130)),
            19 => format!("\x1b[{}{}", n(rng, 5), ["A", "B", "C", "D", "E", "F", "G", "d"][rng.random_range(0..8)]),
            20 => format!("\x1b[{}J", n(rng, 3)),
            21 => format!("\x1b[{}K", n(rng, 3)),
            22 => format!("\x1b[{}{}", n(rng, 4), ["L", "M", "P", "@", "X", "S", "T"][rng.random_range(0..7)]),
            23 => {
                let codes = [
                    "0",
                    "1",
                    "2",
                    "3",
                    "4",
                    "7",
                    "22",
                    "23",
                    "24",
                    "27",
                    "31",
                    "39",
                    "42",
                    "49",
                    "91",
                    "103",
                    "38;5;208",
                    "48;5;17",
                    "38;2;10;20;30",
                    "48;2;1;2;3",
                    "38;5",
                    "38;2;1",
                ];
                format!("\x1b[{}m", codes[rng.random_range(0..codes.len())])
            }
            24 => format!("\x1b[{};{}r", n(rng, 20), n(rng, 45)),
            25 => ["\x1b7", "\x1b8", "\x1bM", "\x1b[s", "\x1b[u"][rng.random_range(0..5)].into(),
            26 => format!("\x1b[?{}{}", ["1", "25", "47", "1049", "2004", "6", "7"][rng.random_range(0..7)], ["h", "l"][rng.random_range(0..2)]),
            27 => "\x1b]0;title\x07".into(),
            28 => "\x1b[>4;2m\x1b[<u\x1b[>5u\x1b[>0q".into(),
            29 => "\x1b[?2026h\x1b[?2026l".into(),
            30 => format!("\x1b[{}t", n(rng, 30)),
            31 => "\x1bc".into(),
            // Edits on a wrapped row while the cursor waits past its last column.
            32 => format!("\x1b[999Gyy\x1b[A\x1b[999Gz\x1b[{}", ["P", "@", "X", "K"][rng.random_range(0..4)]),
            // CSI edge cases: marker not first, DEL inside, subparameters, too many values or intermediates.
            34 => [
                "\x1b[1?2m",
                "\x1b[3\x7f1m",
                "\x1b[38:5:208m",
                "\x1b[48:2::10:20:30m",
                "\x1b[?1;2004h",
                "\x1b[ !p",
                "\x1b[1 ! q",
                "\x1b[>4;2m",
                "\x1b[;5H",
                "\x1b[5;H",
                "\x1b[99999C",
                "\x1b[1:2;3:4K",
            ][rng.random_range(0..12)]
            .into(),
            35 => format!("\x1b[{}m", (0..rng.random_range(28..40)).map(|_| "1").collect::<Vec<_>>().join(";")),
            // A row that wraps with a wide character in its last two columns, then erased there.
            33 => format!("\x1b[999G\x1b[D中y\x1b[A\x1b[999G\x1b[D\x1b[{}", ["X", "1K", "P", "@"][rng.random_range(0..4)]),
            // Broken UTF-8: stray continuation bytes, overlong forms, surrogates, truncated characters.
            36 => {
                let bad: &[&[u8]] = &[
                    b"\x80",
                    b"\xbf\xbf",
                    b"\xc0\x80",
                    b"\xc1\xbf",
                    b"\xe0\x80\x80",
                    b"\xed\xa0\x80",
                    b"\xf4\x90\x80\x80",
                    b"\xf5\x80",
                    b"\xff",
                    b"\xe2\x94",
                    b"\xf0\x9f\x8e",
                    b"\xc2",
                ];
                out.extend_from_slice(bad[rng.random_range(0..bad.len())]);
                continue;
            }
            _ => "x".repeat(rng.random_range(1..150)),
        };
        out.extend_from_slice(piece.as_bytes());
    }
    out
}

#[test]
fn random_streams() {
    let seeds: u64 = std::env::var("SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(300);
    // SEED_FROM: start past a seed already known to fail.
    let from: u64 = std::env::var("SEED_FROM").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    // vt100 itself panics on a few streams (wide characters cut at the edge): nothing to compare
    // against there, so those seeds are counted and skipped; any other panic fails the test.
    thread_local!(static IN_VT100: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.location().is_some_and(|l| l.file().contains("vt100")) {
            IN_VT100.set(true);
        } else {
            hook(info);
        }
    }));
    let mut skipped = vec![];
    for seed in from..seeds {
        let mut rng = StdRng::seed_from_u64(seed);
        let data = random_stream(&mut rng, 4000);
        let rows = rng.random_range(2..30);
        let cols = rng.random_range(2..90);
        let name = format!("seed {seed} {rows}x{cols}");
        let history = rng.random_range(0..50);
        IN_VT100.set(false);
        if let Err(e) = std::panic::catch_unwind(|| replay(&data, rows, cols, history, seed, &name)) {
            if !IN_VT100.get() {
                std::panic::resume_unwind(e);
            }
            skipped.push(seed);
        }
    }
    let _ = std::panic::take_hook();
    assert!(skipped.len() * 100 <= (seeds - from) as usize, "vt100 panicked on too many seeds: {skipped:?}");
    if !skipped.is_empty() {
        eprintln!("random_streams: vt100 panicked on seeds {skipped:?}, skipped");
    }
}

/// More distinct attribute sets than a palette index holds (65536): the palette compacts and the screen
/// and scrollback still match vt100.
#[test]
fn palette_overflow() {
    let mut data = vec![];
    for i in 0..70_000u32 {
        let (r, g, b) = ((i >> 16) as u8, (i >> 8) as u8, i as u8);
        data.extend_from_slice(format!("\x1b[38;2;{r};{g};{b}m{}", (b'a' + (i % 26) as u8) as char).as_bytes());
        if i % 37 == 0 {
            data.extend_from_slice(b"\r\n");
        }
    }
    let mut ours = Term::new(10, 40, 100);
    let mut theirs = vt100::Parser::new(10, 40, 100);
    for (k, chunk) in data.chunks(4096).enumerate() {
        ours.process(chunk);
        theirs.process(chunk);
        // Packed copies made before a compaction carry old palette ids; they must not come back.
        match k % 3 {
            0 => ours = Term::thaw(ours.freeze()),
            1 => ours.pack_all(),
            _ => {}
        }
    }
    compare(&mut ours, &mut theirs, "palette overflow");
    compare_history(&mut ours, &mut theirs, "palette overflow");
    // The thread's shared attribute table is full now: a new screen mixes attributes it has (shared ids)
    // with new ones (it switches to keeping its own keys).
    let mut ours = Term::new(10, 40, 100);
    let mut theirs = vt100::Parser::new(10, 40, 100);
    let mut more = vec![];
    for i in 0..300u32 {
        let fresh = i % 2 == 0;
        let (r, g, b) = if fresh { (255, (i >> 8) as u8 ^ 0x5a, i as u8) } else { (0, 0, (i % 200) as u8) };
        more.extend_from_slice(format!("\x1b[38;2;{r};{g};{b}m{}", (b'a' + (i % 26) as u8) as char).as_bytes());
    }
    for chunk in more.chunks(97) {
        ours.process(chunk);
        theirs.process(chunk);
        ours = Term::thaw(ours.freeze());
    }
    compare(&mut ours, &mut theirs, "after the shared table filled");
}

/// A read that ends inside a UTF-8 character, followed by one that continues with more characters:
/// vte 0.15.0 alone drops the character after the completed one here.
#[test]
fn utf8_split_across_reads() {
    let mut t = Term::new(2, 10, 0);
    t.process(&[b'1', b' ', 0xc2]);
    t.process(&[0xb7, b' ', b'A']);
    let row: String = (0..5).map(|c| t.screen().row(0).get(c).unwrap().contents().to_string()).collect();
    assert_eq!(row, "1 · A");
}

#[test]
#[ignore]
fn shrink_seed() {
    let seed: u64 = std::env::var("SEED").unwrap().parse().unwrap();
    let mut rng = StdRng::seed_from_u64(seed);
    let data = random_stream(&mut rng, 4000);
    let rows = rng.random_range(2..30);
    let cols = rng.random_range(2..90);
    let history = rng.random_range(0..50);
    std::panic::set_hook(Box::new(|_| {}));
    let fails = |n: usize| std::panic::catch_unwind(|| replay(&data[..n], rows, cols, history, seed, "shrink")).is_err();
    let end = (1..=data.len()).find(|&n| fails(n)).expect("no failure");
    let _ = std::panic::take_hook();
    let from = end.saturating_sub(100);
    eprintln!("rows {rows} cols {cols} fails at byte {end}; bytes {from}..{end}: {:?}", String::from_utf8_lossy(&data[from..end]));
    std::panic::set_hook(Box::new(|_| {}));
    let err = std::panic::catch_unwind(|| replay(&data[..end], rows, cols, history, seed, "shrink")).unwrap_err();
    let _ = std::panic::take_hook();
    eprintln!("{}", err.downcast_ref::<String>().cloned().unwrap_or_default());
}

/// Rows packed before a palette compaction and not written after it: their packed bytes hold the old
/// palette ids, so the compaction must not let them be reused.
#[test]
fn palette_compaction_keeps_packed_rows_right() {
    let mut ours = Term::new(10, 40, 100);
    let mut theirs = vt100::Parser::new(10, 40, 100);
    let feed = |ours: &mut Term, theirs: &mut vt100::Parser, data: &[u8]| {
        ours.process(data);
        for b in data {
            theirs.process(std::slice::from_ref(b));
        }
    };
    let mut text = vec![];
    for r in 0..9u32 {
        for c in 0..20u32 {
            // An unused color first, so compaction renumbers the used ones (otherwise old and new ids match).
            text.extend_from_slice(format!("\x1b[38;2;99;{r};{c}m\x1b[38;2;{r};{c};7m{}", (b'a' + c as u8) as char).as_bytes());
        }
        text.extend_from_slice(b"\r\n");
    }
    feed(&mut ours, &mut theirs, &text);
    ours.pack_all();
    // Only colors, no text: the palette fills and compacts while the rows above stay as they are.
    let flood: Vec<u8> = (0..70_000u32).flat_map(|i| format!("\x1b[48;2;{};{};{}m", i >> 16, (i >> 8) & 255, i & 255).into_bytes()).collect();
    feed(&mut ours, &mut theirs, &flood);
    let mut ours = Term::thaw(ours.freeze());
    compare(&mut ours, &mut theirs, "after compaction");
}

/// Several screens parsed in turn, as the server does: whatever they share (the palette lookup cache, the
/// pooled row buffers) must not leak from one into another. Random streams use colors heavily, in a
/// different order per screen, so a shared cache entry from the wrong screen would show as wrong attrs.
#[test]
fn interleaved_screens() {
    let mut rng = StdRng::seed_from_u64(7);
    let streams: Vec<Vec<u8>> = (0..4).map(|_| random_stream(&mut rng, 20_000)).collect();
    let mut ours: Vec<Term> = (0..4).map(|_| Term::new(12, 50, 100)).collect();
    let mut theirs: Vec<vt100::Parser> = (0..4).map(|_| vt100::Parser::new(12, 50, 100)).collect();
    let mut at = [0usize; 4];
    while (0..4).any(|k| at[k] < streams[k].len()) {
        let k = rng.random_range(0..4);
        let n = rng.random_range(1..=200).min(streams[k].len() - at[k]);
        let chunk = &streams[k][at[k]..at[k] + n];
        ours[k].process(chunk);
        for b in chunk {
            theirs[k].process(std::slice::from_ref(b));
        }
        at[k] += n;
        match rng.random_range(0..6) {
            0 => ours[k].pack_live(),
            1 => {
                let t = std::mem::replace(&mut ours[k], Term::new(1, 1, 0));
                ours[k] = Term::thaw(t.freeze());
            }
            _ => {}
        }
        if rng.random_range(0..20) == 0 {
            compare(&mut ours[k], &mut theirs[k], &format!("screen {k} at {}", at[k]));
        }
    }
    for k in 0..4 {
        compare(&mut ours[k], &mut theirs[k], &format!("screen {k} at the end"));
    }
}
