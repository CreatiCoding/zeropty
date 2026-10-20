//! The `zeropty` process's parts (see src/main.rs).

pub mod attach;
pub mod client;
pub mod heap;
pub mod help;
pub mod plugin;
pub mod serve;

use std::path::PathBuf;

/// The server's socket: ZEROPTY_SOCK, else ~/.zeropty/mux.sock.
pub fn sock_path() -> PathBuf {
    if let Ok(p) = std::env::var("ZEROPTY_SOCK") {
        return p.into();
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".zeropty").join("mux.sock")
}

/// Writes to stdout, ignoring errors: `print!` panics on EPIPE ('failed printing to stdout: Broken
/// pipe'), which `zeropty capture-pane -p -t w | head -1` hits once `head` has closed the pipe. The
/// command already ran; the exit code stays the reply's.
pub fn stdout_quiet(s: &str) {
    use std::io::Write as _;
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(s.as_bytes()).and_then(|()| out.flush());
}
