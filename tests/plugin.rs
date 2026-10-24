//! Plugins (zeropty::plugin): their events, the commands they add (embedded, socket, CLI), plugins in
//! other processes (plugin-attach), and that a plugin that panics, hangs or falls behind does not take
//! the engine with it.
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zeropty::embed::{Embedded, Polled};
use zeropty::engine::{Engine, Options};
use zeropty::plugin::{__private, Access, Event, Events, ExitReason, Host, Manifest, Plugin, PluginError, Reply, fail, ok};

/// The repository.
const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn argv(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

/// A directory of its own (short: socket paths are limited to 104 bytes).
fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("zp-pl-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn engine(d: &std::path::Path) -> Embedded {
    Embedded::start(Options { size_dir: d.to_owned(), ..Options::default() })
}

/// Hands every event to the test, and answers `probe-*` commands as told.
struct Probe {
    manifest: Manifest,
    tx: Sender<Event>,
}

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        self.manifest.clone()
    }
    fn event(&mut self, event: &Event, _host: &Host) {
        let _ = self.tx.send(event.clone());
    }
    fn command(&mut self, argv: &[String], _cwd: &str, host: &Host) -> Reply {
        match argv[0].as_str() {
            // Uses the engine from the plugin: the session's name through display-message.
            "probe-name" => {
                let r = host.command(&["display-message", "-p", "-t", argv.get(1).map_or("", String::as_str), "#{session_name}"], "/");
                if r.code != 0 {
                    return r;
                }
                ok(format!("{} says hi to {}", host.name(), r.out))
            }
            "probe-self" => host.command(&["probe-self"], "/"),
            // Runs what it is given, to check what its access lets through.
            c if c.ends_with("-run") => host.command(&argv[1..], "/"),
            "probe-sleep" => {
                std::thread::sleep(Duration::from_millis(argv[1].parse().unwrap()));
                ok("woke\n")
            }
            "probe-panic" => panic!("asked to"),
            "probe-fail" => {
                host.fail("asked to");
                ok("failing\n")
            }
            _ => fail("?\n"),
        }
    }
}

fn probe(name: &str, events: Events, commands: &[&str]) -> (Probe, Receiver<Event>) {
    let (tx, rx) = mpsc::channel();
    let mut m = Manifest::new(name).events(events);
    for c in commands {
        m = m.command(*c);
    }
    (Probe { manifest: m, tx }, rx)
}

/// How long a test waits for what should come at once. Long: under load (the suites run in parallel,
/// with servers and plugin processes starting) a few seconds were not always enough.
const PATIENCE: Duration = Duration::from_secs(30);

/// Events until `f` finds what it wants (or PATIENCE passes).
fn until(rx: &Receiver<Event>, mut f: impl FnMut(&Event) -> bool) -> Vec<Event> {
    let mut seen = Vec::new();
    let end = Instant::now() + PATIENCE;
    loop {
        let left = end.checked_duration_since(Instant::now()).unwrap_or_else(|| panic!("no such event; got {seen:?}"));
        let ev = rx.recv_timeout(left).unwrap_or_else(|_| panic!("no such event; got {seen:?}"));
        let done = f(&ev);
        seen.push(ev);
        if done {
            return seen;
        }
    }
}

fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    let start = Instant::now();
    while !f() {
        assert!(start.elapsed() < PATIENCE, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn lifecycle_and_output_events() {
    let d = dir("ev");
    let e = engine(&d);
    let (p, rx) = probe("probe", Events::ALL, &[]);
    e.add_plugin(p).unwrap();
    assert_eq!(e.command(&argv(&["new-session", "-d", "-s", "a", "-x", "40", "-y", "5", "printf 'hello plugin\\n'; exec cat"]), "/").code, 0);
    let mut out = Vec::new();
    let seen = until(&rx, |ev| {
        if let Event::Output { session, data } = ev {
            assert_eq!(session, "a");
            out.extend_from_slice(data);
        }
        out.windows(12).any(|w| w == b"hello plugin")
    });
    assert_eq!(seen[0], Event::SessionCreated { session: "a".into() });
    until(&rx, |ev| *ev == Event::Activity { session: "a".into() });
    assert_eq!(e.command(&argv(&["rename-session", "-t", "a", "b"]), "/").code, 0);
    until(&rx, |ev| *ev == Event::SessionRenamed { session: "b".into(), from: "a".into() });
    assert_eq!(e.command(&argv(&["kill-session", "-t", "=b"]), "/").code, 0);
    until(&rx, |ev| *ev == Event::SessionExited { session: "b".into(), reason: ExitReason::Killed });
    // A program that ends on its own.
    e.command(&argv(&["new-session", "-d", "-s", "short", "exit 0"]), "/");
    until(&rx, |ev| *ev == Event::SessionExited { session: "short".into(), reason: ExitReason::Exited });
    let _ = std::fs::remove_dir_all(&d);
}

/// A plugin gets only the kinds it asked for.
#[test]
fn only_the_events_asked_for() {
    let d = dir("mask");
    let e = engine(&d);
    let (p, rx) = probe("exits", Events::SESSION_EXITED, &[]);
    e.add_plugin(p).unwrap();
    e.command(&argv(&["new-session", "-d", "-s", "m", "printf x; exec cat"]), "/");
    std::thread::sleep(Duration::from_millis(50));
    e.command(&argv(&["capture-pane", "-p", "-t", "=m:"]), "/");
    e.command(&argv(&["kill-session", "-t", "=m"]), "/");
    let seen = until(&rx, |ev| matches!(ev, Event::SessionExited { .. }));
    assert_eq!(seen.len(), 1, "{seen:?}");
    let _ = std::fs::remove_dir_all(&d);
}

/// With no plugin the engine queues nothing (and reads no output for them); once one asks, it does.
#[test]
fn nothing_queued_without_plugins() {
    let d = dir("zero");
    let mut e = Engine::new(Options { size_dir: d.clone(), ..Options::default() });
    e.command(&argv(&["new-session", "-d", "-s", "z", "printf abc; exec cat"]), "/");
    std::thread::sleep(Duration::from_millis(50));
    e.command(&argv(&["capture-pane", "-p", "-t", "=z:"]), "/");
    assert!(!__private::has_engine_events(&e));
    __private::set_engine_events(&mut e, Events::OUTPUT);
    e.command(&argv(&["send-keys", "-t", "=z:", "-l", "def\n"]), "/");
    // cat echoes it when it gets to it (later under load): captures read it.
    let mut evs = Vec::new();
    wait_for("def as output", || {
        e.command(&argv(&["capture-pane", "-p", "-t", "=z:"]), "/");
        evs.extend(__private::take_engine_events(&mut e));
        evs.iter().any(|ev| matches!(ev, Event::Output { data, .. } if data.windows(3).any(|w| w == b"def")))
    });
    __private::set_engine_events(&mut e, Events::empty());
    e.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn registration_is_checked() {
    let d = dir("reg");
    let e = engine(&d);
    let bad = |m: Manifest| e.add_plugin(Probe { manifest: m, tx: mpsc::channel().0 }).unwrap_err();
    // Callers tell the kinds apart (PluginError), and the text says why.
    assert_eq!(bad(Manifest::new("")), PluginError::BadName(String::new()));
    assert!(matches!(bad(Manifest::new("has space")), PluginError::BadName(_)));
    let future = bad(Manifest::new("future").api(zeropty::plugin::API_VERSION + 1));
    assert!(matches!(future, PluginError::Api { api, supported, .. } if api == supported + 1), "{future:?}");
    assert!(future.to_string().contains("not supported"));
    assert!(matches!(bad(Manifest::new("evil").command("send-keys")), PluginError::BuiltinCommand { .. }));
    assert!(matches!(bad(Manifest::new("evil").command("list-plugins")), PluginError::BuiltinCommand { .. }));
    assert!(matches!(bad(Manifest::new("evil").command("-x")), PluginError::BadCommand { .. }));
    assert!(matches!(bad(Manifest::new("twice").command("x-a").command("x-a")), PluginError::DuplicateCommand { .. }));
    // Every engine command name is reserved, aliases included (read from the engine's table).
    assert!(matches!(bad(Manifest::new("evil").command("resizep")), PluginError::BuiltinCommand { .. }));
    // Unknown event or access bits cannot reach a manifest at all: the typed sets refuse them.
    assert_eq!((Events::from_bits(1 << 30), Access::from_bits(1 << 30)), (None, None));
    e.add_plugin(probe("one", Events::empty(), &["one-cmd"]).0).unwrap();
    assert_eq!(e.add_plugin(probe("one", Events::empty(), &[]).0).unwrap_err(), PluginError::NameTaken("one".into()));
    let taken = e.add_plugin(probe("two", Events::empty(), &["one-cmd"]).0).unwrap_err();
    assert_eq!(taken, PluginError::CommandTaken { plugin: "two".into(), command: "one-cmd".into(), owner: "one".into() });
    // As a reply's err: the same text with a line feed.
    assert_eq!(String::from(taken), "plugin two: command one-cmd is plugin one's\n");
    let list = e.command(&argv(&["list-plugins"]), "/").out;
    assert_eq!(list, "one: running (api 1) events= commands=one-cmd access=read dropped=0\n");
    // Removed: its command is unknown again.
    assert!(e.plugins().remove("one"));
    assert!(e.command(&argv(&["one-cmd"]), "/").err.starts_with("unknown command"));
    assert_eq!(e.command(&argv(&["list-plugins"]), "/").out, "");
    let _ = std::fs::remove_dir_all(&d);
}

/// A plugin's commands come through the embedded call, the socket and the CLI alike, and the plugin
/// can use the engine meanwhile.
#[test]
fn commands_every_way() {
    let d = dir("cmd");
    let e = engine(&d);
    let sock = d.join("s");
    e.listen(&sock).unwrap();
    e.add_plugin(probe("probe", Events::empty(), &["probe-name", "probe-self"]).0).unwrap();
    e.command(&argv(&["new-session", "-d", "-s", "w", "exec cat"]), "/");
    assert_eq!(e.command(&argv(&["probe-name", "=w"]), "/").out, "probe says hi to w\n");
    // A plugin that calls its own command is told no, not left waiting for itself.
    assert!(e.command(&argv(&["probe-self"]), "/").err.contains("cannot call its own"));
    // The socket: replies stay in order behind the plugin's.
    let mut s = connect(&sock);
    s.write_all(b"{\"argv\":[\"probe-name\",\"=w\"],\"cwd\":\"/\"}\n{\"argv\":[\"has-session\",\"-t\",\"=w\"],\"cwd\":\"/\"}\n{\"argv\":[\"list-plugins\"],\"cwd\":\"/\"}\n").unwrap();
    let mut lines = BufReader::new(s.try_clone().unwrap()).lines();
    let r: Reply = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    assert_eq!((r.code, r.out.as_str()), (0, "probe says hi to w\n"));
    let r: Reply = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    assert_eq!((r.code, r.out.as_str()), (0, ""));
    let r: Reply = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    assert!(r.out.starts_with("probe: running"), "{}", r.out);
    // The CLI, as any tmux-style command.
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_zeropty")).args(["probe-name", "=w"]).env("ZEROPTY_SOCK", &sock).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout), "probe says hi to w\n");
    assert!(o.status.success());
    e.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&d);
}

/// Panics in a plugin's hooks are caught: it is marked failed, its command answers with an error, the
/// engine and the other plugins go on, and a plugin of that name may come back.
#[test]
fn a_panicking_plugin_is_cut_off() {
    let d = dir("panic");
    let e = engine(&d);
    let (healthy, rx) = probe("healthy", Events::SESSION_CREATED, &[]);
    e.add_plugin(healthy).unwrap();
    e.add_plugin(probe("bomb", Events::empty(), &["probe-panic"]).0).unwrap();
    let r = e.command(&argv(&["probe-panic"]), "/");
    assert_eq!(r.code, 1);
    assert!(r.err.contains("panicked"), "{}", r.err);
    wait_for("bomb marked failed", || e.command(&argv(&["list-plugins"]), "/").out.contains("bomb: failed (panicked in command: asked to)"));
    assert!(e.command(&argv(&["probe-panic"]), "/").err.starts_with("unknown command"));
    // A plugin panicking in an event hook.
    struct EventBomb;
    impl Plugin for EventBomb {
        fn manifest(&self) -> Manifest {
            Manifest::new("event-bomb").events(Events::SESSION_CREATED)
        }
        fn event(&mut self, _: &Event, _: &Host) {
            panic!("on an event")
        }
    }
    e.add_plugin(EventBomb).unwrap();
    assert_eq!(e.command(&argv(&["new-session", "-d", "-s", "p", "exec cat"]), "/").code, 0);
    until(&rx, |ev| *ev == Event::SessionCreated { session: "p".into() });
    wait_for("event-bomb marked failed", || e.command(&argv(&["list-plugins"]), "/").out.contains("event-bomb: failed (panicked in event: on an event)"));
    // The engine is fine; the healthy plugin still hears.
    assert_eq!(e.command(&argv(&["new-session", "-d", "-s", "q", "exec cat"]), "/").code, 0);
    until(&rx, |ev| *ev == Event::SessionCreated { session: "q".into() });
    assert_eq!(e.command(&argv(&["has-session", "-t", "=p"]), "/").code, 0);
    // Back with the same name.
    e.add_plugin(probe("bomb", Events::empty(), &["probe-panic"]).0).unwrap();
    assert!(e.command(&argv(&["list-plugins"]), "/").out.contains("bomb: running"));
    e.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&d);
}

/// A plugin slow to answer: its caller gets an error at the deadline, everyone else is served
/// meanwhile. A plugin stuck in a hook loses events instead of holding the engine, and is cut off.
#[test]
fn a_slow_plugin_stalls_nothing() {
    let d = dir("slow");
    let e = Arc::new(engine(&d));
    e.plugins().set_timeout(Duration::from_millis(300));
    e.add_plugin(probe("sleepy", Events::empty(), &["probe-sleep"]).0).unwrap();
    e.command(&argv(&["new-session", "-d", "-s", "s", "exec cat"]), "/");
    let start = Instant::now();
    let r = e.command(&argv(&["probe-sleep", "2000"]), "/");
    assert!(r.err.contains("timed out"), "{}", r.err);
    assert!(start.elapsed() < Duration::from_millis(1500), "{:?}", start.elapsed());

    // Stuck in an event hook: events pile up past its queue and are dropped. The deadline is long while
    // they pile up (the renames take longer than 300 ms under load), then short to see it cut off.
    e.plugins().set_timeout(Duration::from_secs(60));
    struct Stuck(Arc<Mutex<bool>>);
    impl Plugin for Stuck {
        fn manifest(&self) -> Manifest {
            Manifest::new("stuck").events(Events::SESSION_RENAMED)
        }
        fn event(&mut self, _: &Event, _: &Host) {
            while !*self.0.lock().unwrap() {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    let release = Arc::new(Mutex::new(false));
    e.add_plugin(Stuck(release.clone())).unwrap();
    let mut worst = Duration::ZERO;
    for k in 0..1500 {
        let (from, to) = if k % 2 == 0 { ("s", "t") } else { ("t", "s") };
        let start = Instant::now();
        assert_eq!(e.command(&argv(&["rename-session", "-t", from, to]), "/").code, 0);
        worst = worst.max(start.elapsed());
    }
    eprintln!("rename-session with a stuck plugin: worst {worst:?}");
    assert!(worst < Duration::from_millis(100), "{worst:?}");
    let list = e.command(&argv(&["list-plugins"]), "/").out;
    let dropped: u64 = list.lines().find(|l| l.starts_with("stuck:")).and_then(|l| l.rsplit("dropped=").next()).unwrap().parse().unwrap();
    assert!(dropped >= 1500 - 1024 - 1, "{list}");
    e.plugins().set_timeout(Duration::from_millis(300));
    wait_for("stuck cut off", || e.command(&argv(&["list-plugins"]), "/").out.contains("stuck: failed (stuck in a hook for more than 300 ms)"));
    *release.lock().unwrap() = true;
    // The engine answered all along.
    assert_eq!(e.command(&argv(&["has-session", "-t", "=s"]), "/").code, 0);
    e.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&d);
}

/// `manifest()` runs on the plugin's own thread under the manifest deadline: one that never returns
/// costs `add` at most that (1 s), on an engine thread's host and a polled one alike, the engine answers
/// meanwhile, the plugin is not added (its name stays free), and it is dropped on its own thread once
/// its manifest returns. One that panics is refused the same way.
#[test]
fn a_stuck_manifest_holds_add_one_second_at_most() {
    struct SlowManifest {
        release: Arc<Mutex<bool>>,
        dropped: Sender<std::thread::ThreadId>,
    }
    impl Plugin for SlowManifest {
        fn manifest(&self) -> Manifest {
            while !*self.release.lock().unwrap() {
                std::thread::sleep(Duration::from_millis(5));
            }
            Manifest::new("slow-manifest").command("slow-cmd")
        }
    }
    impl Drop for SlowManifest {
        fn drop(&mut self) {
            let _ = self.dropped.send(std::thread::current().id());
        }
    }
    let d = dir("manifest");
    let e = Arc::new(engine(&d));
    // A long command deadline: the manifest's is still 1 s at most.
    e.plugins().set_timeout(Duration::from_secs(60));
    let release = Arc::new(Mutex::new(false));
    let (dropped_tx, dropped) = mpsc::channel();
    let adding = {
        let (e, release, dropped_tx) = (e.clone(), release.clone(), dropped_tx.clone());
        std::thread::spawn(move || {
            let start = Instant::now();
            (e.add_plugin(SlowManifest { release, dropped: dropped_tx }), start.elapsed())
        })
    };
    // The engine answers while `add` waits.
    let start = Instant::now();
    assert_eq!(e.command(&argv(&["new-session", "-d", "-s", "m", "exec cat"]), "/").code, 0);
    assert_eq!(e.command(&argv(&["has-session", "-t", "=m"]), "/").code, 0);
    assert!(start.elapsed() < Duration::from_millis(500), "{:?}", start.elapsed());
    let (r, took) = adding.join().unwrap();
    assert_eq!(r.unwrap_err(), PluginError::ManifestTimeout(1000));
    assert!(took >= Duration::from_millis(1000) && took < Duration::from_millis(5000), "{took:?}");
    assert_eq!(e.command(&argv(&["list-plugins"]), "/").out, "");
    assert!(e.command(&argv(&["slow-cmd"]), "/").err.starts_with("unknown command"));
    // Its manifest returns at last: it is dropped there, on its own thread, not added.
    *release.lock().unwrap() = true;
    let on = dropped.recv_timeout(PATIENCE).expect("the refused plugin is dropped");
    assert_ne!(on, std::thread::current().id());
    assert_eq!(e.command(&argv(&["list-plugins"]), "/").out, "");

    // A polled host: `add_plugin` runs on its loop thread, which gets it back after the deadline (the
    // registry's, when shorter than 1 s) and serves on.
    let pd = d.join("polled");
    std::fs::create_dir_all(&pd).unwrap();
    let mut p = Polled::start(Options { size_dir: pd, ..Options::default() });
    p.plugins().set_timeout(Duration::from_millis(200));
    *release.lock().unwrap() = false;
    let start = Instant::now();
    let r = p.add_plugin(SlowManifest { release: release.clone(), dropped: dropped_tx });
    let took = start.elapsed();
    assert_eq!(r.unwrap_err(), PluginError::ManifestTimeout(200));
    assert!(took < Duration::from_millis(2000), "{took:?}");
    assert_eq!(p.command(&argv(&["new-session", "-d", "-s", "pm", "exec cat"]), "/").code, 0);
    // The name is free: a working plugin of that name comes in at once.
    p.add_plugin(probe("slow-manifest", Events::empty(), &["slow-cmd"]).0).unwrap();
    assert!(p.command(&argv(&["list-plugins"]), "/").out.starts_with("slow-manifest: running"));
    *release.lock().unwrap() = true;
    dropped.recv_timeout(PATIENCE).expect("the refused plugin is dropped");

    // A panicking manifest: refused with the panic's text.
    struct Bomb;
    impl Plugin for Bomb {
        fn manifest(&self) -> Manifest {
            panic!("no manifest")
        }
    }
    assert_eq!(p.add_plugin(Bomb).unwrap_err(), PluginError::ManifestPanicked("no manifest".into()));
    p.command(&argv(&["kill-server"]), "/");
    e.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&d);
}

/// A connection that fails a test instead of hanging it when a line never comes.
fn connect(sock: &std::path::Path) -> UnixStream {
    let s = UnixStream::connect(sock).unwrap();
    s.set_read_timeout(Some(PATIENCE)).unwrap();
    s
}

/// A `zeropty serve` the test started (through the CLI): ended with kill-server when the test ends,
/// passed or not, with the plugin programs it started.
struct Served(PathBuf);

impl Drop for Served {
    fn drop(&mut self) {
        let _ = std::process::Command::new(env!("CARGO_BIN_EXE_zeropty")).arg("kill-server").env("ZEROPTY_SOCK", &self.0).output();
    }
}

/// One JSON line.
fn send(s: &mut UnixStream, v: serde_json::Value) {
    s.write_all(format!("{v}\n").as_bytes()).unwrap();
}

fn read(lines: &mut std::io::Lines<BufReader<UnixStream>>) -> serde_json::Value {
    serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap()
}

/// A program as a plugin, through the socket: it hears events, answers calls, runs commands of its own,
/// and leaves when it hangs up.
#[test]
fn a_plugin_in_another_process() {
    let d = dir("remote");
    let e = engine(&d);
    let sock = d.join("s");
    e.listen(&sock).unwrap();
    let mut p = connect(&sock);
    // Refused: a built-in command.
    send(&mut p, serde_json::json!({ "argv": ["plugin-attach", "-n", "far", "-c", "send-keys"], "cwd": "/" }));
    let mut lines = BufReader::new(p.try_clone().unwrap()).lines();
    let r = read(&mut lines);
    assert_eq!(r["code"], 1);
    assert!(r["err"].as_str().unwrap().contains("built-in"));
    // The same connection tries again, properly.
    send(&mut p, serde_json::json!({ "argv": ["plugin-attach", "-n", "far", "-e", "session-created,output", "-c", "far-hello"], "cwd": "/" }));
    let r = read(&mut lines);
    assert_eq!((r["code"].as_i64(), r["out"].as_str()), (Some(0), Some("zeropty-plugin 1\n")));
    assert!(
        e.command(&argv(&["list-plugins"]), "/").out.contains("far: running (api 1) events=session-created,output commands=far-hello access=read dropped=0")
    );

    e.command(&argv(&["new-session", "-d", "-s", "r", "printf 'from r'; exec cat"]), "/");
    assert_eq!(read(&mut lines), serde_json::json!({ "event": "session-created", "session": "r" }));
    let ev = read(&mut lines);
    assert_eq!((ev["event"].as_str(), ev["session"].as_str()), (Some("output"), Some("r")));
    // base64 of "from r"
    assert_eq!(ev["data"], "ZnJvbSBy");

    // A client calls its command; the plugin asks the engine something before it answers.
    let caller = std::thread::spawn({
        let sock = sock.clone();
        move || {
            let mut c = connect(&sock);
            c.write_all(b"{\"argv\":[\"far-hello\",\"x\"],\"cwd\":\"/tmp\"}\n").unwrap();
            let mut l = String::new();
            BufReader::new(c).read_line(&mut l).unwrap();
            serde_json::from_str::<Reply>(&l).unwrap()
        }
    });
    let call = read(&mut lines);
    assert_eq!(call["argv"], serde_json::json!(["far-hello", "x"]));
    assert_eq!(call["cwd"], "/tmp");
    send(&mut p, serde_json::json!({ "id": 5, "argv": ["list-sessions", "-F", "#{session_name}"], "cwd": "/" }));
    let r = read(&mut lines);
    assert_eq!((r["id"].as_u64(), r["out"].as_str()), (Some(5), Some("r\n")));
    send(&mut p, serde_json::json!({ "reply": call["call"], "code": 0, "out": "hello from far\n", "err": "" }));
    let r = caller.join().unwrap();
    assert_eq!((r.code, r.out.as_str()), (0, "hello from far\n"));
    // Embedded callers reach it as well.
    let embedded = std::thread::spawn({
        let sock = sock.clone();
        move || {
            let o = std::process::Command::new(env!("CARGO_BIN_EXE_zeropty")).args(["far-hello"]).env("ZEROPTY_SOCK", &sock).output().unwrap();
            String::from_utf8_lossy(&o.stdout).into_owned()
        }
    });
    let call = read(&mut lines);
    send(&mut p, serde_json::json!({ "reply": call["call"], "code": 0, "out": "via the CLI\n", "err": "" }));
    assert_eq!(embedded.join().unwrap(), "via the CLI\n");

    // Hung up: gone, its command unknown again.
    drop(lines);
    drop(p);
    wait_for("far removed", || e.command(&argv(&["list-plugins"]), "/").out.is_empty());
    assert!(e.command(&argv(&["far-hello"]), "/").err.starts_with("unknown command"));
    e.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&d);
}

/// A program that attaches and never answers: callers get an error at the deadline, and the engine,
/// the socket and other clients go on meanwhile.
#[test]
fn a_plugin_process_that_never_answers() {
    let d = dir("mute");
    let e = engine(&d);
    e.plugins().set_timeout(Duration::from_millis(300));
    let sock = d.join("s");
    e.listen(&sock).unwrap();
    let mut p = connect(&sock);
    send(&mut p, serde_json::json!({ "argv": ["plugin-attach", "-n", "mute", "-e", "all", "-c", "mute-cmd"], "cwd": "/" }));
    let mut lines = BufReader::new(p.try_clone().unwrap()).lines();
    assert_eq!(read(&mut lines)["code"], 0);
    // It reads nothing from here on.
    e.command(&argv(&["new-session", "-d", "-s", "m", "exec cat"]), "/");
    let start = Instant::now();
    let caller = std::thread::spawn({
        let sock = sock.clone();
        move || {
            let mut c = connect(&sock);
            c.write_all(b"{\"argv\":[\"mute-cmd\"],\"cwd\":\"/\"}\n").unwrap();
            let mut l = String::new();
            BufReader::new(c).read_line(&mut l).unwrap();
            serde_json::from_str::<Reply>(&l).unwrap()
        }
    });
    // Meanwhile the socket answers at once.
    std::thread::sleep(Duration::from_millis(50));
    let t = Instant::now();
    let mut c = connect(&sock);
    c.write_all(b"{\"argv\":[\"has-session\",\"-t\",\"=m\"],\"cwd\":\"/\"}\n").unwrap();
    let mut l = String::new();
    BufReader::new(c).read_line(&mut l).unwrap();
    assert!(l.contains("\"code\":0"), "{l}");
    assert!(t.elapsed() < Duration::from_millis(100), "{:?}", t.elapsed());
    let r = caller.join().unwrap();
    assert!(r.err.contains("timed out"), "{}", r.err);
    assert!(start.elapsed() < Duration::from_millis(2000), "{:?}", start.elapsed());
    drop(p);
    e.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&d);
}

