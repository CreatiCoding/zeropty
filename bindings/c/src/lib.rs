//! zeropty's engine for any language, through a C ABI (include/zeropty.h). Requests and replies are the JSON
//! lines of the socket protocol, so a binding needs only strings: {"argv": [...], "cwd": "..."} in,
//! {"code": 0, "out": "...", "err": "..."} out.
//!
//! Every function takes the pointers it is given as the header documents them (an engine from
//! zeropty_open*, NUL-terminated strings); each one is `unsafe` to call from Rust for that reason. No
//! panic crosses the ABI: each call catches its own and answers with a failure.
//!
//! The engine is reached only through `zeropty::embed::Frontend` (commands, listen, plugins and their
//! commands in flight) and, for a polled engine, `Polled`'s loop calls (fd, timeout, process).
//!
//! # Example
//!
//! In C, after `./build.sh` (it puts `zeropty.h` and `libzeropty_c.dylib` in `dist/`); the whole program is
//! [examples/c/hello.c](https://github.com/CreatiCoding/zeropty/blob/main/examples/c/hello.c):
//!
//! ```c
//! #include <stdio.h>
//! #include "zeropty.h"
//!
//! int main(void) {
//!   zeropty *zp = zeropty_open(NULL); /* or "{\"history\": 2000}" */
//!   if (!zp) return 1;
//!   char *reply = zeropty_command(zp, "{\"argv\":[\"new-session\",\"-d\",\"-s\",\"c\",\"exec cat\"],\"cwd\":\"/\"}");
//!   printf("%s\n", reply); /* {"code":0,"out":"","err":""} */
//!   zeropty_free(reply);
//!   zeropty_free(zeropty_command(zp, "{\"argv\":[\"kill-server\"],\"cwd\":\"/\"}"));
//!   zeropty_close(zp);
//!   return 0;
//! }
//! ```
//!
//! ```sh
//! cc examples/c/hello.c -Idist -Ldist -lzeropty_c -o hello && DYLD_LIBRARY_PATH=dist ./hello
//! ```

#![warn(missing_docs)]
#![deny(clippy::undocumented_unsafe_blocks)]

use std::ffi::{CStr, CString, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use zeropty::embed::{Embedded, Frontend, Polled};
use zeropty::engine::{Reply, fail};
use zeropty::plugin::__private::native;

#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn reply_string(r: &Reply) -> *mut c_char {
    let json = serde_json::to_string(r).unwrap_or_else(|_| r#"{"code":1,"out":"","err":"zeropty: reply\n"}"#.into());
    CString::new(json).map(CString::into_raw).unwrap_or(std::ptr::null_mut())
}

/// An engine with a thread of its own (zeropty_open), or run by the host's loop (zeropty_open_polled).
pub enum Zeropty {
    /// From zeropty_open: commands from any thread.
    Thread(Embedded),
    /// From zeropty_open_polled: the host's loop runs it (zeropty_fd, zeropty_timeout_ms, zeropty_process).
    Polled(Box<std::sync::Mutex<Polled>>),
}

impl Zeropty {
    /// The calls both kinds answer the same way.
    fn run<R>(&self, f: impl FnOnce(&dyn Frontend) -> R) -> R {
        match self {
            Zeropty::Thread(e) => f(e),
            Zeropty::Polled(p) => f(&**p),
        }
    }
}

/// The library's version ("0.1.0"), NUL-terminated and static: callers never free it. Matches
/// ZEROPTY_VERSION_STRING in the header it was built with.
#[unsafe(no_mangle)]
pub extern "C" fn zeropty_version() -> *const c_char {
    VERSION.as_ptr().cast()
}

const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");

/// Starts an engine. `options_json` may be NULL or {"history": rows, "size_dir": "path"}. NULL on failure.
///
/// # Safety
/// `options_json` is NULL or a NUL-terminated string.
///
/// ```c
/// zeropty *zp = zeropty_open("{\"history\": 2000}");
/// if (!zp) return 1;
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeropty_open(options_json: *const c_char) -> *mut Zeropty {
    // SAFETY: the caller's contract is open's.
    unsafe { open(options_json, false) }
}

/// Starts an engine with no thread of its own: the host calls zeropty_process when zeropty_fd is readable or
/// zeropty_timeout_ms passed. All calls from the host's loop thread.
///
/// # Safety
/// As zeropty_open.
///
/// ```c
/// zeropty *zp = zeropty_open_polled(NULL);
/// for (;;) {   /* the host's loop; a real one waits on its own fds too */
///   struct pollfd p = { zeropty_fd(zp), POLLIN, 0 };
///   poll(&p, 1, (int)zeropty_timeout_ms(zp));
///   zeropty_process(zp);
/// }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeropty_open_polled(options_json: *const c_char) -> *mut Zeropty {
    // SAFETY: the caller's contract is open's.
    unsafe { open(options_json, true) }
}

