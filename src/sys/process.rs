//! Processes: signalling a session's process group, reaping, what `ps` would show of a process, and who
//! is on the other end of a socket.

/// Names the calling thread (what `sample` and debuggers show), cut to 63 bytes, macOS's limit. macOS
/// names only the calling thread, so a thread that learns its name late (a plugin's, after its manifest)
/// names itself. A name with a NUL is left alone.
pub(crate) fn name_thread(name: &str) {
    let mut end = name.len().min(63);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    let Ok(c) = std::ffi::CString::new(&name[..end]) else { return };
    // SAFETY: `c` is a NUL-terminated string alive for the call, which copies it.
    unsafe { libc::pthread_setname_np(c.as_ptr()) };
}

/// Sends `sig` to process group `pgid` (a session's program leads its own group). A group already gone
/// (ESRCH) is not an error worth reporting: the signal was for ending it.
pub(crate) fn signal_group(pgid: i32, sig: i32) {
    // SAFETY: killpg takes plain integers and touches no memory.
    unsafe { libc::killpg(pgid, sig) };
}

/// Reaps `pid` if it has exited, without waiting. Not a child of this process (ECHILD): nothing to do.
pub(crate) fn reap(pid: i32) {
    let mut status = 0;
    // SAFETY: `status` is a valid c_int for waitpid to write, alive for the call.
    unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
}

/// The name of process `pid` (its command, at most 16 bytes, as ps and tmux show it); empty if unknown.
pub(crate) fn name(pid: i32) -> String {
    // SAFETY: proc_bsdinfo is plain data; all-zero is a valid value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as i32;
    // SAFETY: proc_pidinfo writes at most `size` bytes into `info`, which has exactly that size.
    if unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size) } != size {
        return String::new();
    }
    let comm: Vec<u8> = info.pbi_comm.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&comm).into_owned()
}

/// The working directory of process `pid`; empty if unknown.
pub(crate) fn cwd(pid: i32) -> String {
    // SAFETY: proc_vnodepathinfo is plain data; all-zero is a valid value.
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as i32;
    // SAFETY: proc_pidinfo writes at most `size` bytes into `info`, which has exactly that size.
    if unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDVNODEPATHINFO, 0, (&mut info as *mut libc::proc_vnodepathinfo).cast(), size) } != size {
        return String::new();
    }
    let path: Vec<u8> = info.pvi_cdir.vip_path.iter().flatten().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&path).into_owned()
}

/// Whether the peer of `s` runs as this process's user, or as root.
pub(crate) fn peer_is_same_user(s: &std::os::unix::net::UnixStream) -> bool {
    use std::os::fd::AsRawFd as _;
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: getpeereid writes the two ids into locals alive for the call.
    if unsafe { libc::getpeereid(s.as_raw_fd(), &mut uid, &mut gid) } != 0 {
        return false;
    }
    // SAFETY: geteuid takes nothing and cannot fail.
    uid == 0 || uid == unsafe { libc::geteuid() }
}

/// Runs the calling thread at the interactive QoS class: woken on a performance core at once when work
/// comes (as a default thread, a command handed to the engine's thread under load took ~0.1-0.2 ms).
pub(crate) fn set_interactive_qos() {
    // SAFETY: sets the calling thread's own QoS class; no memory is passed. A refusal leaves the default.
    unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0) };
}

#[cfg(test)]
mod tests {
    #[test]
    fn describes_this_process() {
        let me = std::process::id() as i32;
        assert!(!super::name(me).is_empty());
        assert_eq!(super::cwd(me), std::env::current_dir().unwrap().display().to_string());
        assert_eq!(super::name(-5), "", "no such process");
        let (a, _b) = crate::sys::fd::socket_pair().unwrap();
        assert!(super::peer_is_same_user(&a));
    }
}
