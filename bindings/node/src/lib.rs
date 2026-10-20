//! zeropty's engine in a Node process (N-API). client/embedded.mjs wraps it in the same `zeropty(argv)` call
//! as client/zeropty.mjs, so a program switches between the server and the embedded engine with an import.
//!
//! - `Engine`: an embedded engine, polled by Node's event loop through libuv handles (the default) or on
//!   a thread of its own.
//! - `Client`: a synchronous connection to a zeropty server.
//!
//! The engine is reached only through `zeropty::embed::Frontend` (commands, listen, a plugin's command
//! in flight) and `Polled`'s loop calls, driven by two libuv handles (`Looped`).
//!
//! # Example
//!
//! The module itself, after `./build.sh` (it writes `dist/zeropty.node`); most programs use
//! client/embedded.mjs instead, as in
//! [examples/node/embedded.mjs](https://github.com/CreatiCoding/zeropty/blob/main/examples/node/embedded.mjs):
//!
//! ```js
//! import { createRequire } from 'node:module'
//! const { Engine } = createRequire(import.meta.url)('./dist/zeropty.node')
//!
//! const engine = new Engine({ history: 2000 }) // run by Node's loop; { polled: false } gives it a thread
//! const r = engine.command(['new-session', '-d', '-s', 'w', 'exec cat'], '/')
//! console.log(r) // { code: 0, out: '', err: '' }
//! console.log(engine.command(['list-sessions', '-F', '#{session_name}']).out) // 'w\n'
//! engine.command(['kill-server'])
//! ```

#![warn(missing_docs)]
#![deny(clippy::undocumented_unsafe_blocks)]

use napi::JsDeferred;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::cell::RefCell;
use std::ffi::c_void;
use zeropty::embed::{Embedded, Frontend, Polled};

#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// The engine's version ("0.1.0"), the same as `zeropty --version` and the C ABI's zeropty_version().
#[napi]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// A command's reply, as the CLI's exit code and output.
#[napi(object)]
pub struct Reply {
    /// The exit code: 0 for success.
    pub code: i32,
    /// Standard output.
    pub out: String,
    /// Standard error.
    pub err: String,
}

/// `new Engine(options)`: all optional.
#[napi(object)]
pub struct EngineOptions {
    /// Rows of scrollback per session.
    pub history: Option<u32>,
    /// Where resized sessions' window sizes are written for their programs.
    pub size_dir: Option<String>,
    /// Node's event loop runs the engine on the JS thread, with no thread of its own (the default):
    /// about 0.3 MB less (the thread's allocator heap, stack and page tables), and a burst of output is
    /// parsed a slice at a time between the loop's other work (it held the loop up to 6 ms where the
    /// engine's thread, contending for the CPU, held it up to 30 ms). `false` gives the engine a thread.
    pub polled: Option<bool>,
}

/// zeropty's engine inside this process.
#[napi]
pub struct Engine {
    inner: Inner,
}

enum Inner {
    Thread(Embedded),
    Polled(Box<Looped>),
}

impl Inner {
    /// The calls both kinds answer the same way.
    fn run<R>(&self, f: impl FnOnce(&dyn Frontend) -> R) -> R {
        match self {
            Inner::Thread(e) => f(e),
            Inner::Polled(l) => f(&**l),
        }
    }
}

#[napi]
impl Engine {
    /// `new Engine(options)`: an engine with no sessions, run by Node's loop unless `polled` is false.
    ///
    /// ```js
    /// const engine = new Engine({ history: 2000 })
    /// ```
    #[napi(constructor)]
    pub fn new(env: Env, options: Option<EngineOptions>) -> Result<Self> {
        let mut opts = zeropty::mimalloc::engine_options();
        let mut polled = true;
        if let Some(o) = options {
            if let Some(h) = o.history {
                opts.history = h as usize;
            }
            if let Some(d) = o.size_dir {
                opts.size_dir = d.into();
            }
            polled = o.polled.unwrap_or(true);
        }
        let inner = if polled { Inner::Polled(Looped::start(&env, Polled::start(opts))?) } else { Inner::Thread(Embedded::start(opts)) };
        Ok(Self { inner })
    }

    /// Runs one tmux-style command (`argv[0]` is the command), synchronously, with no server round trip
    /// (`send-keys` p50 0.04-0.06 ms from Node, measured in docs/BENCHMARKS.md).
    ///
    /// ```js
    /// const { code, out } = engine.command(['capture-pane', '-p', '-t', '=w:'])
    /// ```
    #[napi]
    pub fn command(&self, argv: Vec<String>, cwd: Option<String>) -> Reply {
        let cwd = cwd.as_deref().unwrap_or("/");
        let r = self.inner.run(|h| h.command(&argv, cwd));
        Reply { code: r.code, out: r.out, err: r.err }
    }