/// A program that attaches for every event, output included, and never reads while a session writes
/// steadily (~100 KB/s): its event writes fill the socket and give up within reply_wait, so it fails as a connection
/// (`failed (connection: ...)`, as docs/PLUGINS.md says), not as a hook stuck past the deadline, and
/// the engine answers all along. (Before, a write that moved a few bytes then timed out returned that
/// count and `write_all` waited again, so the watchdog fired first.)
#[test]
fn a_socket_plugin_that_never_reads_fails_as_a_connection() {
    let d = dir("deaf");
    let e = engine(&d);
    e.plugins().set_timeout(Duration::from_millis(1000));
    let sock = d.join("s");
    e.listen(&sock).unwrap();
    let mut p = connect(&sock);
    send(&mut p, serde_json::json!({ "argv": ["plugin-attach", "-n", "deaf", "-e", "all", "-c", "deaf-cmd"], "cwd": "/" }));
    let mut lines = BufReader::new(p.try_clone().unwrap()).lines();
    assert_eq!(read(&mut lines)["code"], 0);
    // Reads nothing from here on.
    e.command(&argv(&["new-session", "-d", "-s", "flood", "while :; do printf '%0300d\\n' 0; sleep 0.002; done"]), "/");
    let start = Instant::now();
    let mut list = String::new();
    while start.elapsed() < PATIENCE {
        let t = Instant::now();
        list = e.command(&argv(&["list-plugins"]), "/").out;
        assert!(t.elapsed() < Duration::from_millis(1000), "the engine waited {:?}", t.elapsed());
        if list.contains("deaf: failed") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(list.contains("deaf: failed (connection: "), "{list}");
    assert!(!list.contains("stuck"), "{list}");
    assert_eq!(e.command(&argv(&["has-session", "-t", "=flood"]), "/").code, 0);
    drop(lines);
    drop(p);
    e.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&d);
}

/// The host's own loop runs a polled engine: plugins hear it, and their commands (which use the engine)
/// are answered while the host's thread waits.
#[test]
fn plugins_on_a_polled_engine() {
    let d = dir("polled");
    let mut p = Polled::start(Options { size_dir: d.clone(), ..Options::default() });
    let (probe, rx) = probe("probe", Events::SESSIONS, &["probe-name"]);
    p.add_plugin(probe).unwrap();
    assert_eq!(p.command(&argv(&["new-session", "-d", "-s", "pp", "exec cat"]), "/").code, 0);
    until(&rx, |ev| *ev == Event::SessionCreated { session: "pp".into() });
    assert_eq!(p.command(&argv(&["probe-name", "=pp"]), "/").out, "probe says hi to pp\n");
    p.command(&argv(&["kill-session", "-t", "=pp"]), "/");
    until(&rx, |ev| matches!(ev, Event::SessionExited { session, reason: ExitReason::Killed } if session == "pp"));
    p.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&d);
}

/// The Python example against a real `zeropty serve`: a plugin in another language, nothing recompiled.
#[test]
fn the_python_example_plugin() {
    if !have("python3", "--version") {
        return skip("no python3");
    }
    let d = dir("py");
    let sock = d.join("s");
    let zp = |args: &[&str]| std::process::Command::new(env!("CARGO_BIN_EXE_zeropty")).args(args).env("ZEROPTY_SOCK", &sock).output().unwrap();
    assert!(zp(&["new-session", "-d", "-s", "one", "exec cat"]).status.success());
    let _served = Served(sock.clone());
    let example = format!("{ROOT}/examples/plugins/hello.py");
    let mut child = std::process::Command::new("python3").arg(example).env("ZEROPTY_SOCK", &sock).stdout(std::process::Stdio::null()).spawn().unwrap();
    wait_for("hello attached", || String::from_utf8_lossy(&zp(&["list-plugins"]).stdout).contains("hello: running"));
    assert!(zp(&["new-session", "-d", "-s", "two", "exec cat"]).status.success());
    let mut out = String::new();
    wait_for("hello's answer", || {
        out = String::from_utf8_lossy(&zp(&["hello", "tester"]).stdout).into_owned();
        out.contains("created since attach: two")
    });
    assert!(out.starts_with("hello, tester! sessions: one two"), "{out}");
    let _ = zp(&["kill-server"]);
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&d);
}

