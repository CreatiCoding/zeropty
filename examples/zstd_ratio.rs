//! How much zstd shrinks frozen screens: cargo run --release --example zstd_ratio
use zeropty::term::Term;
fn main() {
    let mut blobs = vec![];
    for name in ["claude1", "claude2"] {
        let data = std::fs::read(format!("{}/tests/streams/{name}.raw", env!("CARGO_MANIFEST_DIR"))).unwrap();
        // Screens at several points of each recording.
        for cut in [0.25, 0.5, 0.75, 1.0] {
            let mut t = Term::new(80, 220, 500);
            t.process(&data[..(data.len() as f64 * cut) as usize]);
            blobs.push((format!("{name}@{cut}"), t.freeze().data().to_vec()));
        }
    }
    for (name, b) in &blobs {
        let l1 = zstd::bulk::compress(b, 1).unwrap().len();
        let l3 = zstd::bulk::compress(b, 3).unwrap().len();
        let l19 = zstd::bulk::compress(b, 19).unwrap().len();
        println!("{name:14} {:5} B -> zstd1 {l1:5}  zstd3 {l3:5}  zstd19 {l19:5}", b.len());
    }
}
