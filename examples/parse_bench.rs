//! ns per byte for zeropty::term (printed as term.rs) vs vt100 on the benchmark's two workloads: recorded claude output and the
//! clear-and-print-31-lines loop. Run: cargo run --release --example parse_bench
use std::time::Instant;
use zeropty::term::Term;

// As in the zeropty server.
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn perl_frame(i: usize) -> Vec<u8> {
    let mut s = format!("\x1b[H\x1b[2J\x1b[1;32mtick {i}\x1b[0m \x1b[38;5;208mstatus\x1b[0m\n");
    for n in 1..=30 {
        s.push_str(&format!("line {n} {}\n", "x".repeat(80)));
    }
    s.into_bytes()
}

fn main() {
    let claude = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/streams/claude1.raw")).unwrap();
    let frames: Vec<u8> = (0..200).flat_map(perl_frame).collect();
    // bench/bulk.sh's 5.4 MB: 60,000 colored lines, read in one go (Sess::slurp) then parsed at once.
    let bulk: Vec<u8> = (0..60_000).flat_map(|i| format!("\x1b[{}mline {i:05} {}\x1b[0m\n", 31 + i % 7, "x".repeat(64)).into_bytes()).collect();
    let reps: usize = std::env::var("REPS").ok().and_then(|v| v.parse().ok()).unwrap_or(10);
    let t = Instant::now();
    for _ in 0..reps {
        let mut ours = Term::new(80, 220, 2000);
        ours.set_onlcr(true);
        ours.process(&bulk);
        ours.pack_live();
    }
    let a = t.elapsed().as_nanos() as f64 / (reps * bulk.len()) as f64;
    println!("bulk    term.rs {a:6.2} ns/byte   ({:.1} ms for {:.1} MB)", a * bulk.len() as f64 / 1e6, bulk.len() as f64 / 1e6);
    if std::env::args().any(|a| a == "bulk") {
        return;
    }
    // What the server does per read in bench/load.sh's `claude` load: a frozen screen thawed, the whole
    // 15 KB recording parsed in one read, its rows packed, the screen frozen again.
    let mut frozen = {
        let mut t = Term::new(80, 220, 2000);
        t.set_onlcr(true);
        t.process(&claude);
        t.freeze()
    };
    let cycles: usize = std::env::var("CYCLES").ok().and_then(|v| v.parse().ok()).unwrap_or(3000);
    let t = Instant::now();
    for _ in 0..cycles {
        let mut live = Term::thaw(frozen);
        live.process(&claude);
        live.pack_live();
        frozen = live.freeze();
    }
    let per = t.elapsed().as_nanos() as f64 / cycles as f64;
    println!("cycle   term.rs {:6.2} ns/byte   ({:.1} us per thaw + parse + pack + freeze of {} B)", per / claude.len() as f64, per / 1e3, claude.len());
    if std::env::args().any(|a| a == "cycle") {
        return;
    }
    for (name, data) in [("claude", &claude), ("frames", &frames)] {
        // Feed in 90-byte reads, about what a line-buffered writer produces per pipe read.
        // MB=<n>: feed n MB of each (40 by default), e.g. long enough to profile.
        let mb: usize = std::env::var("MB").ok().and_then(|v| v.parse().ok()).unwrap_or(40);
        let reps = mb * 1_000_000 / data.len() + 1;
        let t = Instant::now();
        let mut ours = Term::new(80, 220, 2000);
        for _ in 0..reps {
            for chunk in data.chunks(90) {
                ours.process(chunk);
            }
        }
        let a = t.elapsed().as_nanos() as f64 / (reps * data.len()) as f64;
        let t = Instant::now();
        let mut theirs = vt100::Parser::new(80, 220, 2000);
        for _ in 0..reps {
            for chunk in data.chunks(90) {
                theirs.process(chunk);
            }
        }
        let b = t.elapsed().as_nanos() as f64 / (reps * data.len()) as f64;
        println!("{name:7} term.rs {a:6.2} ns/byte   vt100 {b:6.2} ns/byte");
    }
}