/// A child process killed and reaped when the test ends, passing or not.
struct Reaped(std::process::Child);
impl Drop for Reaped {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The newest modification time of the Rust sources under `dir` that the C library is built from
/// (leaving out the binary's own: src/main.rs, src/cli/).
fn newest_source(dir: &std::path::Path, newest: &mut (std::time::SystemTime, PathBuf)) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.ends_with("src/main.rs") || p.ends_with("src/cli") {
            continue;
        }
        if p.is_dir() {
            newest_source(&p, newest);
        } else if p.extension().is_some_and(|x| x == "rs")
            && let Ok(t) = e.metadata().and_then(|m| m.modified())
            && t > newest.0
        {
            *newest = (t, p);
        }
    }
}

/// The C library (bindings/c) built from the current sources: libzeropty_c.dylib in this test's own
/// directory (target/<profile>/deps), which `cargo test --workspace` writes first (the package's lib is
/// cdylib + rlib), or in target/<profile> (`cargo build --workspace`, ./build.sh); the newer of the two.
/// Err says why none is fit to test: missing (a plain `cargo test` builds the root package only), or
/// older than a source of the library, so old code is never tested.
fn c_library() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("no test path: {e}"))?;
    let deps = exe.parent().ok_or("no target directory")?;
    let found = [deps.to_path_buf(), deps.parent().unwrap_or(deps).to_path_buf()]
        .into_iter()
        .map(|d| d.join("libzeropty_c.dylib"))
        .filter_map(|lib| Some((std::fs::metadata(&lib).and_then(|m| m.modified()).ok()?, lib)))
        .max();
    let Some((built, lib)) = found else {
        return Err(format!("no C library in {} (run cargo test --workspace, or ./build.sh first)", deps.display()));
    };
    let mut newest = (std::time::SystemTime::UNIX_EPOCH, PathBuf::new());
    newest_source(&std::path::Path::new(ROOT).join("src"), &mut newest);
    newest_source(&std::path::Path::new(ROOT).join("bindings/c/src"), &mut newest);
    if newest.0 > built {
        return Err(format!("the C library at {} is older than {} (run cargo test --workspace, or ./build.sh first)", lib.display(), newest.1.display()));
    }
    Ok(lib)
}

/// examples/plugins/count.c, as its comment builds it: a C program that embeds the engine through the C
/// ABI (libzeropty_c), adds a plugin with a callback, and runs that plugin's command, which itself asks
/// the engine for the sessions.
#[test]
fn the_c_example_plugin() {
    if !have("cc", "--version") {
        return skip("no C compiler (cc)");
    }
    let lib = match c_library() {
        Ok(lib) => lib,
        Err(why) => return skip(&why),
    };
    let d = dir("count");
    let exe = d.join("count");
    let built = std::process::Command::new("cc")
        .args(["-O2", "-Wall", "-o"])
        .arg(&exe)
        .arg(format!("{ROOT}/examples/plugins/count.c"))
        .arg(format!("-I{ROOT}/bindings/c/include"))
        .arg(&lib)
        .arg(format!("-Wl,-rpath,{}", lib.parent().unwrap().display()))
        .output()
        .unwrap();
    assert!(built.status.success(), "count.c does not build: {}", String::from_utf8_lossy(&built.stderr));
    let o = std::process::Command::new(&exe).current_dir(&d).env("ZEROPTY_PLUGIN_DIR", "").output().unwrap();
    assert!(o.status.success(), "count failed: {}", String::from_utf8_lossy(&o.stderr));
    // One session made of `exec cat` (still there), one of `true` (ended): the plugin heard both and
    // counted the live one through zeropty_host_command.
    assert_eq!(String::from_utf8_lossy(&o.stdout), "{\"code\":0,\"out\":\"created 2, ended 1, live 1\\n\",\"err\":\"\"}\n");
    let _ = std::fs::remove_dir_all(&d);
}

/// examples/plugins/notify.mjs against a real server: a Node plugin through the socket (client/plugin.mjs)
/// hears sessions made, renamed and killed, answers its two commands through the CLI (one of them asking
/// the server for the sessions), and ends by itself when the server goes.
#[test]
fn the_node_example_plugin() {
    if !have("node", "--version") {
        return skip("no node");
    }
    let d = dir("notify");
    let sock = d.join("s");
    let zp = |args: &[&str]| {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_zeropty")).args(args).env("ZEROPTY_SOCK", &sock).env("ZEROPTY_PLUGIN_DIR", "").output().unwrap();
        (o.status.success(), String::from_utf8_lossy(&o.stdout).into_owned())
    };
    assert!(zp(&["new-session", "-d", "-s", "one", "exec cat"]).0);
    let _served = Served(sock.clone());
    let example = format!("{ROOT}/examples/plugins/notify.mjs");
    let mut child = Reaped(std::process::Command::new("node").arg(example).env("ZEROPTY_SOCK", &sock).stdout(std::process::Stdio::null()).spawn().unwrap());
    wait_for("notify attached", || zp(&["list-plugins"]).1.contains("notify: running"));
    let list = zp(&["list-plugins"]).1;
    assert!(
        list.contains("notify: running (api 1) events=session-created,session-exited,session-renamed,activity commands=notify-log,notify-idle access=read "),
        "{list}"
    );
    assert!(zp(&["new-session", "-d", "-s", "two", "exec cat"]).0);
    assert!(zp(&["rename-session", "-t", "=two", "three"]).0);
    assert!(zp(&["kill-session", "-t", "=three"]).0);
    let mut log = String::new();
    wait_for("notify's log", || {
        log = zp(&["notify-log"]).1;
        log.contains("killed three")
    });
    let told: Vec<&str> = log.lines().map(|l| l.split_once(' ').map_or(l, |(_, rest)| rest)).collect();
    assert_eq!(told, ["created two", "renamed two -> three", "killed three"], "{log}");
    // notify-idle asks the server for the sessions from inside the plugin: with 0 seconds, all of them.
    assert_eq!(zp(&["notify-idle", "0"]), (true, "one\n".to_string()));
    let _ = zp(&["kill-server"]);
    // The plugin ends with its connection (await plugin.closed), not killed by the test.
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.0.try_wait().unwrap() {
            break s;
        }
        assert!(start.elapsed() < PATIENCE, "notify.mjs did not end with the server");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success(), "{status}");
    let _ = std::fs::remove_dir_all(&d);
}

/// notify.mjs --desktop hands a session's name to AppleScript as an argument, never as script source: a
/// name holding `"` (which new-session accepts) is shown literally instead of ending the string and
/// running what follows. An `osascript` first on PATH records the argv it was given; the real osascript
/// (when present) then runs that same argv with the display line swapped for one that returns the text.
#[test]
fn the_node_example_plugin_shows_names_literally() {
    if !have("node", "--version") {
        return skip("no node");
    }
    let d = dir("notify-quote");
    let bin = d.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let record = d.join("osascript.args");
    let fake = bin.join("osascript");
    std::fs::write(&fake, format!("#!/bin/sh\nprintf '%s\\0' \"$@\" > '{}.tmp' && mv '{0}.tmp' '{0}'\n", record.display())).unwrap();
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let sock = d.join("s");
    let zp = |args: &[&str]| {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_zeropty")).args(args).env("ZEROPTY_SOCK", &sock).env("ZEROPTY_PLUGIN_DIR", "").output().unwrap();
        (o.status.success(), String::from_utf8_lossy(&o.stdout).into_owned())
    };
    assert!(zp(&["new-session", "-d", "-s", "keep", "exec cat"]).0);
    let _served = Served(sock.clone());
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let _child = Reaped(
        std::process::Command::new("node")
            .args([format!("{ROOT}/examples/plugins/notify.mjs").as_str(), "--desktop"])
            .env("ZEROPTY_SOCK", &sock)
            .env("PATH", path)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for("notify attached", || zp(&["list-plugins"]).1.contains("notify: running"));
    let name = "a\"b & c";
    assert!(zp(&["new-session", "-d", "-s", name, "exec cat"]).0);
    assert!(zp(&["kill-session", "-t", &format!("={name}")]).0);
    wait_for("the notification", || record.exists());
    let raw = std::fs::read(&record).unwrap();
    let args: Vec<&str> = raw.split(|&b| b == 0).filter(|a| !a.is_empty()).map(|a| std::str::from_utf8(a).unwrap()).collect();
    let text = format!("{name} killed");
    assert_eq!(args, ["-e", "on run argv", "-e", "display notification (item 1 of argv) with title \"zeropty\"", "-e", "end run", "--", text.as_str()]);
    let real = std::path::Path::new("/usr/bin/osascript");
    if real.exists() {
        let mut echoed: Vec<&str> = args.clone();
        echoed[3] = "return (item 1 of argv)";
        let o = std::process::Command::new(real).args(&echoed).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(String::from_utf8_lossy(&o.stdout), format!("{text}\n"));
    }
    let _ = zp(&["kill-server"]);
    let _ = std::fs::remove_dir_all(&d);
}

/// What a plugin may run through its host is what its manifest asked for: looking by default, typing,
/// managing sessions and calling other plugins only when asked; kill-server never.
#[test]
fn access_is_what_the_manifest_asked_for() {
    let d = dir("access");
    let e = engine(&d);
    e.command(&argv(&["new-session", "-d", "-s", "w", "exec cat"]), "/");
    e.add_plugin(probe("looker", Events::empty(), &["look-run"]).0).unwrap();
    let mut typist = probe("typist", Events::empty(), &["type-run"]).0;
    typist.manifest = typist.manifest.access(Access::INPUT | Access::CALL);
    e.add_plugin(typist).unwrap();
    let mut boss = probe("boss", Events::empty(), &["boss-run"]).0;
    boss.manifest = boss.manifest.access(Access::ALL);
    e.add_plugin(boss).unwrap();
    // The Probe answers any of its commands by running the rest of argv; its name picks the plugin.
    let run = |cmd: &str, rest: &[&str]| {
        let mut a = vec![cmd];
        a.extend_from_slice(rest);
        e.command(&argv(&a), "/")
    };
    assert_eq!(run("look-run", &["has-session", "-t", "=w"]).code, 0);
    let r = run("look-run", &["send-keys", "-t", "=w:", "-l", "x"]);
    assert!(r.err.contains("plugin looker lacks access input"), "{}", r.err);
    assert!(run("look-run", &["kill-session", "-t", "=w"]).err.contains("lacks access manage"));
    assert!(run("look-run", &["type-run", "has-session"]).err.contains("lacks access call"));
    assert_eq!(run("type-run", &["send-keys", "-t", "=w:", "-l", "typed"]).code, 0);
    assert!(run("type-run", &["new-session", "-d", "-s", "x", "exec cat"]).err.contains("lacks access manage"));
    // Another plugin's command, with CALL.
    assert_eq!(run("type-run", &["look-run", "has-session", "-t", "=w"]).code, 0);
    assert_eq!(run("boss-run", &["new-session", "-d", "-s", "x", "exec cat"]).code, 0);
    assert!(run("boss-run", &["kill-server"]).err.contains("not for plugins"));
    assert_eq!(e.command(&argv(&["has-session", "-t", "=x"]), "/").code, 0);
    let list = e.command(&argv(&["list-plugins"]), "/").out;
    assert!(list.contains("typist: running (api 1) events= commands=type-run access=read,input,call dropped=0"), "{list}");
    e.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&d);
}

