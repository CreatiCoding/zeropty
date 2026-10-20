//! Plugins as native libraries: a `.dylib` (C, or anything that exports C functions) built against
//! nothing but `zeropty.h`, loaded through one documented entry point,
//!
//! ```c
//! int zeropty_plugin_init(const zeropty_plugin_api *api, zeropty_plugin_def *def);
//! ```
//!
//! which fills `def` with the manifest (JSON, as for `zeropty_add_plugin`), the callback and its `user`
//! pointer, and keeps `api` for `api->host_command` and `api->free`. The same library runs two ways:
//!
//! - `zeropty plugin-run lib.dylib` ([`run`]): in a process of its own, attached to the server's socket
//!   like any other program (`plugin-attach`). A crash in the library takes only that process; dropped
//!   into the plugin directory (crate::plugin::launch) it starts with the server and again after a crash.
//! - `zeropty_load_plugin` (the C ABI, bindings/c): in the embedding process, on a plugin thread of its
//!   own, for hosts that want it close (no isolation from crashes then: it is their code).
//!
//! The callback gets the same JSON messages either way: {"start":{"api":1}}, events, calls
//! {"call":7,"argv":[...],"cwd":"/"} to be answered with a reply from malloc, and {"stop":true}.
//!
//! Not part of the stable plugin API (hidden from the docs): this is how zeropty's binary and its C
//! bindings run plugin libraries. Loading one runs its code, so [`load`], [`run`] and
//! [`Library::from_parts`] are `unsafe`: the caller vouches for the library.
use crate::plugin::{API_VERSION, Manifest, Reply, fail};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::io::{BufRead as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::Path;

/// The name of a plugin library's entry point.
pub const ENTRY: &str = "zeropty_plugin_init";

/// A plugin's callback (zeropty_plugin_fn): the message, and the host to run commands through during it.
pub type PluginFn = extern "C" fn(user: *mut c_void, message_json: *const c_char, host: *mut c_void) -> *mut c_char;

/// What a plugin library gets from its host (zeropty_plugin_api). `size` is this struct's size, so that
/// fields added at the end in later versions can be told apart from what an older host passes.
#[repr(C)]
pub struct Api {
    pub api: u32,
    pub size: u32,
    /// Runs one command ({"argv":[...],"cwd":"/"}) for the plugin, from inside its callback; the reply
    /// JSON is freed with `free`.
    pub host_command: extern "C" fn(host: *mut c_void, request_json: *const c_char) -> *mut c_char,
    pub free: extern "C" fn(s: *mut c_char),
}

/// What a plugin library gives back (zeropty_plugin_def).
#[repr(C)]
pub struct Def {
    /// Its manifest as JSON, valid as long as the library is loaded.
    pub manifest_json: *const c_char,
    pub callback: Option<PluginFn>,
    pub user: *mut c_void,
}

type InitFn = extern "C" fn(api: *const Api, def: *mut Def) -> c_int;

/// A plugin library, loaded (never unloaded: its code may run until the process ends), or a C callback.
pub struct Library {
    manifest: Manifest,
    callback: PluginFn,
    user: *mut c_void,
}

// SAFETY: made only through `load` or `from_parts`, whose callers vouch (docs/PLUGINS.md asks it of
// plugin libraries) that the callback may be called from any one thread at a time with this `user`.
unsafe impl Send for Library {}

impl Library {
    /// A plugin from its parts (zeropty_add_plugin).
    ///
    /// # Safety
    ///
    /// `callback` must be safe to call with `user`, any NUL-terminated message and a host pointer, from
    /// any one thread at a time, for as long as the Library lives; what it returns must be NULL or a
    /// NUL-terminated string from malloc(3).
    pub unsafe fn from_parts(manifest: Manifest, callback: PluginFn, user: *mut c_void) -> Self {
        Self { manifest, callback, user }
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
}

/// Loads `path` and calls its entry point with `api`.
///
/// # Safety
///
/// This runs the library's code (its initializers and `zeropty_plugin_init`) in this process, and makes
/// its callback callable through the Library: the library must be a zeropty plugin library as
/// docs/PLUGINS.md describes it (the `zeropty_plugin_init` signature of zeropty.h, a manifest string that
/// lives as long as the library, a callback meeting `from_parts`' contract). zeropty cannot check this:
/// it is the trust of running a program.
pub unsafe fn load(path: &Path, api: &'static Api) -> Result<Library, String> {
    let c_path = CString::new(path.as_os_str().as_encoded_bytes()).map_err(|_| "plugin: bad path\n".to_string())?;
    // SAFETY: a NUL-terminated path; loading runs the library's initializers, which our caller vouched for.
    let handle = unsafe { libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
    if handle.is_null() {
        return Err(format!("plugin {}: {}\n", path.display(), dl_error()));
    }
    let entry = CString::new(ENTRY).unwrap();
    // SAFETY: `handle` is a library dlopen just returned, `entry` a NUL-terminated name.
    let sym = unsafe { libc::dlsym(handle, entry.as_ptr()) };
    if sym.is_null() {
        return Err(format!("plugin {}: no {ENTRY} (docs/PLUGINS.md)\n", path.display()));
    }
    // SAFETY: the caller vouches that the symbol is zeropty_plugin_init as zeropty.h declares it.
    let init: InitFn = unsafe { std::mem::transmute::<*mut c_void, InitFn>(sym) };
    let mut def = Def { manifest_json: std::ptr::null(), callback: None, user: std::ptr::null_mut() };
    let code = init(api, &mut def);
    if code != 0 {
        return Err(format!("plugin {}: {ENTRY} returned {code}\n", path.display()));
    }
    let (Some(callback), false) = (def.callback, def.manifest_json.is_null()) else {
        return Err(format!("plugin {}: {ENTRY} gave no manifest or no callback\n", path.display()));
    };
    // SAFETY: not NULL (checked above); zeropty.h has the library keep a NUL-terminated manifest alive.
    let manifest = Manifest::from_json(&unsafe { CStr::from_ptr(def.manifest_json) }.to_string_lossy())?;
    Ok(Library { manifest, callback, user: def.user })
}

fn dl_error() -> String {
    // SAFETY: dlerror takes no arguments; it returns NULL or a NUL-terminated message.
    let e = unsafe { libc::dlerror() };
    if e.is_null() {
        return "dlopen failed".into();
    }
    // SAFETY: not NULL, and NUL-terminated (dlerror's contract), valid until the next dl call.
    unsafe { CStr::from_ptr(e) }.to_string_lossy().into_owned()
}

impl Library {
    /// One message to the callback, with `host`; its answer, if any (freed here with free(3)).
    pub fn send(&self, message: &str, host: *mut c_void) -> Option<String> {
        let message = CString::new(message).ok()?;
        // Sound by the contract of `from_parts` / `load`, the only ways to make a Library.
        let r = (self.callback)(self.user, message.as_ptr(), host);
        if r.is_null() {
            return None;
        }
        // SAFETY: not NULL; the callback's contract (from_parts) makes it a NUL-terminated string from malloc.
        let s = unsafe { CStr::from_ptr(r) }.to_string_lossy().into_owned();
        // SAFETY: from malloc (as above), ours now, and not used after.
        unsafe { libc::free(r.cast()) };
        Some(s)
    }

    /// The callback's answer to a call, as a reply.
    pub fn reply(&self, answer: Option<String>, cmd: &str) -> Reply {
        match answer.map(|r| serde_json::from_str::<serde_json::Value>(&r)) {
            Some(Ok(v)) => crate::plugin::wire::reply_from_json(&v),
            Some(Err(e)) => fail(format!("{cmd}: bad reply from plugin: {e}\n")),
            None => fail(format!("{cmd}: no reply from plugin\n")),
        }
    }
}

/// The start message.
pub fn start_message() -> String {
    format!("{{\"start\":{{\"api\":{API_VERSION}}}}}")
}

// ---------------------------------------------------------------- plugin-run

/// The connection of `zeropty plugin-run`: one thread, like examples/plugins/hello.py. While a command of
/// the plugin's waits for its reply, what else comes is kept for after.
struct Conn {
    out: UnixStream,
    lines: std::io::BufReader<UnixStream>,
    /// Messages kept for after, each with its line as it came (passed on as is: key order matters, and
    /// serde_json's Value does not keep it).
    backlog: std::collections::VecDeque<(serde_json::Value, String)>,
    next_id: u64,
}

impl Conn {
    fn send(&mut self, v: &serde_json::Value) -> std::io::Result<()> {
        let mut line = serde_json::to_vec(v)?;
        line.push(b'\n');
        self.out.write_all(&line)
    }

    /// The next message: kept ones first. None at the end of the connection.
    fn next(&mut self) -> Option<(serde_json::Value, String)> {
        if let Some(v) = self.backlog.pop_front() {
            return Some(v);
        }
        self.read()
    }

    fn read(&mut self) -> Option<(serde_json::Value, String)> {
        let mut line = String::new();
        loop {
            line.clear();
            match self.lines.read_line(&mut line) {
                Ok(0) | Err(_) => return None,
                Ok(_) => {
                    if let Ok(v) = serde_json::from_str(&line) {
                        let len = line.trim_end().len();
                        line.truncate(len);
                        return Some((v, line));
                    }
                }
            }
        }
    }

    /// Runs a command on the server and waits for its reply.
    fn command(&mut self, request: &serde_json::Value) -> serde_json::Value {
        let id = self.next_id;
        self.next_id += 1;
        let msg = serde_json::json!({ "id": id, "argv": request["argv"], "cwd": request["cwd"].as_str().unwrap_or("/") });
        if self.send(&msg).is_err() {
            return serde_json::json!({ "code": 1, "out": "", "err": "zeropty: lost server\n" });
        }
        loop {
            match self.read() {
                Some((v, _)) if v["id"].as_u64() == Some(id) && v.get("call").is_none() => return v,
                Some(kept) => self.backlog.push_back(kept),
                None => return serde_json::json!({ "code": 1, "out": "", "err": "zeropty: lost server\n" }),
            }
        }
    }
}

extern "C" fn run_host_command(host: *mut c_void, request_json: *const c_char) -> *mut c_char {
    let reply = if host.is_null() || request_json.is_null() {
        serde_json::json!({ "code": 1, "out": "", "err": "zeropty: null argument\n" })
    } else {
        // SAFETY: `host` is the Conn `run` passes to the callback, alive for the whole run; the callback
        // runs on run's thread while run does not hold a reference to it (it reaches it through `host` too).
        let conn = unsafe { &mut *(host as *mut Conn) };
        // SAFETY: not NULL (checked above), NUL-terminated by zeropty.h's contract for host_command.
        let text = unsafe { CStr::from_ptr(request_json) }.to_string_lossy();
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(req) => conn.command(&req),
            Err(e) => serde_json::json!({ "code": 1, "out": "", "err": format!("bad request: {e}\n") }),
        }
    };
    // In the order of every other reply: code, out, err.
    let reply = serde_json::to_string(&crate::plugin::wire::reply_from_json(&reply)).unwrap_or_default();
    CString::new(reply).map(CString::into_raw).unwrap_or(std::ptr::null_mut())
}

extern "C" fn run_free(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: zeropty.h has the library free with this only what run_host_command returned (CString::into_raw).
        drop(unsafe { CString::from_raw(s) });
    }
}

static RUN_API: Api = Api { api: API_VERSION, size: std::mem::size_of::<Api>() as u32, host_command: run_host_command, free: run_free };

/// `zeropty plugin-run lib.dylib`: loads the library, attaches it to the server at `sock` and serves it
/// until the server goes away. Err: why it could not start.
///
/// # Safety
///
/// As `load`: this runs the library's code.
pub unsafe fn run(path: &Path, sock: &Path) -> Result<(), String> {
    // SAFETY: passed on to our caller.
    let lib = unsafe { load(path, &RUN_API)? };
    let stream = crate::sys::fd::connect(sock).map_err(|e| format!("no server running on {}: {e}\n", sock.display()))?;
    let mut conn =
        Conn { out: stream.try_clone().map_err(|e| e.to_string())?, lines: std::io::BufReader::new(stream), backlog: Default::default(), next_id: 1 };
    conn.send(&serde_json::json!({ "argv": lib.manifest.attach_argv(), "cwd": "/" })).map_err(|e| format!("zeropty: {e}\n"))?;
    let (attached, _) = conn.read().ok_or("zeropty: lost server\n")?;
    if attached["code"] != 0 {
        return Err(attached["err"].as_str().unwrap_or("plugin-attach failed\n").to_owned());
    }
    let host = &mut conn as *mut Conn as *mut c_void;
    lib.send(&start_message(), host);
    // `conn` is reached through `host` from inside the callback as well, so only through it from here on.
    // SAFETY: `host` points at `conn`, alive here; no other reference to it is live meanwhile (the
    // callback's ends with its call).
    while let Some((msg, line)) = unsafe { &mut *(host as *mut Conn) }.next() {
        if msg.get("event").is_some() {
            lib.send(&line, host);
        } else if let Some(id) = msg["call"].as_u64() {
            let cmd = msg["argv"][0].as_str().unwrap_or("").to_owned();
            let r = lib.reply(lib.send(&line, host), &cmd);
            let reply = serde_json::json!({ "reply": id, "code": r.code, "out": r.out, "err": r.err });
            // SAFETY: as above.
            if unsafe { &mut *(host as *mut Conn) }.send(&reply).is_err() {
                break;
            }
        }
    }
    lib.send("{\"stop\":true}", host);
    Ok(())
}
