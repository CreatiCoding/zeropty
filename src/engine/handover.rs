//! Moving sessions between engines in different processes, their programs left running: an zeropty server
//! and a program embedding the engine, or an old server and a new one. Each session's screen goes as a
//! record (Engine::export_session) and its terminal's fds along with it over the Unix socket (SCM_RIGHTS).
//! A session leaves the engine handing it over only once the other side says it took it; on any failure
//! before that, the taker lets go of what it took, so every session stays in exactly one place.
//!
//! On a connection to an zeropty server, after one request line:
//!   export-sessions [name...]  the server sends its sessions (all, or those named), then a line naming
//!                              those it does not have; the taker answers with a line saying which it
//!                              took, and the server forgets those.
//!   import-sessions            the server answers "ready", the giver sends sessions as above, and the
//!                              server answers with a line saying which it took.
//! A session goes as `[record length u32 LE, with the fds][record]`; a length of 0 ends them.
//!
//! # Example
//!
//! A program embedding the engine takes over the sessions of a running `zeropty` server, whose
//! programs go on as they were, and later hands them back. `pull-sessions` and `push-sessions` are
//! ordinary commands, so the CLI, the socket and [`crate::embed`] reach [`pull`] and [`push`] the same
//! way; a Rust host holding the engine can also call them directly:
//!
//! ```no_run
//! use std::path::Path;
//! use zeropty::{embed::Embedded, engine::Options};
//!
//! let zp = Embedded::start(Options::default());
//! let sock = format!("{}/.zeropty/mux.sock", std::env::var("HOME").unwrap());
//!
//! // Every session of the server at `sock` moves into this process (all of them: no names given).
//! let r = zp.command(&["pull-sessions", "-s", sock.as_str()].map(String::from), "/");
//! assert_eq!(r.code, 0, "{}", r.err);
//!
//! // The same through the function, on the engine's thread: hand the session `work` back.
//! let back = sock.clone();
//! let r = zp.with(move |engine| zeropty::engine::handover::push(engine, Path::new(&back), &["work".to_string()]));
//! print!("{}{}", r.out, r.err);
//! ```
//!
//! `zeropty restart-server` is built on the same two calls (a new server pulls every session of the
//! old one); [docs/USAGE.md](https://github.com/CreatiCoding/zeropty/blob/main/docs/USAGE.md#moving-sessions)
//! describes the commands and the protocol.

use super::backend::Backend;
use super::{Engine, Reply, fail};
use crate::sys::fd::{recv_with_fds as recv_fds, send_with_fds as send_fds};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// How long either side waits for the other.
const TIMEOUT: Duration = Duration::from_secs(10);

/// No session record is near this (a screen with 2000 rows of scrollback is ~1 MB at most).
const RECORD_MAX: usize = 256 << 20;

/// The taker's answer: the names it took, and why it did not take the others.
#[derive(Serialize, Deserialize, Default)]
struct Taken {
    taken: Vec<String>,
    failed: Vec<String>,
}

/// pull-sessions: takes the sessions named (all when none are) from the zeropty server at `sock`.
///
/// The reply lists the sessions taken, one name a line; code 1 with `can't find session: <name>` for a
/// name the server does not have (the others are still taken). On a failure midway the server keeps
/// every session.
///
/// # Example
///
/// ```
/// use zeropty::{embed::Embedded, engine::{handover, Engine, Options}};
///
/// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
/// // Another process's server, played here by an engine listening on a socket of its own.
/// let sock = std::env::temp_dir().join(format!("zeropty-doc-pull-{}.sock", std::process::id()));
/// let server = Embedded::start(Options::default());
/// server.listen(&sock).expect("listen");
/// server.command(&argv(&["new-session", "-d", "-s", "doc-pull", "exec cat"]), "/");
///
/// let mut engine = Engine::new(Options::default());
/// let r = handover::pull(&mut engine, &sock, &["doc-pull".to_string(), "nope".to_string()]);
/// assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (1, "doc-pull\n", "can't find session: nope\n"));
/// assert_eq!(engine.session_names(), ["doc-pull"]); // the same program, now here
/// assert_eq!(server.command(&argv(&["has-session", "-t", "=doc-pull"]), "/").code, 1);
///
/// engine.command(&argv(&["kill-server"]), "/");
/// drop(server);
/// let _ = std::fs::remove_file(&sock);
/// ```
pub fn pull<B: Backend>(engine: &mut Engine<B>, sock: &Path, names: &[String]) -> Reply {
    let mut t = Taken::default();
    let r = (|| -> std::io::Result<Vec<String>> {
        let s = connect(sock)?;
        request(&s, "export-sessions", names)?;
        receive(engine, &s, &mut t)?;
        let missing: Vec<String> = read_line(&s)?;
        write_line(&s, &t)?;
        Ok(missing)
    })();
    if r.is_err() {
        // The server keeps them.
        t.taken.iter().for_each(|n| engine.forget_session(n));
    }
    report(r, t)
}

