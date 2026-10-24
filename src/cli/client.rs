//! The CLI as a client: one command to the server over its socket (starting a server for new-session,
//! as tmux does), and restart-server, which hands every session to a new server.

use super::sock_path;
use std::io::{BufRead as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use zeropty::engine::{Reply, fail};
use zeropty::protocol::{Request, request_line};
use zeropty::server::connect;

/// sockaddr_un.sun_path is 104 bytes on macOS; a longer path fails to bind with no useful error.
const SUN_PATH_MAX: usize = 104;

/// Runs `argv` on the server and prints its reply; returns the exit code.
pub fn run(argv: Vec<String>) -> i32 {
    if sock_path().as_os_str().len() >= SUN_PATH_MAX {
        eprintln!("zeropty: socket path too long (max 103 bytes): {}", sock_path().display());
        return 1;
    }
    let starts_server = matches!(argv[0].as_str(), "new-session" | "new" | "pull-sessions");
    let stream = match connect(&sock_path()) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("zeropty: {e}");
            return 1;
        }
        Err(_) if starts_server => match spawn_server_at(&sock_path()) {
            Some(s) => s,
            None => {
                eprintln!("zeropty: could not start server");
                return 1;
            }
        },
        Err(_) => {
            eprintln!("no server running on {}", sock_path().display());
            return 1;
        }
    };
    let cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default();
    let reply = match send(&stream, &Request { argv, cwd, raw: false }) {
        Some(r) => r,
        None => {
            eprintln!("zeropty: lost server");
            return 1;
        }
    };
    super::stdout_quiet(&reply.out);
    eprint!("{}", reply.err);
    reply.code
}

/// Sends `req` on `stream` and reads its reply line; None if the server is gone. A reply that does not
/// parse reads as a failure.
fn send(mut stream: &UnixStream, req: &Request) -> Option<Reply> {
    stream.write_all(&request_line(req)).ok()?;
    let mut buf = String::new();
    std::io::BufReader::new(stream).read_line(&mut buf).ok()?;
    Some(serde_json::from_str(&buf).unwrap_or_else(|_| fail("bad reply\n")))
}

/// One command on a connection; the reply ("lost server" if there is none).
pub fn ask(stream: &UnixStream, argv: &[&str]) -> Reply {
    let req = Request { argv: argv.iter().map(|a| a.to_string()).collect(), cwd: "/".into(), raw: false };
    send(stream, &req).unwrap_or_else(|| fail("lost server\n"))
}

/// Starts `zeropty serve` detached from this process, listening at `bind`, like tmux does on the first
/// new-session; returns a connection to it once it answers.
fn spawn_server_at(bind: &Path) -> Option<UnixStream> {
    use std::os::unix::process::CommandExt as _;
    let exe = std::env::current_exe().ok()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("serve");
    if bind != sock_path() {
        cmd.arg("--bind").arg(bind);
    }
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).process_group(0).spawn().ok()?;
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        if let Ok(s) = connect(bind) {
            return Some(s);
        }
    }
    None
}

/// `restart-server`: a new server (this binary, so also an upgrade) takes every session of the running one,
/// whose programs go on as they were, then takes its socket's place. Commands sent in between reach the
/// old server, which has fewer sessions by then (a few ms).
pub fn restart_server() -> i32 {
    let path = sock_path();
    if let Err(e) = connect(&path) {
        match e.kind() {
            std::io::ErrorKind::PermissionDenied => eprintln!("zeropty: {e}"),
            _ => eprintln!("no server running on {}", path.display()),
        }
        return 1;
    }
    let mut bind = path.clone().into_os_string();
    bind.push(".new");
    let bind = PathBuf::from(bind);
    if bind.as_os_str().len() >= SUN_PATH_MAX {
        eprintln!("zeropty: socket path too long for restart-server (max 99 bytes): {}", path.display());
        return 1;
    }
    let _ = std::fs::remove_file(&bind);
    let Some(new) = spawn_server_at(&bind) else {
        eprintln!("zeropty: could not start a new server");
        return 1;
    };
    let path_s = path.display().to_string();
    let r = ask(&new, &["pull-sessions", "-s", &path_s]);
    if r.code != 0 {
        // Some stayed with the old server: it goes on, and the new one leaves, giving back what it took.
        eprint!("{}", r.err);
        ask(&new, &["push-sessions", "-s", &path_s]);
        ask(&new, &["kill-server"]);
        let _ = std::fs::remove_file(&bind);
        eprintln!("zeropty: server not restarted");
        return 1;
    }
    if let Ok(old) = connect(&path) {
        ask(&old, &["kill-server"]);
    }
    if let Err(e) = std::fs::rename(&bind, &path) {
        eprintln!("zeropty: {}: {e}", path.display());
        return 1;
    }
    super::stdout_quiet(&r.out);
    0
}