    /// The same without holding the JS thread while a plugin answers. Always a `Promise<Reply>`: an
    /// engine command's is already resolved (it ran synchronously, in microseconds), a plugin's command
    /// resolves when its reply comes (the engine goes on meanwhile; polled, the reply wakes Node's loop
    /// through the engine's fd). `command` instead waits for a plugin, up to its deadline: a plugin
    /// attached to this same process through `listen` cannot answer while the JS thread waits.
    #[napi(ts_return_type = "Promise<Reply>")]
    pub fn command_start<'env>(&self, env: &'env Env, argv: Vec<String>, cwd: Option<String>) -> Result<Object<'env>> {
        let cwd = cwd.as_deref().unwrap_or("/");
        let call = self.inner.run(|h| h.command_start(&argv, cwd));
        let (deferred, promise) = env.create_deferred::<Reply, Resolve>()?;
        // One shape for every command: callers never branch on Reply vs Promise.
        if let Some(r) = call.poll() {
            settle(deferred, r);
            return Ok(promise);
        }
        match &self.inner {
            // Waited for on a thread of its own (the engine's thread is not held either).
            // If no thread can be started the call is waited for here instead (up to the plugin's
            // deadline), so the Promise always settles.
            Inner::Thread(_) => {
                let slot = std::sync::Arc::new(std::sync::Mutex::new(Some((call, deferred))));
                let theirs = std::sync::Arc::clone(&slot);
                let spawned = std::thread::Builder::new().name("zeropty-call".into()).spawn(move || {
                    if let Some((call, deferred)) = theirs.lock().unwrap_or_else(|e| e.into_inner()).take() {
                        settle(deferred, call.wait());
                    }
                });
                let unsent = if spawned.is_err() { slot.lock().unwrap_or_else(|e| e.into_inner()).take() } else { None };
                if let Some((call, deferred)) = unsent {
                    settle(deferred, call.wait());
                }
            }
            // Polled after each turn of the engine (Looped::run).
            Inner::Polled(l) => l.pending.borrow_mut().push((call, deferred)),
        }
        Ok(promise)
    }

    /// Answers commands on a Unix socket at `path` too, as the zeropty server does: the zeropty CLI
    /// (`ZEROPTY_SOCK=path zeropty send-keys ...`), attach and socket clients reach this engine's sessions,
    /// with no server process. Polled, its connections are served on Node's loop.
    ///
    /// ```js
    /// engine.listen(`${process.env.HOME}/.zeropty/app.sock`) // then: ZEROPTY_SOCK=... zeropty attach -t w
    /// ```
    #[napi]
    pub fn listen(&self, path: String) -> Result<()> {
        let path = std::path::PathBuf::from(path);
        self.inner.run(|h| h.listen(&path)).map_err(|e| Error::from_reason(format!("zeropty: {e}")))
    }
}

// ---------------------------------------------------------------- the server, synchronously

/// A connection to an zeropty server, used synchronously: each command is written and its reply read on
/// the calling thread, blocking, with no event loop in between (client/zeropty.mjs's async socket waits
/// for libuv to wake the JS thread twice per command).
///
/// ```js
/// const { Client } = createRequire(import.meta.url)('./dist/zeropty.node')
/// const c = new Client(`${process.env.HOME}/.zeropty/mux.sock`)
/// console.log(c.command(['list-sessions']).out)
/// c.close()
/// ```
#[napi]
pub struct Client {
    sock: String,
    stream: Option<std::os::unix::net::UnixStream>,
    buf: Vec<u8>,
}

#[napi]
impl Client {
    /// `sock`: the server's socket path. Connects on the first command.
    #[napi(constructor)]
    pub fn new(sock: String) -> Self {
        Self { sock, stream: None, buf: Vec::new() }
    }

    /// Runs one command on the server; throws if there is no server (or it went away).
    #[napi]
    pub fn command(&mut self, argv: Vec<String>, cwd: Option<String>) -> Result<Reply> {
        let req = serde_json::json!({ "argv": argv, "cwd": cwd.as_deref().unwrap_or("/"), "raw": true });
        let mut line = req.to_string().into_bytes();
        line.push(b'\n');
        self.ask(&line).map_err(|e| {
            self.stream = None;
            Error::from_reason(format!("zeropty: {}: {e}", self.sock))
        })
    }

