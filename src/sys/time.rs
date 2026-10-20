//! Time as the engine counts it: milliseconds since the process first asked (monotonic, for timers and
//! quiet spells), and wall-clock seconds (#{session_created}).

use std::time::Instant;

/// Milliseconds since the first call in this process; never goes back.
pub(crate) fn now_ms() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Seconds since 1970.
pub(crate) fn unix_now() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as u32)
}

/// Nanoseconds since 1970, to tell this process's files from an earlier one's with the same pid.
pub(crate) fn unix_nanos() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)
}

/// `secs` as ctime gives it ("Sat Oct 18 20:45:00 2026"), in local time, as tmux shows session_created.
pub(crate) fn ctime(secs: u32) -> String {
    let t = secs as libc::time_t;
    let mut buf = [0 as libc::c_char; 32];
    // SAFETY: ctime_r writes at most 26 bytes (NUL included) into `buf`, which holds 32.
    let p = unsafe { libc::ctime_r(&t, buf.as_mut_ptr()) };
    if p.is_null() {
        return String::new();
    }
    // SAFETY: `p` is `buf`, NUL-terminated by ctime_r, alive until the end of this function.
    unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn clocks() {
        let a = super::now_ms();
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert!(super::now_ms() >= a + 5);
        assert!(super::unix_now() > 1_700_000_000);
        let c = super::ctime(super::unix_now());
        assert_eq!(c.split_whitespace().count(), 5, "{c}");
    }
}