/// # Safety
/// `options_json` is NULL or a NUL-terminated string.
unsafe fn open(options_json: *const c_char, polled: bool) -> *mut Zeropty {
    catch_unwind(|| {
        let mut opts = zeropty::mimalloc::engine_options();
        if !options_json.is_null() {
            // SAFETY: not NULL (checked above), and NUL-terminated by the caller's contract.
            let text = unsafe { CStr::from_ptr(options_json) }.to_string_lossy();
            let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            if let Some(h) = v["history"].as_u64() {
                opts.history = h as usize;
            }
            if let Some(d) = v["size_dir"].as_str() {
                opts.size_dir = d.into();
            }
        }
        let a = if polled { Zeropty::Polled(Box::new(std::sync::Mutex::new(Polled::start(opts)))) } else { Zeropty::Thread(Embedded::start(opts)) };
        Box::into_raw(Box::new(a))
    })
    .unwrap_or(std::ptr::null_mut())
}

/// Runs one command; returns the reply as JSON, to be freed with zeropty_free. Never NULL for a valid engine.
///
/// # Safety
/// `zeropty` is NULL or from zeropty_open*, not closed; `request_json` is NULL or a NUL-terminated string.
///
/// ```c
/// char *r = zeropty_command(zp, "{\"argv\":[\"capture-pane\",\"-p\",\"-t\",\"=w:\"],\"cwd\":\"/\"}");
/// /* r: {"code":0,"out":"<the screen>\n","err":""} */
/// zeropty_free(r);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeropty_command(zeropty: *mut Zeropty, request_json: *const c_char) -> *mut c_char {
    if zeropty.is_null() || request_json.is_null() {
        return reply_string(&fail("zeropty: null argument\n"));
    }
    let r = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: not NULL (checked above), and NUL-terminated by the caller's contract.
        let text = unsafe { CStr::from_ptr(request_json) }.to_string_lossy();
        let v: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => return fail(format!("bad request: {e}\n")),
        };
        let argv: Vec<String> = v["argv"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default();
        let cwd = v["cwd"].as_str().unwrap_or("/");
        // SAFETY: not NULL (checked above), from zeropty_open* and not closed, by the caller's contract.
        unsafe { &*zeropty }.run(|h| h.command(&argv, cwd))
    }));
    reply_string(&r.unwrap_or_else(|_| fail("zeropty: internal error\n")))
}

/// Answers commands on a Unix socket at `path` too, as the zeropty server does: the zeropty CLI (ZEROPTY_SOCK=path),
/// attach and socket clients reach this engine's sessions. A polled engine serves it in zeropty_process.
/// 0, or -1 if it could not listen there (a server already answers on it, the path is too long...).
///
/// # Safety
/// As zeropty_command, with `path` a NUL-terminated string.
///
/// ```c
/// if (zeropty_listen(zp, "/tmp/my-app.sock") != 0) perror("zeropty_listen");
/// /* in a shell: ZEROPTY_SOCK=/tmp/my-app.sock zeropty attach -t w */
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeropty_listen(zeropty: *mut Zeropty, path: *const c_char) -> i32 {
    if zeropty.is_null() || path.is_null() {
        return -1;
    }
    let r = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: not NULL (checked above), and NUL-terminated by the caller's contract.
        let path = std::path::PathBuf::from(unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned());
        // SAFETY: not NULL (checked above), from zeropty_open* and not closed, by the caller's contract.
        unsafe { &*zeropty }.run(|h| h.listen(&path))
    }));
    match r {
        Ok(Ok(())) => 0,
        _ => -1,
    }
}