    /// Closes the connection (the next command opens another).
    #[napi]
    pub fn close(&mut self) {
        self.stream = None
    }
}

impl Client {
    fn ask(&mut self, line: &[u8]) -> std::io::Result<Reply> {
        use std::io::{Read as _, Write as _};
        // A connection the server closed meanwhile (restart-server, kill-server) reads as ended: replaced
        // before sending, never after (a command sent may have run; sending it again could run it twice).
        if let Some(s) = &self.stream {
            use std::os::fd::AsRawFd as _;
            let mut b = 0u8;
            // SAFETY: recv writes at most 1 byte into `b`, alive for the call; the fd is `s`'s, open.
            let n = unsafe { libc_recv(s.as_raw_fd(), (&mut b as *mut u8).cast(), 1, MSG_PEEK | MSG_DONTWAIT) };
            if n == 0 {
                self.stream = None;
            }
        }
        if self.stream.is_none() {
            self.stream = Some(std::os::unix::net::UnixStream::connect(&self.sock)?);
        }
        let s = self.stream.as_mut().unwrap();
        s.write_all(line)?;
        // "<code> <out bytes> <err bytes>\n", then out and err.
        self.buf.clear();
        let mut chunk = [0u8; 65536];
        let (mut head, mut need) = (None, usize::MAX);
        while self.buf.len() < need {
            let n = s.read(&mut chunk)?;
            if n == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            self.buf.extend_from_slice(&chunk[..n]);
            if head.is_none()
                && let Some(nl) = self.buf.iter().position(|&b| b == b'\n')
            {
                let text = std::str::from_utf8(&self.buf[..nl]).map_err(std::io::Error::other)?;
                let v: Vec<i64> = text.split(' ').filter_map(|x| x.parse().ok()).collect();
                let [code, out, err] = v[..] else { return Err(std::io::Error::other("bad reply")) };
                head = Some((code as i32, nl + 1, out as usize));
                need = nl + 1 + out as usize + err as usize;
            }
        }
        let (code, at, out) = head.unwrap();
        let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        Ok(Reply { code, out: text(&self.buf[at..at + out]), err: text(&self.buf[at + out..need]) })
    }
}

// ---------------------------------------------------------------- libuv, from the Node binary

const UV_READABLE: i32 = 1;
const UV_POLL: i32 = 8;
const UV_TIMER: i32 = 13;

unsafe extern "C" {
    fn uv_handle_size(kind: i32) -> usize;
    fn uv_poll_init(lp: *mut c_void, handle: *mut c_void, fd: i32) -> i32;
    fn uv_poll_start(handle: *mut c_void, events: i32, cb: extern "C" fn(*mut c_void, i32, i32)) -> i32;
    fn uv_timer_init(lp: *mut c_void, handle: *mut c_void) -> i32;
    fn uv_timer_start(handle: *mut c_void, cb: extern "C" fn(*mut c_void), timeout: u64, repeat: u64) -> i32;
    fn uv_unref(handle: *mut c_void);
    fn uv_close(handle: *mut c_void, cb: extern "C" fn(*mut c_void));
}

type Resolve = Box<dyn FnOnce(Env) -> Result<Reply>>;

/// Resolves a commandStart promise with `r` (from any thread).
fn settle(d: JsDeferred<Reply, Resolve>, r: zeropty::engine::Reply) {
    d.resolve(Box::new(move |_| Ok(Reply { code: r.code, out: r.out, err: r.err })))
}

/// A polled engine with the libuv handles that run it: a poll on its fd and a timer for its timeout.
/// Neither keeps Node running (as the engine's thread does not); a pending commandStart does, through
/// its promise.
struct Looped {
    engine: RefCell<Polled>,
    poll: *mut c_void,
    timer: *mut c_void,
    /// commandStart calls of plugins' commands, polled after each turn.
    pending: RefCell<Vec<(zeropty::plugin::Call, JsDeferred<Reply, Resolve>)>>,
}

/// A libuv handle, zeroed, with `data` (its first field) pointing at the engine.
fn handle(kind: i32, data: *const Looped) -> *mut c_void {
    // SAFETY: uv_handle_size only looks up the size of a handle type.
    let size = unsafe { uv_handle_size(kind) };
    // SAFETY: calloc returns a zeroed block of `size` bytes, or NULL.
    let h = unsafe { libc_calloc(1, size) };
    assert!(!h.is_null(), "zeropty: out of memory for a libuv handle");
    // SAFETY: every libuv handle starts with its `data` pointer, and the block is at least a handle.
    unsafe { *(h as *mut *const Looped) = data };
    h
}

