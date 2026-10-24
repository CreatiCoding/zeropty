//! Processes: signalling a session's process group, reaping, what `ps` would show of a process, and who
//! is on the other end of a socket.

/// Names the calling thread (what `sample` and debuggers show), cut to 63 bytes, macOS's limit. macOS
/// names only the calling thread, so a thread that learns its name late (a plugin's, after its manifest)
/// names itself. A name with a NUL is left alone.
pub(crate) fn name_thread(name: &str) {
    // Linux takes 15 bytes and a NUL.
    let mut end = name.len().min(if cfg!(target_os = "linux") { 15 } else { 63 });
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    let Ok(c) = std::ffi::CString::new(&name[..end]) else { return };
    // SAFETY: `c` is a NUL-terminated string alive for the call, which copies it.
    #[cfg(target_os = "macos")]
    unsafe {
        libc::pthread_setname_np(c.as_ptr())
    };
    // SAFETY: as above; pthread_self is the calling thread.
    #[cfg(target_os = "linux")]
    unsafe {
        libc::pthread_setname_np(libc::pthread_self(), c.as_ptr())
    };
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

#[cfg(target_os = "macos")]
/// What `ps` shows of process `pid`, or None if there is no such process.
fn bsd_info(pid: i32) -> Option<libc::proc_bsdinfo> {
    // SAFETY: proc_bsdinfo is plain data; all-zero is a valid value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as i32;
    // SAFETY: proc_pidinfo writes at most `size` bytes into `info`, which has exactly that size.
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size) };
    (n == size).then_some(info)
}

#[cfg(target_os = "macos")]
/// The name of process `pid` (its command, at most 16 bytes, as ps and tmux show it); empty if unknown.
pub(crate) fn name(pid: i32) -> String {
    let Some(info) = bsd_info(pid) else { return String::new() };
    let comm: Vec<u8> = info.pbi_comm.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&comm).into_owned()
}

#[cfg(target_os = "macos")]
/// Whether a live member of process group `pgid` is `root` or descends from it. Any member, not just
/// the leader: a pipeline's leader is its first command, often gone while the rest run (`true | sleep`).
/// A group with no member under `root` is not taken: a stranger's may have reused the number.
pub(crate) fn group_under(pgid: i32, root: i32) -> bool {
    // PROC_PGRP_ONLY in <libproc.h>; libc does not name it.
    const PROC_PGRP_ONLY: u32 = 2;
    if pgid <= 0 {
        return false;
    }
    let mut pids = [0i32; 64];
    let size = std::mem::size_of_val(&pids) as i32;
    // SAFETY: proc_listpids writes at most `size` bytes of pids into `pids`, which has exactly that size.
    let n = unsafe { libc::proc_listpids(PROC_PGRP_ONLY, pgid as u32, pids.as_mut_ptr().cast(), size) };
    let n = (n.max(0) as usize / std::mem::size_of::<i32>()).min(pids.len());
    pids[..n].iter().any(|&p| p > 0 && descends(p, root))
}

#[cfg(target_os = "macos")]
/// Whether process `pid` is `root` or descends from it.
fn descends(mut pid: i32, root: i32) -> bool {
    // Parents up to the session's program, or to launchd; a bound on the depth, in case of a cycle.
    for _ in 0..256 {
        if pid == root {
            return true;
        }
        match bsd_info(pid) {
            Some(i) if i.pbi_ppid as i32 > 1 => pid = i.pbi_ppid as i32,
            _ => return false,
        }
    }
    false
}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
/// Runs the calling thread at the interactive QoS class: woken on a performance core at once when work
/// comes (as a default thread, a command handed to the engine's thread under load took ~0.1-0.2 ms).
pub(crate) fn set_interactive_qos() {
    // SAFETY: sets the calling thread's own QoS class; no memory is passed. A refusal leaves the default.
    unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0) };
}

// Linux: the same from /proc.

