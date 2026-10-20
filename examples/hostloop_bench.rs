//! What a host's loop and its commands cost with no plugin registered, to compare builds (the plugin
//! system must cost nothing when unused): cargo run --release --example hostloop_bench
//!
//! - `Polled` idle turn: `process()` then `timeout_ms()`, back to back, with one quiet session (the turn
//!   a host's loop takes when its timer fires: HostLoop, the services, the plugins' check).
//! - `Polled` busy turn: the same on another engine whose session replays recorded claude output.
//! - `Polled::command` and `Embedded::command`: display-message, back to back (the command path's checks).
//!
//! N (default 200000) turns or commands each; ROUNDS (default 5) repeats, reporting each round's median.
use std::time::{Duration, Instant};
use zeropty::embed::{Embedded, Polled};
use zeropty::engine::Options;

fn argv(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

/// Nanoseconds per call of `f`, timed in batches of 100 (one call is below the clock's resolution).
fn per_call(n: usize, mut f: impl FnMut()) -> Duration {
    let mut batches = Vec::with_capacity(n / 100);
    for _ in 0..n / 100 {
        let t = Instant::now();
        for _ in 0..100 {
            f();
        }
        batches.push(t.elapsed() / 100);
    }
    median(batches)
}

fn main() {
    let n: usize = std::env::var("N").ok().and_then(|v| v.parse().ok()).unwrap_or(200_000);
    let rounds: usize = std::env::var("ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
    let trace = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/replay_trace.pl");
    let stream = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/streams/claude2.trace");
    let dir = std::env::temp_dir().join(format!("zeropty-hostloop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let opts = || Options { size_dir: dir.clone(), ..Options::default() };

    let mut p = Polled::start(opts());
    p.command(&argv(&["new-session", "-d", "-s", "quiet", "-x", "120", "-y", "40", "exec cat"]), "/");
    let mut b = Polled::start(opts());
    b.command(&argv(&["new-session", "-d", "-s", "busy", "-x", "220", "-y", "80", &format!("exec perl {trace} {stream} 0.3")]), "/");
    let e = Embedded::start(opts());
    e.command(&argv(&["new-session", "-d", "-s", "quiet", "-x", "120", "-y", "40", "exec cat"]), "/");
    std::thread::sleep(Duration::from_millis(500));
    let display = argv(&["display-message", "-p", "-t", "=quiet:", "#{session_name}"]);
    let mut timeout = 0u64;
    for round in 0..rounds {
        // Busy: the replay keeps output coming, so turns read and parse.
        let busy = per_call(n / 20, || {
            b.process();
            timeout = timeout.wrapping_add(b.timeout_ms());
        });
        // Idle: one quiet session, nothing due but the timers.
        let idle = per_call(n, || {
            p.process();
            timeout = timeout.wrapping_add(p.timeout_ms());
        });
        let polled_cmd = per_call(n, || {
            std::hint::black_box(p.command(&display, "/"));
        });
        let embedded_cmd = per_call(n, || {
            std::hint::black_box(e.command(&display, "/"));
        });
        println!("round {round}: polled idle turn {idle:?}, polled busy turn {busy:?}, Polled::command {polled_cmd:?}, Embedded::command {embedded_cmd:?}");
    }
    std::hint::black_box(timeout);
    p.command(&argv(&["kill-server"]), "/");
    b.command(&argv(&["kill-server"]), "/");
    e.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&dir);
}