const MSG_PEEK: i32 = 0x2;
const MSG_DONTWAIT: i32 = 0x80;

unsafe extern "C" {
    #[link_name = "recv"]
    fn libc_recv(fd: i32, buf: *mut c_void, len: usize, flags: i32) -> isize;
    #[link_name = "calloc"]
    fn libc_calloc(n: usize, size: usize) -> *mut c_void;
    #[link_name = "free"]
    fn libc_free(p: *mut c_void);
}

impl Looped {
    fn start(env: &Env, engine: Polled) -> Result<Box<Self>> {
        let lp = env.get_uv_event_loop()? as *mut c_void;
        let mut l =
            Box::new(Looped { engine: RefCell::new(engine), poll: std::ptr::null_mut(), timer: std::ptr::null_mut(), pending: RefCell::new(Vec::new()) });
        let me: *const Looped = &*l;
        l.poll = handle(UV_POLL, me);
        l.timer = handle(UV_TIMER, me);
        let fd = l.engine.borrow().fd();
        // SAFETY: `lp` is Node's loop; the handles are zeroed blocks of their types' sizes, owned by `l`
        // until Drop closes them; the callbacks have libuv's signatures.
        unsafe {
            if uv_poll_init(lp, l.poll, fd) != 0 || uv_timer_init(lp, l.timer) != 0 {
                return Err(Error::from_reason("zeropty: libuv handles"));
            }
            uv_poll_start(l.poll, UV_READABLE, on_poll);
            uv_unref(l.poll);
            uv_unref(l.timer);
        }
        l.arm();
        Ok(l)
    }

    /// The timer, set to when the engine is next due on its own.
    fn arm(&self) {
        let ms = self.engine.borrow().timeout_ms();
        // SAFETY: `timer` was initialized in start and is not closed before Drop.
        unsafe { uv_timer_start(self.timer, on_timer, ms, 0) };
    }

    fn run(&self) {
        self.engine.borrow_mut().process();
        let mut pending = self.pending.borrow_mut();
        if !pending.is_empty() {
            let mut i = 0;
            while i < pending.len() {
                match pending[i].0.poll() {
                    Some(r) => settle(pending.swap_remove(i).1, r),
                    None => i += 1,
                }
            }
        }
        drop(pending);
        self.arm();
    }
}

/// Each call runs on the JS thread, then sets the timer to when the engine is next due.
impl Frontend for Looped {
    fn command(&self, argv: &[String], cwd: &str) -> zeropty::engine::Reply {
        let r = self.engine.borrow_mut().command(argv, cwd);
        self.arm();
        r
    }

    fn listen(&self, path: &std::path::Path) -> std::io::Result<()> {
        let r = self.engine.borrow_mut().listen(path);
        self.arm();
        r
    }

    fn command_start(&self, argv: &[String], cwd: &str) -> zeropty::plugin::Call {
        let call = self.engine.borrow_mut().command_start(argv, cwd);
        // Due by the call's deadline at the latest.
        self.arm();
        call
    }

    fn add_plugin_boxed(&self, plugin: Box<dyn zeropty::plugin::Plugin>) -> std::result::Result<(), zeropty::plugin::PluginError> {
        let r = self.engine.borrow_mut().add_plugin_boxed(plugin);
        self.arm();
        r
    }
}

impl Drop for Looped {
    fn drop(&mut self) {
        for h in [self.poll, self.timer] {
            // SAFETY: both handles are initialized and open; clearing `data` first means a callback
            // libuv still runs finds no engine, and on_close frees the block once libuv is done with it.
            unsafe {
                *(h as *mut *const Looped) = std::ptr::null();
                uv_close(h, on_close);
            }
        }
    }
}

/// The engine behind a handle, until Drop clears it.
fn looped<'a>(h: *mut c_void) -> Option<&'a Looped> {
    // SAFETY: `h` is one of our handles (libuv passes it to its callbacks); its `data` is NULL or the
    // boxed Looped, which outlives the handle's open life (Drop clears `data` before closing it).
    unsafe { (*(h as *mut *const Looped)).as_ref() }
}

extern "C" fn on_poll(h: *mut c_void, _status: i32, _events: i32) {
    if let Some(l) = looped(h) {
        l.run();
    }
}

extern "C" fn on_timer(h: *mut c_void) {
    if let Some(l) = looped(h) {
        l.run();
    }
}

extern "C" fn on_close(h: *mut c_void) {
    // SAFETY: libuv is done with the handle once its close callback runs; the block came from calloc.
    unsafe { libc_free(h) }
}