/// plugin-attach answers only once the plugin is registered: a program told it attached is listed and
/// its command routed at once, every time (an attach acknowledged first raced with both).
#[test]
fn attached_means_registered() {
    let d = dir("ack");
    let e = engine(&d);
    let sock = d.join("s");
    e.listen(&sock).unwrap();
    for k in 0..40 {
        let mut p = connect(&sock);
        // A command before the attach on the same connection: its reply is owed first.
        send(&mut p, serde_json::json!({ "argv": ["has-session", "-t", "=none"], "cwd": "/" }));
        send(&mut p, serde_json::json!({ "argv": ["plugin-attach", "-n", format!("r{k}"), "-c", format!("r{k}-cmd")], "cwd": "/" }));
        let mut lines = BufReader::new(p.try_clone().unwrap()).lines();
        assert_eq!(read(&mut lines)["code"], 1);
        assert_eq!(read(&mut lines)["out"], "zeropty-plugin 1\n");
        assert!(e.command(&argv(&["list-plugins"]), "/").out.contains(&format!("r{k}: running")));
        let caller = std::thread::spawn({
            let cmd = format!("r{k}-cmd");
            let sock = sock.clone();
            move || {
                let mut c = connect(&sock);
                c.write_all(format!("{{\"argv\":[\"{cmd}\"],\"cwd\":\"/\"}}\n").as_bytes()).unwrap();
                let mut l = String::new();
                BufReader::new(c).read_line(&mut l).unwrap();
                serde_json::from_str::<Reply>(&l).unwrap()
            }
        });
        let call = read(&mut lines);
        send(&mut p, serde_json::json!({ "reply": call["call"], "code": 0, "out": "routed\n", "err": "" }));
        assert_eq!(caller.join().unwrap().out, "routed\n");
    }
    e.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&d);
}

/// Whether `tool arg` runs and succeeds (python3, node: the external examples' interpreters). The Node
/// module's own test is in its package (bindings/node/tests/plugin.rs), so that cargo builds the module first.
fn have(tool: &str, arg: &str) -> bool {
    std::process::Command::new(tool).arg(arg).output().is_ok_and(|o| o.status.success())
}

/// A test that needs a tool this machine lacks. It is
/// reported as skipped, and fails instead when ZEROPTY_REQUIRE_ALL=1 (the release check,
/// `ZEROPTY_REQUIRE_ALL=1 cargo test --release --workspace`), so a green run of that check never hides an external-plugin or isolation test that
/// did not execute.
///
/// The line goes straight to the process's stderr, not through `eprintln!`, which the test harness
/// captures and throws away for a passing test: a plain `cargo test` shows it beside its summary.
fn skip(why: &str) {
    if std::env::var("ZEROPTY_REQUIRE_ALL").is_ok_and(|v| v == "1") {
        panic!("{why}, and ZEROPTY_REQUIRE_ALL=1 requires every test to run");
    }
    use std::io::Write as _;
    let test = std::thread::current().name().unwrap_or("?").to_string();
    let _ = writeln!(std::io::stderr(), "SKIPPED {test}: {why} (set ZEROPTY_REQUIRE_ALL=1 to fail instead)");
}

/// Compiles examples/plugins/tally.c into `d` (None without a C compiler).
fn tally_dylib(d: &std::path::Path) -> Option<PathBuf> {
    let out = d.join("tally.dylib");
    let root = ROOT;
    let built = std::process::Command::new("cc")
        .args(["-dynamiclib", "-O2", "-I", &format!("{root}/bindings/c/include"), "-o"])
        .arg(&out)
        .arg(format!("{root}/examples/plugins/tally.c"))
        .status()
        .is_ok_and(|s| s.success());
    if !built {
        skip("no C compiler (cc)");
    }
    built.then_some(out)
}