/// push-sessions: hands the sessions named (all when none are) to the zeropty server at `sock`.
///
/// The reply lists the sessions the server took; this engine lets go of those only. A name the server
/// already has is not taken, and stays here.
///
/// # Example
///
/// ```
/// use zeropty::{embed::Embedded, engine::{handover, Engine, Options}};
///
/// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
/// let sock = std::env::temp_dir().join(format!("zeropty-doc-push-{}.sock", std::process::id()));
/// let server = Embedded::start(Options::default());
/// server.listen(&sock).expect("listen");
///
/// let mut engine = Engine::new(Options::default());
/// engine.command(&argv(&["new-session", "-d", "-s", "doc-push", "exec cat"]), "/");
/// let r = handover::push(&mut engine, &sock, &[]); // every session
/// assert_eq!((r.code, r.out.as_str()), (0, "doc-push\n"), "{}", r.err);
/// assert!(engine.session_names().is_empty());
/// assert_eq!(server.command(&argv(&["has-session", "-t", "=doc-push"]), "/").code, 0);
///
/// server.command(&argv(&["kill-server"]), "/");
/// drop(server);
/// let _ = std::fs::remove_file(&sock);
/// ```
pub fn push<B: Backend>(engine: &mut Engine<B>, sock: &Path, names: &[String]) -> Reply {
    let names = if names.is_empty() { engine.session_names() } else { names.to_vec() };
    let mut t = Taken::default();
    let r = (|| -> std::io::Result<Vec<String>> {
        let s = connect(sock)?;
        request(&s, "import-sessions", &[])?;
        if read_line::<String>(&s)? != "ready" {
            return Err(std::io::Error::other("the server did not take sessions"));
        }
        let missing = send(engine, &s, &names)?;
        t = read_line(&s)?;
        t.taken.iter().for_each(|n| engine.forget_session(n));
        Ok(missing)
    })();
    report(r, t)
}

/// The server's half of pull-sessions, on connection `s` after its request line.
///
/// # Example
///
/// A server of one's own: one connection, its request line read a byte at a time (fds follow it, which a
/// buffered read would drop).
///
/// ```
/// use std::io::Read as _;
/// use zeropty::engine::{handover, Engine, Options};
///
/// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
/// let path = std::env::temp_dir().join(format!("zeropty-doc-serve-export-{}.sock", std::process::id()));
/// let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
/// let server = std::thread::spawn(move || {
///     let mut giver = Engine::new(Options::default());
///     giver.command(&argv(&["new-session", "-d", "-s", "doc-export", "exec cat"]), "/");
///     let (s, _) = listener.accept().unwrap();
///     let (mut line, mut b) = (Vec::new(), [0u8]);
///     while { (&s).read_exact(&mut b).unwrap(); b[0] != b'\n' } { line.push(b[0]) }
///     let req: serde_json::Value = serde_json::from_slice(&line).unwrap();
///     assert_eq!(req["argv"][0], "export-sessions");
///     handover::prepare(&s).unwrap();
///     handover::serve_export(&mut giver, &s, &[]).unwrap();
///     giver.session_names()
/// });
/// let mut taker = Engine::new(Options::default());
/// let r = handover::pull(&mut taker, &path, &[]);
/// assert_eq!(r.out, "doc-export\n", "{}", r.err);
/// assert!(server.join().unwrap().is_empty());
/// taker.command(&["kill-server".to_string()], "/");
/// let _ = std::fs::remove_file(&path);
/// ```
pub fn serve_export<B: Backend>(engine: &mut Engine<B>, s: &UnixStream, names: &[String]) -> std::io::Result<()> {
    let names = if names.is_empty() { engine.session_names() } else { names.to_vec() };
    let missing = send(engine, s, &names)?;
    write_line(s, &missing)?;
    let t: Taken = read_line(s)?;
    t.taken.iter().for_each(|n| engine.forget_session(n));
    Ok(())
}

