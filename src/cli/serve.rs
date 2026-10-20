//! `zeropty serve`: the server process. One thread and one kqueue, like tmux: the engine's sessions and,
//! tagged HOST_TAG, the server's socket and connections, and the plugins' wake-ups (programs attach as
//! plugins through the socket; with ZEROPTY_PLUGIN_DIR set, those in that directory are started).

use super::{heap, sock_path};
use std::path::PathBuf;
use zeropty::engine::{Engine, Options, Reply, ok};
use zeropty::host::{HostLoop, Source, Waiting, Yield};
use zeropty::plugin::Plugins;
use zeropty::server::{Hooks, Server};

/// The system allocator's settings for the server, read by libmalloc only as a process starts. It is
/// left with what libSystem and the kernel interfaces allocate (sessions' screens are mimalloc's), and
/// these trim its zones and per-CPU magazines to one thread's worth: 144-240 KB less at every load
/// (1 idle session 1233 -> 1089 KB, 400 replaying claude 3.0 -> 2.8 MB).
const MALLOC_ENV: [(&str, &str); 4] = [("MallocNanoZone", "0"), ("MallocMaxMagazines", "1"), ("MallocSpaceEfficient", "1"), ("MallocMediumZone", "0")];
/// Set on the server's re-exec: the MALLOC_ENV names it added, taken out again so sessions' programs
/// (multithreaded ones like claude) start with the environment they would have had.
const MALLOC_ADDED: &str = "ZEROPTY_MALLOC_ADDED";

/// Starts this server again with MALLOC_ENV, unless it already was; then takes back out of the
/// environment what was added.
fn malloc_settings() {
    match std::env::var(MALLOC_ADDED) {
        Ok(added) => {
            // SAFETY: still one thread (the server's loop and engine are not started yet), so nothing reads
            // the environment concurrently.
            unsafe {
                for k in added.split(',').filter(|k| !k.is_empty()) {
                    std::env::remove_var(k);
                }
                std::env::remove_var(MALLOC_ADDED);
            }
        }
        Err(_) => {
            use std::os::unix::process::CommandExt as _;
            let added: Vec<&str> = MALLOC_ENV.iter().filter(|(k, _)| std::env::var_os(k).is_none()).map(|(k, _)| *k).collect();
            let Ok(exe) = std::env::current_exe() else { return };
            let mut cmd = std::process::Command::new(exe);
            cmd.args(std::env::args_os().skip(1)).env(MALLOC_ADDED, added.join(","));
            for (k, v) in MALLOC_ENV.iter().filter(|(k, _)| added.contains(k)) {
                cmd.env(k, v);
            }
            // Returns only if the exec failed: the server then runs with the allocator as it is.
            let _ = cmd.exec();
        }
    }
}

/// The server; with `bind`, listening there instead of at sock_path() (restart-server moves it there).
pub fn run(bind: Option<PathBuf>) {
    malloc_settings();
    // Freed pages back to the OS at once (ZEROPTY_PURGE_MS to delay it).
    zeropty::mimalloc::purge_after_ms(std::env::var("ZEROPTY_PURGE_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(0));

    let path = bind.unwrap_or_else(sock_path);
    let engine = Engine::try_new(Options {
        size_dir: path.parent().map(PathBuf::from).unwrap_or_else(std::env::temp_dir),
        after_compress: Some(zeropty::mimalloc::collect),
        heap_bytes: Some(heap::bytes),
        ..Options::default()
    });
    let mut engine = match engine {
        Ok(e) => e,
        Err(e) => {
            eprintln!("zeropty: kqueue: {e}");
            return;
        }
    };
    let hooks = Hooks { command: Some(|argv| (argv.first().map(String::as_str) == Some("mstats")).then(mstats)), exit_on_kill: true };
    let mut srv = match Server::bind(&engine, &path, hooks) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("zeropty: {e}");
            return;
        }
    };
    // Programs attach as plugins through the socket (plugin-attach); with none, the loop pays a branch.
    let plugins = Plugins::new();
    if let Err(e) = plugins.attach(&engine) {
        // Plugins still work, woken by the loop's watchdog turns instead of at once.
        eprintln!("zeropty: plugins' wake-up: {e}");
    }
    srv.set_plugins(plugins.clone());
    // The plugin directory's programs, only when ZEROPTY_PLUGIN_DIR names one: they attach through the
    // socket, so they start once the server holds its final path (at once, or after restart-server's
    // rename).
    let final_path = sock_path();
    let _launcher = zeropty::plugin::__private::launch::configured_dir().and_then(|dir| {
        zeropty::plugin::__private::launch::Autostart {
            dir,
            sock: final_path.clone(),
            runner: std::env::current_exe().ok(),
            after_gone: (path != final_path).then(|| path.clone()),
        }
        .spawn()
    });
    // One thread waits on the engine and serves the socket: the changes ride along with each wait.
    let mut host = HostLoop::new(Some(srv), Waiting::WithEngine);
    host.set_plugins(plugins);
    loop {
        if let Err(e) = host.turn(&mut engine, Source::Wait(None), Yield::Never, |_, _, _| {}) {
            // The queue itself failed (not one of its changes): said, then a pause, not a busy loop.
            // The sessions are kept: a queue that recovers (out of memory for a moment) loses nothing.
            eprintln!("zeropty: event queue: {e}");
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

/// mimalloc's own counters and the live allocations by size, for finding where memory goes.
fn mstats() -> Reply {
    /// mimalloc's output callback: `arg` is the `String` passed to mi_stats_print_out below.
    unsafe extern "C" fn collect(msg: *const std::ffi::c_char, arg: *mut std::ffi::c_void) {
        // SAFETY: `arg` is `&mut out` from mstats, alive and not otherwise borrowed while mimalloc prints.
        let out = unsafe { &mut *(arg as *mut String) };
        // SAFETY: mimalloc passes a NUL-terminated message, valid for the duration of the callback.
        out.push_str(&unsafe { std::ffi::CStr::from_ptr(msg) }.to_string_lossy());
    }
    let mut out = String::from("live allocations by size (<= bytes: count):");
    for (size, count) in heap::live_by_size() {
        out.push_str(&format!(" {size}:{count}"));
    }
    out.push('\n');
    // SAFETY: `collect` matches mimalloc's output callback type and only appends to `out`, which outlives
    // the call.
    unsafe {
        libmimalloc_sys::mi_stats_print_out(Some(collect), &mut out as *mut String as *mut std::ffi::c_void);
    }
    ok(out)
}