/// Frees a string zeropty_command returned.
///
/// # Safety
/// `s` is NULL or a string from zeropty_command, not freed yet.
///
/// ```c
/// zeropty_free(zeropty_command(zp, "{\"argv\":[\"kill-server\"],\"cwd\":\"/\"}")); /* each reply once */
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeropty_free(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: from CString::into_raw in reply_string (via zeropty_command), not freed yet.
        drop(unsafe { CString::from_raw(s) });
    }
}

/// Stops the engine's thread. Sessions are not killed: run "kill-server" first to end them.
///
/// # Safety
/// `zeropty` is NULL or from zeropty_open*, not closed yet; nothing uses it after.
///
/// ```c
/// zeropty_free(zeropty_command(zp, "{\"argv\":[\"kill-server\"],\"cwd\":\"/\"}")); /* end the sessions */
/// zeropty_close(zp); /* zp is gone: no call on it after this */
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeropty_close(zeropty: *mut Zeropty) {
    if !zeropty.is_null() {
        // SAFETY: from Box::into_raw in open, not closed yet, and not used after (the caller's contract).
        drop(unsafe { Box::from_raw(zeropty) });
    }
}

/// The polled engine behind `zeropty`, locked; None for an engine with its own thread.
///
/// # Safety
/// `zeropty` is NULL or from zeropty_open*, not closed, and stays so for `'a`.
unsafe fn polled<'a>(zeropty: *mut Zeropty) -> Option<std::sync::MutexGuard<'a, Polled>> {
    // SAFETY: the caller's contract: NULL (None) or a live engine for 'a.
    match unsafe { zeropty.as_ref() }? {
        Zeropty::Polled(p) => p.lock().ok(),
        Zeropty::Thread(_) => None,
    }
}

/// zeropty_open_polled's engine: the fd to watch for reading; -1 for an engine with its own thread.
///
/// # Safety
/// As zeropty_close, without closing it.
///
/// See zeropty_open_polled for the loop.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeropty_fd(zeropty: *mut Zeropty) -> i32 {
    // SAFETY: the caller's contract is polled's.
    unsafe { polled(zeropty) }.map_or(-1, |p| p.fd())
}

/// zeropty_open_polled's engine: milliseconds until zeropty_process is due even with zeropty_fd not readable.
///
/// # Safety
/// As zeropty_fd.
///
/// See zeropty_open_polled for the loop.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeropty_timeout_ms(zeropty: *mut Zeropty) -> i64 {
    // SAFETY: the caller's contract is polled's.
    unsafe { polled(zeropty) }.map_or(-1, |p| p.timeout_ms() as i64)
}

/// zeropty_open_polled's engine: handles what is waiting, without blocking.
///
/// # Safety
/// As zeropty_fd.
///
/// See zeropty_open_polled for the loop.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeropty_process(zeropty: *mut Zeropty) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: the caller's contract is polled's.
        if let Some(mut p) = unsafe { polled(zeropty) } {
            p.process();
        }
    }));
}

// ---------------------------------------------------------------- commands in flight

/// A command in flight (zeropty_command_start): a plugin's command, its reply picked up later.
pub struct ZeroptyCall(zeropty::plugin::Call);

/// A request's argv and cwd, or the reply saying why it is not one.
///
/// # Safety
/// `request_json` is a NUL-terminated string.
unsafe fn request(request_json: *const c_char) -> Result<(Vec<String>, String), Reply> {
    // SAFETY: NUL-terminated by the caller's contract.
    let text = unsafe { CStr::from_ptr(request_json) }.to_string_lossy();
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| fail(format!("bad request: {e}\n")))?;
    let argv = v["argv"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default();
    Ok((argv, v["cwd"].as_str().unwrap_or("/").to_owned()))
}

