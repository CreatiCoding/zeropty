//! What a capture of a quiet (deep-frozen) session costs, back to back, embedded (BUSY=1: a busy one):
//! cargo run --release --example capture_bench
use std::time::Instant;
use zeropty::embed::Embedded;
use zeropty::engine::Options;

fn main() {
    let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let e = Embedded::start(Options { history: 2000, ..Options::default() });
    let raw = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/streams/claude1.raw");
    // BUSY=1: a session replaying real claude work instead (captured while it prints).
    let busy = std::env::var("BUSY").is_ok();
    let trace = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/replay_trace.pl");
    let stream = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/streams/claude2.trace");
    let cmd = if busy { format!("exec perl {trace} {stream} 0.3") } else { format!("cat {raw}; exec sleep 1000") };
    e.command(&argv(&["new-session", "-d", "-s", "q", "-x", "220", "-y", "80", &cmd]), "/");
    std::thread::sleep(std::time::Duration::from_secs(if busy { 2 } else { 12 }));
    let n: u32 = std::env::var("N").ok().and_then(|v| v.parse().ok()).unwrap_or(2000);
    for args in [&["capture-pane", "-p", "-t", "=q:"][..], &["capture-pane", "-p", "-e", "-S", "-200", "-t", "=q:"][..]] {
        let gap: u64 = std::env::var("GAP_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        let mut v = Vec::new();
        for _ in 0..n {
            if gap > 0 {
                std::thread::sleep(std::time::Duration::from_millis(gap));
            }
            let t = Instant::now();
            e.command(&argv(args), "/");
            v.push(t.elapsed());
        }
        v.sort();
        println!("{:?}: p50 {:?} p95 {:?}", args, v[v.len() / 2], v[v.len() * 95 / 100]);
    }
    e.command(&argv(&["kill-server"]), "/");
}