/// The server's half of push-sessions, on connection `s` after its request line.
///
/// # Example
///
/// ```
/// use std::io::Read as _;
/// use zeropty::engine::{handover, Engine, Options};
///
/// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
/// let path = std::env::temp_dir().join(format!("zeropty-doc-serve-import-{}.sock", std::process::id()));
/// let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
/// let server = std::thread::spawn(move || {
///     let (s, _) = listener.accept().unwrap();
///     let (mut line, mut b) = (Vec::new(), [0u8]);
///     while { (&s).read_exact(&mut b).unwrap(); b[0] != b'\n' } { line.push(b[0]) }
///     let mut taker = Engine::new(Options::default());
///     handover::prepare(&s).unwrap();
///     handover::serve_import(&mut taker, &s).unwrap();
///     taker
/// });
/// let mut giver = Engine::new(Options::default());
/// giver.command(&argv(&["new-session", "-d", "-s", "doc-import", "exec cat"]), "/");
/// let r = handover::push(&mut giver, &path, &[]);
/// assert_eq!(r.out, "doc-import\n", "{}", r.err);
/// let mut taker = server.join().unwrap();
/// assert_eq!(taker.session_names(), ["doc-import"]);
/// taker.command(&argv(&["kill-server"]), "/");
/// let _ = std::fs::remove_file(&path);
/// ```
pub fn serve_import<B: Backend>(engine: &mut Engine<B>, s: &UnixStream) -> std::io::Result<()> {
    write_line(s, &"ready")?;
    let mut t = Taken::default();
    let r = receive(engine, s, &mut t).and_then(|()| write_line(s, &t));
    if r.is_err() {
        // The giver keeps them.
        t.taken.iter().for_each(|n| engine.forget_session(n));
    }
    r
}

fn report(r: std::io::Result<Vec<String>>, t: Taken) -> Reply {
    match r {
        Err(e) => fail(format!("handover: {e}\n")),
        Ok(missing) => {
            let mut err = t.failed.concat();
            for n in missing {
                err.push_str(&format!("can't find session: {n}\n"));
            }
            Reply { code: i32::from(!err.is_empty()), out: t.taken.iter().map(|n| format!("{n}\n")).collect(), err }
        }
    }
}

fn connect(sock: &Path) -> std::io::Result<UnixStream> {
    let s = crate::sys::fd::connect_server(sock).map_err(|e| std::io::Error::new(e.kind(), format!("{}: {e}", sock.display())))?;
    s.set_read_timeout(Some(TIMEOUT))?;
    s.set_write_timeout(Some(TIMEOUT))?;
    Ok(s)
}

/// Sets connection `s` (a server's, non-blocking) up for a handover.
///
/// ```
/// let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
/// a.set_nonblocking(true).unwrap(); // as a server keeps its clients
/// zeropty::engine::handover::prepare(&a).unwrap();
/// assert_eq!(a.read_timeout().unwrap(), Some(std::time::Duration::from_secs(10)));
/// ```
pub fn prepare(s: &UnixStream) -> std::io::Result<()> {
    s.set_nonblocking(false)?;
    s.set_read_timeout(Some(TIMEOUT))?;
    s.set_write_timeout(Some(TIMEOUT))
}

fn request(s: &UnixStream, cmd: &str, names: &[String]) -> std::io::Result<()> {
    let argv: Vec<&str> = std::iter::once(cmd).chain(names.iter().map(String::as_str)).collect();
    write_line(s, &serde_json::json!({ "argv": argv, "cwd": "" }))
}

fn write_line(s: &UnixStream, v: &impl Serialize) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(v).map_err(std::io::Error::other)?;
    line.push(b'\n');
    (&*s).write_all(&line)
}

/// One line, a byte at a time: what follows it may carry fds, which a read past it would drop.
fn read_line<T: DeserializeOwned>(s: &UnixStream) -> std::io::Result<T> {
    let mut line = Vec::new();
    let mut b = [0u8];
    loop {
        (&*s).read_exact(&mut b)?;
        if b[0] == b'\n' {
            break;
        }
        line.push(b[0]);
        if line.len() > RECORD_MAX {
            return Err(std::io::Error::other("line too long"));
        }
    }
    serde_json::from_slice(&line).map_err(|e| std::io::Error::other(format!("{e}: {}", String::from_utf8_lossy(&line))))
}

/// Sends sessions `names` (each with its fds), then the end; returns the names it does not have.
fn send<B: Backend>(engine: &mut Engine<B>, s: &UnixStream, names: &[String]) -> std::io::Result<Vec<String>> {
    let mut missing = vec![];
    for n in names {
        match engine.export_session(n) {
            Some((record, fds)) => {
                send_fds(s, &(record.len() as u32).to_le_bytes(), &fds)?;
                (&*s).write_all(&record)?;
            }
            None => missing.push(n.clone()),
        }
    }
    (&*s).write_all(&0u32.to_le_bytes())?;
    Ok(missing)
}

/// Takes in sessions until the end, noting each in `t`.
fn receive<B: Backend>(engine: &mut Engine<B>, s: &UnixStream, t: &mut Taken) -> std::io::Result<()> {
    loop {
        let mut head = [0u8; 4];
        let fds = recv_fds(s, &mut head)?;
        let len = u32::from_le_bytes(head) as usize;
        // Fds not taken in are closed as `fds` drops.
        if len == 0 || len > RECORD_MAX {
            return if len == 0 { Ok(()) } else { Err(std::io::Error::other("session record too long")) };
        }
        let mut record = vec![0; len];
        (&*s).read_exact(&mut record)?;
        match engine.import_session(&record, fds) {
            Ok(name) => t.taken.push(name),
            Err(e) => t.failed.push(e),
        }
    }
}
