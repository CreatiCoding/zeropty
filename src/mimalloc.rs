//! Settings for hosts whose global allocator is mimalloc (the zeropty server and both bindings): the
//! engine frees most of its memory in bursts (screens frozen, compressed), and mimalloc keeps freed pages
//! unless told otherwise.

/// mi_option_purge_delay: 15 in mimalloc v2's and v3's mimalloc.h (between eager_commit_delay = 14 and
/// use_numa_nodes = 16); libmimalloc-sys does not export it.
const MI_OPTION_PURGE_DELAY: libmimalloc_sys::mi_option_t = 15;

/// Freed pages go back to the OS after `ms` (0: at once). Screens are unpacked while busy and packed when
/// quiet, and a burst of new sessions otherwise leaves its peak behind (400 idle sessions kept 1.3 MB of
/// their parsing peak with mimalloc's default).
///
/// ```
/// // In a host whose #[global_allocator] is mimalloc::MiMalloc, before it starts the engine:
/// zeropty::mimalloc::purge_after_ms(0);
/// ```
pub fn purge_after_ms(ms: std::ffi::c_long) {
    // SAFETY: sets one of mimalloc's options by its number; no memory is passed.
    unsafe { libmimalloc_sys::mi_option_set(MI_OPTION_PURGE_DELAY, ms) };
}

/// Has mimalloc give back what it can now: for `engine::Options::after_compress` (screens just
/// compressed freed most of their bytes; without this mimalloc v2 kept the pages: 400 idle sessions at
/// 2.1 MB, against 1.8 MB with v3).
///
/// ```
/// use zeropty::engine::Options;
///
/// let opts = Options { after_compress: Some(zeropty::mimalloc::collect), ..Options::default() };
/// # drop(opts);
/// ```
pub fn collect() {
    // SAFETY: mi_collect only returns this thread's (and with `true`, abandoned) free pages to the OS.
    unsafe { libmimalloc_sys::mi_collect(true) };
}

/// Engine options for an embedding host whose global allocator is mimalloc, set up as the zeropty server
/// sets itself up: freed pages go back to the OS at once, and pages compressed screens freed are
/// collected (without it, 400 idle sessions kept 1.3 MB of their parsing peak).
///
/// ```
/// use zeropty::embed::Embedded;
///
/// // With `#[global_allocator] static A: mimalloc::MiMalloc = mimalloc::MiMalloc;` in the host:
/// let zp = Embedded::start(zeropty::mimalloc::engine_options());
/// # drop(zp);
/// ```
pub fn engine_options() -> crate::engine::Options {
    purge_after_ms(0);
    crate::engine::Options { after_compress: Some(collect), ..crate::engine::Options::default() }
}
