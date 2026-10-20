//! Plugins in other processes, in any language: a program connects to the socket (crate::server) and
//! sends, as its first command,
//!
//! ```text
//! {"argv":["plugin-attach","-n","notify","-e","session-created,session-exited","-c","notify-log"],"cwd":"/"}
//! ```
//!
//! It gets the usual reply line ({"code":0,"out":"zeropty-plugin 1\n","err":""}) once it is registered
//! (list-plugins shows it, its commands are routed to it), and from then on the connection carries JSON
//! lines both ways:
//!
//! - zeropty -> plugin: events, {"event":"session-created","session":"w"} (Event::to_json), and the
//!   commands it added, {"call":7,"argv":["notify-log"],"cwd":"/"}, each to be answered with
//!   {"reply":7,"code":0,"out":"...","err":""}.
//! - plugin -> zeropty: any command its access allows (Manifest::access, `-a`),
//!   {"id":3,"argv":["capture-pane","-p","-t","=w:"],"cwd":"/"}, answered with
//!   {"id":3,"code":0,"out":"...","err":""} (in the order sent).
//!
//! Closing the connection removes the plugin. Here it is a Plugin like any other (a thread of its own,
//! its queue, its deadline), so a slow or stuck program loses events or gets its commands timed out, and
//! never holds the engine: the socket is blocking on these threads, never on the engine's. A call is
//! waited for `reply_wait(deadline)` (7/8 of it): a reply later than that fails that call only and is
//! dropped when it comes; the program stays attached (the watchdog never sees the wait as stuck).
use crate::plugin::wire::{call_json, reply_from_json, reply_json};
use crate::plugin::{Access, Event, Events, Host, Manifest, Plugin};
use crate::plugin::{Reply, fail};
use std::collections::HashMap;
use std::io::{BufRead as _, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The longest line read from a plugin (a reply with a large capture in it).
const LINE_MAX: u64 = 64 << 20;

pub struct Remote {
    manifest: Manifest,
    /// Written by the plugin's thread (events, calls) and the reader (replies to its commands).
    out: Arc<Mutex<UnixStream>>,
    /// For `start`: the stream to read, what was read past the plugin-attach line, and what is to be
    /// written before anything else (replies the server owed the connection, then the attach's).
    input: Option<(UnixStream, Vec<u8>, Vec<u8>)>,
    calls: Arc<Mutex<HashMap<u64, Sender<Reply>>>>,
    next_call: u64,
}

/// `plugin-attach -n name [-e events] [-c command,...] [-a access] [-v api]` -> its manifest.
pub fn parse_attach(argv: &[String]) -> Result<Manifest, String> {
    let (mut name, mut events, mut commands, mut api, mut access) = (None, Events::empty(), Vec::new(), crate::plugin::API_VERSION, Access::empty());
    let mut k = 0;
    while k < argv.len() {
        let value = argv.get(k + 1).cloned();
        match (argv[k].as_str(), value) {
            ("-n", Some(v)) => name = Some(v),
            ("-e", Some(v)) => events = Events::parse(&v)?,
            ("-c", Some(v)) => commands.extend(v.split(',').map(str::trim).filter(|c| !c.is_empty()).map(str::to_owned)),
            ("-a", Some(v)) => access = Access::parse(&v)?,
            ("-v", Some(v)) => api = v.parse().map_err(|_| format!("plugin-attach: bad api version {v}\n"))?,
            (flag, _) => {
                return Err(format!("plugin-attach: unexpected {flag} (usage: plugin-attach -n name [-e events] [-c commands] [-a access] [-v api])\n"));
            }
        }
        k += 2;
    }
    let name = name.ok_or("plugin-attach: which name? (-n)\n")?;
    let mut m = Manifest::new(name).events(events).access(access).api(api);
    m.commands = commands;
    Ok(m)
}

impl Remote {
    /// The plugin on `stream`, `pending` being what it sent after plugin-attach and `owed` what is to be
    /// written to it first. Its waits follow the registry's deadline (`Host::deadline`) when it runs.
    /// Nothing is done to the stream until `start`, on the plugin's own thread: the
    /// server's thread neither blocks on it nor changes it before the plugin is registered.
    pub fn new(manifest: Manifest, stream: UnixStream, pending: Vec<u8>, owed: Vec<u8>) -> std::io::Result<Self> {
        let reader = stream.try_clone()?;
        Ok(Self { manifest, out: Arc::new(Mutex::new(stream)), input: Some((reader, pending, owed)), calls: Arc::default(), next_call: 1 })
    }
}

/// Writes `line` and a line feed within `budget` in all, counted from the call (waiting for the lock
/// included). The socket's write timeout bounds one `write`, and on macOS a write that moved some bytes
/// before timing out returns that count, so `write_all` could wait a full timeout per write: a program
/// that reads a trickle, or not at all while the buffer was nearly full, held an event hook past the
/// deadline and the watchdog cut it off as stuck. Here the timeout is what is left of the budget, and
/// running out is `TimedOut`, so such a program is failed as a connection that does not read, before
/// the watchdog's deadline (reply_wait is 7/8 of it).
fn write_line(out: &Mutex<UnixStream>, line: &str, budget: Duration) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(line.len() + 1);
    buf.extend_from_slice(line.as_bytes());
    buf.push(b'\n');
    write_bytes(out, &buf, budget)
}