/// A plugin library (zeropty.h only) run by `zeropty plugin-run` against a real server: its command
/// answers through the CLI, it asks the server for the sessions, and it leaves with the server.
#[test]
fn a_plugin_library_out_of_process() {
    let d = dir("lib");
    let Some(lib) = tally_dylib(&d) else { return };
    let sock = d.join("s");
    let zp = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_zeropty")).args(args).env("ZEROPTY_SOCK", &sock).env("ZEROPTY_PLUGIN_DIR", "").output().unwrap()
    };
    assert!(zp(&["new-session", "-d", "-s", "one", "exec cat"]).status.success());
    let _served = Served(sock.clone());
    // Ended if the test fails (it would otherwise keep the test's output open).
    let mut child = Reaped(
        std::process::Command::new(env!("CARGO_BIN_EXE_zeropty"))
            .arg("plugin-run")
            .arg(&lib)
            .env("ZEROPTY_SOCK", &sock)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for("tally attached", || String::from_utf8_lossy(&zp(&["list-plugins"]).stdout).contains("tally: running"));
    assert!(zp(&["new-session", "-d", "-s", "two", "exec cat"]).status.success());
    let mut out = String::new();
    wait_for("tally's count", || {
        out = String::from_utf8_lossy(&zp(&["tally"]).stdout).into_owned();
        out == "created 1, ended 0, live 2\n"
    });
    // A library that is not a plugin is refused with the reason.
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_zeropty")).args(["plugin-run", "/usr/lib/libz.dylib"]).env("ZEROPTY_SOCK", &sock).output().unwrap();
    assert!(!o.status.success());
    let _ = zp(&["kill-server"]);
    // plugin-run ends with its server.
    let start = Instant::now();
    while child.0.try_wait().unwrap().is_none() {
        assert!(start.elapsed() < PATIENCE, "plugin-run outlived its server");
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// The plugin directory: opt-in (nothing starts without ZEROPTY_PLUGIN_DIR, even with a `plugins`
/// directory next to the socket); named, the server starts what is in it, starts it again when it dies,
/// and a new server from restart-server starts its own, so plugins come through a restart with nothing
/// to do.
#[test]
fn plugins_start_with_the_server_and_survive_a_restart() {
    if !have("python3", "--version") {
        return skip("no python3");
    }
    let d = dir("auto");
    let Some(lib) = tally_dylib(&d) else { return };
    let plugins = d.join("plugins");
    std::fs::create_dir(&plugins).unwrap();
    std::fs::copy(&lib, plugins.join("tally.dylib")).unwrap();
    let hello = plugins.join("hello");
    std::fs::write(&hello, format!("#!/bin/sh\nexec python3 {ROOT}/examples/plugins/hello.py\n")).unwrap();
    std::fs::set_permissions(&hello, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    // Not a plugin: not executable.
    std::fs::write(plugins.join("README"), "notes\n").unwrap();
    let sock = d.join("s");
    let run = |args: &[&str], dir: Option<&std::path::Path>| {
        let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_zeropty"));
        c.args(args).env("ZEROPTY_SOCK", &sock).env_remove("ZEROPTY_PLUGIN_DIR");
        if let Some(dir) = dir {
            c.env("ZEROPTY_PLUGIN_DIR", dir);
        }
        String::from_utf8_lossy(&c.output().unwrap().stdout).into_owned()
    };
    // Not asked for: the same directory, next to the socket, starts nothing.
    let unasked = d.join("plugins");
    assert_eq!(unasked, plugins);
    run(&["new-session", "-d", "-s", "zero", "exec cat"], None);
    let served = Served(sock.clone());
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(run(&["list-plugins"], None), "");
    run(&["kill-server"], None);
    drop(served);
    wait_for("the first server gone", || UnixStream::connect(&sock).is_err());
    let zp = |args: &[&str]| run(args, Some(&plugins));
    zp(&["new-session", "-d", "-s", "one", "exec cat"]);
    let _served = Served(sock.clone());
    let both = |list: &str| list.contains("hello: running") && list.contains("tally: running");
    wait_for("both plugins started", || both(&zp(&["list-plugins"])));
    assert_eq!(zp(&["list-plugins"]).lines().count(), 2);
    assert!(zp(&["hello", "x"]).starts_with("hello, x! sessions: one"));
    // A plugin that dies is started again.
    let killed = std::process::Command::new("pkill").args(["-f", &format!("plugin-run {}", plugins.join("tally.dylib").display())]).status().unwrap();
    assert!(killed.success());
    wait_for("tally gone", || !zp(&["list-plugins"]).contains("tally: running"));
    wait_for("tally back", || zp(&["list-plugins"]).contains("tally: running"));
    // restart-server: the sessions move, and the plugins come back on the new server.
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_zeropty"))
        .arg("restart-server")
        .env("ZEROPTY_SOCK", &sock)
        .env("ZEROPTY_PLUGIN_DIR", &plugins)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    wait_for("plugins on the new server", || both(&zp(&["list-plugins"])));
    assert!(zp(&["hello", "y"]).starts_with("hello, y! sessions: one"));
    zp(&["kill-server"]);
    // kill-server ends the programs it started.
    wait_for("plugin programs ended", || {
        let ps = std::process::Command::new("pgrep").args(["-f", &plugins.display().to_string()]).output().unwrap();
        ps.stdout.is_empty()
    });
    let _ = std::fs::remove_dir_all(&d);
}

/// A plugin is out from the moment it failed, not from the host's next pump: on a polled engine, which
/// pumps only inside its calls, the command after the failure is already unknown, and the name free.
#[test]
fn failed_means_out_at_once() {
    let d = dir("failnow");
    let mut p = Polled::start(Options { size_dir: d.clone(), ..Options::default() });
    p.add_plugin(probe("quitter", Events::empty(), &["probe-fail"]).0).unwrap();
    assert_eq!(p.command(&argv(&["probe-fail"]), "/").out, "failing\n");
    let r = p.command(&argv(&["probe-fail"]), "/");
    assert!(r.err.starts_with("unknown command"), "{}", r.err);
    p.add_plugin(probe("quitter", Events::empty(), &["probe-fail"]).0).unwrap();
    assert!(p.command(&argv(&["list-plugins"]), "/").out.starts_with("quitter: running"));
    let _ = std::fs::remove_dir_all(&d);
}

/// Waits in a hook for `release`, counting the events it got.
struct Blocked {
    manifest: Manifest,
    release: Arc<Mutex<bool>>,
    seen: Arc<std::sync::atomic::AtomicUsize>,
}

impl Plugin for Blocked {
    fn manifest(&self) -> Manifest {
        self.manifest.clone()
    }
    fn event(&mut self, _: &Event, _: &Host) {
        self.seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        while !*self.release.lock().unwrap() {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn dropped(list: &str, name: &str) -> u64 {
    list.lines().find(|l| l.starts_with(&format!("{name}:"))).and_then(|l| l.rsplit("dropped=").next()).unwrap().parse().unwrap()
}

/// A plugin that wants output and stopped taking it (its queue is full) costs the engine nothing per
/// read: the engine stops copying output (the reads are counted as dropped) until the queue drains, and
/// commands answer as fast as without it under an output flood.
#[test]
fn a_full_output_plugin_costs_the_engine_nothing() {
    let d = dir("mute");
    let e = engine(&d);
    // Long: the plugin is to stay registered, full, while it is measured.
    e.plugins().set_timeout(Duration::from_secs(120));
    // A steady flood (~4 MB/s). Not `yes`: unthrottled, it starves embedded commands for seconds with or
    // without plugins, which would measure the engine's reads, not the plugin's cost.
    let flood = r#"exec perl -e '$|=1; while (1) { print "x" x 4000, "\n"; select(undef, undef, undef, 0.001) }'"#;
    assert_eq!(e.command(&argv(&["new-session", "-d", "-s", "flood", flood]), "/").code, 0);
    std::thread::sleep(Duration::from_millis(300));
    let measure = || {
        let mut v = Vec::new();
        for _ in 0..400 {
            let start = Instant::now();
            assert_eq!(e.command(&argv(&["display-message", "-p", "-t", "=flood:", "#{session_name}"]), "/").out, "flood\n");
            v.push(start.elapsed());
        }
        v.sort();
        (v[v.len() / 2], v[v.len() * 99 / 100], v[v.len() - 1])
    };
    let without = measure();
    let (release, seen) = (Arc::new(Mutex::new(false)), Arc::new(std::sync::atomic::AtomicUsize::new(0)));
    e.add_plugin(Blocked { manifest: Manifest::new("full").events(Events::OUTPUT | Events::ACTIVITY), release: release.clone(), seen: seen.clone() }).unwrap();
    wait_for("its queue full", || dropped(&e.command(&argv(&["list-plugins"]), "/").out, "full") > 0);
    // Muted: what the engine queues between pumps holds no output (no bytes copied for nobody).
    for _ in 0..50 {
        let copied = e.with(|en| __private::take_engine_events(en).iter().any(|ev| matches!(ev, Event::Output { .. } | Event::Activity { .. })));
        assert!(!copied, "the engine still builds output events for a full plugin");
        std::thread::sleep(Duration::from_millis(2));
    }
    let with = measure();
    eprintln!("display-message under a flood p50/p99/max: without a plugin {without:?}, with a full one {with:?}");
    let bound = (without.2 * 4).max(Duration::from_millis(50));
    assert!(with.2 < bound, "with a full plugin: {with:?}, without: {without:?}");
    // The reads it misses are counted as dropped all along.
    let before = dropped(&e.command(&argv(&["list-plugins"]), "/").out, "full");
    wait_for("more dropped", || dropped(&e.command(&argv(&["list-plugins"]), "/").out, "full") > before);
    // Drained, it gets output again: more events than its queue held.
    *release.lock().unwrap() = true;
    wait_for("output again", || seen.load(std::sync::atomic::Ordering::Relaxed) > 1024 + 64);
    assert!(e.command(&argv(&["list-plugins"]), "/").out.contains("full: running"));
    e.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&d);
}

/// The peak memory (macOS's lifetime-max physical footprint) of the server process `pid`.
#[cfg(target_os = "macos")]
fn peak_footprint(pid: i32) -> u64 {
    // SAFETY: an all-zero rusage_info_v4 is valid (integers only).
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is a live rusage_info_v4, the struct RUSAGE_INFO_V4 asks for.
    let r = unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V4, (&raw mut info).cast()) };
    assert_eq!(r, 0, "proc_pid_rusage({pid}): {}", std::io::Error::last_os_error());
    info.ri_lifetime_max_phys_footprint
}

/// The peak memory (Linux's peak resident set, VmHWM) of the server process `pid`.
#[cfg(target_os = "linux")]
fn peak_footprint(pid: i32) -> u64 {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
    let kb = status.lines().find_map(|l| l.strip_prefix("VmHWM:")).and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok());
    kb.expect("VmHWM") << 10
}

/// The memory side of a_full_output_plugin_costs_the_engine_nothing: a 200 MB burst (`yes`, read in
/// one slurp) while a socket plugin that wants output never reads, so its queue stays full the whole
/// time (a long deadline keeps it registered). Output events come in pieces of at most 64 KB and each
/// plugin's queue has a byte budget, so the server's peak footprint is the same as with no plugin, give
/// or take the budgets (engine 16 MB, copy 16 MB, the plugin's queue 8 MB). When one event held the whole
/// burst, the same run took the server from 141 MB to 656 MB, one 268 MB allocation held by the queue.
///
/// Opt-in (`ZEROPTY_REQUIRE_ALL=1`, the release check): it compares two processes' peak
/// footprints, which a loaded machine can move, while the budgets themselves are pinned by the
/// deterministic unit tests (`engine::tap` bursts_come_in_bounded_pieces, the registry's byte budget).
#[test]
fn a_burst_to_a_plugin_that_never_reads_stays_within_the_budgets() {
    if !std::env::var("ZEROPTY_REQUIRE_ALL").is_ok_and(|v| v == "1") {
        use std::io::Write as _;
        let _ = writeln!(
            std::io::stderr(),
            "SKIPPED a_burst_to_a_plugin_that_never_reads_stays_within_the_budgets: memory measurement, runs with ZEROPTY_REQUIRE_ALL=1"
        );
        return;
    }
    let d = dir("burst");
    let peak = |plugin: bool| -> u64 {
        let sock = d.join(if plugin { "p" } else { "n" });
        let run = |args: &[&str]| {
            let o = std::process::Command::new(env!("CARGO_BIN_EXE_zeropty"))
                .args(args)
                .env("ZEROPTY_SOCK", &sock)
                .env("ZEROPTY_PLUGIN_TIMEOUT_MS", "120000")
                .env_remove("ZEROPTY_PLUGIN_DIR")
                .output()
                .unwrap();
            (o.status.code(), String::from_utf8_lossy(&o.stdout).into_owned())
        };
        assert_eq!(run(&["new-session", "-d", "-s", "idle", "exec cat"]).0, Some(0));
        let _served = Served(sock.clone());
        // The server is the parent of its sessions' programs.
        let pane = run(&["display-message", "-p", "-t", "=idle:", "#{pane_pid}"]).1;
        let ps = std::process::Command::new("ps").args(["-o", "ppid=", "-p", pane.trim()]).output().unwrap();
        let server: i32 = String::from_utf8_lossy(&ps.stdout).trim().parse().unwrap();
        let deaf = plugin.then(|| {
            let mut p = connect(&sock);
            send(&mut p, serde_json::json!({ "argv": ["plugin-attach", "-n", "deaf", "-e", "output"], "cwd": "/" }));
            let mut lines = BufReader::new(p.try_clone().unwrap()).lines();
            assert_eq!(read(&mut lines)["code"], 0);
            p // reads nothing more
        });
        assert_eq!(run(&["new-session", "-d", "-s", "burst", "yes | head -c 209715200"]).0, Some(0));
        wait_for("the burst read", || run(&["has-session", "-t", "=burst"]).0 != Some(0));
        if plugin {
            let list = run(&["list-plugins"]).1;
            // Still registered, its queue held: the worst case for memory.
            assert!(list.contains("deaf: running"), "{list}");
        }
        let peak = peak_footprint(server);
        drop(deaf);
        run(&["kill-server"]);
        peak
    };
    let without = peak(false);
    let with = peak(true);
    let mb = |b: u64| b >> 20;
    eprintln!("server peak footprint over a 200 MB burst: {} MB without a plugin, {} MB with one that never reads", mb(without), mb(with));
    assert!(with < without + (64 << 20), "{} MB with the plugin, {} MB without", mb(with), mb(without));
    let _ = std::fs::remove_dir_all(&d);
}

/// Plugin A calls B's command while B, answering, calls A's: refused at once ("call cycle") instead of
/// both waiting for each other until their deadlines, and both stay running.
#[test]
fn call_cycles_are_refused() {
    let d = dir("cycle");
    let e = engine(&d);
    e.plugins().set_timeout(Duration::from_secs(20));
    let caller = |name: &str| Probe { manifest: Manifest::new(name).command(format!("{name}-run")).access(Access::CALL), tx: mpsc::channel().0 };
    e.add_plugin(caller("a")).unwrap();
    e.add_plugin(caller("b")).unwrap();
    // A chain without a cycle goes through.
    assert_eq!(e.command(&argv(&["a-run", "b-run", "list-sessions"]), "/").code, 0);
    let start = Instant::now();
    let r = e.command(&argv(&["a-run", "b-run", "a-run", "list-sessions"]), "/");
    assert!(r.err.contains("call cycle (b -> a -> b)"), "{}", r.err);
    assert!(start.elapsed() < Duration::from_secs(5), "{:?}", start.elapsed());
    let list = e.command(&argv(&["list-plugins"]), "/").out;
    assert!(list.contains("a: running") && list.contains("b: running"), "{list}");
    // And afterwards, nobody waits for anybody.
    assert_eq!(e.command(&argv(&["b-run", "a-run", "list-sessions"]), "/").code, 0);
    e.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&d);
}

/// The hook watchdog counts a plugin's own time: one whose hook waits on Host::command (here another
/// plugin's slow command) longer than the deadline in all is not "stuck", while the slow one is judged
/// on its own hooks.
#[test]
fn waiting_on_the_host_is_not_being_stuck() {
    let d = dir("wait");
    let e = engine(&d);
    e.plugins().set_timeout(Duration::from_millis(1000));
    e.add_plugin(probe("sleeper", Events::empty(), &["probe-sleep"]).0).unwrap();
    struct Patient(Sender<Vec<Reply>>);
    impl Plugin for Patient {
        fn manifest(&self) -> Manifest {
            Manifest::new("patient").events(Events::SESSION_CREATED).access(Access::CALL)
        }
        fn event(&mut self, _: &Event, host: &Host) {
            // 4 x 300 ms waited, against a deadline of 1000 ms.
            let _ = self.0.send((0..4).map(|_| host.command(&["probe-sleep", "300"], "/")).collect());
        }
    }
    let (tx, rx) = mpsc::channel();
    e.add_plugin(Patient(tx)).unwrap();
    e.command(&argv(&["new-session", "-d", "-s", "w", "exec cat"]), "/");
    let replies = rx.recv_timeout(PATIENCE).unwrap();
    let errs: Vec<&str> = replies.iter().map(|r| r.err.as_str()).collect();
    assert!(replies.iter().all(|r| r.out == "woke\n"), "{errs:?}");
    let list = e.command(&argv(&["list-plugins"]), "/").out;
    assert!(list.contains("patient: running") && list.contains("sleeper: running"), "{list}");
    // Host::command gives up at half the deadline, with the reason.
    let (tx, rx) = mpsc::channel();
    struct Impatient(Sender<Reply>);
    impl Plugin for Impatient {
        fn manifest(&self) -> Manifest {
            Manifest::new("impatient").events(Events::SESSION_CREATED).access(Access::CALL)
        }
        fn event(&mut self, _: &Event, host: &Host) {
            let _ = self.0.send(host.command(&["probe-sleep", "800"], "/"));
        }
    }
    e.add_plugin(Impatient(tx)).unwrap();
    e.command(&argv(&["new-session", "-d", "-s", "v", "exec cat"]), "/");
    let r = rx.recv_timeout(PATIENCE).unwrap();
    assert!(r.err.contains("probe-sleep: no answer within 500 ms"), "{}", r.err);
    e.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&d);
}