/// zeropty_command without waiting for a plugin: a plugin's command comes back in flight, anything else
/// (and a bad request) answered already. Poll it with zeropty_call_poll after zeropty_process (polled: the
/// plugin's reply turns zeropty_fd readable, and zeropty_timeout_ms counts the call's deadline). Free it
/// with zeropty_call_free. NULL only for a NULL argument.
///
/// # Safety
/// As zeropty_command.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeropty_command_start(zeropty: *mut Zeropty, request_json: *const c_char) -> *mut ZeroptyCall {
    if zeropty.is_null() || request_json.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: not NULL (checked above), and NUL-terminated by the caller's contract.
    let call = catch_unwind(AssertUnwindSafe(|| match unsafe { request(request_json) } {
        Err(r) => zeropty::plugin::Call::ready(r),
        // SAFETY: not NULL (checked above), from zeropty_open* and not closed, by the caller's contract.
        Ok((argv, cwd)) => unsafe { &*zeropty }.run(|h| h.command_start(&argv, &cwd)),
    }))
    .unwrap_or_else(|_| zeropty::plugin::Call::ready(fail("zeropty: internal error\n")));
    Box::into_raw(Box::new(ZeroptyCall(call)))
}

/// The reply JSON once it came (or the failure once the plugin stopped or the deadline passed), to be
/// freed with zeropty_free; NULL while it is pending. Gives the reply once.
///
/// # Safety
/// `call` is NULL or from zeropty_command_start, not freed yet.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeropty_call_poll(call: *mut ZeroptyCall) -> *mut c_char {
    // SAFETY: NULL or a live call, by the caller's contract.
    let Some(c) = (unsafe { call.as_ref() }) else { return reply_string(&fail("zeropty: null argument\n")) };
    match catch_unwind(AssertUnwindSafe(|| c.0.poll())) {
        Ok(Some(r)) => reply_string(&r),
        Ok(None) => std::ptr::null_mut(),
        Err(_) => reply_string(&fail("zeropty: internal error\n")),
    }
}

/// Milliseconds until the call's deadline (when zeropty_call_poll gives up on the plugin); -1 for NULL.
///
/// # Safety
/// As zeropty_call_poll.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeropty_call_due_ms(call: *mut ZeroptyCall) -> i64 {
    // SAFETY: NULL or a live call, by the caller's contract.
    unsafe { call.as_ref() }.map_or(-1, |c| c.0.due_ms() as i64)
}

/// Frees a call. One the plugin did not start on yet is then never run.
///
/// # Safety
/// `call` is NULL or from zeropty_command_start, not freed yet; nothing uses it after.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeropty_call_free(call: *mut ZeroptyCall) {
    if !call.is_null() {
        // SAFETY: from Box::into_raw in zeropty_command_start, not freed yet (the caller's contract).
        drop(unsafe { Box::from_raw(call) });
    }
}

// ---------------------------------------------------------------- plugins (zeropty::plugin)

/// What a C plugin's callback gets to run commands with, valid during the callback only.
pub struct ZeroptyHost(zeropty::plugin::Host);

/// A plugin's callback: `message_json` is one line of the plugin protocol (docs/PLUGINS.md):
/// {"start":{"api":1}}, an event {"event":...}, a call {"call":1,"argv":[...],"cwd":"/"}, or {"stop":true}.
/// For a call it returns the reply ({"code":0,"out":"...","err":""}) in memory from malloc, which zeropty
/// frees; for the rest, NULL.
pub type PluginFn = extern "C" fn(user: *mut c_void, message_json: *const c_char, host: *mut ZeroptyHost) -> *mut c_char;

/// A C callback (zeropty_add_plugin) or a plugin library (zeropty_load_plugin) as a Plugin.
struct CPlugin(native::Library);

impl CPlugin {
    fn send(&self, message: &str, host: &zeropty::plugin::Host) -> Option<String> {
        let mut h = ZeroptyHost(host.clone());
        self.0.send(message, &mut h as *mut ZeroptyHost as *mut c_void)
    }
}