/// `write_line` for bytes as they are.
fn write_bytes(out: &Mutex<UnixStream>, bytes: &[u8], budget: Duration) -> std::io::Result<()> {
    let until = Instant::now() + budget;
    let mut s = out.lock().unwrap();
    let mut rest = bytes;
    while !rest.is_empty() {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, format!("not read for {} ms", budget.as_millis())));
        }
        s.set_write_timeout(Some(left))?;
        match s.write(rest) {
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(n) => rest = &rest[n..],
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, format!("not read for {} ms", budget.as_millis())));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

impl Plugin for Remote {
    fn manifest(&self) -> Manifest {
        self.manifest.clone()
    }

    fn start(&mut self, host: &Host) {
        let Some((stream, pending, owed)) = self.input.take() else { return };
        // Blocking from here on, on this plugin's threads. A program that stops reading fails its plugin
        // rather than holding the plugin's thread for good.
        let sent = stream.set_nonblocking(false).and_then(|()| write_bytes(&self.out, &owed, crate::plugin::reply_wait(host.deadline())));
        if let Err(e) = sent {
            host.fail(format!("connection: {e}"));
            return;
        }
        let (out, calls, host) = (self.out.clone(), self.calls.clone(), host.clone());
        let _ = std::thread::Builder::new().name(format!("zeropty-plugin-{}-in", self.manifest.name)).spawn(move || {
            read_lines(std::io::Cursor::new(pending).chain(stream), &out, &calls, &host);
            // The program went away: calls waiting for it fail now, and it leaves the registry.
            calls.lock().unwrap().clear();
            host.detach();
        });
    }

    fn event(&mut self, event: &Event, host: &Host) {
        if let Err(e) = write_line(&self.out, &event.to_json(), crate::plugin::reply_wait(host.deadline())) {
            host.fail(format!("connection: {e}"));
        }
    }

    fn command(&mut self, argv: &[String], cwd: &str, host: &Host) -> Reply {
        let id = self.next_call;
        self.next_call += 1;
        let (tx, rx) = mpsc::channel();
        self.calls.lock().unwrap().insert(id, tx);
        if let Err(e) = write_line(&self.out, &call_json(id, argv, cwd), crate::plugin::reply_wait(host.deadline())) {
            self.calls.lock().unwrap().remove(&id);
            host.fail(format!("connection: {e}"));
            return fail(format!("{}: plugin connection: {e}\n", argv[0]));
        }
        // Not the whole deadline: the watchdog cuts off a hook running past it, and whether it saw this
        // wait first would be a race (one late reply then cost the plugin its connection). Giving up
        // clearly before it, a late reply only fails its own call; the reader drops it when it comes.
        let wait = crate::plugin::reply_wait(host.deadline());
        let r = rx.recv_timeout(wait).unwrap_or_else(|e| match e {
            mpsc::RecvTimeoutError::Timeout => {
                fail(format!("{}: plugin timed out (no reply within {} ms; a later reply is ignored)\n", argv[0], wait.as_millis()))
            }
            mpsc::RecvTimeoutError::Disconnected => fail(format!("{}: plugin went away\n", argv[0])),
        });
        self.calls.lock().unwrap().remove(&id);
        r
    }

    fn stop(&mut self, _host: &Host) {
        // The reader sees the end and leaves; the program sees its connection close.
        let _ = self.out.lock().unwrap().shutdown(std::net::Shutdown::Both);
    }
}

/// The plugin's lines: replies to calls, and its own commands (run, and answered in order).
fn read_lines(input: impl std::io::Read, out: &Mutex<UnixStream>, calls: &Mutex<HashMap<u64, Sender<Reply>>>, host: &Host) {
    let mut input = std::io::BufReader::new(input);
    let mut line = Vec::new();
    loop {
        line.clear();
        match (&mut input).take(LINE_MAX).read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) if line.last() != Some(&b'\n') => return,
            Ok(_) => {}
        }
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&line) else {
            let _ = write_line(out, &serde_json::json!({ "error": "bad line: not JSON" }).to_string(), crate::plugin::reply_wait(host.deadline()));
            continue;
        };
        if let Some(id) = v["reply"].as_u64() {
            if let Some(tx) = calls.lock().unwrap().remove(&id) {
                let _ = tx.send(reply_from_json(&v));
            }
            continue;
        }
        if let Some(argv) = v["argv"].as_array() {
            let argv: Vec<String> = argv.iter().filter_map(|a| a.as_str().map(str::to_owned)).collect();
            let r = host.command(&argv, v["cwd"].as_str().unwrap_or("/"));
            if write_line(out, &reply_json(&r, "id", v["id"].as_u64().unwrap_or(0)), crate::plugin::reply_wait(host.deadline())).is_err() {
                return;
            }
            continue;
        }
        let _ =
            write_line(out, &serde_json::json!({ "error": "bad line: neither a reply nor a command" }).to_string(), crate::plugin::reply_wait(host.deadline()));
    }
}