/// A command whose caller gave up is never run afterwards: `Host::command` past half the deadline drops
/// what the engine did not start yet ("not run"), and so does a client's call still queued behind a
/// busy plugin at the deadline. Otherwise a send-keys or kill-session reported failed would happen later.
#[test]
fn given_up_commands_are_not_run_later() {
    let d = dir("cancel");
    // A polled engine runs nothing a plugin asks until the host calls process: the plugin's wait runs out
    // with the request still in the engine's inbox.
    let pd = d.join("polled");
    std::fs::create_dir_all(&pd).unwrap();
    let mut p = Polled::start(Options { size_dir: pd, ..Options::default() });
    p.plugins().set_timeout(Duration::from_millis(200));
    struct Late(Receiver<()>, Sender<Reply>);
    impl Plugin for Late {
        fn manifest(&self) -> Manifest {
            Manifest::new("late").access(Access::MANAGE)
        }
        fn start(&mut self, host: &Host) {
            let _ = self.0.recv();
            let _ = self.1.send(host.command(&["new-session", "-d", "-s", "late", "exec cat"], "/"));
        }
    }
    let ((go, wait), (tx, rx)) = (mpsc::channel(), mpsc::channel());
    p.add_plugin(Late(wait, tx)).unwrap();
    go.send(()).unwrap();
    let r = rx.recv_timeout(PATIENCE).unwrap();
    assert!(r.err.contains("no answer within 100 ms") && r.err.contains("not run"), "{}", r.err);
    // The host's loop turns now: the request it finds was given up, and is dropped.
    for _ in 0..20 {
        p.process();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_ne!(p.command(&argv(&["has-session", "-t", "=late"]), "/").code, 0, "run after being reported failed");
    drop(p);

    // A client's call queued behind a hook that runs past the deadline.
    let e = engine(&d);
    e.plugins().set_timeout(Duration::from_millis(300));
    let ran = Arc::new(Mutex::new(false));
    struct Marker(Arc<Mutex<bool>>);
    impl Plugin for Marker {
        fn manifest(&self) -> Manifest {
            Manifest::new("marker").command("mk-sleep").command("mk-mark")
        }
        fn command(&mut self, argv: &[String], _: &str, _: &Host) -> Reply {
            if argv[0] == "mk-sleep" {
                std::thread::sleep(Duration::from_millis(800));
            } else {
                *self.0.lock().unwrap() = true;
            }
            ok("")
        }
    }
    e.add_plugin(Marker(ran.clone())).unwrap();
    let e = Arc::new(e);
    let sleeper = {
        let e = e.clone();
        std::thread::spawn(move || e.command(&argv(&["mk-sleep"]), "/"))
    };
    std::thread::sleep(Duration::from_millis(50));
    let r = e.command(&argv(&["mk-mark"]), "/");
    assert!(r.err.contains("timed out (busy; not run)"), "{}", r.err);
    assert!(sleeper.join().unwrap().err.contains("timed out"));
    // The sleep ends and the plugin's thread takes the next message: the given-up call is skipped.
    std::thread::sleep(Duration::from_millis(1000));
    assert!(!*ran.lock().unwrap(), "mk-mark ran after its caller was told it timed out");
    let _ = std::fs::remove_dir_all(&d);
}

/// The stuck-hook watchdog runs on the engine's loop: an idle engine (no session, no client, nothing to
/// wake it) still turns often enough to cut a stuck plugin off near its deadline.
#[test]
fn the_watchdog_runs_when_idle() {
    let d = dir("idle");
    let e = Arc::new(engine(&d));
    e.plugins().set_timeout(Duration::from_millis(300));
    let release = Arc::new(Mutex::new(false));
    struct Hang(Arc<Mutex<bool>>);
    impl Plugin for Hang {
        fn manifest(&self) -> Manifest {
            Manifest::new("hang").command("hang")
        }
        fn command(&mut self, _: &[String], _: &str, _: &Host) -> Reply {
            while !*self.0.lock().unwrap() {
                std::thread::sleep(Duration::from_millis(5));
            }
            ok("")
        }
    }
    e.add_plugin(Hang(release.clone())).unwrap();
    // The call times out on the caller's side, without waking the engine.
    assert!(e.command(&argv(&["hang"]), "/").err.contains("timed out"));
    let start = Instant::now();
    // list() reads the registry without pumping: only the engine's own loop can have marked it.
    while !e.plugins().list().out.contains("hang: failed (stuck in a hook") {
        assert!(start.elapsed() < Duration::from_secs(3), "not cut off while idle: {}", e.plugins().list().out);
        std::thread::sleep(Duration::from_millis(10));
    }
    *release.lock().unwrap() = true;
    let _ = std::fs::remove_dir_all(&d);
}

/// Dropping an engine waits for its plugins' threads up to the shutdown wait, then leaves a stuck one
/// behind instead of blocking the host.
#[test]
fn shutdown_waits_as_long_as_told() {
    let d = dir("shut");
    let e = engine(&d);
    let release = Arc::new(Mutex::new(false));
    struct Hang(Arc<Mutex<bool>>);
    impl Plugin for Hang {
        fn manifest(&self) -> Manifest {
            Manifest::new("hang").events(Events::SESSION_CREATED)
        }
        fn event(&mut self, _: &Event, _: &Host) {
            while !*self.0.lock().unwrap() {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    e.add_plugin(Hang(release.clone())).unwrap();
    e.plugins().set_shutdown_wait(Duration::from_millis(50));
    e.command(&argv(&["new-session", "-d", "-s", "w", "exec cat"]), "/");
    std::thread::sleep(Duration::from_millis(100));
    e.command(&argv(&["kill-server"]), "/");
    let start = Instant::now();
    drop(e);
    assert!(start.elapsed() < Duration::from_millis(500), "{:?}", start.elapsed());
    *release.lock().unwrap() = true;
    let _ = std::fs::remove_dir_all(&d);
}

/// The watchdog's cap on how long a host sleeps: none without plugins, a quarter of the deadline (10 ms
/// to 1 s) with them; a polled host's `timeout_ms` honours it.
#[test]
fn the_watchdog_cap_is_a_quarter_deadline() {
    use zeropty::plugin::Plugins;
    struct Quiet;
    impl Plugin for Quiet {
        fn manifest(&self) -> Manifest {
            Manifest::new("quiet")
        }
    }
    let p = Plugins::new();
    assert_eq!(p.watchdog_ms(), None, "no plugin: no cap");
    p.add(Quiet).unwrap();
    for (deadline, cap) in [(10_000, 1000), (4_000, 1000), (2_000, 500), (300, 75), (40, 10), (1, 10)] {
        p.set_timeout(Duration::from_millis(deadline));
        assert_eq!(p.watchdog_ms(), Some(cap), "deadline {deadline} ms");
    }
    assert!(p.remove("quiet"));
    assert_eq!(p.watchdog_ms(), None, "removed: no cap");
    p.shutdown();

    let d = dir("cap");
    let mut polled = Polled::start(Options { size_dir: d.clone(), ..Options::default() });
    // Idle with no plugin: only the engine's own timers (its sweep, 250 ms).
    assert!(polled.timeout_ms() > 75, "{}", polled.timeout_ms());
    polled.add_plugin(Quiet).unwrap();
    polled.plugins().set_timeout(Duration::from_millis(300));
    assert!(polled.timeout_ms() <= 75, "{}", polled.timeout_ms());
    polled.plugins().set_timeout(Duration::from_millis(40));
    assert!(polled.timeout_ms() <= 10, "{}", polled.timeout_ms());
    drop(polled);
    let _ = std::fs::remove_dir_all(&d);
}

/// Out-of-process hooks give up clearly before the watchdog would: a socket plugin that answers a call
/// late fails that call only, stays attached and answers the next one. (Waiting the whole deadline made
/// it a race whether the plugin was cut off as stuck: 4 runs in 5 were.)
#[test]
fn a_late_reply_leaves_a_socket_plugin_running() {
    let d = dir("late");
    let e = engine(&d);
    // Roomy for a loaded machine: the round trips below must fit well inside it.
    let deadline = Duration::from_millis(1000);
    e.plugins().set_timeout(deadline);
    let sock = d.join("s");
    e.listen(&sock).unwrap();
    let mut p = connect(&sock);
    send(&mut p, serde_json::json!({ "argv": ["plugin-attach", "-n", "late", "-c", "late-cmd"], "cwd": "/" }));
    let mut lines = BufReader::new(p.try_clone().unwrap()).lines();
    assert_eq!(read(&mut lines)["code"], 0);
    let call = |sock: PathBuf| {
        std::thread::spawn(move || {
            let mut c = connect(&sock);
            c.write_all(b"{\"argv\":[\"late-cmd\"],\"cwd\":\"/\"}\n").unwrap();
            let mut l = String::new();
            BufReader::new(c).read_line(&mut l).unwrap();
            serde_json::from_str::<Reply>(&l).unwrap()
        })
    };
    for round in 0..3 {
        // Late: answered just past the deadline.
        let caller = call(sock.clone());
        let c = read(&mut lines);
        std::thread::sleep(deadline + Duration::from_millis(100));
        send(&mut p, serde_json::json!({ "reply": c["call"], "code": 0, "out": "late\n", "err": "" }));
        let r = caller.join().unwrap();
        // Either side may give up first: the plugin thread ("a later reply is ignored") or, when that
        // thread started late on a loaded machine, the caller's own deadline ("plugin timed out").
        assert!(r.err.contains("timed out"), "round {round}: {}", r.err);
        // Long enough for the watchdog to have run several times since.
        std::thread::sleep(deadline);
        let list = e.command(&argv(&["list-plugins"]), "/").out;
        assert!(list.contains("late: running"), "round {round}: {list}");
        // On time: answered.
        let caller = call(sock.clone());
        let c = read(&mut lines);
        send(&mut p, serde_json::json!({ "reply": c["call"], "code": 0, "out": "on time\n", "err": "" }));
        let r = caller.join().unwrap();
        assert_eq!((r.code, r.out.as_str()), (0, "on time\n"), "round {round}: {}", r.err);
    }
    drop(lines);
    drop(p);
    e.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&d);
}

/// A polled host's thread waiting for a slow plugin command goes on serving its engine: another
/// client's display-message through the socket is answered meanwhile, and the session's output read.
#[test]
fn a_slow_plugin_command_does_not_stall_a_polled_engine() {
    let d = dir("pslow");
    let sock = d.join("s");
    let mut p = Polled::start(Options { size_dir: d.clone(), ..Options::default() });
    p.listen(&sock).unwrap();
    let (probe, _rx) = probe("probe", Events::empty(), &["probe-sleep"]);
    p.add_plugin(probe).unwrap();
    assert_eq!(p.command(&argv(&["new-session", "-d", "-s", "ps", "exec cat"]), "/").code, 0);
    let client = std::thread::spawn({
        let sock = sock.clone();
        move || {
            std::thread::sleep(Duration::from_millis(200));
            let t = Instant::now();
            let mut c = connect(&sock);
            c.write_all(b"{\"argv\":[\"display-message\",\"-p\",\"-t\",\"=ps\",\"#{session_name}\"],\"cwd\":\"/\"}\n").unwrap();
            let mut l = String::new();
            BufReader::new(c).read_line(&mut l).unwrap();
            (t.elapsed(), serde_json::from_str::<Reply>(&l).unwrap())
        }
    });
    let start = Instant::now();
    let r = p.command(&argv(&["probe-sleep", "1500"]), "/");
    assert_eq!((r.code, r.out.as_str()), (0, "woke\n"), "{}", r.err);
    assert!(start.elapsed() >= Duration::from_millis(1500));
    let (took, r) = client.join().unwrap();
    assert_eq!(r.out, "ps\n", "{}", r.err);
    // Served while the plugin slept (1.3 s were left), not after.
    assert!(took < Duration::from_millis(500), "{took:?}");
    p.command(&argv(&["kill-server"]), "/");
    drop(p);
    let _ = std::fs::remove_dir_all(&d);
}

/// CPU time of the calling thread.
fn thread_cpu() -> Duration {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

/// A polled host starts a plugin command and goes on with its loop: `command_start` returns at once,
/// the engine answers other commands meanwhile, and the plugin's reply turns `fd()` readable, so a loop
/// that sleeps on fd() and timeout_ms() picks it up in a few turns instead of spinning.
#[test]
fn a_polled_host_polls_a_plugin_command() {
    let d = dir("pstart");
    let mut p = Polled::start(Options { size_dir: d.clone(), ..Options::default() });
    let (probe, _rx) = probe("probe", Events::empty(), &["probe-sleep"]);
    p.add_plugin(probe).unwrap();
    // An engine command comes back answered.
    let call = p.command_start(&argv(&["new-session", "-d", "-s", "pq", "exec cat"]), "/");
    assert_eq!(call.poll().map(|r| r.code), Some(0));
    assert!(call.poll().unwrap().err.contains("answered already"));
    let start = Instant::now();
    let call = p.command_start(&argv(&["probe-sleep", "400"]), "/");
    assert!(start.elapsed() < Duration::from_millis(100), "{:?}", start.elapsed());
    assert!(call.poll().is_none());
    // The host's thread is free: the engine answers meanwhile.
    assert_eq!(p.command(&argv(&["display-message", "-p", "-t", "=pq", "#{session_name}"]), "/").out, "pq\n");
    let (cpu, mut turns) = (thread_cpu(), 0);
    let r = loop {
        let mut fds = libc::pollfd { fd: p.fd(), events: libc::POLLIN, revents: 0 };
        unsafe { libc::poll(&mut fds, 1, p.timeout_ms().min(i32::MAX as u64) as i32) };
        p.process();
        turns += 1;
        if let Some(r) = call.poll() {
            break r;
        }
    };
    assert_eq!((r.code, r.out.as_str()), (0, "woke\n"), "{}", r.err);
    assert!(start.elapsed() >= Duration::from_millis(400));
    // Woken by the reply (and the watchdog's turns, 1 s apart by default), not every millisecond.
    assert!(turns < 20, "{turns} turns");
    assert!(thread_cpu() - cpu < Duration::from_millis(20), "{:?}", thread_cpu() - cpu);
    p.command(&argv(&["kill-server"]), "/");
    drop(p);
    let _ = std::fs::remove_dir_all(&d);
}

/// A started call that the plugin never answers times out on time: `timeout_ms()` counts its deadline,
/// so a host sleeping on fd() wakes for it, and the command, never started, is reported not run.
#[test]
fn a_started_call_times_out_on_time() {
    let d = dir("pdue");
    let mut p = Polled::start(Options { size_dir: d.clone(), ..Options::default() });
    p.plugins().set_timeout(Duration::from_millis(1200));
    let (probe, _rx) = probe("probe", Events::empty(), &["probe-sleep", "probe-name"]);
    p.add_plugin(probe).unwrap();
    // Out of phase with the engine's 250 ms sweep, which would otherwise wake the host near the deadline.
    std::thread::sleep(Duration::from_millis(125));
    // The plugin is busy for 3 s: the second call waits in its queue past the deadline.
    let busy = p.command_start(&argv(&["probe-sleep", "3000"]), "/");
    let start = Instant::now();
    let queued = p.command_start(&argv(&["probe-name", "x"]), "/");
    // Due by the call's deadline at the latest.
    assert!(p.timeout_ms() <= queued.due_ms(), "{} > {}", p.timeout_ms(), queued.due_ms());
    let r = loop {
        let mut fds = libc::pollfd { fd: p.fd(), events: libc::POLLIN, revents: 0 };
        unsafe { libc::poll(&mut fds, 1, p.timeout_ms().min(i32::MAX as u64) as i32) };
        p.process();
        if let Some(r) = queued.poll() {
            break r;
        }
    };
    let took = start.elapsed();
    assert!(r.err.contains("timed out (busy; not run)"), "{}", r.err);
    // On time: the engine's own timers (the 250 ms sweep, the watchdog's quarter deadline) would wake
    // the host up to 250 ms late. 150 ms of slack: the whole suite runs in parallel, and a 60 ms bound
    // failed now and then under that load while still on time.
    // 1 ms early at most: the engine counts whole milliseconds, so a deadline set at 10.9 ms is due at
    // 1210, 1199.1 ms later. kqueue's wakeups came late enough to hide it; epoll's (Linux) do not.
    assert!(took >= Duration::from_millis(1199) && took < Duration::from_millis(1350), "{took:?}");
    drop(busy);
    drop(p);
    let _ = std::fs::remove_dir_all(&d);
}

/// `Polled::command` waiting for a slow plugin sleeps on the engine's kqueue: the wait costs the
/// host's thread next to no CPU (it turned every millisecond before).
#[test]
fn a_blocking_polled_command_does_not_spin() {
    let d = dir("pspin");
    let mut p = Polled::start(Options { size_dir: d.clone(), ..Options::default() });
    let (probe, _rx) = probe("probe", Events::empty(), &["probe-sleep"]);
    p.add_plugin(probe).unwrap();
    // `stats` counts the engine's kqueue waits ("loops").
    let loops = |p: &mut Polled| {
        let out = p.command(&argv(&["stats"]), "/").out;
        out.split(' ').skip_while(|w| *w != "loops").nth(1).unwrap().parse::<u64>().unwrap()
    };
    let before = loops(&mut p);
    let (cpu, start) = (thread_cpu(), Instant::now());
    let r = p.command(&argv(&["probe-sleep", "800"]), "/");
    assert_eq!(r.out, "woke\n", "{}", r.err);
    let (cpu, took) = (thread_cpu() - cpu, start.elapsed());
    let turns = loops(&mut p) - before;
    assert!(took >= Duration::from_millis(800));
    // Woken by the reply and the engine's own timers (the 250 ms sweep), not every millisecond.
    assert!(turns < 20, "{turns} turns of the engine's loop over {took:?}");
    assert!(cpu < Duration::from_millis(10), "{cpu:?} of CPU over {took:?}");
    drop(p);
    let _ = std::fs::remove_dir_all(&d);
}

/// A started call dropped before the plugin got to it is never run.
#[test]
fn a_dropped_call_is_not_run() {
    let d = dir("pdrop");
    let mut p = Polled::start(Options { size_dir: d.clone(), ..Options::default() });
    let (probe, _rx) = probe("probe", Events::empty(), &["probe-sleep", "probe-fail"]);
    p.add_plugin(probe).unwrap();
    let busy = p.command_start(&argv(&["probe-sleep", "300"]), "/");
    // Would mark the plugin failed, were it run.
    drop(p.command_start(&argv(&["probe-fail"]), "/"));
    let start = Instant::now();
    while busy.poll().is_none() || start.elapsed() < Duration::from_millis(500) {
        p.process();
        std::thread::sleep(Duration::from_millis(5));
    }
    let list = p.command(&argv(&["list-plugins"]), "/").out;
    assert!(list.starts_with("probe: running"), "{list}");
    drop(p);
    let _ = std::fs::remove_dir_all(&d);
}
