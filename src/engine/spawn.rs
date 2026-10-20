//! Starting programs: a session's (through /bin/sh, its output into a socket pair, the environment a
//! program under tmux would see), and a pipe-pane command's. Each leads its own process group, so
//! signals for it (C-c, kill-session) reach whatever it started.
//!
//! The OS side of super::backend::SystemPrograms: the engine reaches these only through `Programs`.

use super::backend::{Spawn, Spawned};
use crate::sys::{fd, process};
use std::os::fd::{AsFd as _, AsRawFd as _, OwnedFd};
use std::os::unix::process::CommandExt as _;

/// Starts `p`; the error is the reply's text.
pub(super) fn session(p: &Spawn<'_>) -> Result<Spawned, String> {
    // stdout and stderr share one channel, as they share one terminal under tmux.
    let (reader, writer) = fd::output_channel().map_err(|e| format!("socketpair: {e}\n"))?;
    let writer2 = writer.try_clone().map_err(|e| format!("socketpair: {e}\n"))?;
    let (input, stdin) = fd::pipe().map_err(|e| format!("pipe: {e}\n"))?;
    // Which fds are the session's terminal, for the shim (zeropty-tty): only these two files, wherever a
    // program has them (fd 0-2, a dup, an inherited fd). A program whose stdout is redirected to a file
    // or a pipe then sees no terminal there, as under tmux.
    let tty_id = format!("{},{}", file_id(&input).map_err(|e| format!("spawn: {e}\n"))?, file_id(&writer).map_err(|e| format!("spawn: {e}\n"))?);
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.arg("-c")
        .arg(p.command)
        .current_dir(p.dir)
        .stdin(input)
        .stdout(writer)
        .stderr(writer2)
        .env("TERM", "xterm-256color")
        .env("ZEROPTY_COLS", p.cols.to_string())
        .env("ZEROPTY_ROWS", p.rows.to_string())
        .env("COLUMNS", p.cols.to_string())
        .env("LINES", p.rows.to_string())
        .env("ZEROPTY_SESSION", p.name)
        .env("ZEROPTY_SIZE_FILE", p.size_file)
        .env("ZEROPTY_TTY_ID", tty_id)
        // Sessions are not inside tmux even when the server was started from one; claude reads these.
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .process_group(0);
    for (k, v) in p.env {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
    for kv in &p.extra_env {
        if let Some((k, v)) = kv.split_once('=') {
            cmd.env(k, v);
        }
    }
    fd::set_nonblocking(reader.as_raw_fd()).map_err(|e| format!("spawn: {e}\n"))?;
    let child = fd::spawn(&mut cmd).map_err(|e| format!("spawn: {e}\n"))?;
    // `cmd` holds the write ends; dropping it here leaves only the child's, so EOF arrives when it exits.
    drop(cmd);
    let pid = child.id() as i32;
    if let Err(e) = fd::set_nonblocking(stdin.as_raw_fd()) {
        // Its input would block the engine: no session. Its group goes as kill-session's would.
        process::signal_group(pid, libc::SIGKILL);
        return Err(format!("spawn: {e}\n"));
    }
    // The Child handle is not needed: exit comes from kqueue and waitpid reaps.
    drop(child);
    Ok(Spawned { pid, stdin, out: reader })
}

/// "dev:ino" of the file `fd` is open on: the same for every fd on that file (a dup, an inherited
/// copy), different for each end of a pipe or socket pair and for anything else.
fn file_id(fd: &OwnedFd) -> std::io::Result<String> {
    // SAFETY: libc::stat is plain integers, for which all zeroes is a valid value.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: fstat fills `st`, which we own, from an fd held open by `fd`.
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut st) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(format!("{}:{}", st.st_dev, st.st_ino))
}

/// Starts pipe-pane's `command` reading a socket; returns (pid, our end: non-blocking, SO_NOSIGPIPE).
/// Its output goes nowhere.
pub(super) fn pipe_command(command: &str) -> Result<(i32, OwnedFd), String> {
    let (ours, theirs) = fd::socket_pair().map_err(|e| format!("pipe-pane: {e}\n"))?;
    let _ = ours.shutdown(std::net::Shutdown::Read);
    fd::set_nonblocking(ours.as_raw_fd()).and_then(|()| fd::no_sigpipe(ours.as_fd())).map_err(|e| format!("pipe-pane: {e}\n"))?;
    let mut c = std::process::Command::new("/bin/sh");
    c.arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::from(OwnedFd::from(theirs)))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0);
    let child = fd::spawn(&mut c).map_err(|e| format!("pipe-pane: {e}\n"))?;
    drop(c);
    Ok((child.id() as i32, ours.into()))
}
