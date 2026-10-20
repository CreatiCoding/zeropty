//! The engine embedded in this (test) process: commands from several threads, and what one costs.
use std::time::{Duration, Instant};
use zeropty::embed::{Embedded, Polled};
use zeropty::engine::Options;

/// Taken (shared) by the tests that start processes, and alone by the one that checks a socket left
/// behind is taken over. A process forked on another thread holds a copy of every descriptor until it
/// execs, the listener's included (FD_CLOEXEC closes it only at exec): for those microseconds the old
/// socket still accepts connections and looks like a live server. Measured: 3 to 6 failures in 12 runs
/// of this file in parallel, the socket answering 30-80 us more; 0 in 25 run one test at a time.
///
/// Test-local on purpose, not zeropty's own (sys::fd::FORKS, which every fd the crate makes and every
/// process it starts goes through). That one is held for one spawn or one fd at a time; this one for a
/// whole test, which is what the check needs: every test here that starts anything (its engines'
/// sessions, and CLI children it spawns itself with std, which zeropty's lock never sees) holds it
/// shared from start to end, so while the listen test holds it alone, no other test in this binary is
/// running at all. zeropty's lock cannot be held across a test: alone, the test's own engine thread
/// (which takes it shared to make the listener and its socket pairs) would wait on the test, which waits
/// on that thread; shared, std does not promise that a spawn queued on another test's engine lets that
/// thread through (RwLock's docs: a waiting writer "might or might not block" new readers).
static FORKS: std::sync::RwLock<()> = std::sync::RwLock::new(());

