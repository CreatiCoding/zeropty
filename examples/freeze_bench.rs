//! Cost of one freeze + thaw of a screen full of claude output: cargo run --release --example freeze_bench
use std::time::Instant;
use zeropty::term::Term;
fn main() {
    let data = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/streams/claude1.raw")).unwrap();
    for replays in [1, 5, 20] {
        let mut t = Term::new(80, 220, 2000);
        for _ in 0..replays {
            t.process(&data)
        }
        let n = 2000;
        let (mut f_ns, mut t_ns, mut p_ns) = (0u128, 0u128, 0u128);
        let mut size = 0;
        for _ in 0..n {
            let a = Instant::now();
            let f = t.freeze();
            f_ns += a.elapsed().as_nanos();
            size = f.bytes();
            let a = Instant::now();
            t = Term::thaw(f);
            t_ns += a.elapsed().as_nanos();
            let a = Instant::now();
            t.process(&data);
            p_ns += a.elapsed().as_nanos();
        }
        println!(
            "{replays:2} replays: freeze {:6.1} us  thaw {:6.1} us  process one replay {:6.1} us  frozen {size} B",
            f_ns as f64 / n as f64 / 1000.0,
            t_ns as f64 / n as f64 / 1000.0,
            p_ns as f64 / n as f64 / 1000.0
        );
    }
}
