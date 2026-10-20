//! What a host chooses for its engine (`Options`), and the engine's timing and size constants, each with
//! the measurement that set it.

use std::path::PathBuf;

/// How long a session's output waits after its pipe turns readable, to take a burst in one read.
/// Claude redraws about ten times a second, each redraw one write; every read unpacks and repacks the
/// rows it touches, so fewer reads is less work (400 claude sessions: 5.2% CPU at 100 ms, 3.7% at
/// 300 ms). capture reads whatever is pending first, so what a caller reads is never late; and a pipe
/// filling up (BURST_LOWAT) is read at once, so a program printing a lot is not held back.
const COALESCE_MS_DEFAULT: u64 = 300;

/// A burst (see COALESCE_MS_DEFAULT) ends at a drain that finds less than this.
pub(super) const BURST_END: usize = 4096;

/// Sessions read per pass of the event loop; see Engine::tick_until.
pub(super) const DRAIN_BATCH: usize = 32;

/// While a read waits for its coalesce boundary, this much output waiting in the pipe has it read at once.
pub(super) const BURST_LOWAT: u32 = 16 * 1024;

/// ZEROPTY_COALESCE_MS, else COALESCE_MS_DEFAULT.
pub(super) fn coalesce_ms() -> u64 {
    static V: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("ZEROPTY_COALESCE_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(COALESCE_MS_DEFAULT))
}

/// Quiet this long, a session's whole screen (its Term, palette and all) is frozen into bytes until its
/// next output or capture. Most of a thousand sessions are quiet. A busy one stays a Term, but its rows
/// are packed again after every read (Term::pack_live).
const FREEZE_AFTER_MS_DEFAULT: u64 = 250;

/// ZEROPTY_FREEZE_MS, else FREEZE_AFTER_MS_DEFAULT.
pub(super) fn freeze_after_ms() -> u64 {
    static V: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var("ZEROPTY_FREEZE_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(FREEZE_AFTER_MS_DEFAULT))
}

/// How often quiet screens are looked for (frozen, compressed).
pub(super) const SWEEP_EVERY_MS: u64 = 250;

/// The longest a session's hold grows to: output more than this apart is treated as separate bursts.
pub(super) const HOLD_MAX_MS: u64 = 4000;

/// The longest hold. A session printing every second or two is frozen in between: since rows are packed
/// after every read, freezing costs little (400 sessions printing a claude screen every 2 s: 3.3 MB with
/// holds up to 4 s, 2.1 MB with 800 ms, same CPU). Above the coalesce interval plus jitter, so a session
/// read every 300 ms is not frozen between reads.
pub(super) const HOLD_CAP_MS: u64 = 800;

/// Quiet this long, a packed screen is also compressed (zstd): about half the bytes again, for the
/// sessions that sit idle, at the cost of a decompression when they next print or are captured.
pub(super) const DEEPEN_AFTER_MS: u64 = 10_000;

/// Quiet this long with every screen frozen, the engine frees its scratch and has the allocator give
/// memory back (Options::after_compress).
pub(super) const IDLE_TRIM_MS: u64 = 2000;

/// After kill-session's SIGHUP, whatever is left of the process group gets SIGKILL this much later.
pub(super) const KILL_GRACE_MS: u64 = 2000;

/// The most read ahead of parsing at once; a program printing more waits for the parsing past this.
pub(super) const SLURP_MAX: usize = 64 << 20;
/// Read ahead in steps of this.
pub(super) const SLURP_STEP: usize = 256 << 10;
/// Read ahead this much, an empty pipe is waited on briefly (SLURP_QUIET_MS) before parsing.
pub(super) const SLURP_WAIT_FROM: usize = 64 << 10;
pub(super) const SLURP_QUIET_MS: i32 = 1;

/// pipe-pane: output its command has not taken yet is held up to this; past it, the rest is dropped (a
/// stuck command must not grow the engine without end).
pub(super) const PIPE_PENDING_MAX: usize = 8 << 20;

/// The longest line held while the tty is canonical.
pub(super) const LINE_MAX: usize = 64 << 10;

/// What the host gives the engine.
///
/// # Example
///
/// Start from the defaults (`ZEROPTY_HISTORY` and `ZEROPTY_RECORD` are read there) and change what you
/// need:
///
/// ```
/// use zeropty::{embed::Embedded, engine::Options};
///
/// let zp = Embedded::start(Options { history: 2000, ..Options::default() });
/// # drop(zp);
/// ```
pub struct Options {
    /// Scrollback rows kept per screen.
    pub history: usize,
    /// Keep each session's output as `<name>.raw` and `<name>.trace` here (tests, benchmarks).
    pub record_dir: Option<PathBuf>,
    /// Called after screens were compressed (the zeropty server returns freed pages to the OS there).
    pub after_compress: Option<fn()>,
    /// Bytes allocated, if the host's allocator counts them (shown by `stats`).
    pub heap_bytes: Option<fn() -> usize>,
    /// Where a resized session's window size is written for its program to read (shim/shim.c).
    pub size_dir: PathBuf,
    /// The most a burst is read ahead of parsing at once (SLURP_MAX). A host whose own loop runs the
    /// engine (embed::Polled) takes less, so a program printing megabytes is parsed in slices between
    /// the host's own work instead of holding its loop (5.4 MB at once: 29-32 ms in Node).
    pub slurp_max: usize,
}

impl Default for Options {
    /// ZEROPTY_HISTORY rows of scrollback (500), ZEROPTY_RECORD for `record_dir`, sizes in the
    /// temporary directory.
    fn default() -> Self {
        Self {
            history: std::env::var("ZEROPTY_HISTORY").ok().and_then(|v| v.parse().ok()).unwrap_or(500),
            record_dir: std::env::var("ZEROPTY_RECORD").ok().map(PathBuf::from),
            after_compress: None,
            heap_bytes: None,
            size_dir: std::env::temp_dir(),
            slurp_max: SLURP_MAX,
        }
    }
}