/// (ppid, pgrp) of process `pid` from /proc/<pid>/stat, or None if there is no such process.
#[cfg(target_os = "linux")]
fn stat(pid: i32) -> Option<(i32, i32)> {
    if pid <= 0 {
        return None;
    }
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // "pid (comm) state ppid pgrp ...": comm may hold spaces and parentheses, so split after the last ')'.
    let mut f = s[s.rfind(')')? + 1..].split_whitespace().skip(1);
    Some((f.next()?.parse().ok()?, f.next()?.parse().ok()?))
}

/// The name of process `pid` (its command, at most 15 bytes on Linux); empty if unknown.
#[cfg(target_os = "linux")]
pub(crate) fn name(pid: i32) -> String {
    if pid <= 0 {
        return String::new();
    }
    std::fs::read_to_string(format!("/proc/{pid}/comm")).map(|s| s.trim_end_matches('\n').to_owned()).unwrap_or_default()
}

/// Whether a live member of process group `pgid` is `root` or descends from it (see the macOS one).
#[cfg(target_os = "linux")]
pub(crate) fn group_under(pgid: i32, root: i32) -> bool {
    if pgid <= 0 {
        return false;
    }
    let Ok(dir) = std::fs::read_dir("/proc") else { return false };
    dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok()).any(|p| stat(p).is_some_and(|(_, g)| g == pgid) && descends(p, root))
}

/// Whether process `pid` is `root` or descends from it.
#[cfg(target_os = "linux")]
fn descends(mut pid: i32, root: i32) -> bool {
    for _ in 0..256 {
        if pid == root {
            return true;
        }
        match stat(pid) {
            Some((ppid, _)) if ppid > 1 => pid = ppid,
            _ => return false,
        }
    }
    false
}

/// The working directory of process `pid`; empty if unknown.
#[cfg(target_os = "linux")]
pub(crate) fn cwd(pid: i32) -> String {
    if pid <= 0 {
        return String::new();
    }
    std::fs::read_link(format!("/proc/{pid}/cwd")).map(|p| p.display().to_string()).unwrap_or_default()
}

/// Whether the peer of `s` runs as this process's user, or as root.
#[cfg(target_os = "linux")]
pub(crate) fn peer_is_same_user(s: &std::os::unix::net::UnixStream) -> bool {
    use std::os::fd::AsRawFd as _;
    // SAFETY: ucred is plain data; all-zero is a valid value.
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of_val(&cred) as libc::socklen_t;
    // SAFETY: getsockopt writes at most `len` bytes into `cred`, which has exactly that size.
    if unsafe { libc::getsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut cred as *mut libc::ucred).cast(), &mut len) } != 0 {
        return false;
    }
    // SAFETY: geteuid takes nothing and cannot fail.
    cred.uid == 0 || cred.uid == unsafe { libc::geteuid() }
}

/// Linux has no QoS classes: the default scheduling stands.
#[cfg(target_os = "linux")]
pub(crate) fn set_interactive_qos() {}

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

    #[test]
    fn tells_a_descendants_group() {
        use std::os::unix::process::CommandExt as _;
        let me = std::process::id() as i32;
        let mut child = std::process::Command::new("/bin/sleep").arg("5").process_group(0).spawn().unwrap();
        let job = child.id() as i32;
        assert!(super::group_under(job, me), "a child leading its own group");
        assert!(super::group_under(job, job), "the program's own group");
        assert!(!super::group_under(job, job + 1_000_000), "not under a stranger");
        assert!(!super::group_under(-5, me), "no such group");
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(!super::group_under(job, me), "gone");
        // A pipeline whose first command (the group's leader) ended: the rest still count.
        let mut sh = std::process::Command::new("/bin/sh").args(["-c", "sleep 5 & exit 0"]).process_group(0).spawn().unwrap();
        let group = sh.id() as i32;
        sh.wait().unwrap();
        assert!(!super::group_under(group, me), "the sleep was reparented away from us");
        let mut first = std::process::Command::new("true").process_group(0).spawn().unwrap();
        let group = first.id() as i32;
        let mut rest = std::process::Command::new("/bin/sleep").arg("5").process_group(group).spawn().unwrap();
        first.wait().unwrap();
        assert!(super::group_under(group, me), "a member outlives the leader");
        rest.kill().unwrap();
        rest.wait().unwrap();
    }
}
