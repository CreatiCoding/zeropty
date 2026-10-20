//! A zeropty plugin in Rust: `activity` counts what each session printed and remembers how sessions
//! ended, and adds the command `activity` that reports it (asking the engine for the live sessions).
//!
//!     cargo run --release --example plugin_activity                 # a short demo, then exits
//!     cargo run --release --example plugin_activity -- /tmp/a.sock  # also on a socket, until Ctrl-C:
//!     ZEROPTY_SOCK=/tmp/a.sock zeropty activity
use std::collections::BTreeMap;
use zeropty::embed::Embedded;
use zeropty::engine::Options;
use zeropty::plugin::{Event, Events, Host, Manifest, Plugin, Reply, ok};

#[derive(Default)]
struct Activity {
    /// session -> (bytes printed, reads)
    printed: BTreeMap<String, (u64, u64)>,
    ended: Vec<String>,
}

impl Plugin for Activity {
    fn manifest(&self) -> Manifest {
        Manifest::new("activity").events(Events::OUTPUT | Events::SESSIONS).command("activity")
    }

    fn event(&mut self, event: &Event, _host: &Host) {
        match event {
            Event::Output { session, data } => {
                let e = self.printed.entry(session.clone()).or_default();
                e.0 += data.len() as u64;
                e.1 += 1;
            }
            Event::SessionRenamed { session, from } => {
                if let Some(v) = self.printed.remove(from) {
                    self.printed.insert(session.clone(), v);
                }
            }
            Event::SessionExited { session, reason } => {
                self.printed.remove(session);
                self.ended.push(format!("{session} ({})", reason.as_str()));
            }
            _ => {}
        }
    }

    fn command(&mut self, _argv: &[String], _cwd: &str, host: &Host) -> Reply {
        // The engine's own view, through the same commands a client has.
        let live = host.command(&["list-sessions", "-F", "#{session_name}"], "/").out;
        let mut out = String::new();
        for s in live.lines() {
            let (bytes, reads) = self.printed.get(s).copied().unwrap_or_default();
            out.push_str(&format!("{s}: {bytes} bytes in {reads} reads\n"));
        }
        if !self.ended.is_empty() {
            out.push_str(&format!("ended: {}\n", self.ended.join(", ")));
        }
        ok(out)
    }
}

fn main() {
    let e = Embedded::start(Options::default());
    e.add_plugin(Activity::default()).expect("add the plugin");
    let run = |a: &[&str]| e.command(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "/");
    run(&["new-session", "-d", "-s", "counter", "seq 1 2000; exec cat"]);
    run(&["new-session", "-d", "-s", "brief", "echo bye"]);
    std::thread::sleep(std::time::Duration::from_millis(600));
    print!("{}", run(&["activity"]).out);
    print!("{}", run(&["list-plugins"]).out);
    match std::env::args().nth(1) {
        Some(sock) => {
            e.listen(std::path::Path::new(&sock)).expect("listen");
            eprintln!("listening on {sock}: ZEROPTY_SOCK={sock} zeropty activity");
            loop {
                std::thread::park();
            }
        }
        None => {
            run(&["kill-server"]);
        }
    }
}