fn argv(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

fn wait_screen(e: &Embedded, target: &str, f: impl Fn(&str) -> bool) -> String {
    let start = Instant::now();
    loop {
        let r = e.command(&argv(&["capture-pane", "-p", "-t", target]), "/");
        if f(&r.out) {
            return r.out;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "timed out: {:?}", r.out);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn commands_from_threads() {
    let _forks = FORKS.read().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("zeropty-embed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let e = std::sync::Arc::new(Embedded::start(Options { size_dir: dir.clone(), ..Options::default() }));
    assert_eq!(e.command(&argv(&["new-session", "-d", "-s", "c", "-x", "40", "-y", "20", "exec cat"]), "/").code, 0);
    // Four threads type into the session at once; every line arrives whole.
    let threads: Vec<_> = (0..4)
        .map(|t| {
            let e = e.clone();
            std::thread::spawn(move || {
                for k in 0..3 {
                    assert_eq!(e.command(&argv(&["send-keys", "-t", "=c:", "-l", &format!("t{t}k{k}\n")]), "/").code, 0);
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let screen = wait_screen(&e, "=c:", |o| o.lines().filter(|l| l.starts_with('t')).count() == 12);
    for t in 0..4 {
        for k in 0..3 {
            assert!(screen.lines().any(|l| l == format!("t{t}k{k}")), "t{t}k{k} missing:\n{screen}");
        }
    }
    // What a command costs here: no other process, no message to the engine's thread.
    let n = 2000;
    let start = Instant::now();
    for _ in 0..n {
        e.command(&argv(&["capture-pane", "-p", "-t", "=c:"]), "/");
    }
    let per = start.elapsed() / n;
    eprintln!("embedded capture-pane: {per:?} each");
    assert!(per < Duration::from_millis(1), "{per:?}");
    e.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Runs a polled engine as its host's loop would, for `ms`: waits on its fd, at most its timeout.
fn run_for(p: &mut Polled, ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        let wait = p.timeout_ms().min(left.as_millis() as u64 + 1);
        let mut fd = libc::pollfd { fd: p.fd(), events: libc::POLLIN, revents: 0 };
        unsafe { libc::poll(&mut fd, 1, wait as i32) };
        p.process();
    }
}

fn read_bytes(p: &mut Polled) -> u64 {
    let stats = p.command(&argv(&["stats"]), "/").out;
    stats.split_whitespace().skip_while(|w| *w != "read_bytes").nth(1).unwrap().parse().unwrap()
}

/// With no thread of its own, the engine reads output and sees programs end only through its host's
/// loop: its fd turns readable for them.
#[test]
fn polled_by_the_host_loop() {
    let _forks = FORKS.read().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("zeropty-polled-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = Polled::start(Options { size_dir: dir.clone(), ..Options::default() });
    let r = p.command(&argv(&["new-session", "-d", "-s", "w", "-x", "30", "-y", "5", "read l; echo \"got $l\"; sleep 1.5; exit"]), "/");
    assert_eq!(r.code, 0, "{}", r.err);
    p.command(&argv(&["send-keys", "-t", "=w:", "-l", "hi\n"]), "/");
    // Read by the loop, not by a capture (which reads what is pending itself).
    run_for(&mut p, 1000);
    assert_eq!(read_bytes(&mut p), "got hi\n".len() as u64);
    let screen = p.command(&argv(&["capture-pane", "-p", "-t", "=w:"]), "/");
    assert!(screen.out.contains("got hi"), "{:?} {:?}", screen.out, screen.err);
    // The program ends: the session with it.
    run_for(&mut p, 1500);
    assert_eq!(p.command(&argv(&["has-session", "-t", "=w"]), "/").code, 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A burst (4.6 MB here) is taken a slice per `process` call, so the host's loop turns in between; the
/// rest is due at once, so all of it reaches the screen with the host waiting only as it is told to.
#[test]
fn polled_takes_a_burst_in_slices() {
    let _forks = FORKS.read().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("zeropty-slices-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = Polled::start(Options { size_dir: dir.clone(), ..Options::default() });
    let burst = r#"perl -e 'for (0..59999) { printf "line %05d %s\n", $_, "x" x 64 }'; exec sleep 100"#;
    assert_eq!(p.command(&argv(&["new-session", "-d", "-s", "b", "-x", "100", "-y", "5", burst]), "/").code, 0);
    let (total, start) = (60000 * 76, Instant::now());
    while read_bytes(&mut p) < total && start.elapsed() < Duration::from_secs(10) {
        run_for(&mut p, 10);
    }
    assert_eq!(read_bytes(&mut p), total, "after {:?}", start.elapsed());
    let screen = p.command(&argv(&["capture-pane", "-p", "-t", "=b:"]), "/").out;
    assert!(screen.contains("line 59999"), "{screen:?}");
    p.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The zeropty CLI on a socket an embedded engine listens on.
fn cli(sock: &std::path::Path, a: &[&str]) -> std::process::Command {
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_zeropty"));
    c.args(a).env("ZEROPTY_SOCK", sock).stdin(std::process::Stdio::null());
    c
}

fn out(c: &mut std::process::Command) -> (i32, String) {
    let o = c.output().unwrap();
    (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned())
}

#[test]
fn cli_reaches_an_embedded_engine() {
    let _forks = FORKS.read().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("zeropty-listen-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("s");
    let e = Embedded::start(Options { size_dir: dir.clone(), ..Options::default() });
    e.listen(&sock).unwrap();
    // A second listener on a socket that answers is refused.
    assert!(Embedded::start(Options { size_dir: dir.clone(), ..Options::default() }).listen(&sock).is_err());
    assert_eq!(e.command(&argv(&["new-session", "-d", "-s", "h", "exec cat"]), "/").code, 0);
    // The host's session, from the CLI; and one the CLI starts, seen by the host.
    assert_eq!(out(&mut cli(&sock, &["send-keys", "-t", "=h:", "-l", "from-cli\n"])).0, 0);
    wait_screen(&e, "=h:", |o| o.lines().filter(|l| *l == "from-cli").count() == 2);
    assert_eq!(out(&mut cli(&sock, &["new-session", "-d", "-s", "c", "exec cat"])).0, 0);
    assert_eq!(e.command(&argv(&["send-keys", "-t", "=c:", "-l", "from-host\n"]), "/").code, 0);
    // wait-pane: a reply the server holds back until the screen says so.
    let (code, screen) = out(&mut cli(&sock, &["wait-pane", "-t", "=c:", "-m", "from-host", "-T", "5000"]));
    assert_eq!(code, 0, "{screen}");
    // A terminal attached through the socket: keys in, the session's output and its end out, as frames.
    {
        use std::io::{BufRead as _, Read as _, Write as _};
        use zeropty::protocol::{ClientFrame, Frame as _, ServerFrame};
        let mut t = std::os::unix::net::UnixStream::connect(&sock).unwrap();
        t.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        t.write_all(b"{\"argv\":[\"attach\",\"-t\",\"c\"],\"cwd\":\"/\"}\n").unwrap();
        let mut r = std::io::BufReader::new(t.try_clone().unwrap());
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        assert!(line.contains("\"code\":0"), "{line}");
        t.write_all(&ClientFrame::Input(b"typed\n").to_vec()).unwrap();
        let (mut seen, mut ended) = (Vec::new(), false);
        let mut killed = false;
        while !ended {
            let mut head = [0u8; 5];
            r.read_exact(&mut head).unwrap();
            let mut body = vec![0u8; u32::from_le_bytes(head[1..5].try_into().unwrap()) as usize];
            r.read_exact(&mut body).unwrap();
            ended = ServerFrame::decode(head[0], &body) == Ok(ServerFrame::Ended);
            seen.extend_from_slice(&body);
            if !killed && String::from_utf8_lossy(&seen).contains("typed") {
                killed = true;
                assert_eq!(e.command(&argv(&["kill-session", "-t", "=c"]), "/").code, 0);
            }
        }
        assert!(killed, "{:?}", String::from_utf8_lossy(&seen));
        assert_eq!(e.command(&argv(&["new-session", "-d", "-s", "c", "exec cat"]), "/").code, 0);
    }
    // kill-server from the CLI ends the sessions, not the host, which goes on listening.
    assert_eq!(out(&mut cli(&sock, &["kill-server"])).0, 0);
    let (code, list) = out(&mut cli(&sock, &["list-sessions"]));
    assert!(code != 0 || list.is_empty(), "{list}");
    assert_eq!(out(&mut cli(&sock, &["new-session", "-d", "-s", "again", "exec cat"])).0, 0);
    assert_eq!(e.command(&argv(&["has-session", "-t", "=again"]), "/").code, 0);
    e.command(&argv(&["kill-server"]), "/");
    drop(e);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_reaches_a_polled_engine() {
    let _forks = FORKS.read().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("zeropty-listen-polled-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("s");
    let mut p = Polled::start(Options { size_dir: dir.clone(), ..Options::default() });
    p.listen(&sock).unwrap();
    assert_eq!(p.command(&argv(&["new-session", "-d", "-s", "h", "exec cat"]), "/").code, 0);
    // The CLI runs while this thread runs the engine's loop, as a host's would.
    let ask = |p: &mut Polled, a: &[&str]| {
        let child = cli(&sock, a).stdout(std::process::Stdio::piped()).spawn().unwrap();
        let start = Instant::now();
        let mut child = child;
        loop {
            run_for(p, 5);
            if let Some(st) = child.try_wait().unwrap() {
                let mut s = String::new();
                std::io::Read::read_to_string(child.stdout.as_mut().unwrap(), &mut s).unwrap();
                return (st.code().unwrap_or(-1), s);
            }
            assert!(start.elapsed() < Duration::from_secs(5), "{a:?} timed out");
        }
    };
    assert_eq!(ask(&mut p, &["send-keys", "-t", "=h:", "-l", "from-cli\n"]).0, 0);
    let (code, screen) = ask(&mut p, &["wait-pane", "-t", "=h:", "-m", "from-cli", "-T", "5000"]);
    assert_eq!(code, 0, "{screen}");
    assert_eq!(ask(&mut p, &["new-session", "-d", "-s", "c", "exec cat"]).0, 0);
    assert_eq!(p.command(&argv(&["has-session", "-t", "=c"]), "/").code, 0);
    p.command(&argv(&["kill-server"]), "/");
    let _ = std::fs::remove_dir_all(&dir);
}

/// listen never deletes what is not a socket, leaves an existing directory's permissions alone, and makes
/// the socket private.
#[test]
fn listen_is_careful_with_the_path() {
    let _alone = FORKS.write().unwrap_or_else(|e| e.into_inner());
    use std::os::unix::fs::PermissionsExt as _;
    let dir = std::env::temp_dir().join(format!("zeropty-careful-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let e = Embedded::start(Options { size_dir: dir.clone(), ..Options::default() });
    let file = dir.join("notes.txt");
    std::fs::write(&file, "keep me").unwrap();
    assert!(e.listen(&file).is_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "keep me");
    let sock = dir.join("s");
    e.listen(&sock).unwrap();
    assert_eq!(std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o755);
    // A server that is gone removes its socket.
    drop(e);
    assert!(!sock.exists(), "socket removed");
    // One left behind (a process that exited without removing it) is taken over. A datagram socket
    // stands for it: no stream connects to it, even from a process holding a copy of its fd.
    drop(std::os::unix::net::UnixDatagram::bind(&sock).unwrap());
    let e2 = Embedded::start(Options { size_dir: dir.clone(), ..Options::default() });
    // The closed listener can still answer for a moment: a process the other tests spawn on another thread
    // holds a copy of every descriptor of this process, close-on-exec ones included, until its exec is done,
    // and connections to it queue meanwhile (a probe right after close succeeded 99 times in 3000 while a
    // thread spawned /usr/bin/true; this test failed 1 run in 4 under `cargo test`). The take-over itself is
    // what is checked, so it may wait for those copies to go.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match e2.listen(&sock) {
            Ok(()) => break,
            Err(err) if err.kind() == std::io::ErrorKind::AddrInUse && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(1)),
            Err(err) => panic!("the stale socket was not taken over: {err}"),
        }
    }
    // A directory it makes is private.
    let fresh = dir.join("new").join("s");
    let e3 = Embedded::start(Options { size_dir: dir.clone(), ..Options::default() });
    e3.listen(&fresh).unwrap();
    assert_eq!(std::fs::metadata(dir.join("new")).unwrap().permissions().mode() & 0o777, 0o700);
    drop((e2, e3));
    let _ = std::fs::remove_dir_all(&dir);
}
