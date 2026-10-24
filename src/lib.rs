//! zeropty: tmux's session commands without PTYs, as a library. Sessions run plain programs on pipes; a
//! terminal emulator keeps each one's screen; tmux-style commands read and drive them.
//!
//! It is a session engine for AI agents: it runs programs (a shell, `cat`, a coding agent) as sessions,
//! keeps their screens, sends them keys and tells you when they are done, through tmux's own command
//! format (`new-session`, `send-keys`, `capture-pane`, ...). It uses no PTYs, so there is no PTY limit,
//! and it is built to hold 1000 sessions on one macOS machine (measurements:
//! [docs/BENCHMARKS.md](https://github.com/CreatiCoding/zeropty/blob/main/docs/BENCHMARKS.md)).
//!
//! The same engine runs three ways:
//!
//! | how | what | where |
//! |---|---|---|
//! | standalone server | `zeropty serve`, driven by the `zeropty` CLI or JSON lines on a Unix socket | `src/main.rs`, [`server`] |
//! | embedded, own thread | [`embed::Embedded`]: commands from any thread, no server process | [`embed`] |
//! | embedded, host's loop | [`embed::Polled`]: no thread at all, the host polls a kqueue fd | [`embed`], [`host`] |
//!
//! Other languages get the embedded engine through the C ABI (`bindings/c`, `zeropty.h`) and the
//! Node module (`bindings/node`, `client/embedded.mjs`).
//!
//! # Example
//!
//! Start an engine in this process, run `cat` as a session, type a line into it and read the screen:
//!
//! ```
//! use zeropty::{embed::Embedded, engine::Options};
//!
//! let zp = Embedded::start(Options::default());
//! let run = |args: &[&str]| zp.command(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "/");
//!
//! assert_eq!(run(&["new-session", "-d", "-s", "doc-crate", "-x", "40", "-y", "5", "exec cat"]).code, 0);
//! run(&["send-keys", "-t", "=doc-crate:", "-l", "hello"]);
//! run(&["send-keys", "-t", "=doc-crate:", "Enter"]);
//!
//! // `cat` prints the line back once it reads it; capture-pane always sees the latest output.
//! let mut screen = String::new();
//! for _ in 0..200 {
//!     screen = run(&["capture-pane", "-p", "-t", "=doc-crate:"]).out;
//!     if screen.matches("hello").count() == 2 { break }
//!     std::thread::sleep(std::time::Duration::from_millis(10));
//! }
//! assert!(screen.starts_with("hello\nhello\n"), "{screen:?}");
//!
//! run(&["kill-server"]); // ends every session; dropping `zp` stops the engine's thread
//! ```
//!
//! Every command, its flags and the socket protocol are listed in
//! [docs/USAGE.md](https://github.com/CreatiCoding/zeropty/blob/main/docs/USAGE.md); the commands run the
//! same way through [`engine::Engine::command`], [`embed::Embedded::command`],
//! [`embed::Polled::command`], the CLI and the socket. Waiting for text without polling:
//! [`engine::Engine::wait_start`]. Runnable programs:
//! [examples/](https://github.com/CreatiCoding/zeropty/tree/main/examples).
//!
//! # Platform
//!
//! macOS and Linux. The engine waits on kqueue (macOS) or epoll (Linux, 5.3 or later for pidfds) behind
//! one set of types, and programs that need a terminal get one through a shim
//! ([shim/shim.c](https://github.com/CreatiCoding/zeropty/blob/main/shim/shim.c)) loaded with
//! `DYLD_INSERT_LIBRARIES` (macOS) or `LD_PRELOAD` (Linux).
//!
//! # Layers
//!
//! Dependencies point down this list, never up:
//!
//! | module     | responsibility |
//! |------------|----------------|
//! | `embed`    | the engine inside a host process: on a thread of its own, or run by the host's loop (`Frontend`) |
//! | `host`     | one turn of a host's loop (events, the server as a `Service`, reads, attached terminals), the same for every host |
//! | `server`   | the socket in front of an engine: connections, attach, wait-pane, handover requests, the plugins' commands and `plugin-attach` |
//! | `plugin`   | plugins: a registry of Rust plugins, programs on the socket and C libraries, each on threads of its own, fed the engine's session events (`engine::tap`) |
//! | `protocol` | the socket's bytes: request and reply lines, typed attach frames (pure) |
//! | `engine`   | sessions, their screens and every command, over a `Backend` (`Poller`, `Clock`, `Programs`); `engine::handover` moves sessions between processes |
//! | `term`     | the terminal emulator: bytes in, a screen out (pure, no I/O but its cold store) |
//! | `sys`      | the OS: kqueue, fds, processes, time (crate-private), and `tty`, the attaching user's terminal |
//!
//! A host picks its front end: the `zeropty` binary (src/main.rs) runs `engine` + `server` through
//! `host`; a program embeds `embed::Embedded` or `embed::Polled`, through the C ABI (bindings/c) or Node
//! (bindings/node), and may `listen` to answer the socket protocol as well.
//!
//! # Safety
//!
//! Every `unsafe` block, in this library, the `zeropty` binary and both bindings, says why it is sound
//! (`clippy::undocumented_unsafe_blocks` is denied in each). They are:
//!
//! - library: the OS calls in `sys` (kqueue, read/write/poll, SCM_RIGHTS, killpg/waitpid, proc_pidinfo,
//!   the terminal's modes and its SIGWINCH handler); the zstd allocator hooks in `term::cold`; one UTF-8
//!   read in `term::cell`; mimalloc's `mi_option_set` and `mi_collect` in `mimalloc`; and the length of
//!   the read-ahead buffer in `engine::session` after `sys::fd::read_uninit` filled it;
//! - plugins: loading plugin libraries and calling their C entry point and callback
//!   (`plugin::__private::native`, dlopen/dlsym/free), and `geteuid`/`atexit` in the plugin directory's
//!   launcher (`plugin::__private::launch`);
//! - the binary (src/main.rs, src/cli/): the counting `GlobalAlloc` in `cli::heap`, and in `cli::serve`
//!   the environment cleared before any thread starts and mimalloc's statistics callback;
//! - bindings/c: the C ABI's pointers and strings from the caller; bindings/node: libuv's handles.
//!
//! # Compatibility
//!
//! Items moved by the layered refactor keep their old paths, deprecated (each `#[deprecated]` note
//! says where the item went):
//! `zeropty::handover` (now `engine::handover`),
//! `engine::set_nonblocking`, `server::{Request, frame, FRAME_*}` (now `protocol`, with typed frames).
//! `Engine::kq` is deprecated for `Engine::watch` and `AsFd`; `Engine::poll`, which swallowed the
//! queue's failures, for `Engine::try_poll`. Gone: `engine::kq`, whose raw `kevent`s became the opaque
//! `engine::{Event, Change}`; `export_session`/`import_session` lend and take fds as
//! `BorrowedFd`/`OwnedFd`. New: `Engine<B: Backend = System>` (plain `Engine` is the same type as
//! before), `host::HostLoop::turn` returns the queue's failure, `embed::Polled::try_process`.

#![warn(missing_docs)]
#![deny(clippy::undocumented_unsafe_blocks)]

pub mod embed;
pub mod engine;
pub mod host;
pub mod mimalloc;
pub mod plugin;
pub mod protocol;
pub mod server;
pub mod term;

mod sys;

pub use sys::tty;

/// Sessions between engines in different processes.
#[deprecated(since = "0.2.0", note = "moved to zeropty::engine::handover")]
pub mod handover {
    pub use crate::engine::handover::*;
}
