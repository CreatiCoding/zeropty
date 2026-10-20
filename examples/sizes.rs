//! Sizes of a frozen and a live screen: cargo run --release --example sizes
use zeropty::term::{Frozen, Term};
fn main() {
    for name in ["claude1", "claude2"] {
        let data = std::fs::read(format!("{}/tests/streams/{name}.raw", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let mut t = Term::new(80, 220, 500);
        for c in data.chunks(4096) {
            t.process(c)
        }
        t.pack_all();
        let live = t.approx_bytes();
        let f = t.freeze();
        println!("{name}: live approx {live} B, frozen {} B", f.bytes());
    }
    println!("size_of Term {} Frozen {}", std::mem::size_of::<Term>(), std::mem::size_of::<Frozen>());
}
