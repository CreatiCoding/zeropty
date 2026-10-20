//! The engine embedded in a Rust program, no server: cargo run --release --example embed
//!
//! Starts a shell as a session, types a command into it, waits for the answer and reads the screen.
use std::time::{Duration, Instant};
use zeropty::embed::Embedded;
use zeropty::engine::{Options, Reply};

fn main() {
    let zp = Embedded::start(Options::default());
    let run = |args: &[&str]| -> Reply { zp.command(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "/") };

    // A session is a program on pipes: `sh` here, any program works (`exec cat`, a coding agent...).
    let r = run(&["new-session", "-d", "-s", "demo", "-x", "60", "-y", "10", "sh"]);
    assert_eq!(r.code, 0, "{}", r.err);

    // Keys go through the engine's line discipline, as through a PTY: Enter ends the line.
    run(&["send-keys", "-t", "=demo:", "-l", "echo \"answer: $((6 * 7))\""]);
    run(&["send-keys", "-t", "=demo:", "Enter"]);

    // capture-pane reads the output pending first, so the screen is never stale.
    let start = Instant::now();
    let screen = loop {
        let screen = run(&["capture-pane", "-p", "-t", "=demo:"]).out;
        if screen.contains("answer: 42") || start.elapsed() > Duration::from_secs(5) {
            break screen;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    print!("{}", screen.trim_end_matches('\n'));
    println!();

    // Every session's name and size, in tmux's format language.
    print!("{}", run(&["list-sessions", "-F", "#{session_name} #{window_width}x#{window_height}"]).out);

    // kill-server ends the sessions; dropping `zp` stops the engine's thread.
    run(&["kill-server"]);
}
