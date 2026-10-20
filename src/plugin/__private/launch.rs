//! Plugins that start with the server: every program in the directory ZEROPTY_PLUGIN_DIR names is started
//! by `zeropty serve`, with ZEROPTY_SOCK naming the server, and started again whenever it exits. A program
//! attaches itself (`plugin-attach`), so the directory takes any language; a `.dylib` there is run by
//! `zeropty plugin-run` (crate::plugin::native).
//!
//! Opt-in: without ZEROPTY_PLUGIN_DIR the server starts nothing. A server that executes whatever lies in
//! a well-known directory would be a new, implicit way of running code for an existing command (`serve`,
//! and every `new-session` that starts a server); naming the directory is the user's consent to it.
//!
//! That is also how plugins survive `restart-server`: the old server's connections close, its plugin
//! programs see EOF and exit, and the new server starts its own once it holds the socket's path.
//!
//! The programs run as this user, like the server; a directory or a file that another user could write
//! is skipped (and said so on stderr), as tmux refuses a socket directory others can write.
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A program's first restart comes after this; each quick exit doubles it, up to RESTART_MAX.
const RESTART_MIN: Duration = Duration::from_millis(500);
const RESTART_MAX: Duration = Duration::from_secs(30);
/// A program that ran this long before it exited starts again after RESTART_MIN.
const RAN_WELL: Duration = Duration::from_secs(30);
/// A program's log (`<name>.log` in the directory) starts over past this.
const LOG_MAX: u64 = 1 << 20;

/// Process groups of the programs running, ended when the process exits (kill-server).
static RUNNING: Mutex<Vec<i32>> = Mutex::new(Vec::new());

extern "C" fn end_all() {
    // try_lock: exit may come while a supervisor holds it; the programs then see their connection close.
    if let Ok(groups) = RUNNING.try_lock() {
        for &g in groups.iter() {
            crate::sys::process::signal_group(g, libc::SIGTERM);
        }
    }
}

/// What to start, and how.
#[derive(Clone, Debug)]
pub struct Autostart {
    /// Where the programs are.
    pub dir: PathBuf,
    /// The socket they attach to (ZEROPTY_SOCK).
    pub sock: PathBuf,
    /// What runs a `.dylib` (`<runner> plugin-run <file>`): the zeropty binary. None: dylibs are skipped.
    pub runner: Option<PathBuf>,
    /// Wait for this path to be gone before starting anything: a server started by restart-server
    /// listens at `<socket>.new` until it takes the socket's place.
    pub after_gone: Option<PathBuf>,
}

/// The programs started, supervised on threads of their own. Dropping it stops them.
pub struct Launcher {
    stop: Arc<AtomicBool>,
    pub programs: Vec<PathBuf>,
}

impl Drop for Launcher {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        end_all();
    }
}

/// The plugin directory: ZEROPTY_PLUGIN_DIR made absolute, None when it is unset or empty (the default:
/// no plugin starts by itself).
pub fn configured_dir() -> Option<PathBuf> {
    let d = std::env::var_os("ZEROPTY_PLUGIN_DIR").filter(|d| !d.is_empty())?;
    std::path::absolute(PathBuf::from(d)).ok()
}

/// Whether only this user (or root) can change `meta`'s file: owned by us, not writable by group or others.
fn ours(meta: &std::fs::Metadata) -> bool {
    // SAFETY: geteuid takes no arguments and cannot fail.
    let me = unsafe { libc::geteuid() };
    (meta.uid() == me || meta.uid() == 0) && meta.mode() & 0o022 == 0
}

