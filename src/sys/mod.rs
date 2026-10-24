//! The OS layer: the kernel interfaces the rest of the crate uses, each wrapped once with its gotchas
//! (EINTR, EWOULDBLOCK, close-on-exec, SIGPIPE) handled here and its failures as `io::Result`. Nothing
//! else in the crate, nor the `zeropty` binary, calls `libc` for I/O: the engine's logic is written
//! against these functions (through the `Poller`, `Clock` and `Programs` traits in crate::engine for
//! its event loop, time and processes) and the pure modules beside it. Every `unsafe` block here states
//! why it is sound.
//!
//! - `kq`: the kqueue the engine and its hosts wait on, its changes and events as opaque types (an
//!   epoll on Linux, `epoll.rs`, behind the same types).
//! - `fd`: non-blocking reads (also into uninitialized memory) and writes on borrowed fds, `poll`,
//!   socket pairs, passing fds over a Unix socket.
//! - `process`: what the engine asks of processes (signals, reaping, name and directory, peers).
//! - `time`: the engine's monotonic milliseconds and wall-clock seconds.
//! - `tty`: the user's terminal for an attaching client (raw mode, size, SIGWINCH); public as
//!   `zeropty::tty`.

pub(crate) mod fd;
#[cfg(target_os = "macos")]
pub(crate) mod kq;
// Linux: epoll behind the same types.
#[cfg(target_os = "linux")]
#[path = "epoll.rs"]
pub(crate) mod kq;
pub(crate) mod process;
pub(crate) mod time;
pub mod tty;