impl zeropty::plugin::Plugin for CPlugin {
    fn manifest(&self) -> zeropty::plugin::Manifest {
        self.0.manifest().clone()
    }
    fn start(&mut self, host: &zeropty::plugin::Host) {
        self.send(&native::start_message(), host);
    }
    fn event(&mut self, event: &zeropty::plugin::Event, host: &zeropty::plugin::Host) {
        self.send(&event.to_json(), host);
    }
    fn command(&mut self, argv: &[String], cwd: &str, host: &zeropty::plugin::Host) -> Reply {
        let answer = self.send(&zeropty::plugin::__private::call_json(1, argv, cwd), host);
        self.0.reply(answer, &argv[0])
    }
    fn stop(&mut self, host: &zeropty::plugin::Host) {
        self.send("{\"stop\":true}", host);
    }
}

fn error_string(e: String) -> *mut c_char {
    CString::new(e).map(CString::into_raw).unwrap_or(std::ptr::null_mut())
}

/// Registers the plugin `p` makes on the engine: NULL, or why not (free with zeropty_free).
///
/// # Safety
/// `zeropty` is from zeropty_open*, not NULL and not closed.
unsafe fn add(zeropty: *mut Zeropty, p: impl FnOnce() -> Result<CPlugin, String>) -> *mut c_char {
    let r = catch_unwind(AssertUnwindSafe(|| {
        let p = p()?;
        // SAFETY: a live engine, by the caller's contract.
        unsafe { &*zeropty }.run(|h| h.add_plugin_boxed(Box::new(p))).map_err(String::from)
    }));
    match r {
        Ok(Ok(())) => std::ptr::null_mut(),
        Ok(Err(e)) => error_string(e),
        Err(_) => error_string("zeropty: internal error\n".into()),
    }
}

/// Adds a plugin written in C (or anything that can hand out a C function): `manifest_json` is
/// {"name": "x", "events": "session-created,output" (or a list), "commands": ["x-do"], "access": ["input"],
/// "api": 1}, and `callback` gets every message on the plugin's own thread, `user` passed back as it is.
/// NULL on success, else the error (free with zeropty_free).
///
/// # Safety
/// `zeropty` comes from zeropty_open / zeropty_open_polled and is not closed yet, `manifest_json` is a
/// NUL-terminated string (both checked for NULL), and `callback` keeps the contract in zeropty.h.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeropty_add_plugin(zeropty: *mut Zeropty, manifest_json: *const c_char, callback: Option<PluginFn>, user: *mut c_void) -> *mut c_char {
    let (Some(f), false, false) = (callback, zeropty.is_null(), manifest_json.is_null()) else { return error_string("zeropty: null argument\n".into()) };
    let make = || {
        // SAFETY: not NULL (checked above), and NUL-terminated by the caller's contract.
        let manifest = zeropty::plugin::Manifest::from_json(&unsafe { CStr::from_ptr(manifest_json) }.to_string_lossy())?;
        // SAFETY: the two function types differ only in the host pointer's type (ZeroptyHost here, void
        // in the library's table), which is passed through untouched: the same C ABI.
        let callback = unsafe { std::mem::transmute::<PluginFn, native::PluginFn>(f) };
        // SAFETY: zeropty.h states the callback's contract (any one thread at a time, replies from
        // malloc); the C caller vouches for it as for any callback it hands over.
        Ok(CPlugin(unsafe { native::Library::from_parts(manifest, callback, user) }))
    };
    // SAFETY: a live engine (checked for NULL above), by the caller's contract.
    unsafe { add(zeropty, make) }
}

extern "C" fn api_host_command(host: *mut c_void, request_json: *const c_char) -> *mut c_char {
    // SAFETY: the library got `host` in its callback and passes it back during that call, as zeropty.h
    // asks; the request is its NUL-terminated string.
    unsafe { zeropty_host_command(host.cast(), request_json) }
}

extern "C" fn api_free(s: *mut c_char) {
    // SAFETY: the library frees with this only what api_host_command returned.
    unsafe { zeropty_free(s) }
}

