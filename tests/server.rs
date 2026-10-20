//! The server end to end, through the CLI and the socket, with plain programs (no claude, no shim):
//! each test runs its own server on its own socket.
use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

struct Server {
    dir: PathBuf,
}

impl Server {
    fn new(name: &str) -> Self {
        // Short: a Unix socket path is limited to 104 bytes.
        let dir = std::env::temp_dir().join(format!("zeropty-t-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn sock(&self) -> PathBuf {
        self.dir.join("s")
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_zeropty")).args(args).env("ZEROPTY_SOCK", self.sock()).output().unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let o = self.run(args);
        assert!(o.status.success(), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap()
    }

    /// Captures until `f` holds (the program needs a moment to print).
    fn wait_capture(&self, args: &[&str], f: impl Fn(&str) -> bool) -> String {
        let start = Instant::now();
        loop {
            let out = self.ok(args);
            if f(&out) {
                return out;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "timed out; last capture:\n{out}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.run(&["kill-server"]);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn sessions_keys_and_screen() {
    let s = Server::new("keys");
    s.ok(&["new-session", "-d", "-s", "c", "-x", "40", "-y", "10", "exec cat"]);
    assert!(s.run(&["new-session", "-d", "-s", "c", "exec cat"]).stderr.starts_with(b"duplicate session"));
    assert!(s.run(&["has-session", "-t", "=c"]).status.success());
    assert!(!s.run(&["has-session", "-t", "=nope"]).status.success());
    assert!(s.ok(&["list-sessions"]).contains("c:"));
    let pid: i32 = s.ok(&["display-message", "-p", "-t", "=c:", "#{pane_pid}"]).trim().parse().unwrap();
    assert!(pid > 1);
    s.ok(&["send-keys", "-t", "=c:", "-l", "hello there"]);
    s.ok(&["send-keys", "-t", "=c:", "Enter"]);
    // cat prints the line back once it has it (no tty, so no echo of the typing itself).
    s.wait_capture(&["capture-pane", "-p", "-t", "=c:"], |o| o.lines().next() == Some("hello there"));
    s.ok(&["kill-session", "-t", "=c"]);
    assert!(!s.run(&["has-session", "-t", "=c"]).status.success());
}

#[test]
fn output_colors_history_and_line_feeds() {
    let s = Server::new("out");
    s.ok(&["new-session", "-d", "-s", "o", "-x", "30", "-y", "5", "printf 'one\\ntwo\\n\\033[31mred\\033[0m\\n'; seq 1 20; exec sleep 100"]);
    let screen = s.wait_capture(&["capture-pane", "-p", "-t", "=o:"], |o| o.contains("20"));
    // A line feed returns the carriage, as a PTY's ONLCR does: every line starts at column 0.
    assert_eq!(screen.lines().take(4).collect::<Vec<_>>(), ["17", "18", "19", "20"]);
    let all = s.ok(&["capture-pane", "-p", "-S", "-100", "-t", "=o:"]);
    let lines: Vec<&str> = all.lines().collect();
    assert_eq!(&lines[..4], ["one", "two", "red", "1"], "history");
    let colored = s.ok(&["capture-pane", "-p", "-e", "-S", "-100", "-t", "=o:"]);
    assert!(colored.lines().nth(2).unwrap().contains("\x1b[0;31mred\x1b[0m"), "colors: {colored:?}");
}

#[test]
fn environment_and_paste() {
    let s = Server::new("env");
    s.ok(&["new-session", "-d", "-s", "e", "-x", "40", "-y", "5", "-e", "FOO=bar baz", "echo \"[$FOO]\"; exec cat"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=e:"], |o| o.starts_with("[bar baz]"));
    let file = s.dir.join("buf");
    std::fs::write(&file, "pasted line\n").unwrap();
    s.ok(&["load-buffer", "-b", "b1", file.to_str().unwrap()]);
    // Without -r the buffer's line feeds become carriage returns (a typed Enter); cat gets a line.
    s.ok(&["paste-buffer", "-d", "-b", "b1", "-t", "=e:"]);
    s.ok(&["send-keys", "-t", "=e:", "-l", "\n"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=e:"], |o| o.contains("pasted line"));
    assert!(!s.run(&["paste-buffer", "-b", "b1", "-t", "=e:"]).status.success(), "-d deleted the buffer");
}

/// The server runs with its allocator settings (MALLOC_ENV), and its sessions' programs start without
/// them: only a setting the user had made reaches them.
#[test]
fn allocator_settings_stay_with_the_server() {
    let s = Server::new("malloc");
    let o = Command::new(env!("CARGO_BIN_EXE_zeropty"))
        .args(["new-session", "-d", "-s", "m", "-x", "60", "-y", "8", "env | grep -E '^(Malloc|ZEROPTY_MALLOC)' | sort; echo END; exec cat"])
        .env("ZEROPTY_SOCK", s.sock())
        .env("MallocMaxMagazines", "4")
        .env_remove("MallocNanoZone")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let screen = s.wait_capture(&["capture-pane", "-p", "-t", "=m:"], |o| o.contains("END"));
    assert_eq!(screen.trim_end(), "MallocMaxMagazines=4\nEND");
    let pid = String::from_utf8(Command::new("lsof").arg("-t").arg(s.sock()).output().unwrap().stdout).unwrap();
    let pid = pid.lines().next().expect("server pid").trim().to_owned();
    let env = String::from_utf8(Command::new("ps").args(["-E", "-ww", "-o", "command=", "-p", &pid]).output().unwrap().stdout).unwrap();
    // Only the allocator's part is shown on failure: the rest of an environment can hold tokens.
    let malloc: Vec<&str> = env.split_whitespace().filter(|w| w.starts_with("Malloc")).collect();
    for setting in ["MallocNanoZone=0", "MallocSpaceEfficient=1", "MallocMediumZone=0", "MallocMaxMagazines=4"] {
        assert!(malloc.contains(&setting), "server started without {setting}: {malloc:?}");
    }
}

#[test]
fn session_ends_with_its_program() {
    let s = Server::new("end");
    s.ok(&["new-session", "-d", "-s", "x", "exit 0"]);
    let start = Instant::now();
    while s.run(&["has-session", "-t", "=x"]).status.success() {
        assert!(start.elapsed() < Duration::from_secs(5), "session outlived its program");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// One connection, several commands in one write: the replies come back in order, and the connection
/// stays open; a client stuck mid-line holds nobody up.
#[test]
fn connection_kept_and_pipelined() {
    let s = Server::new("conn");
    s.ok(&["new-session", "-d", "-s", "p", "-x", "20", "-y", "5", "exec sleep 100"]);
    let mut stuck = UnixStream::connect(s.sock()).unwrap();
    stuck.write_all(br#"{"argv": ["has-ses"#).unwrap();
    let mut c = UnixStream::connect(s.sock()).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let req = |argv: &[&str]| format!("{}\n", serde_json::json!({ "argv": argv, "cwd": "/" }));
    let batch = [req(&["has-session", "-t", "=p"]), req(&["has-session", "-t", "=q"]), req(&["display-message", "-p", "-t", "=p:", "w#{pane_width}"])];
    c.write_all(batch.concat().as_bytes()).unwrap();
    let mut got = Vec::new();
    let mut buf = [0u8; 4096];
    while got.iter().filter(|&&b| b == b'\n').count() < 3 {
        let n = c.read(&mut buf).unwrap();
        assert!(n > 0);
        got.extend_from_slice(&buf[..n]);
    }
    let replies: Vec<serde_json::Value> = got.split(|&b| b == b'\n').take(3).map(|l| serde_json::from_slice(l).unwrap()).collect();
    assert_eq!(replies[0]["code"], 0);
    assert_eq!(replies[1]["code"], 1);
    assert_eq!(replies[2]["out"], "w20\n");
    stuck.write_all(b"sion\", \"-t\", \"=p\"], \"cwd\": \"/\"}\n").unwrap();
    stuck.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let n = stuck.read(&mut buf).unwrap();
    let r: serde_json::Value = serde_json::from_slice(&buf[..n - 1]).unwrap();
    assert_eq!(r["code"], 0, "{r}");
}

/// An attached connection (what `zeropty attach` uses), driven through its frames.
struct Attached {
    s: UnixStream,
    input: Vec<u8>,
    /// Everything the session printed to this terminal so far, and whether it ended.
    seen: Vec<u8>,
    ended: bool,
}

impl Attached {
    fn new(server: &Server, argv: &[&str]) -> Self {
        let mut s = UnixStream::connect(server.sock()).unwrap();
        s.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
        s.write_all(format!("{}\n", serde_json::json!({ "argv": argv, "cwd": "/" })).as_bytes()).unwrap();
        let mut a = Self { s, input: Vec::new(), seen: Vec::new(), ended: false };
        let start = Instant::now();
        while !a.input.contains(&b'\n') {
            assert!(start.elapsed() < Duration::from_secs(5), "no reply to attach");
            a.fill();
        }
        let n = a.input.iter().position(|&b| b == b'\n').unwrap();
        let reply: serde_json::Value = serde_json::from_slice(&a.input[..n]).unwrap();
        assert_eq!(reply["code"], 0, "{reply}");
        a.input.drain(..=n);
        a
    }

    fn fill(&mut self) {
        let mut buf = [0u8; 65536];
        if let Ok(n) = self.s.read(&mut buf) {
            self.input.extend_from_slice(&buf[..n]);
        }
    }

    /// Frames until `f` holds over all the output seen so far; returns it.
    fn until(&mut self, f: impl Fn(&str, bool) -> bool) -> String {
        let start = Instant::now();
        loop {
            while self.input.len() >= 5 {
                let len = u32::from_le_bytes(self.input[1..5].try_into().unwrap()) as usize;
                if self.input.len() < 5 + len {
                    break;
                }
                match self.input[0] {
                    0 => self.seen.extend_from_slice(&self.input[5..5 + len]),
                    1 => self.ended = true,
                    k => panic!("frame kind {k}"),
                }
                self.input.drain(..5 + len);
            }
            let text = String::from_utf8_lossy(&self.seen).into_owned();
            if f(&text, self.ended) {
                return text;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "timed out; got {text:?} ended {}", self.ended);
            self.fill();
        }
    }

    fn send(&mut self, kind: u8, body: &[u8]) {
        let mut f = vec![kind];
        f.extend_from_slice(&(body.len() as u32).to_le_bytes());
        f.extend_from_slice(body);
        self.s.write_all(&f).unwrap();
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        let mut b = cols.to_le_bytes().to_vec();
        b.extend_from_slice(&rows.to_le_bytes());
        self.send(1, &b);
    }
}

fn width(s: &Server, t: &str) -> String {
    s.ok(&["display-message", "-p", "-t", t, "#{pane_width}x#{pane_height}"]).trim().to_owned()
}

#[test]
fn attach_sizes_the_session_and_restores_it() {
    let s = Server::new("att");
    // The program counts SIGWINCHes (sh runs the trap between its sleeps).
    s.ok(&["new-session", "-d", "-s", "w", "-x", "40", "-y", "10", "n=0; trap 'n=$((n+1)); echo winch$n' WINCH; echo ready; while :; do sleep 0.05; done"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=w:"], |o| o.contains("ready"));
    let mut a = Attached::new(&s, &["attach", "-t", "w", "-x", "30", "-y", "8"]);
    // The screen comes first, drawn for the terminal.
    a.until(|o, _| o.contains("ready") && o.contains("\x1b[2J"));
    assert_eq!(width(&s, "=w:"), "30x8");
    let size_file = std::fs::read_dir(&s.dir).unwrap().flatten().find(|e| e.file_name().to_string_lossy().starts_with("zeropty-size-")).expect("size file");
    assert_eq!(std::fs::read_to_string(size_file.path()).unwrap(), "30 8\n");
    a.until(|o, _| o.contains("winch1"));
    a.resize(50, 12);
    a.until(|o, _| o.contains("winch2"));
    assert_eq!(width(&s, "=w:"), "50x12");
    a.send(2, &[]); // detach
    let start = Instant::now();
    while width(&s, "=w:") != "40x10" {
        assert!(start.elapsed() < Duration::from_secs(5), "size not restored: {}", width(&s, "=w:"));
        std::thread::sleep(Duration::from_millis(20));
    }
    s.wait_capture(&["capture-pane", "-p", "-t", "=w:"], |o| o.contains("winch3"));
}

#[test]
fn attach_keys_read_only_and_end() {
    let s = Server::new("att2");
    s.ok(&["new-session", "-d", "-s", "c", "-x", "40", "-y", "10", "exec cat"]);
    let mut ro = Attached::new(&s, &["attach", "-r", "-t", "c"]);
    ro.send(0, b"ignored\n");
    let mut rw = Attached::new(&s, &["attach", "-t", "c"]);
    rw.send(0, b"typed\n");
    // cat prints the typed line back; both terminals see it, and only it.
    for a in [&mut rw, &mut ro] {
        let o = a.until(|o, _| o.contains("typed"));
        assert!(!o.contains("ignored"), "a read-only terminal's keys reached the session: {o:?}");
    }
    s.ok(&["kill-session", "-t", "=c"]);
    rw.until(|_, ended| ended);
    ro.until(|_, ended| ended);
}

/// A capture while a coalesced read was pending must leave the session's output still reported: the
/// pipe's filter waited for 16 KB then, and was left that way, so later small output went unread.
#[test]
fn output_after_a_capture_is_still_read() {
    let s = Server::new("lowat");
    s.ok(&["new-session", "-d", "-s", "l", "-x", "20", "-y", "5", "echo one; sleep 0.6; echo two; exec sleep 100"]);
    // Polls every 20 ms, so it lands inside the 300 ms coalescing window after "one".
    s.wait_capture(&["capture-pane", "-p", "-t", "=l:"], |o| o.contains("one"));
    std::thread::sleep(Duration::from_millis(1500));
    let stats = s.ok(&["stats"]);
    let read: u64 = stats.split_whitespace().skip_while(|w| *w != "read_bytes").nth(1).unwrap().parse().unwrap();
    assert!(read >= 8, "\"two\" was not read without a capture: {stats}");
}

/// Keys the program is not reading yet fill its input pipe; the rest waits in the server and goes in
/// as the program reads, in order and whole.
#[test]
fn input_bigger_than_the_pipe_arrives_whole() {
    let s = Server::new("big");
    s.ok(&["new-session", "-d", "-s", "b", "-x", "40", "-y", "5", "printf '\\033]zeropty-tty;0\\007'; echo READY; head -c 200000 | cksum; exec sleep 100"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=b:"], |o| o.starts_with("READY"));
    let data: String = (0..200_000).map(|k| (b'a' + (k % 26) as u8) as char).collect();
    s.ok(&["send-keys", "-t", "=b:", "-l", &data]);
    let want = {
        let mut c = Command::new("cksum").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
        c.stdin.take().unwrap().write_all(data.as_bytes()).unwrap();
        String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap().split_whitespace().next().unwrap().to_owned()
    };
    s.wait_capture(&["capture-pane", "-p", "-t", "=b:"], |o| o.split_whitespace().nth(1) == Some(want.as_str()));
}

/// Output much bigger than the pipe, read ahead of parsing (Sess::slurp), lands whole and in order,
/// with what follows a pause in the middle of it.
#[test]
fn bulk_output_lands_whole() {
    let s = Server::new("bulk");
    s.ok(&["new-session", "-d", "-s", "u", "-x", "20", "-y", "5", "seq 1 300000; sleep 0.05; seq 300001 300100; exec sleep 100"]);
    let out = s.wait_capture(&["capture-pane", "-p", "-S", "-200", "-t", "=u:"], |o| o.lines().any(|l| l == "300100"));
    let lines: Vec<u32> = out.lines().filter(|l| !l.is_empty()).map(|l| l.parse().unwrap()).collect();
    assert!(lines.len() > 150 && lines.windows(2).all(|w| w[1] == w[0] + 1), "{lines:?}");
    let stats = s.ok(&["stats"]);
    let read: u64 = stats.split_whitespace().skip_while(|w| *w != "read_bytes").nth(1).unwrap().parse().unwrap();
    let want: u64 = (1..=300_100u64).map(|n| n.to_string().len() as u64 + 1).sum();
    assert_eq!(read, want, "{stats}");
}

/// `--help` is answered by the program itself (stdout, exit 0, no server needed), and every command it
/// lists is one the server knows.
#[test]
fn help_needs_no_server_and_lists_known_commands() {
    let s = Server::new("help");
    for arg in ["--help", "-h", "help"] {
        let o = s.run(&[arg]);
        assert_eq!(o.status.code(), Some(0), "{arg}: {o:?}");
        assert!(o.stderr.is_empty(), "{arg}: {o:?}");
        assert!(String::from_utf8_lossy(&o.stdout).contains("usage: zeropty <command>"), "{arg}: {o:?}");
    }
    assert!(!s.sock().exists(), "--help started a server");
    // Without arguments: the one-line usage on stderr, exit 2, as before.
    let o = s.run(&[]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("usage: zeropty serve | <tmux command> ..."));

    // Command names: a listed line's first word, words in parentheses, and words after one ending in ','.
    let help = String::from_utf8(s.run(&["--help"]).stdout).unwrap();
    let mut names = Vec::new();
    for line in help.lines().filter(|l| l.starts_with("  ") && l.as_bytes()[2].is_ascii_lowercase()) {
        let mut more = true;
        for tok in line.split_whitespace() {
            if !(more || tok.starts_with('(')) {
                break;
            }
            more = tok.ends_with(',');
            names.push(tok.trim_matches(|c| c == '(' || c == ')' || c == ',').to_owned());
        }
    }
    for want in ["new-session", "send-keys", "capture-pane", "wait-pane", "grep-panes", "attach", "restart-server", "rename", "mem", "resizep"] {
        assert!(names.iter().any(|n| n == want), "{want} missing from --help: {names:?}");
    }
    // `help <command>` knows every listed name (and serve): its synopsis line, a purpose, a docs link.
    let usage = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/USAGE.md")).unwrap();
    let usage_anchors: Vec<String> = usage
        .lines()
        .filter_map(|l| l.strip_prefix("##").map(|h| h.trim_start_matches('#').trim()))
        .map(|h| h.chars().filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-').collect::<String>().replace(' ', "-").to_lowercase())
        .collect();
    for name in names.iter().map(String::as_str).chain(["serve"]) {
        let o = s.run(&["help", name]);
        let out = String::from_utf8_lossy(&o.stdout);
        assert_eq!(o.status.code(), Some(0), "help {name}: {o:?}");
        let synopsis = out.lines().next().unwrap();
        assert!(synopsis.starts_with("usage: zeropty ") && synopsis.contains(name), "help {name}: {out}");
        // The anchor is a docs/USAGE.md heading, slugged the way GitHub does.
        let anchor = out.split("docs/USAGE.md#").nth(1).expect(&out).trim();
        assert!(usage_anchors.iter().any(|a| a == anchor), "help {name}: no heading for #{anchor}");
    }
    let o = s.run(&["--help", "resizep"]);
    assert!(String::from_utf8_lossy(&o.stdout).starts_with("usage: zeropty resize-window (resizew), resize-pane (resizep) -t target"), "{o:?}");
    let o = s.run(&["help", "nope"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty() && String::from_utf8_lossy(&o.stderr).contains("unknown command: nope"), "{o:?}");
    assert!(!s.sock().exists(), "help <command> started a server");
    s.ok(&["new-session", "-d", "-s", "h", "exec cat"]);
    // These would attach, start or stop servers, or take sessions: they are covered by their own tests.
    let skip = ["new-session", "new", "attach-session", "attach", "a", "restart-server", "kill-server", "pull-sessions", "push-sessions"];
    for name in names.iter().filter(|n| !skip.contains(&n.as_str())) {
        let o = s.run(&[name]);
        assert!(!String::from_utf8_lossy(&o.stderr).contains("unknown command"), "{name}: {o:?}");
    }
}

/// `zeropty capture-pane -p -t w | head -1`: a reader that closes the pipe early must not make the CLI
/// panic ('failed printing to stdout: Broken pipe', exit 101): help and every command's output.
#[test]
fn output_survives_a_closed_pipe() {
    let s = Server::new("epipe");
    s.ok(&["new-session", "-d", "-s", "p", "-x", "20", "-y", "50", "exec cat"]);
    for args in [&["--help"][..], &["help", "capture-pane"], &["capture-pane", "-p", "-t", "=p:"], &["list-sessions"]] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_zeropty"))
            .args(args)
            .env("ZEROPTY_SOCK", s.sock())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        // Close the read end before the child writes: its first write gets EPIPE.
        drop(child.stdout.take());
        let mut err = String::new();
        child.stderr.take().unwrap().read_to_string(&mut err).unwrap();
        let status = child.wait().unwrap();
        assert_eq!(status.code(), Some(0), "{args:?}: {err}");
        assert!(err.is_empty(), "{args:?}: {err}");
    }
}

/// shell/zeropty.zsh, the zsh function talking to the server from inside the shell: the same output,
/// errors and exit codes as the program, with arguments JSON can only carry escaped.
#[test]
fn zsh_function_answers_like_the_program() {
    let s = Server::new("zsh");
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/shell/zeropty.zsh");
    let zsh = |cmd: &str| {
        Command::new("/bin/zsh")
            .args(["-fc", &format!("source {script}; {cmd}")])
            .env("ZEROPTY_SOCK", s.sock())
            .env("ZEROPTY_BIN", env!("CARGO_BIN_EXE_zeropty"))
            .output()
            .unwrap()
    };
    // No server yet: new-session runs the program, which starts one.
    assert!(zsh("zeropty new-session -d -s z -x 40 -y 6 'exec cat'").status.success());
    let keys = "q\"uo\\\\te\ttab 한글 \x1b[1mbold\x1b[0m";
    let o = zsh(&format!("zeropty send-keys -t '=z:' -l $'{}' && zeropty send-keys -t '=z:' Enter", keys.replace('\x1b', "\\e").replace('\'', "\\'")));
    assert!(o.status.success(), "{o:?}");
    let program = s.wait_capture(&["capture-pane", "-p", "-e", "-t", "=z:"], |o| o.contains("bold"));
    let o = zsh("zeropty capture-pane -p -e -t '=z:'");
    assert_eq!(String::from_utf8(o.stdout).unwrap(), program);
    let o = zsh("zeropty --help");
    assert!(o.status.success() && o.stdout.starts_with(b"zeropty: "), "{o:?}");
    let o = zsh("zeropty has-session -t '=nope'");
    assert_eq!((o.status.code(), String::from_utf8_lossy(&o.stderr).as_ref()), (Some(1), "can't find session: nope\n"));
}

/// shell/zeropty_bash.c, the bash builtin (built here with clang): the same as the program, in macOS's
/// /bin/bash, including running the program when there is no server yet.
#[test]
fn bash_builtin_answers_like_the_program() {
    let s = Server::new("bash");
    let lib = s.dir.join("zeropty_bash.dylib");
    let cc =
        Command::new("clang").args(["-dynamiclib", "-O2", "-o"]).arg(&lib).arg(concat!(env!("CARGO_MANIFEST_DIR"), "/shell/zeropty_bash.c")).status().unwrap();
    assert!(cc.success());
    let bash = |cmd: &str| {
        Command::new("/bin/bash")
            .args(["-c", &format!("enable -f {} zeropty && {cmd}", lib.display())])
            .env("ZEROPTY_SOCK", s.sock())
            .env("ZEROPTY_BIN", env!("CARGO_BIN_EXE_zeropty"))
            .output()
            .unwrap()
    };
    let o = bash("zeropty new-session -d -s b -x 40 -y 6 'exec cat'");
    assert!(o.status.success(), "{o:?}");
    let o = bash(r#"zeropty send-keys -t =b: -l $'q"uo\\te\ttab 한글 \e[1mbold\e[0m' && zeropty send-keys -t =b: Enter"#);
    assert!(o.status.success(), "{o:?}");
    // cat's line, bold (the echo before it shows the escape as ^[[1m).
    let program = s.wait_capture(&["capture-pane", "-p", "-e", "-t", "=b:"], |o| o.contains("\x1b[0;1mbold"));
    assert_eq!(String::from_utf8(bash("zeropty capture-pane -p -e -t =b:").stdout).unwrap(), program);
    let o = bash("zeropty --help");
    assert!(o.status.success() && o.stdout.starts_with(b"zeropty: "), "{o:?}");
    let o = bash("zeropty has-session -t =nope");
    assert_eq!((o.status.code(), String::from_utf8_lossy(&o.stderr).as_ref()), (Some(1), "can't find session: nope\n"));
}

/// wait-pane answers once its text shows or the session goes quiet, without the client polling.
#[test]
fn wait_pane_text_quiet_timeout_and_end() {
    let s = Server::new("wait");
    s.ok(&["new-session", "-d", "-s", "w", "-x", "40", "-y", "5", "printf '\\033]zeropty-tty;0\\007'; sleep 0.5; echo READY; exec cat"]);
    let t = Instant::now();
    s.ok(&["wait-pane", "-t", "=w", "-m", "READY", "-T", "3000"]);
    assert!(t.elapsed() >= Duration::from_millis(400), "answered before the text showed");
    assert!(s.ok(&["capture-pane", "-p", "-t", "=w:"]).contains("READY"));
    // Already on the screen: at once.
    let t = Instant::now();
    s.ok(&["wait-pane", "-t", "=w", "-m", "READY", "-T", "3000"]);
    assert!(t.elapsed() < Duration::from_millis(200));
    // Text written in two pieces, the second by moving the cursor back onto the row.
    s.ok(&["send-keys", "-t", "=w:", "-l", "TAS\n"]);
    let waiter = {
        let sock = s.sock();
        std::thread::spawn(move || {
            Command::new(env!("CARGO_BIN_EXE_zeropty"))
                .args(["wait-pane", "-t", "=w", "-m", "TASK-DONE", "-T", "3000"])
                .env("ZEROPTY_SOCK", sock)
                .output()
                .unwrap()
        })
    };
    std::thread::sleep(Duration::from_millis(300));
    s.ok(&["send-keys", "-t", "=w:", "-l", "\x1b[A\x1b[4GK-DONE\n"]);
    assert!(waiter.join().unwrap().status.success());
    // Quiet: cat printed nothing for 300 ms, counted from what it just printed.
    s.ok(&["send-keys", "-t", "=w:", "-l", "x\n"]);
    let t = Instant::now();
    s.ok(&["wait-pane", "-t", "=w", "-q", "300", "-T", "3000"]);
    assert!(t.elapsed() >= Duration::from_millis(250) && t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    let o = s.run(&["wait-pane", "-t", "=w", "-m", "NEVER", "-T", "200"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("timed out"));
    assert!(!s.run(&["wait-pane", "-t", "=w"]).status.success(), "nothing to wait for");
    // The session ends while waited on.
    let sock = s.sock();
    let waiter = std::thread::spawn(move || {
        Command::new(env!("CARGO_BIN_EXE_zeropty")).args(["wait-pane", "-t", "=w", "-m", "NEVER", "-T", "5000"]).env("ZEROPTY_SOCK", sock).output().unwrap()
    });
    std::thread::sleep(Duration::from_millis(200));
    s.ok(&["kill-session", "-t", "=w"]);
    let o = waiter.join().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("session ended"));
}

/// Text on the main screen while a program runs on the alternate one shows again when it leaves: the
/// whole screen is searched then, not only the rows that read wrote.
#[test]
fn wait_pane_sees_the_screen_switched_back() {
    let s = Server::new("waitalt");
    s.ok(&["new-session", "-d", "-s", "a", "-x", "40", "-y", "5", "printf '\\033]zeropty-tty;0\\007'; echo MAINTEXT; printf '\\033[?1049h'; exec cat"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=a:"], |o| !o.contains("MAINTEXT"));
    let sock = s.sock();
    let waiter = std::thread::spawn(move || {
        Command::new(env!("CARGO_BIN_EXE_zeropty")).args(["wait-pane", "-t", "=a", "-m", "MAINTEXT", "-T", "3000"]).env("ZEROPTY_SOCK", sock).output().unwrap()
    });
    std::thread::sleep(Duration::from_millis(300));
    s.ok(&["send-keys", "-t", "=a:", "-l", "\x1b[?1049l"]);
    let o = waiter.join().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

/// Commands sent after a wait-pane on the same connection are answered after it, in order.
#[test]
fn wait_pane_holds_its_connection_in_order() {
    let s = Server::new("waitord");
    s.ok(&["new-session", "-d", "-s", "o", "-x", "20", "-y", "5", "exec cat"]);
    let mut c = UnixStream::connect(s.sock()).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let req = |argv: &[&str]| format!("{}\n", serde_json::json!({ "argv": argv, "cwd": "/" }));
    c.write_all([req(&["wait-pane", "-t", "=o", "-m", "GO", "-T", "3000"]), req(&["has-session", "-t", "=o"])].concat().as_bytes()).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    c.set_nonblocking(true).unwrap();
    let mut buf = [0u8; 4096];
    assert!(c.read(&mut buf).is_err(), "answered before GO showed");
    c.set_nonblocking(false).unwrap();
    s.ok(&["send-keys", "-t", "=o:", "-l", "GO\n"]);
    let mut got = Vec::new();
    while got.iter().filter(|&&b| b == b'\n').count() < 2 {
        let n = c.read(&mut buf).unwrap();
        assert!(n > 0);
        got.extend_from_slice(&buf[..n]);
    }
    let lines: Vec<serde_json::Value> = got.split(|&b| b == b'\n').filter(|l| !l.is_empty()).map(|l| serde_json::from_slice(l).unwrap()).collect();
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().all(|l| l["code"] == 0));
}

/// grep-panes: every session's screen (and scrollback with -S) searched, "name: line" per match, sessions by
/// name, code 1 when nothing matches; output not read yet is read first.
#[test]
fn grep_panes_finds_text_across_sessions() {
    let s = Server::new("grep");
    s.ok(&["new-session", "-d", "-s", "b", "-x", "40", "-y", "3", "printf 'old ASK-ME\\n1\\n2\\n3\\n4\\n'; exec cat"]);
    s.ok(&["new-session", "-d", "-s", "a", "-x", "40", "-y", "5", "echo 'please ASK-ME now'; exec cat"]);
    s.ok(&["new-session", "-d", "-s", "c", "-x", "40", "-y", "5", "printf '\\033]zeropty-tty;0\\007'; echo nothing here; exec cat"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=b:"], |o| o.contains('4'));
    s.wait_capture(&["capture-pane", "-p", "-t", "=a:"], |o| o.contains("ASK-ME"));
    assert_eq!(s.ok(&["grep-panes", "ASK-ME"]), "a: please ASK-ME now\n");
    // Scrolled off b's 3 rows: found only with -S.
    assert_eq!(s.ok(&["grep-panes", "-S", "-10", "ASK-ME"]), "a: please ASK-ME now\nb: old ASK-ME\n");
    let o = s.run(&["grep-panes", "NOWHERE"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(o.stdout.is_empty());
    // Printed just now, not read by the server yet: found all the same.
    s.ok(&["send-keys", "-t", "=c:", "-l", "FRESH\n"]);
    assert_eq!(s.ok(&["grep-panes", "FRESH"]), "c: FRESH\n");
}

/// Flags zeropty does not know are errors, not ignored; the ones it took without acting on now act.
#[test]
fn flags_are_checked_and_acted_on() {
    let s = Server::new("flags");
    // new-session -P -F, and -A on one that exists.
    assert_eq!(
        s.ok(&["new-session", "-d", "-P", "-F", "#{session_name} #{pane_width}x#{pane_height}", "-s", "c", "-x", "30", "-y", "5", "exec cat"]),
        "c 30x5\n"
    );
    let o = s.run(&["capture-pane", "-p", "-Z", "-t", "=c:"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("unknown flag -Z"), "{:?}", o);
    assert_eq!(s.ok(&["new-session", "-A", "-d", "-P", "-s", "c", "exec cat"]), "c:\n");
    assert!(!s.run(&["new-session", "-d", "-s", "c", "exec cat"]).status.success());
    // send-keys -N, -H.
    s.ok(&["send-keys", "-N", "3", "-t", "=c:", "-l", "ab"]);
    s.ok(&["send-keys", "-H", "-t", "=c:", "43", "44", "0a"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=c:"], |o| o.starts_with("abababCD\n"));
    assert!(!s.run(&["send-keys", "-N", "0", "-t", "=c:", "x"]).status.success());
    // capture-pane -S/-E ranges as tmux numbers lines (negative: scrollback).
    s.ok(&["new-session", "-d", "-s", "n", "-x", "20", "-y", "5", "seq 1 20; exec sleep 100"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=n:"], |o| o.contains("20"));
    let cap = |a: &[&str]| s.ok(&[&["capture-pane", "-p", "-t", "=n:"][..], a].concat());
    // The screen: 17 18 19 20 and the empty row the last line feed left the cursor on.
    assert_eq!(cap(&["-S", "-3", "-E", "-1"]), "14\n15\n16\n");
    assert_eq!(cap(&["-S", "1", "-E", "2"]), "18\n19\n");
    assert_eq!(cap(&["-S", "-", "-E", "0"]), (1..=17).map(|i| format!("{i}\n")).collect::<String>());
    assert_eq!(cap(&["-E", "-2"]), "15\n16\n17\n", "-E above -S: swapped");
    assert_eq!(cap(&["-S", "-2"]), "15\n16\n17\n18\n19\n20\n\n");
    // -N keeps each row's full width.
    assert_eq!(cap(&["-N", "-E", "0"]), format!("17{}\n", " ".repeat(18)));
    // Without -p: into a buffer, then pasted.
    assert_eq!(s.ok(&["capture-pane", "-b", "cap", "-S", "0", "-E", "1", "-t", "=n:"]), "");
    s.ok(&["paste-buffer", "-b", "cap", "-s", "+", "-t", "=c:"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=c:"], |o| o.contains("17+18+"));
    // list-sessions -F, kill-session -a.
    assert_eq!(s.ok(&["list-sessions", "-F", "#{session_name}"]), "c\nn\n");
    s.ok(&["kill-session", "-a", "-t", "=n"]);
    assert_eq!(s.ok(&["list-sessions", "-F", "#{session_name}"]), "n\n");
}

/// Key names as the bytes an xterm sends: F keys, Insert, modifiers on arrows and characters.
#[test]
fn key_names_as_xterm_sends_them() {
    let s = Server::new("keys2");
    // Prints each byte it reads in hex, a line per read.
    s.ok(&["new-session", "-d", "-s", "k", "-x", "80", "-y", "30", "printf '\\033]zeropty-tty;0\\007'; echo READY; exec perl -e '$|=1; while(sysread(STDIN,$b,64)){print join(\" \",map{sprintf \"%02x\",ord}split //,$b),\"\\n\"}'"]);
    let cases: &[(&str, &str)] = &[
        ("F1", "1b 4f 50"),
        ("F5", "1b 5b 31 35 7e"),
        ("F12", "1b 5b 32 34 7e"),
        ("IC", "1b 5b 32 7e"),
        ("C-Up", "1b 5b 31 3b 35 41"),
        ("S-F5", "1b 5b 31 35 3b 32 7e"),
        ("M-x", "1b 78"),
        ("C-M-a", "1b 01"),
        ("M-Enter", "1b 0d"),
        ("C-Space", "00"),
        ("S-Tab", "09"),
        ("BTab", "1b 5b 5a"),
        ("C-c", "03"),
        ("^d", "04"),
        ("enter", "0d"),
        ("Hello", "48 65 6c 6c 6f"),
    ];
    // Keys sent once it is in raw mode (the session prints READY after): before, they would be a line's.
    s.wait_capture(&["capture-pane", "-p", "-t", "=k:"], |o| o.starts_with("READY"));
    for (i, (key, want)) in cases.iter().enumerate() {
        s.ok(&["send-keys", "-t", "=k:", key]);
        s.wait_capture(&["capture-pane", "-p", "-t", "=k:"], |o| o.lines().filter(|l| !l.is_empty()).count() > i + 1);
        let screen = s.ok(&["capture-pane", "-p", "-t", "=k:"]);
        assert_eq!(screen.lines().nth(i + 1), Some(*want), "{key}");
    }
}

/// shim/shim.c built here (arm64 and arm64e: system binaries are arm64e), for programs run through
/// zeropty-tty with it.
fn shim(s: &Server) -> PathBuf {
    let lib = s.dir.join("shim.dylib");
    let cc = Command::new("clang")
        .args(["-dynamiclib", "-O2", "-arch", "arm64", "-arch", "arm64e", "-o"])
        .arg(&lib)
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/shim/shim.c"))
        .status()
        .unwrap();
    assert!(cc.success());
    lib
}

/// Terminal programs through zeropty-tty: less (a SIP-protected binary, run from a signed copy, reading keys
/// from /dev/tty) and python's REPL (libedit: needs tcgetpgrp to answer, or it edits nothing and shows no
/// prompt).
#[test]
fn terminal_programs_through_zeropty_tty() {
    let s = Server::new("tty");
    let lib = shim(&s);
    let tty = concat!(env!("CARGO_MANIFEST_DIR"), "/zeropty-tty");
    let env = format!("ZEROPTY_SHIM={} ZEROPTY_CACHE={}", lib.display(), s.dir.display());
    // Some macOS installs (GitHub's macos-15 runners) kill an ad-hoc re-signed copy of a system binary
    // at launch (SIGKILL, 137), so nothing run through a copy works there. Probe with /usr/bin/true the
    // same way and skip loudly rather than fail on the machine instead of the code.
    let probe = Command::new("sh").arg("-c").arg(format!("{env} {tty} /usr/bin/true")).status().unwrap();
    if probe.code() != Some(0) {
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), "SKIPPED terminal_programs_through_zeropty_tty: a re-signed copy of /usr/bin/true did not run here ({probe})");
        return;
    }
    let file = s.dir.join("lines.txt");
    std::fs::write(&file, (1..=100).map(|i| format!("{i}\n")).collect::<String>()).unwrap();
    s.ok(&["new-session", "-d", "-s", "l", "-x", "40", "-y", "6", &format!("{env} {tty} less {}; echo EXIT=$?; exec sleep 100", file.display())]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=l:"], |o| o.starts_with("1\n2\n"));
    s.ok(&["send-keys", "-t", "=l:", "G"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=l:"], |o| o.contains("100\n(END)"));
    s.ok(&["send-keys", "-t", "=l:", "q"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=l:"], |o| o.contains("EXIT=0"));
    if std::path::Path::new("/Library/Developer/CommandLineTools/usr/bin/python3").exists() {
        s.ok(&["new-session", "-d", "-s", "p", "-x", "40", "-y", "6", &format!("{env} {tty} python3 -q")]);
        s.wait_capture(&["capture-pane", "-p", "-t", "=p:"], |o| o.starts_with(">>>"));
        s.ok(&["send-keys", "-t", "=p:", "print(6*7)", "Enter"]);
        s.wait_capture(&["capture-pane", "-p", "-t", "=p:"], |o| o.contains(">>> print(6*7)\n42\n>>>"));
    }
}

/// Only the session's own pipe and socket are a terminal to a program run through zeropty-tty: its
/// stdout piped on or redirected to a file, or its stdin from /dev/null, is not, and its mode changes
/// (raw mode) never land in that output. Outside a session nothing is a terminal (the shim is inherited
/// by children through DYLD_INSERT_LIBRARIES, so this is a program the session's program started).
#[test]
fn zeropty_tty_answers_only_for_the_session_fds() {
    let s = Server::new("ttyid");
    let lib = shim(&s);
    let tty = concat!(env!("CARGO_MANIFEST_DIR"), "/zeropty-tty");
    // Raw mode on stdin (an OSC to the session when it is the terminal), then which of 0-2 are terminals.
    let src = s.dir.join("probe.c");
    std::fs::write(
        &src,
        r#"#include <stdio.h>
#include <termios.h>
#include <unistd.h>
int main(void) {
  struct termios t;
  if (tcgetattr(0, &t) == 0) { t.c_lflag &= ~(ICANON | ECHO); tcsetattr(0, TCSANOW, &t); }
  printf("tty=%d%d%d\n", isatty(0), isatty(1), isatty(2));
  return 0;
}
"#,
    )
    .unwrap();
    let probe = s.dir.join("probe");
    assert!(Command::new("clang").arg("-o").arg(&probe).arg(&src).status().unwrap().success());
    let run = format!("ZEROPTY_SHIM={} ZEROPTY_CACHE={} {tty} {}", lib.display(), s.dir.display(), probe.display());
    let file = s.dir.join("out.txt");
    s.ok(&[
        "new-session",
        "-d",
        "-s",
        "t",
        "-x",
        "40",
        "-y",
        "8",
        &format!(
            "{run} | od -c | grep -c 033; {run} </dev/null; {run} >{}; env -u ZEROPTY_TTY_ID {run}; env -u ZEROPTY_TTY_ID {run} | od -c | grep -c 033; echo DONE; exec sleep 100",
            file.display()
        ),
    ]);
    let screen = s.wait_capture(&["capture-pane", "-p", "-t", "=t:"], |o| o.contains("DONE"));
    // Piped on: no OSC in the pipe (0 lines with ESC). From /dev/null: stdin is no terminal, so no raw mode.
    // Without ZEROPTY_TTY_ID (a session from a server before it, kept across restart-server): the
    // session's pipe and socket as the program starts with them, still not a pipe on.
    assert!(screen.starts_with("0\ntty=011\ntty=111\n0\nDONE\n"), "{screen:?}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "tty=101\n");
    let outside = Command::new(tty)
        .arg(&probe)
        .env("ZEROPTY_SHIM", &lib)
        .env("ZEROPTY_CACHE", &s.dir)
        .env_remove("ZEROPTY_TTY_ID")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&outside.stdout), "tty=000\n");
}

/// zeropty-tty names its signed copy after the name the program was run by, not the link's target
/// (claude is a link to claude.exe), so ps and pgrep find it under that name.
#[test]
fn zeropty_tty_copy_keeps_the_name_it_was_run_by() {
    let s = Server::new("ttyname");
    let link = s.dir.join("mytrue");
    std::os::unix::fs::symlink("/usr/bin/true", &link).unwrap();
    // Whether the copy then runs depends on the machine (see terminal_programs_through_zeropty_tty).
    let _ = Command::new(concat!(env!("CARGO_MANIFEST_DIR"), "/zeropty-tty")).arg(&link).env("ZEROPTY_CACHE", &s.dir).status().unwrap();
    let names: Vec<String> = std::fs::read_dir(s.dir.join("tty"))
        .unwrap()
        .flat_map(|d| std::fs::read_dir(d.unwrap().path()).unwrap())
        .map(|f| f.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().any(|n| n == "mytrue"), "{names:?}");
    assert!(!names.iter().any(|n| n == "true"), "{names:?}");
}

/// The line discipline a PTY applies to keys, as tmux's sessions have it: Enter ends a line, C-c
/// interrupts, BSpace/C-w/C-u edit the line, C-d ends input, typing echoes; and raw mode (from the
/// shim, or printed by the program itself) passes keys as they are.
#[test]
fn keys_go_through_a_line_discipline() {
    let s = Server::new("ldisc");
    let read = "read x; echo \"got=[$x]\"; exec sleep 100";
    for (name, keys, want) in [
        ("r", &["abc", "Enter"][..], "abc\ngot=[abc]\n"),
        ("b", &["abc", "BSpace", "d", "Enter"], "abd\ngot=[abd]\n"),
        ("w", &["one two", "C-w", "three", "Enter"], "one three\ngot=[one three]\n"),
        ("u", &["junk", "C-u", "ok", "Enter"], "ok\ngot=[ok]\n"),
        ("k", &["한글x", "BSpace", "BSpace", "Enter"], "한\ngot=[한]\n"),
    ] {
        s.ok(&["new-session", "-d", "-s", name, "-x", "40", "-y", "5", read]);
        s.ok(&[&["send-keys", "-t", &format!("={name}:")][..], keys].concat());
        let screen = s.wait_capture(&["capture-pane", "-p", "-t", &format!("={name}:")], |o| o.contains("got="));
        assert!(screen.starts_with(want), "{name}: {screen:?}");
    }
    // C-c: the program's process group gets SIGINT (sh with it: the session ends).
    s.ok(&["new-session", "-d", "-s", "c", "sleep 100; echo NOT-INTERRUPTED; exec sleep 100"]);
    s.ok(&["send-keys", "-t", "=c:", "C-c"]);
    let start = Instant::now();
    while s.run(&["has-session", "-t", "=c"]).status.success() {
        assert!(start.elapsed() < Duration::from_secs(3), "C-c did not interrupt");
        std::thread::sleep(Duration::from_millis(20));
    }
    // C-d on an empty line: end of input.
    s.ok(&["new-session", "-d", "-s", "e", "-x", "40", "-y", "5", "cat; echo CAT-ENDED; exec sleep 100"]);
    s.ok(&["send-keys", "-t", "=e:", "hi", "Enter", "C-d"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=e:"], |o| o.starts_with("hi\nhi\nCAT-ENDED\n"));
    // Raw mode printed by the program: no echo, no line, CR as it is.
    // READY is printed after the mode change: once it is on the screen, so is the mode (no fixed sleep,
    // which a loaded machine outran).
    s.ok(&[
        "new-session",
        "-d",
        "-s",
        "raw",
        "-x",
        "40",
        "-y",
        "5",
        "printf '\\033]zeropty-tty;0\\007READY\\n'; exec perl -e '$|=1; sysread(STDIN,$b,3); printf \"%vd\\n\", $b; sleep 100'",
    ]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=raw:"], |o| o.contains("READY"));
    s.ok(&["send-keys", "-t", "=raw:", "a", "Enter", "C-c"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=raw:"], |o| o.contains("\n97.13.3\n"));
    // Typed while cooked, then the program turns raw: what was typed reaches it, before what follows.
    // The program waits until the test has seen "ab" echoed (typed while cooked), then turns raw and
    // says RAW; "c" follows only after that, whatever the machine's load (fixed sleeps were outrun).
    let go = s.dir.join("late.go");
    // As on a tty, the line typed is readable once the mode turns raw: a read may return "ab" alone,
    // so the program reads until it has all three bytes (one sysread made the test depend on timing).
    let cmd = format!(
        "while [ ! -e '{}' ]; do sleep 0.02; done; printf '\\033]zeropty-tty;0\\007RAW\\n'; exec perl -e '$|=1; my $b = \"\"; while (length($b) < 3) {{ sysread(STDIN, my $x, 3 - length($b)) or last; $b .= $x }} print \"[$b]\\n\"; sleep 100'",
        go.display()
    );
    s.ok(&["new-session", "-d", "-s", "late", "-x", "40", "-y", "5", &cmd]);
    s.ok(&["send-keys", "-t", "=late:", "ab"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=late:"], |o| o.starts_with("ab"));
    std::fs::write(&go, "").unwrap();
    s.wait_capture(&["capture-pane", "-p", "-t", "=late:"], |o| o.contains("RAW"));
    s.ok(&["send-keys", "-t", "=late:", "c"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=late:"], |o| o.contains("[abc]"));
}

/// tmux's format variables (those a session here has) and list-sessions as tmux prints it.
#[test]
fn formats_as_tmux_has_them() {
    let s = Server::new("fmt");
    let dir = std::fs::canonicalize(&s.dir).unwrap();
    s.ok(&["new-session", "-d", "-s", "f", "-x", "40", "-y", "5", "-c", dir.to_str().unwrap(), "seq 1 8; printf abc; exec sleep 100"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=f:"], |o| o.contains("abc"));
    let f = "#{session_name} #{pane_current_command} #{cursor_x},#{cursor_y} #{alternate_on} #{history_size} #{session_windows} #{window_index}.#{pane_index} #{pane_dead} [#{nope}]";
    assert_eq!(s.ok(&["display-message", "-p", "-t", "=f:", f]), "f sleep 3,4 0 4 1 0.0 0 []\n");
    assert_eq!(s.ok(&["display-message", "-p", "-t", "=f:", "#{pane_current_path}"]).trim(), dir.to_str().unwrap());
    let created: i64 = s.ok(&["display-message", "-p", "-t", "=f:", "#{session_created}"]).trim().parse().unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    assert!((now - created).abs() < 10);
    let list = s.ok(&["list-sessions"]);
    assert!(list.starts_with("f: 1 windows (created ") && list.ends_with(")\n"), "{list:?}");
}

/// pipe-pane, rename-session, resize-window, set-environment -g.
#[test]
fn pipe_rename_resize_environment() {
    let s = Server::new("cmds");
    // Raw: what is sent comes back from cat as it is, with no echo.
    s.ok(&["new-session", "-d", "-s", "p", "-x", "40", "-y", "5", "printf '\\033]zeropty-tty;0\\007'; exec cat"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=p:"], |_| true);
    std::thread::sleep(Duration::from_millis(200));
    let log = s.dir.join("pipe.log");
    s.ok(&["pipe-pane", "-t", "=p:", &format!("cat > {}", log.display())]);
    s.ok(&["send-keys", "-t", "=p:", "-l", "one\n"]);
    let start = Instant::now();
    while std::fs::read_to_string(&log).unwrap_or_default() != "one\n" {
        assert!(start.elapsed() < Duration::from_secs(3), "{:?}", std::fs::read_to_string(&log));
        std::thread::sleep(Duration::from_millis(20));
    }
    // -o with one running: closed; output after it does not go there.
    s.ok(&["pipe-pane", "-o", "-t", "=p:", "cat > /dev/null"]);
    s.ok(&["send-keys", "-t", "=p:", "-l", "two\n"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=p:"], |o| o.contains("two"));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "one\n");
    // rename-session.
    s.ok(&["rename-session", "-t", "=p", "q"]);
    assert!(s.run(&["has-session", "-t", "=q"]).status.success());
    assert!(!s.run(&["has-session", "-t", "=p"]).status.success());
    s.ok(&["new-session", "-d", "-s", "p", "exec sleep 100"]);
    assert!(!s.run(&["rename-session", "-t", "=q", "p"]).status.success(), "duplicate");
    // resize-window: the screen and what the program is told.
    s.ok(&["resize-window", "-t", "=q", "-x", "60", "-y", "7"]);
    assert_eq!(s.ok(&["display-message", "-p", "-t", "=q:", "#{pane_width}x#{pane_height}"]), "60x7\n");
    // set-environment -g: in sessions made after.
    s.ok(&["set-environment", "-g", "ZEROPTY_T_VAR", "hello"]);
    assert_eq!(s.ok(&["show-environment", "-g", "ZEROPTY_T_VAR"]), "ZEROPTY_T_VAR=hello\n");
    s.ok(&["new-session", "-d", "-s", "e", "-x", "40", "-y", "3", "echo \"[$ZEROPTY_T_VAR]\"; exec sleep 100"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=e:"], |o| o.starts_with("[hello]"));
    s.ok(&["set-environment", "-gu", "ZEROPTY_T_VAR"]);
    s.ok(&["new-session", "-d", "-s", "e2", "-x", "40", "-y", "3", "echo \"[$ZEROPTY_T_VAR]\"; exec sleep 100"]);
    s.wait_capture(&["capture-pane", "-p", "-t", "=e2:"], |o| o.starts_with("[]"));
}
