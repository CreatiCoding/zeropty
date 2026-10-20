//! Replays the recorded claude output in a loop, for profiling: cargo run --release --example claude_loop
fn main() {
    let data = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/streams/claude1.raw")).unwrap();
    let mut t = zeropty::term::Term::new(80, 220, 2000);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(12);
    while std::time::Instant::now() < until {
        for c in data.chunks(512) {
            t.process(c)
        }
    }
}
