//! What send-keys costs embedded, to a session at rest (frozen between keys, GAP_MS apart): one running
//! cat with the line discipline (cooked, echoing) and one in raw mode. cargo run --release --example sendkeys_bench
use std::time::{Duration, Instant};
use zeropty::embed::Embedded;
use zeropty::engine::Options;

fn main() {
    let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let e = Embedded::start(Options::default());
    e.command(&argv(&["new-session", "-d", "-s", "cooked", "exec cat"]), "/");
    e.command(&argv(&["new-session", "-d", "-s", "raw", "printf '\\033]zeropty-tty;0\\007'; exec cat"]), "/");
    std::thread::sleep(Duration::from_secs(1));
    let n: usize = std::env::var("N").ok().and_then(|v| v.parse().ok()).unwrap_or(60);
    let gap: u64 = std::env::var("GAP_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(300);
    let mut v = [Vec::new(), Vec::new()];
    for _ in 0..n {
        for (k, t) in ["=cooked:", "=raw:"].iter().enumerate() {
            std::thread::sleep(Duration::from_millis(gap / 2));
            let at = Instant::now();
            e.command(&argv(&["send-keys", "-t", t, "-l", "x"]), "/");
            v[k].push(at.elapsed());
        }
    }
    for (k, name) in ["cooked", "raw"].iter().enumerate() {
        v[k].sort();
        println!("send-keys {name:7} p50 {:?} p95 {:?}", v[k][n / 2], v[k][n * 95 / 100]);
    }
    e.command(&argv(&["kill-server"]), "/");
}