static API: native::Api =
    native::Api { api: zeropty::plugin::API_VERSION, size: std::mem::size_of::<native::Api>() as u32, host_command: api_host_command, free: api_free };

/// Loads a plugin library (one exporting zeropty_plugin_init, docs/PLUGINS.md) into this process and adds
/// it, as zeropty_add_plugin would. `zeropty plugin-run` runs the same library in a process of its own.
/// NULL on success, else the error (free with zeropty_free).
///
/// # Safety
/// `zeropty` comes from zeropty_open / zeropty_open_polled and is not closed yet, `path` is a
/// NUL-terminated string (both checked for NULL); loading the library runs its code.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeropty_load_plugin(zeropty: *mut Zeropty, path: *const c_char) -> *mut c_char {
    if zeropty.is_null() || path.is_null() {
        return error_string("zeropty: null argument\n".into());
    }
    // SAFETY: not NULL (checked above), and NUL-terminated by the caller's contract.
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned();
    // SAFETY: loading a library runs its code; the C caller chose to load this one (zeropty.h).
    let make = || unsafe { native::load(std::path::Path::new(&path), &API) }.map(CPlugin);
    // SAFETY: a live engine (checked for NULL above), by the caller's contract.
    unsafe { add(zeropty, make) }
}

/// From a plugin's callback: runs a command on the engine (as zeropty_command does) and returns the reply
/// JSON, to be freed with zeropty_free. Safe with either kind of engine (it goes through the engine's loop).
///
/// # Safety
/// `host` is the one the callback was given, used during that call only, and `request_json` a
/// NUL-terminated string (both checked for NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeropty_host_command(host: *mut ZeroptyHost, request_json: *const c_char) -> *mut c_char {
    if host.is_null() || request_json.is_null() {
        return reply_string(&fail("zeropty: null argument\n"));
    }
    // SAFETY: not NULL (checked above), and NUL-terminated by the caller's contract.
    let r = catch_unwind(AssertUnwindSafe(|| match unsafe { request(request_json) } {
        Err(r) => r,
        // SAFETY: the callback's host, alive during the callback (the caller's contract).
        Ok((argv, cwd)) => unsafe { &*host }.0.command(&argv, &cwd),
    }));
    reply_string(&r.unwrap_or_else(|_| fail("zeropty: internal error\n")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CREATED: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" {
        fn strdup(s: *const c_char) -> *mut c_char;
    }

    extern "C" fn callback(user: *mut c_void, message: *const c_char, host: *mut ZeroptyHost) -> *mut c_char {
        // SAFETY: what zeropty.h asks: engines from zeropty_open*, NUL-terminated strings, each
        // reply freed once, the host used during its callback only.
        unsafe {
            assert_eq!(user as usize, 42);
            let m: serde_json::Value = serde_json::from_str(&CStr::from_ptr(message).to_string_lossy()).unwrap();
            if m["event"] == "session-created" {
                CREATED.fetch_add(1, Ordering::SeqCst);
            }
            if m["call"].is_u64() {
                let req = CString::new(r##"{"argv":["list-sessions","-F","#{session_name}"],"cwd":"/"}"##).unwrap();
                let r = zeropty_host_command(host, req.as_ptr());
                let sessions: serde_json::Value = serde_json::from_str(&CStr::from_ptr(r).to_string_lossy()).unwrap();
                zeropty_free(r);
                let reply = serde_json::json!({ "code": 0, "out": format!("c sees {}", sessions["out"].as_str().unwrap()), "err": "" }).to_string();
                let reply = CString::new(reply).unwrap();
                return strdup(reply.as_ptr());
            }
            std::ptr::null_mut()
        }
    }

    fn command(a: *mut Zeropty, json: &str) -> serde_json::Value {
        // SAFETY: what zeropty.h asks: engines from zeropty_open*, NUL-terminated strings, each
        // reply freed once, the host used during its callback only.
        unsafe {
            let req = CString::new(json).unwrap();
            let r = zeropty_command(a, req.as_ptr());
            let v = serde_json::from_str(&CStr::from_ptr(r).to_string_lossy()).unwrap();
            zeropty_free(r);
            v
        }
    }

    #[test]
    fn a_c_plugin_hears_sessions_and_answers_its_command() {
        // SAFETY: what zeropty.h asks: engines from zeropty_open*, NUL-terminated strings, each
        // reply freed once, the host used during its callback only.
        unsafe {
            let a = zeropty_open(std::ptr::null());
            let m = CString::new(r#"{"name":"c-probe","events":["session-created"],"commands":["c-hello"]}"#).unwrap();
            let err = zeropty_add_plugin(a, m.as_ptr(), Some(callback), 42 as *mut c_void);
            assert!(err.is_null());
            // A built-in name is refused, with the reason.
            let bad = CString::new(r#"{"name":"c-bad","commands":["send-keys"]}"#).unwrap();
            let err = zeropty_add_plugin(a, bad.as_ptr(), Some(callback), 42 as *mut c_void);
            assert!(CStr::from_ptr(err).to_string_lossy().contains("built-in"));
            zeropty_free(err);
            assert_eq!(command(a, r#"{"argv":["new-session","-d","-s","cx","exec cat"],"cwd":"/"}"#)["code"], 0);
            let r = command(a, r#"{"argv":["c-hello"],"cwd":"/"}"#);
            assert_eq!(r["out"], "c sees cx\n", "{r}");
            let start = std::time::Instant::now();
            while CREATED.load(Ordering::SeqCst) == 0 {
                assert!(start.elapsed() < std::time::Duration::from_secs(5));
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            command(a, r#"{"argv":["kill-server"],"cwd":"/"}"#);
            zeropty_close(a);
        }
    }

    /// A polled engine's host starts a plugin command and goes on with its loop, polling the call.
    #[test]
    fn a_polled_host_starts_a_plugin_command() {
        // SAFETY: what zeropty.h asks: engines from zeropty_open*, NUL-terminated strings, each
        // reply freed once, the host used during its callback only.
        unsafe {
            let a = zeropty_open_polled(std::ptr::null());
            let m = CString::new(r#"{"name":"c-start","commands":["c-hello"]}"#).unwrap();
            assert!(zeropty_add_plugin(a, m.as_ptr(), Some(callback), 42 as *mut c_void).is_null());
            let req = CString::new(r#"{"argv":["c-hello"],"cwd":"/"}"#).unwrap();
            let call = zeropty_command_start(a, req.as_ptr());
            assert!(!call.is_null() && zeropty_call_due_ms(call) > 0);
            let start = std::time::Instant::now();
            let reply = loop {
                let mut fds = libc_pollfd { fd: zeropty_fd(a), events: 1, revents: 0 };
                poll(&mut fds, 1, zeropty_timeout_ms(a) as i32);
                zeropty_process(a);
                let r = zeropty_call_poll(call);
                if !r.is_null() {
                    let v: serde_json::Value = serde_json::from_str(&CStr::from_ptr(r).to_string_lossy()).unwrap();
                    zeropty_free(r);
                    break v;
                }
                assert!(start.elapsed() < std::time::Duration::from_secs(10));
            };
            // No session yet: the plugin saw an empty list through the engine.
            assert_eq!(reply["out"], "c sees ", "{reply}");
            zeropty_call_free(call);
            // An engine command (and a bad request) comes back answered.
            let req = CString::new(r#"{"argv":["list-sessions"],"cwd":"/"}"#).unwrap();
            let call = zeropty_command_start(a, req.as_ptr());
            let r = zeropty_call_poll(call);
            assert!(!r.is_null());
            zeropty_free(r);
            zeropty_call_free(call);
            let bad = CString::new("{").unwrap();
            let call = zeropty_command_start(a, bad.as_ptr());
            let r = zeropty_call_poll(call);
            assert!(CStr::from_ptr(r).to_string_lossy().contains("bad request"));
            zeropty_free(r);
            zeropty_call_free(call);
            zeropty_close(a);
        }
    }

    #[repr(C)]
    struct libc_pollfd {
        fd: i32,
        events: i16,
        revents: i16,
    }
    unsafe extern "C" {
        fn poll(fds: *mut libc_pollfd, n: u32, timeout: i32) -> i32;
    }

    /// The plugin library example (examples/plugins/tally.c, zeropty.h only) loaded into this process.
    #[test]
    fn a_plugin_library_in_process() {
        // SAFETY: what zeropty.h asks: engines from zeropty_open*, NUL-terminated strings, each
        // reply freed once, the host used during its callback only.
        unsafe {
            let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
            let lib = std::env::temp_dir().join(format!("zp-tally-{}.dylib", std::process::id()));
            let built = std::process::Command::new("cc")
                .args(["-dynamiclib", "-O2", "-I", &format!("{root}/bindings/c/include"), "-o"])
                .arg(&lib)
                .arg(format!("{root}/examples/plugins/tally.c"))
                .status()
                .is_ok_and(|s| s.success());
            if !built {
                // As tests/plugin.rs's skip: ZEROPTY_REQUIRE_ALL=1 requires it to run.
                assert!(
                    !std::env::var("ZEROPTY_REQUIRE_ALL").is_ok_and(|v| v == "1"),
                    "no C compiler (cc), and ZEROPTY_REQUIRE_ALL=1 requires every test to run"
                );
                // Straight to stderr: `eprintln!` is captured, and lost, for a passing test.
                use std::io::Write as _;
                let _ = writeln!(std::io::stderr(), "SKIPPED a_plugin_library_in_process: no C compiler (cc) (set ZEROPTY_REQUIRE_ALL=1 to fail instead)");
                return;
            }
            let a = zeropty_open(std::ptr::null());
            let path = CString::new(lib.to_str().unwrap()).unwrap();
            let err = zeropty_load_plugin(a, path.as_ptr());
            assert!(err.is_null(), "{}", CStr::from_ptr(err).to_string_lossy());
            // Not a plugin library: refused with the reason.
            let not = CString::new("/usr/lib/libz.dylib").unwrap();
            let err = zeropty_load_plugin(a, not.as_ptr());
            assert!(CStr::from_ptr(err).to_string_lossy().contains("zeropty_plugin_init"));
            zeropty_free(err);
            assert_eq!(command(a, r#"{"argv":["new-session","-d","-s","t1","exec cat"],"cwd":"/"}"#)["code"], 0);
            let start = std::time::Instant::now();
            loop {
                let r = command(a, r#"{"argv":["tally"],"cwd":"/"}"#);
                if r["out"] == "created 1, ended 0, live 1\n" {
                    break;
                }
                assert!(start.elapsed() < std::time::Duration::from_secs(5), "{r}");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            command(a, r#"{"argv":["kill-server"],"cwd":"/"}"#);
            zeropty_close(a);
            let _ = std::fs::remove_file(&lib);
        }
    }

    /// The header's ZEROPTY_VERSION_* macros describe this library (scripts/release.mjs keeps them in step).
    #[test]
    fn header_matches_library_version() {
        let header = include_str!("../include/zeropty.h");
        let define = |name: &str| -> String {
            let line = header.lines().find(|l| l.starts_with(&format!("#define {name} "))).unwrap_or_else(|| panic!("{name} missing"));
            line[format!("#define {name} ").len()..].trim().trim_matches('"').to_owned()
        };
        let v = env!("CARGO_PKG_VERSION");
        let core = v.split(['-', '+']).next().unwrap();
        let parts: Vec<&str> = core.split('.').collect();
        assert_eq!(define("ZEROPTY_VERSION_STRING"), v);
        assert_eq!([define("ZEROPTY_VERSION_MAJOR"), define("ZEROPTY_VERSION_MINOR"), define("ZEROPTY_VERSION_PATCH")], [parts[0], parts[1], parts[2]]);
        // SAFETY: zeropty_version returns a static NUL-terminated string.
        let runtime = unsafe { std::ffi::CStr::from_ptr(super::zeropty_version()) };
        assert_eq!(runtime.to_str().unwrap(), v);
        assert!(header.contains("const char *zeropty_version(void);"));
    }
}
