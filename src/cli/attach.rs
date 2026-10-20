//! `zeropty attach [-r] [-t target]`: a session on this terminal. Keys go to it (not with -r), what it prints
//! comes back as is, and the session takes this terminal's size while attached. Ctrl-b d detaches (and q
//! or Ctrl-c with -r). Runs in the client only; the server just copies bytes (see Server::attach).
//!
//! The terminal itself (raw mode, size, SIGWINCH) is `zeropty::tty`'s; what goes on the socket is
//! `zeropty::protocol`'s. This module is the loop between them, and the key filter (`Keys`), which is
//! pure.

use super::{client, sock_path};
use std::io::{Read as _, Write as _};
use std::os::fd::AsFd as _;
use std::os::unix::net::UnixStream;
use zeropty::protocol::{ClientFrame, Frame as _, Frames, Request, ServerFrame, request_line};
use zeropty::tty;

const PREFIX: u8 = 0x02; // Ctrl-b, as tmux

/// Attributes, cursor, keypad, bracketed paste, mouse reporting (all encodings), focus events,
/// modifyOtherKeys and the kitty keyboard stack, back to a plain terminal's.
const RESET_MODES: &[u8] =
    b"\x1b[0m\x1b[?25h\x1b[?1l\x1b>\x1b[?2004l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[?1015l\x1b[?1004l\x1b[>4m\x1b[<99u";

/// The size to give the session: this terminal's, or 24x80 when it reports none.
fn window_size() -> (u16, u16) {
    tty::window_size(std::io::stdout().as_fd()).unwrap_or((24, 80))
}

/// The session to attach to when none was named: the only one, or the one picked from a numbered list.
fn choose() -> Result<String, String> {
    let s = UnixStream::connect(sock_path()).map_err(|_| format!("no server running on {}", sock_path().display()))?;
    let out = client::ask(&s, &["list-sessions"]).out;
    let names: Vec<&str> = out.lines().filter_map(|l| l.split(':').next()).collect();
    match names.len() {
        0 => Err("no sessions".into()),
        1 => Ok(names[0].to_owned()),
        _ => {
            for (k, l) in out.lines().enumerate() {
                eprintln!("{:3}) {l}", k + 1);
            }
            eprint!("session? ");
            let mut pick = String::new();
            std::io::stdin().read_line(&mut pick).map_err(|e| e.to_string())?;
            let pick = pick.trim();
            match pick.parse::<usize>() {
                Ok(k) if (1..=names.len()).contains(&k) => Ok(names[k - 1].to_owned()),
                _ if names.contains(&pick) => Ok(pick.to_owned()),
                _ => Err(format!("no such session: {pick}")),
            }
        }
    }
}

/// What typed keys mean while attached: the prefix (Ctrl-b) and its commands, -r's way out.
struct Keys {
    read_only: bool,
    /// The last key was the prefix.
    prefix: bool,
}

/// What a read of keys comes to.
#[derive(Debug, PartialEq, Eq)]
enum Typed {
    /// Keys for the session (possibly none).
    Send(Vec<u8>),
    /// Leave, sending the keys before it.
    Detach(Vec<u8>),
}

impl Keys {
    /// The keys `typed` for the session, and whether they ask to detach.
    fn filter(&mut self, typed: &[u8]) -> Typed {
        let mut keys = Vec::with_capacity(typed.len());
        for &x in typed {
            if self.prefix {
                self.prefix = false;
                match x {
                    b'd' => return Typed::Detach(keys),
                    PREFIX => keys.push(PREFIX),
                    _ => keys.extend_from_slice(&[PREFIX, x]),
                }
            } else if x == PREFIX {
                self.prefix = true;
            } else if self.read_only && (x == b'q' || x == 0x03) {
                return Typed::Detach(keys);
            } else {
                keys.push(x);
            }
        }
        Typed::Send(keys)
    }
}

/// `zeropty attach [-r] [-t target]`; returns the exit code.
pub fn run(argv: &[String]) -> i32 {
    let (mut target, mut read_only) = (None, false);
    let mut k = 0;
    while k < argv.len() {
        match argv[k].as_str() {
            "-t" => {
                k += 1;
                target = argv.get(k).cloned();
            }
            "-r" => read_only = true,
            a => {
                eprintln!("usage: zeropty attach [-r] [-t target] (unknown {a})");
                return 2;
            }
        }
        k += 1;
    }
    if !tty::is_terminal(std::io::stdin().as_fd()) || !tty::is_terminal(std::io::stdout().as_fd()) {
        eprintln!("zeropty attach: not a terminal");
        return 1;
    }
    let target = match target.map(Ok).unwrap_or_else(choose) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("zeropty attach: {e}");
            return 1;
        }
    };
    match attach(&target, read_only) {
        Ok(true) => println!("[exited: {target}]"),
        Ok(false) => println!("[detached (from session {target})]"),
        Err(e) => {
            eprint!("{e}");
            return 1;
        }
    }
    0
}