/// The programs in `dir`: executable files and `.dylib`s, by name, skipping dot files and what others
/// could have written.
pub fn programs(dir: &Path) -> Vec<PathBuf> {
    let Ok(meta) = std::fs::metadata(dir) else { return Vec::new() };
    if !ours(&meta) {
        eprintln!("zeropty: plugins: {} is writable by others: skipped", dir.display());
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
        .filter(|p| {
            let Ok(m) = std::fs::metadata(p) else { return false };
            let wanted = m.is_file() && (m.permissions().mode() & 0o111 != 0 || is_dylib(p));
            if wanted && !ours(&m) {
                eprintln!("zeropty: plugins: {} is writable by others: skipped", p.display());
                return false;
            }
            wanted
        })
        .collect();
    found.sort();
    found
}

fn is_dylib(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "dylib")
}

impl Autostart {
    /// Starts every program found (None if there is none), each on a supervising thread.
    pub fn spawn(self) -> Option<Launcher> {
        let mut found = programs(&self.dir);
        if self.runner.is_none() {
            found.retain(|p| !is_dylib(p));
        }
        if found.is_empty() {
            return None;
        }
        static AT_EXIT: std::sync::Once = std::sync::Once::new();
        // SAFETY: end_all is an extern "C" fn with no arguments, as atexit wants, and only signals.
        AT_EXIT.call_once(|| unsafe {
            libc::atexit(end_all);
        });
        let stop = Arc::new(AtomicBool::new(false));
        for p in &found {
            let (cfg, p, stop) = (self.clone(), p.clone(), stop.clone());
            let name = format!("zeropty-autostart-{}", p.file_name().unwrap_or_default().to_string_lossy());
            let _ = std::thread::Builder::new().name(name).spawn(move || supervise(&cfg, &p, &stop));
        }
        Some(Launcher { stop, programs: found })
    }
}

/// Sleeps `d` unless told to stop meanwhile; whether to go on.
fn pause(d: Duration, stop: &AtomicBool) -> bool {
    let end = Instant::now() + d;
    while Instant::now() < end {
        if stop.load(Relaxed) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50).min(end.saturating_duration_since(Instant::now())));
    }
    !stop.load(Relaxed)
}

/// Runs `program` until told to stop, starting it again when it exits (later each time it exits soon).
fn supervise(cfg: &Autostart, program: &Path, stop: &AtomicBool) {
    if let Some(gone) = &cfg.after_gone {
        while gone.exists() {
            if !pause(Duration::from_millis(50), stop) {
                return;
            }
        }
    }
    let mut delay = RESTART_MIN;
    // A program taken out of the directory is not started again.
    while !stop.load(Relaxed) && program.exists() {
        let started = Instant::now();
        let mut cmd = match &cfg.runner {
            Some(runner) if is_dylib(program) => {
                let mut c = std::process::Command::new(runner);
                c.arg("plugin-run").arg(program);
                c
            }
            _ => std::process::Command::new(program),
        };
        let mut log_name = program.file_name().unwrap_or_default().to_owned();
        log_name.push(".log");
        let log = log_file(&cfg.dir.join(log_name));
        cmd.env("ZEROPTY_SOCK", &cfg.sock).current_dir(&cfg.dir).stdin(std::process::Stdio::null()).process_group(0);
        if let Some(log) = log {
            if let Ok(err) = log.try_clone() {
                cmd.stderr(err);
            }
            cmd.stdout(log);
        }
        match crate::sys::fd::spawn(&mut cmd) {
            Ok(mut child) => {
                let group = child.id() as i32;
                RUNNING.lock().unwrap().push(group);
                let _ = child.wait();
                RUNNING.lock().unwrap().retain(|&g| g != group);
            }
            Err(e) => eprintln!("zeropty: plugins: {}: {e}", program.display()),
        }
        if started.elapsed() >= RAN_WELL {
            delay = RESTART_MIN;
        }
        if !pause(delay, stop) {
            return;
        }
        delay = (delay * 2).min(RESTART_MAX);
    }
}

/// The program's log, started over when it grew past LOG_MAX.
fn log_file(path: &Path) -> Option<std::fs::File> {
    let long = std::fs::metadata(path).is_ok_and(|m| m.len() > LOG_MAX);
    std::fs::OpenOptions::new().create(true).append(!long).write(true).truncate(long).open(path).ok()
}
