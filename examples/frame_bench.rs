//! What a session costs per redraw when it goes quiet between frames, as in the server (parse, then
//! freeze after 250 ms, then thaw at the next output): cargo run --release --example frame_bench
use std::time::Instant;
use zeropty::term::{Frozen, Term};

fn active_frame(i: usize) -> Vec<u8> {
    let mut f = format!("\x1b[H\x1b[2J\x1b[1;32mtick {i}\x1b[0m \x1b[38;5;208mstatus\x1b[0m\n").into_bytes();
    for n in 1..=30 {
        f.extend_from_slice(format!("line {n} {}\n", "x".repeat(80)).as_bytes());
    }
    f
}

fn run(name: &str, frames: &[Vec<u8>]) {
    let n = 200;
    let mut held: Vec<Option<Frozen>> = (0..n).map(|_| Some(Term::new(80, 220, 2000).freeze())).collect();
    let (mut p, mut f, mut t) = (0u128, 0u128, 0u128);
    let rounds: usize = std::env::var("ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(60);
    for r in 0..rounds + 5 {
        let frame = &frames[r % frames.len()];
        for h in held.iter_mut() {
            let a = Instant::now();
            let mut term = Term::thaw(h.take().unwrap());
            let b = Instant::now();
            term.process(frame);
            term.pack_live();
            let c = Instant::now();
            *h = Some(term.freeze());
            let d = Instant::now();
            if r >= 5 {
                t += (b - a).as_nanos();
                p += (c - b).as_nanos();
                f += (d - c).as_nanos();
            }
        }
    }
    let per = |x: u128| x as f64 / (rounds * n) as f64 / 1000.0;
    println!("{name:8} per frame: process {:6.2} us  freeze {:6.2} us  thaw {:6.2} us", per(p), per(f), per(t));
}

fn main() {
    if std::env::var_os("ONLY_CLAUDE").is_none() {
        run("active", &(0..10).map(active_frame).collect::<Vec<_>>());
    }
    let claude = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/streams/claude1.raw")).unwrap();
    run("claude", &[claude]);
}
