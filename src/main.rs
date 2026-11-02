//! zeropty: tmux's session commands without PTYs, as a server and its client.
//!
//! Each session's program talks to plain pipes; shim/shim.dylib (loaded through zeropty-tty) or a runtime
//! adapter (docs/USAGE.md) makes those pipes look like a terminal. That sidesteps macOS's kernel cap of 999 PTYs
//! (PTMX_MAX_HARD in xnu tty_ptmx.c), so the session count is bounded only by memory.
//!
//! The binary is both the server (`zeropty serve`) and a tmux-compatible client (`zeropty send-keys -t ...`).
//! The sessions and commands live in the library (zeropty::engine), the socket in front of them too
//! (zeropty::server, which an embedded engine can take as well); this crate is the process around them:
//! - `cli::client`: one command to a server (starting one for new-session), and restart-server.
//! - `cli::attach`: a session on this terminal.
//! - `cli::serve`: the server process (its allocator settings and loop, its plugins).
//! - `cli::plugin`: `plugin-run`, a plugin library in a process of its own.
//! - `cli::help`: `--help` and `help <command>`, with no server.
//! - `cli::heap`: the global allocator, counting what it hands out (`stats`, `mstats`).

#![deny(clippy::undocumented_unsafe_blocks)]

mod cli;

#[global_allocator]
static GLOBAL: cli::heap::Counting = cli::heap::Counting;

const VERSION: &str = concat!("zeropty ", env!("CARGO_PKG_VERSION"));

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match argv.first().map(String::as_str) {
        // Answered here, so help works with no server running.
        Some("-h" | "--help" | "help") => match argv.get(1) {
            Some(cmd) => std::process::exit(cli::help::help_for(cmd)),
            None => cli::stdout_quiet(cli::help::HELP),
        },
        Some("serve") => cli::serve::run(argv.get(1).filter(|a| *a == "--bind").and(argv.get(2)).map(std::path::PathBuf::from)),
        Some("restart-server") => std::process::exit(cli::client::restart_server()),
        Some("plugin-run") => std::process::exit(cli::plugin::run(&argv[1..])),
        Some("attach-session" | "attach" | "a") => std::process::exit(cli::attach::run(&argv[1..])),
        // tmux prints its version for -V; --version is the spelling most tools accept. Both stay (docs/VERSIONING.md).
        Some("-V" | "--version") => println!("{VERSION}"),
        None => {
            eprintln!("usage: zeropty serve | <tmux command> ...");
            std::process::exit(2)
        }
        Some(_) => std::process::exit(cli::client::run(argv)),
    }
}