/// Attaches to `target` until it ends (true) or this terminal detaches (false). The error is the line
/// to print.
fn attach(target: &str, read_only: bool) -> Result<bool, String> {
    let lost = |_| "zeropty attach: lost server\n".to_string();
    let mut sock = UnixStream::connect(sock_path()).map_err(|_| format!("no server running on {}\n", sock_path().display()))?;
    let (rows, cols) = window_size();
    let mut args = vec!["attach".to_string(), "-t".into(), target.to_owned(), "-x".into(), cols.to_string(), "-y".into(), rows.to_string()];
    if read_only {
        args.push("-r".into());
    }
    sock.write_all(&request_line(&Request { argv: args, cwd: String::new(), raw: false })).map_err(lost)?;
    // The reply line, then frames (what arrives after the line is kept for them).
    let mut input = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    let reply_end = loop {
        if let Some(n) = input.iter().position(|&b| b == b'\n') {
            break n;
        }
        match sock.read(&mut buf) {
            Ok(0) | Err(_) => return Err("zeropty attach: lost server\n".into()),
            Ok(n) => input.extend_from_slice(&buf[..n]),
        }
    };
    let reply: serde_json::Value = serde_json::from_slice(&input[..reply_end]).unwrap_or_default();
    if reply["code"] != 0 {
        return Err(reply["err"].as_str().unwrap_or("zeropty attach: failed\n").to_owned());
    }
    input.drain(..=reply_end);

    let stdin = std::io::stdin();
    // This terminal raw (keys as they are, no echo) until `_raw` drops, on a panic too.
    let _raw = tty::RawMode::enter(stdin.as_fd()).map_err(|e| format!("zeropty attach: raw mode: {e}\n"))?;
    let winch = tty::Winch::install().map_err(|e| format!("zeropty attach: SIGWINCH: {e}\n"))?;
    let mut out = std::io::stdout().lock();
    // Its alternate screen, left on the way out.
    let _ = out.write_all(b"\x1b[?1049h");
    let ended = pump(&mut sock, &mut out, &winch, &mut input, &mut buf, &mut Keys { read_only, prefix: false });
    if !ended {
        let _ = sock.write_all(&ClientFrame::Detach.to_vec());
    }
    // The session's program may have turned on modes this terminal keeps after it leaves the alternate
    // screen (claude turns on mouse reporting, focus events and modifyOtherKeys): all off again.
    let _ = out.write_all(RESET_MODES);
    let _ = out.write_all(b"\x1b[?1049l");
    let _ = out.flush();
    Ok(ended)
}

/// Copies frames to the terminal and keys to the session until the session ends (true), the user
/// detaches or the server goes away (false).
fn pump(sock: &mut UnixStream, out: &mut impl std::io::Write, winch: &tty::Winch, input: &mut Vec<u8>, buf: &mut [u8], keys: &mut Keys) -> bool {
    let stdin = std::io::stdin();
    loop {
        // Frames already here.
        let mut frames = Frames::<ServerFrame>::new(input);
        for f in frames.by_ref() {
            match f {
                Ok(ServerFrame::Output(bytes)) => {
                    let _ = out.write_all(bytes);
                }
                Ok(ServerFrame::Ended) => return true,
                // A frame of a newer server's: not for this client.
                Err(zeropty::protocol::FrameError::UnknownKind(_) | zeropty::protocol::FrameError::BadBody(_)) => {}
                Err(zeropty::protocol::FrameError::TooLong(_)) => return false,
            }
        }
        let used = frames.used();
        input.drain(..used);
        let _ = out.flush();
        let Ok([from_server, typed, resized]) = tty::poll([sock.as_fd(), stdin.as_fd(), winch.as_fd()], -1) else { return false };
        if from_server {
            match sock.read(buf) {
                Ok(0) | Err(_) => return false,
                Ok(n) => input.extend_from_slice(&buf[..n]),
            }
        }
        if resized {
            winch.clear();
            let (rows, cols) = window_size();
            let _ = sock.write_all(&ClientFrame::Resize { rows, cols }.to_vec());
        }
        if typed {
            let mut b = [0u8; 4096];
            let n = match tty::read(stdin.as_fd(), &mut b) {
                Ok(0) | Err(_) => return false,
                Ok(n) => n,
            };
            let (send, detach) = match keys.filter(&b[..n]) {
                Typed::Send(k) => (k, false),
                Typed::Detach(k) => (k, true),
            };
            if !keys.read_only && !send.is_empty() {
                let _ = sock.write_all(&ClientFrame::Input(&send).to_vec());
            }
            if detach {
                return false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Keys, PREFIX, Typed};

    #[test]
    fn the_prefix() {
        let mut k = Keys { read_only: false, prefix: false };
        assert_eq!(k.filter(b"ab"), Typed::Send(b"ab".to_vec()));
        assert_eq!(k.filter(&[b'x', PREFIX]), Typed::Send(b"x".to_vec()), "the prefix waits for its key");
        assert_eq!(k.filter(b"c"), Typed::Send(vec![PREFIX, b'c']), "not a command: both go");
        assert_eq!(k.filter(&[PREFIX, PREFIX]), Typed::Send(vec![PREFIX]), "C-b C-b sends one");
        assert_eq!(k.filter(&[b'y', PREFIX, b'd', b'z']), Typed::Detach(b"y".to_vec()), "keys before it go, after it not");
        assert_eq!(k.filter(b"q"), Typed::Send(b"q".to_vec()));
    }

    #[test]
    fn read_only_leaves_on_q_or_ctrl_c() {
        let mut k = Keys { read_only: true, prefix: false };
        assert_eq!(k.filter(b"ab"), Typed::Send(b"ab".to_vec()));
        assert_eq!(k.filter(b"q"), Typed::Detach(vec![]));
        assert_eq!(k.filter(&[0x03]), Typed::Detach(vec![]));
    }
}
