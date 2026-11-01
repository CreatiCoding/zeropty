//! Sessions moving between servers, and between a server and an embedded engine, with their programs
//! left running (src/handover.rs). Plain programs, no claude.
use std::io::{BufRead as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{Duration, Instant};
use zeropty::embed::Embedded;
use zeropty::engine::Options;

struct Server {
    dir: PathBuf,
}

impl Server {
    fn new(name: &str) -> Self {
        // Short: a Unix socket path is limited to 104 bytes.
        let dir = std::env::temp_dir().join(format!("zeropty-h-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn sock(&self) -> PathBuf {
        self.dir.join("s")
    }

    fn sock_str(&self) -> String {
        self.sock().display().to_string()
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_zeropty")).args(args).env("ZEROPTY_SOCK", self.sock()).output().unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let o = self.run(args);
        assert!(o.status.success(), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap()
    }

    fn has(&self, name: &str) -> bool {
        self.run(&["has-session", "-t", name]).status.success()
    }

    fn wait_capture(&self, target: &str, f: impl Fn(&str) -> bool) -> String {
        wait(|| self.ok(&["capture-pane", "-p", "-t", target]), f)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.run(&["kill-server"]);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn wait(mut get: impl FnMut() -> String, f: impl Fn(&str) -> bool) -> String {
    let start = Instant::now();
    loop {
        let out = get();
        if f(&out) {
            return out;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "timed out; last:\n{out}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn alive(pid: i32) -> bool {
    // A zombie still answers kill(pid, 0): ps tells it apart.
    let stat = Command::new("ps").args(["-o", "stat=", "-p", &pid.to_string()]).output().unwrap();
    let stat = String::from_utf8_lossy(&stat.stdout);
    !stat.trim().is_empty() && !stat.trim().starts_with('Z')
}

fn gone(pid: i32) -> bool {
    let stat = Command::new("ps").args(["-o", "stat=", "-p", &pid.to_string()]).output().unwrap();
    String::from_utf8_lossy(&stat.stdout).trim().is_empty()
}

fn pid_of(s: &Server, t: &str) -> i32 {
    s.ok(&["display-message", "-p", "-t", t, "#{pane_pid}"]).trim().parse().unwrap()
}

#[test]
fn pull_moves_a_session_with_its_program() {
    let a = Server::new("pa");
    let b = Server::new("pb");
    a.ok(&["new-session", "-d", "-s", "c", "-x", "40", "-y", "8", "printf 'one\\n\\033[32mgreen\\033[0m e\\314\\201\\n'; seq 1 30; exec cat"]);
    a.ok(&["new-session", "-d", "-s", "d", "exec sleep 100"]);
    a.wait_capture("=c:", |o| o.contains("30"));
    let before = a.ok(&["capture-pane", "-p", "-e", "-S", "-100", "-t", "=c:"]);
    let pid = pid_of(&a, "=c:");

    // b's server starts for it, like new-session.
    assert_eq!(b.ok(&["pull-sessions", "-s", &a.sock_str(), "c"]), "c\n");
    assert!(!a.has("=c") && a.has("=d"), "c left a, d stayed");
    assert_eq!(b.ok(&["capture-pane", "-p", "-e", "-S", "-100", "-t", "=c:"]), before, "the same screen and history");
    assert_eq!(pid_of(&b, "=c:"), pid, "the same program");
    assert!(alive(pid));
    b.ok(&["send-keys", "-t", "=c:", "-l", "moved"]);
    b.ok(&["send-keys", "-t", "=c:", "Enter"]);
    b.wait_capture("=c:", |o| o.lines().any(|l| l == "moved"));

    // Names it does not have, and names already taken here, stay where they are.
    let o = b.run(&["pull-sessions", "-s", &a.sock_str(), "nope"]);
    assert!(!o.status.success() && String::from_utf8_lossy(&o.stderr).contains("can't find session: nope"));
    b.ok(&["new-session", "-d", "-s", "d", "exec sleep 100"]);
    let o = b.run(&["pull-sessions", "-s", &a.sock_str(), "d"]);
    assert!(String::from_utf8_lossy(&o.stderr).contains("duplicate session: d"), "{o:?}");
    assert!(a.has("=d"), "d stayed in a");
    let o = b.run(&["pull-sessions", "-s", &b.sock_str()]);
    assert!(String::from_utf8_lossy(&o.stderr).contains("this server"), "not from itself");

    // Ended from b: a, its parent, reaps it.
    b.ok(&["kill-session", "-t", "=c"]);
    wait(|| gone(pid).to_string(), |o| o == "true");
}

/// A session pushed on keeps its size file: resized where it went, its program reads the new size there.
#[test]
fn push_back_and_resize_after_moving() {
    let a = Server::new("ra");
    let b = Server::new("rb");
    a.ok(&["new-session", "-d", "-s", "r", "-x", "30", "-y", "6", "while read l; do echo \"size $(cat \"$ZEROPTY_SIZE_FILE\" 2>/dev/null)\"; done"]);
    b.ok(&["new-session", "-d", "-s", "x", "exec sleep 100"]);
    assert_eq!(a.ok(&["push-sessions", "-s", &b.sock_str()]), "r\n");
    assert!(!a.has("=r") && b.has("=r"));

    // A terminal attaches to it in b at 50x9: the session takes that size and tells its program.
    let mut t = UnixStream::connect(b.sock()).unwrap();
    t.write_all(b"{\"argv\":[\"attach\",\"-t\",\"=r\",\"-x\",\"50\",\"-y\",\"9\"],\"cwd\":\"/\"}\n").unwrap();
    let mut reply = String::new();
    std::io::BufReader::new(&t).read_line(&mut reply).unwrap();
    assert!(reply.contains("\"code\":0"), "{reply}");
    b.ok(&["send-keys", "-t", "=r:", "-l", "\n"]);
    b.wait_capture("=r:", |o| o.lines().any(|l| l == "size 50 9"));
    drop(t);

    // And back to a, all of them.
    assert_eq!(b.ok(&["push-sessions", "-s", &a.sock_str()]).lines().count(), 2);
    assert!(a.has("=r") && a.has("=x") && !b.has("=r"));
    a.ok(&["send-keys", "-t", "=r:", "-l", "\n"]);
    a.wait_capture("=r:", |o| o.lines().filter(|l| l.starts_with("size")).count() == 2);
}

/// An embedded engine takes sessions from a server and gives them back.
#[test]
fn embedded_and_server() {
    let s = Server::new("em");
    s.ok(&["new-session", "-d", "-s", "w", "-x", "40", "-y", "6", "echo ready; exec cat"]);
    s.wait_capture("=w:", |o| o.contains("ready"));
    let pid = pid_of(&s, "=w:");
    let e = Embedded::start(Options { size_dir: s.dir.clone(), ..Options::default() });
    let run = |a: &[&str]| e.command(&a.iter().map(|x| x.to_string()).collect::<Vec<_>>(), "/");
    let r = run(&["pull-sessions", "-s", &s.sock_str()]);
    assert_eq!((r.code, r.out.as_str()), (0, "w\n"), "{}", r.err);
    assert!(!s.has("=w"));
    run(&["send-keys", "-t", "=w:", "-l", "in the host\n"]);
    wait(|| run(&["capture-pane", "-p", "-t", "=w:"]).out, |o| o.lines().any(|l| l == "in the host"));
    let r = run(&["push-sessions", "-s", &s.sock_str(), "w"]);
    assert_eq!((r.code, r.out.as_str()), (0, "w\n"), "{}", r.err);
    assert_eq!(run(&["has-session", "-t", "=w"]).code, 1);
    s.wait_capture("=w:", |o| o.contains("in the host"));
    assert_eq!(pid_of(&s, "=w:"), pid);
    // Nothing to take from a server that is not there: the session stays.
    let r = run(&["push-sessions", "-s", "/nonexistent/zeropty.sock"]);
    assert_eq!(r.code, 1);
}

/// restart-server: a new server process takes over the socket and every session, programs untouched.
#[test]
fn restart_server_keeps_sessions() {
    let s = Server::new("rs");
    s.ok(&["new-session", "-d", "-s", "k", "-x", "30", "-y", "5", "echo before; exec cat"]);
    s.ok(&["new-session", "-d", "-s", "l", "exec sleep 100"]);
    s.wait_capture("=k:", |o| o.contains("before"));
    // The socket of another server process: another file at the same path.
    let server_sock = |s: &Server| std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(s.sock()).unwrap());
    let (old, pid) = (server_sock(&s), pid_of(&s, "=k:"));
    let out = s.ok(&["restart-server"]);
    let mut moved: Vec<&str> = out.lines().collect();
    moved.sort();
    assert_eq!(moved, ["k", "l"]);
    assert_ne!(server_sock(&s), old, "another server process");
    assert_eq!(pid_of(&s, "=k:"), pid);
    s.ok(&["send-keys", "-t", "=k:", "-l", "after\n"]);
    s.wait_capture("=k:", |o| o.contains("before") && o.lines().any(|l| l == "after"));
}

fn signal(pid: i32, sig: &str) {
    assert!(Command::new("kill").args([sig, &pid.to_string()]).status().unwrap().success());
}

/// A program stopped while restart-server runs is still there after it. In the old server's session its
/// group became orphaned when the old server exited, and the kernel hung up every member of an orphaned
/// group that has a stopped one (a C-z'd program, or one with a stopped child): the session vanished.
#[test]
fn restart_server_keeps_a_stopped_program() {
    let s = Server::new("st");
    s.ok(&["new-session", "-d", "-s", "z", "exec sleep 100"]);
    s.ok(&["new-session", "-d", "-s", "y", "exec sleep 100"]);
    let (z, y) = (pid_of(&s, "=z:"), pid_of(&s, "=y:"));
    // SAFETY: getsid takes a pid and touches no memory.
    assert_eq!(unsafe { libc::getsid(z) }, z, "the program leads a session of its own");
    signal(z, "-STOP");
    wait(
        || Command::new("ps").args(["-o", "stat=", "-p", &z.to_string()]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap(),
        |o| o.trim().starts_with('T'),
    );
    let mut moved: Vec<String> = s.ok(&["restart-server"]).lines().map(String::from).collect();
    moved.sort();
    assert_eq!(moved, ["y", "z"]);
    // The kernel's hangup came as the old server exited, right after restart-server answered.
    std::thread::sleep(Duration::from_millis(300));
    assert!(alive(z) && alive(y), "both programs are alive");
    assert!(s.has("=z") && s.has("=y"), "both sessions are in the new server");
    signal(z, "-CONT");
}
