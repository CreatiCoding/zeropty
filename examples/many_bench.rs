//! ns per byte when N screens take turns, as in the server: cargo run --release --example many_bench
use std::time::Instant;
use zeropty::term::Term;
fn main() {
    let data = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/streams/claude1.raw")).unwrap();
    for n in [1usize, 20, 200, 1000] {
        let mut terms: Vec<Term> = (0..n).map(|_| Term::new(80, 220, 2000)).collect();
        for t in terms.iter_mut() {
            t.process(&data)
        } // warm up: scrollback and rows allocated
        let rounds = (60_000_000 / (data.len() * n)).max(1);
        let t0 = Instant::now();
        for _ in 0..rounds {
            for t in terms.iter_mut() {
                t.process(&data)
            }
        }
        println!("{n:5} screens: {:6.2} ns/byte", t0.elapsed().as_nanos() as f64 / (rounds * n * data.len()) as f64);
    }
}
