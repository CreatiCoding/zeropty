//! The tmux-style commands: one table (`COMMANDS`) of names, flags and handlers, and the handlers. Every
//! front end (CLI, socket, embedded library) reaches them through `Engine::command`, so a command line
//! means the same everywhere.
//!
//! wait-pane and attach are not here: they last, so a host runs them (Engine::wait_start, Engine::attach).

use super::args::{Args, parse};
use super::backend::{Backend, Clock as _, Poller as _, Programs as _, Spawn};
use super::format::{expand, only_server_vars, reads_screen, without_session};
use super::options::KILL_GRACE_MS;
use super::render::{self, Capture};
use super::reply::{CmdError, Reply, fail, ok};
use super::session::{PanePipe, Recorder, Screenful, Sess, Started};
use super::{Engine, Lookup};
use crate::sys::kq::{Change, Interest};
use crate::sys::time::{ctime, unix_now};
use crate::term::{MAX_SIZE, TTY_COOKED, Term};
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// A command as it reaches its handler: its name as typed, its flags, and the caller's directory.
struct Call<'a> {
    name: &'a str,
    args: Args,
    cwd: &'a str,
}

/// What a handler returns: the reply, or why the command failed.
type Outcome = Result<Reply, CmdError>;

/// One command of engine type `E`: the names it answers to, its flags, and its handler.
struct Command<E> {
    names: &'static [&'static str],
    /// Its flags as a getopt spec (see super::args::parse); None: its arguments are not read.
    flags: Option<&'static str>,
    /// It may run on a host's own thread holding the engine (Engine::command_fast): reading a screen,
    /// sending keys and lookups, with no reading-ahead or allocating that would belong on the engine's.
    fast: bool,
    run: fn(&mut E, Call) -> Outcome,
}

/// capture-pane's flags. -q (quiet) changes nothing here: zeropty reports no errors it would silence.
const CAPTURE_SPEC: &str = "b:eE:JNpqS:t:";

impl<B: Backend> Engine<B> {
    /// Every command, by name. A const of the engine's type, so each engine type (the real one, the tests'
    /// fakes) gets its own table of plain function pointers.
    const COMMANDS: &'static [Command<Self>] = &[
        // The most frequent first: the table is searched in order.
        Command { names: &["capture-pane", "capturep"], flags: Some(CAPTURE_SPEC), fast: true, run: |e, c| e.capture(c) },
        Command { names: &["send-keys", "send"], flags: Some("HlN:t:"), fast: true, run: |e, c| e.send_keys(c) },
        Command { names: &["has-session", "has"], flags: Some("t:"), fast: true, run: |e, c| e.find(c.args.get('t')).map(|_| ok("")) },
        // -n (window name), -E, -D, -X: about windows, the environment update list and attached clients,
        // none of which a session here has; accepted so tmux command lines run.
        Command { names: &["new-session", "new"], flags: Some("AdDEPXc:e:F:n:s:x:y:"), fast: false, run: |e, c| e.new_session(c) },
        Command { names: &["kill-session"], flags: Some("aCt:"), fast: false, run: |e, c| e.kill_session(c) },
        Command { names: &["display-message", "display"], flags: Some("pF:t:"), fast: true, run: |e, c| e.display(c) },
        Command { names: &["load-buffer", "loadb"], flags: Some("b:"), fast: false, run: |e, c| e.load_buffer(c) },
        Command { names: &["paste-buffer", "pasteb"], flags: Some("b:dprs:t:"), fast: true, run: |e, c| e.paste_buffer(c) },
        Command { names: &["list-sessions", "ls"], flags: Some("F:"), fast: true, run: |e, c| Ok(e.list(c)) },
        Command { names: &["stats"], flags: None, fast: false, run: |e, _| Ok(e.stats_reply()) },
        Command { names: &["grep-panes"], flags: Some("S:"), fast: false, run: |e, c| Ok(e.grep_panes(c)) },
        Command { names: &["pipe-pane", "pipep"], flags: Some("Oot:"), fast: false, run: |e, c| e.pipe_pane(c) },
        Command { names: &["rename-session", "rename"], flags: Some("t:"), fast: false, run: |e, c| e.rename_session(c) },
        Command { names: &["resize-window", "resizew", "resize-pane", "resizep"], flags: Some("t:x:y:"), fast: false, run: |e, c| e.resize(c) },
        Command { names: &["set-environment", "setenv"], flags: Some("gut:"), fast: false, run: |e, c| e.set_environment(c) },
        Command { names: &["show-environment", "showenv"], flags: Some("gt:"), fast: false, run: |e, c| e.show_environment(c) },
        // A host answers it later (wait_start, wait_check): here it would hold the engine meanwhile.
        Command { names: &["wait-pane"], flags: None, fast: false, run: |_, _| Err(CmdError::new("wait-pane: the host waits (Engine::wait_start)\n")) },
        Command { names: &["mem"], flags: Some("t:"), fast: false, run: |e, c| e.mem(c) },
        Command { names: &["pull-sessions", "push-sessions"], flags: Some("s:"), fast: false, run: |e, c| e.handover(c) },
        // The host decides whether it ends too (the zeropty server exits).
        Command {
            names: &["kill-server"],
            flags: None,
            fast: false,
            run: |e, _| {
                e.hangup_all();
                Ok(ok(""))
            },
        },
    ];

    fn lookup(name: &str) -> Option<&'static Command<Self>> {
        Self::COMMANDS.iter().find(|c| c.names.contains(&name))
    }

    /// Whether `name` is one of the table's names, aliases included: crate::plugin::is_reserved keeps
    /// them from plugins, read from here so that a command added to the table is reserved with it.
    pub(crate) fn is_command(name: &str) -> bool {
        Self::lookup(name).is_some()
    }

    /// Every name in the table, aliases included (for the tests tying the reserved names to it).
    #[cfg(test)]
    pub(crate) fn command_names() -> impl Iterator<Item = &'static str> {
        Self::COMMANDS.iter().flat_map(|c| c.names.iter().copied())
    }

    /// Runs one tmux-style command (`argv[0]` is the command), with relative paths taken from `cwd`.
    ///
    /// # Example
    ///
    /// ```
    /// use zeropty::engine::{Engine, Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut e = Engine::new(Options::default());
    /// // -c is taken from cwd when relative; the session's program runs there.
    /// let r = e.command(&argv(&["new-session", "-d", "-s", "doc-cmd", "-x", "30", "-y", "3", "-c", ".", "exec cat"]), "/tmp");
    /// assert_eq!(r.code, 0, "{}", r.err);
    ///
    /// let r = e.command(&argv(&["display-message", "-p", "-t", "=doc-cmd:", "#{session_name} #{window_width}x#{window_height}"]), "/");
    /// assert_eq!(r.out, "doc-cmd 30x3\n");
    ///
    /// assert_eq!(e.command(&argv(&["kill-session", "-t", "=doc-cmd"]), "/").code, 0);
    /// let r = e.command(&argv(&["has-session", "-t", "=doc-cmd"]), "/");
    /// assert_eq!((r.code, r.err.as_str()), (1, "can't find session: doc-cmd\n"));
    /// ```
    pub fn command(&mut self, argv: &[String], cwd: &str) -> Reply {
        let Some((name, rest)) = argv.split_first() else { return fail("no command\n") };
        match Self::lookup(name) {
            Some(c) => self.run(c, name, rest, cwd),
            None => fail(format!("unknown command: {name}\n")),
        }
    }

    /// Command `c`, typed as `name` with arguments `rest`.
    fn run(&mut self, c: &Command<Self>, name: &str, rest: &[String], cwd: &str) -> Reply {
        let args = match c.flags {
            Some(spec) => match parse(name, rest, spec) {
                Ok(a) => a,
                Err(e) => return e.into(),
            },
            None => Args { rest: rest.to_vec(), ..Args::default() },
        };
        (c.run)(self, Call { name, args, cwd }).unwrap_or_else(Reply::from)
    }

    /// The commands a host may run on its own thread, holding the engine, without waking the engine's:
    /// reading a screen (and the output pending for it), sending keys, and lookups. None: run it on the
    /// engine's thread (Engine::command), where the reading, parsing and allocating are done.
    ///
    /// # Example
    ///
    /// ```
    /// use zeropty::engine::{Engine, Options};
    ///
    /// let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    /// let mut e = Engine::new(Options::default());
    /// assert!(e.command_fast(&argv(&["new-session", "-d", "-s", "doc-fast", "exec cat"]), "/").is_none());
    /// e.command(&argv(&["new-session", "-d", "-s", "doc-fast", "exec cat"]), "/");
    /// let r = e.command_fast(&argv(&["has-session", "-t", "=doc-fast"]), "/").expect("a lookup is fast");
    /// assert_eq!(r.code, 0);
    /// e.command(&argv(&["kill-server"]), "/");
    /// ```
    pub fn command_fast(&mut self, argv: &[String], cwd: &str) -> Option<Reply> {
        let (name, rest) = argv.split_first()?;
        // A capture reads the output not read yet itself: handing it to the engine's thread cost its
        // wake-up, ~0.1 ms, which busy sessions nearly always paid.
        let c = Self::lookup(name).filter(|c| c.fast)?;
        Some(self.run(c, name, rest, cwd))
    }

    fn new_session(&mut self, c: Call) -> Outcome {
        let a = &c.args;
        let name = a.get('s').map(str::to_owned).unwrap_or_else(|| self.names.count().to_string());
        if let Some(slot) = self.names.find(&name, &self.slots) {
            // -A: there already is one, which is what was wanted (tmux attaches to it; -d: nothing more).
            if a.has('A') {
                return Ok(self.created(slot, a));
            }
            return Err(CmdError::new(format!("duplicate session: {name}\n")));
        }
        let cols: u16 = a.get('x').and_then(|v| v.parse().ok()).unwrap_or(80);
        let rows: u16 = a.get('y').and_then(|v| v.parse().ok()).unwrap_or(24);
        // What the screen allows; the program is told the same size.
        let (rows, cols) = (rows.clamp(1, MAX_SIZE), cols.clamp(1, MAX_SIZE));
        let command = if a.rest.is_empty() { std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()) } else { a.rest.join(" ") };
        let generation = self.next_generation();
        let size_file = self.size_file(generation);
        let program = Spawn {
            command: &command,
            dir: a.get('c').unwrap_or(c.cwd),
            name: &name,
            rows,
            cols,
            size_file: &size_file,
            env: &self.env,
            extra_env: a.all('e').collect(),
        };
        let run = self.programs.spawn(&program).map_err(CmdError::new)?;

        let mut term = Term::new(rows, cols, self.opts.history);
        term.set_onlcr(true);
        term.set_tty(TTY_COOKED);
        let mut s = Sess::new(
            Started {
                generation,
                name: name.as_str().into(),
                pid: run.pid,
                stdin: Some(run.stdin),
                out: Some(run.out),
                size: (rows, cols),
                base: (rows, cols),
                created: unix_now(),
                term,
            },
            self.clock.now_ms(),
        );
        s.record = self.opts.record_dir.as_ref().and_then(|d| Recorder::open(d, &name));
        let slot = self.insert(s);
        if !self.watch_session(slot) {
            // Already gone: its exit event will never come.
            let reply = self.created(slot, a);
            self.remove(slot);
            return Ok(reply);
        }
        Ok(self.created(slot, a))
    }

    /// new-session's reply: with -P, the session described by -F (tmux's default "#{session_name}:").
    fn created(&self, slot: usize, a: &Args) -> Reply {
        if !a.has('P') {
            return ok("");
        }
        ok(format!("{}\n", self.expand(slot, a.get('F').unwrap_or("#{session_name}:"))))
    }

    /// kill-session [-a] -t target: the session, or with -a every other one.
    fn kill_session(&mut self, c: Call) -> Outcome {
        let target = self.find(c.args.get('t'))?;
        let slots: Vec<usize> = if c.args.has('a') { self.slots.used().filter(|&i| i != target).collect() } else { vec![target] };
        for slot in slots {
            let pgid = self.sess(slot).pid;
            // tmux sends SIGHUP; anything still alive after the grace period is killed outright.
            self.programs.signal_group(pgid, libc::SIGHUP);
            self.kills.push((self.clock.now_ms() + KILL_GRACE_MS, pgid));
            self.remove_as(slot, super::tap::ExitReason::Killed);
        }
        Ok(ok(""))
    }

    /// send-keys [-H] [-l] [-N count] -t target key...: key names (Enter, C-c, F5, M-x ...) or text, -l
    /// all as text, -H each as a byte in hexadecimal; -N sends the whole lot that many times.
    fn send_keys(&mut self, c: Call) -> Outcome {
        let a = &c.args;
        let slot = self.find_as(a.get('t'), Lookup::Pane)?;
        let repeat: usize = match a.get('N') {
            None => 1,
            Some(n) => n.parse().ok().filter(|&n| n > 0).ok_or_else(|| CmdError::new(format!("send-keys: repeat count invalid: {n}\n")))?,
        };
        self.modes_now(slot);
        let app_cursor = self.sess(slot).app_cursor;
        let mut once = vec![];
        for k in &a.rest {
            if a.has('H') {
                let b = u8::from_str_radix(k, 16).map_err(|_| CmdError::new(format!("send-keys: invalid hex byte: {k}\n")))?;
                once.push(b);
            } else if a.has('l') {
                once.extend_from_slice(k.as_bytes());
            } else {
                once.extend(super::keys::key_bytes(k, app_cursor));
            }
        }
        self.type_keys(slot, &once.repeat(repeat))?;
        Ok(ok(""))
    }

    fn capture(&mut self, c: Call) -> Outcome {
        let slot = self.find_as(c.args.get('t'), Lookup::Pane)?;
        // Coalescing may have left output unread; a capture always sees all of it.
        self.drain_now(slot);
        self.read_now(slot);
        let text = self.render(slot, &Capture::from_args(&c.args))?;
        Ok(self.captured(text, &c.args))
    }

    /// Without -p, tmux keeps a capture in a paste buffer (-b, else buffer0) and prints nothing.
    fn captured(&mut self, text: String, a: &Args) -> Reply {
        if a.has('p') {
            return ok(text);
        }
        self.buffers.insert(a.get('b').unwrap_or("buffer0").to_owned(), text.into_bytes());
        ok("")
    }

    /// capture-pane's output for session `slot` as it is now, changing nothing: packed rows are unpacked
    /// into copies and a frozen screen is read from a copy, so it stays frozen (and this can run on any
    /// thread holding the engine).
    pub(super) fn render(&self, slot: usize, c: &Capture) -> Result<String, CmdError> {
        self.slots.get(slot).expect("a found session").term.read(|t| render::capture(t.view(), c)).map_err(|r| CmdError::new(r.err))
    }

    /// `grep-panes [-S -n] text`: every session's screen (and with -S its last n rows of scrollback)
    /// searched for `text`, as "name: line" per line that has it, sessions by name; code 1 when none
    /// has it, as grep. Screens are read without thawing them (render): 1000 frozen screens stay frozen,
    /// where capturing each from a client thawed them all (+0.5 MB until they froze again). 1000
    /// sessions: 11-13 ms, against 77-101 ms capturing each from Node and 2.9-6.1 s from the CLI.
    fn grep_panes(&mut self, c: Call) -> Reply {
        let Some(text) = c.args.rest.first().cloned() else { return fail("grep-panes: what text?\n") };
        let mut slots: Vec<usize> = self.slots.used().collect();
        slots.sort_by(|&x, &y| self.slots.get(x).unwrap().name.cmp(&self.slots.get(y).unwrap().name));
        let range = Capture::from_args(&c.args);
        let mut out = String::new();
        for slot in slots {
            // Output not read yet is read first, as for a capture.
            self.drain_now(slot);
            let screen = self.render(slot, &range).unwrap_or_default();
            let name = &self.slots.get(slot).unwrap().name;
            for line in screen.lines().filter(|l| l.contains(&text)) {
                out.push_str(name);
                out.push_str(": ");
                out.push_str(line);
                out.push('\n');
            }
        }
        Reply { code: i32::from(out.is_empty()), out, err: String::new() }
    }

    /// display-message [-p] [-F format | format] -t target: the format with its variables filled in
    /// (printed with or without -p: a session here has no status line to show it on).
    fn display(&mut self, c: Call) -> Outcome {
        let fmt = c.args.get('F').map(str::to_owned).or_else(|| c.args.rest.first().cloned()).unwrap_or_default();
        let slot = match self.find(c.args.get('t')) {
            Ok(slot) => slot,
            // A -t naming no session is not an error to tmux: the format is printed with the session's
            // variables empty, and the exit code is 0. (An empty -t means the current session to tmux,
            // which a session here does not have: it fails as before.)
            Err(_) if c.args.get('t').is_some_and(|t| !t.is_empty()) => return Ok(ok(format!("{}\n", without_session(&fmt)))),
            // The server's own variables need no session: `display -p '#{version}'` asks a running
            // server (or embedded engine) which version it is, with no session yet.
            Err(_) if c.args.get('t').is_none() && only_server_vars(&fmt) => return Ok(ok(format!("{}\n", without_session(&fmt)))),
            Err(e) => return Err(e),
        };
        if reads_screen(&fmt) {
            self.drain_now(slot);
        }
        Ok(ok(format!("{}\n", self.expand(slot, &fmt))))
    }

    /// pipe-pane [-o] -t target [command]: what the session prints from now on also goes to `command`'s
    /// input (tmux's -O, its default); without a command, or with -o when one runs, the pipe closes.
    fn pipe_pane(&mut self, c: Call) -> Outcome {
        let slot = self.find_as(c.args.get('t'), Lookup::Pane)?;
        // What it printed before goes to the screen only.
        self.drain_now(slot);
        let tag = self.key(slot).udata();
        let s = self.slots.get_mut(slot).expect("a found session");
        let had = s.extra.as_mut().and_then(|x| x.pipe.take()).is_some();
        s.trim_extra();
        let command = c.args.rest.join(" ");
        if command.is_empty() || (c.args.has('o') && had) {
            return Ok(ok(""));
        }
        let (pid, fd) = self.programs.spawn_pipe(&command).map_err(CmdError::new)?;
        // Gone already (ESRCH): its end of the socket is closed, and the first write says so.
        let _ = self.poller.apply(&[Change::new(pid, Interest::Exit, tag)]);
        s.extra().pipe = Some(Box::new(PanePipe { fd, pid, udata: tag, pending: Vec::new() }));
        Ok(ok(""))
    }

    /// rename-session -t target new-name
    fn rename_session(&mut self, c: Call) -> Outcome {
        let slot = self.find(c.args.get('t'))?;
        let new = c.args.rest.first().ok_or_else(|| CmdError::new("rename-session: new name?\n"))?.clone();
        if new.is_empty() || new.contains(':') || new.contains('.') {
            return Err(CmdError::new(format!("bad session name: {new}\n")));
        }
        if self.names.find(&new, &self.slots).is_some_and(|other| other != slot) {
            return Err(CmdError::new(format!("duplicate session: {new}\n")));
        }
        let old = std::mem::replace(&mut self.sess(slot).name, new.as_str().into());
        self.names.remove(&old, slot, &self.slots);
        self.names.insert(&new, slot);
        self.tap.emit(super::tap::kind::SESSION_RENAMED, || super::tap::Event::SessionRenamed { session: new.clone(), from: old.into() });
        Ok(ok(""))
    }

    /// resize-window / resize-pane -t target [-x cols] [-y rows]: the session's size from now on (its
    /// program gets SIGWINCH), also once attached terminals leave.
    fn resize(&mut self, c: Call) -> Outcome {
        let a = &c.args;
        let slot = self.find_as(a.get('t'), Lookup::Window)?;
        let s = self.slots.get(slot).unwrap();
        let num = |flag: char, cur: u16| -> Result<u16, CmdError> {
            a.get(flag).map_or(Ok(cur), |v| v.parse::<u16>().ok().filter(|&n| n > 0).ok_or_else(|| CmdError::new(format!("resize: -{flag} invalid: {v}\n"))))
        };
        let (rows, cols) = (num('y', s.rows)?, num('x', s.cols)?);
        let (rows, cols) = (rows.min(MAX_SIZE), cols.min(MAX_SIZE));
        self.sess(slot).base = (rows, cols);
        self.resize_session(slot, rows, cols);
        Ok(ok(""))
    }

    /// set-environment -g [-u] name [value]: for the sessions made after (as tmux's global environment).
    /// Without -g, tmux sets it for new windows of the session, which a session here never gets: it
    /// changes nothing, as there.
    fn set_environment(&mut self, c: Call) -> Outcome {
        let a = &c.args;
        if !a.has('g') {
            // tmux words this one its own way, with the target as given.
            self.find(a.get('t')).map_err(|_| CmdError::new(format!("no such session: {}\n", a.get('t').unwrap_or(""))))?;
        }
        let name = a.rest.first().ok_or_else(|| CmdError::new("set-environment: name?\n"))?.clone();
        let value = if a.has('u') { None } else { Some(a.rest.get(1).cloned().unwrap_or_default()) };
        if a.has('g') {
            self.env.retain(|(k, _)| *k != name);
            self.env.push((name, value));
        }
        Ok(ok(""))
    }

    /// show-environment -g [name]: what set-environment -g changed, as tmux prints it (-name: removed).
    fn show_environment(&mut self, c: Call) -> Outcome {
        if !c.args.has('g') {
            return Err(CmdError::new("show-environment: only -g (a session here has the environment it started with)\n"));
        }
        let only = c.args.rest.first();
        let out: String = self
            .env
            .iter()
            .filter(|(k, _)| only.is_none_or(|n| n == k))
            .map(|(k, v)| match v {
                Some(v) => format!("{k}={v}\n"),
                None => format!("-{k}\n"),
            })
            .collect();
        if let (Some(n), true) = (only, out.is_empty()) {
            return Err(CmdError::new(format!("unknown variable: {n}\n")));
        }
        Ok(ok(out))
    }

    /// A tmux format for session `slot`: each #{name} replaced by its value; names zeropty does not have
    /// become empty, as unknown ones do in tmux.
    fn expand(&self, slot: usize, fmt: &str) -> String {
        expand(fmt, |name| self.format_var(slot, name))
    }

    /// A tmux format variable for session `slot`. A session here is one window with one pane, so the
    /// window's and the pane's come from it too.
    fn format_var(&self, slot: usize, name: &str) -> String {
        if name == "version" {
            return super::VERSION.into();
        }
        let s = self.slots.get(slot).expect("a found session");
        let screen = |f: &dyn Fn(&crate::term::Screen) -> String| s.term.read(|t| f(t.view()));
        match name {
            "session_name" => s.name.to_string(),
            "session_id" => format!("${}", s.generation),
            "session_created" => s.created.to_string(),
            "session_attached" => s.extra.as_ref().map_or(0, |x| x.viewers.len()).to_string(),
            "session_windows" | "window_panes" => "1".into(),
            "window_index" | "pane_index" | "pane_dead" | "pane_in_mode" => "0".into(),
            "pane_id" => format!("%{}", s.generation),
            "window_id" => format!("@{}", s.generation),
            "pane_pid" => s.pid.to_string(),
            "pane_width" | "window_width" => s.cols.to_string(),
            "pane_height" | "window_height" => s.rows.to_string(),
            // As tmux on macOS: the name of the tty's foreground process group's leader, here the
            // session's process (the program, or the shell running it).
            "pane_current_command" | "window_name" => self.programs.name(s.pid),
            "pane_current_path" => self.programs.cwd(s.pid),
            "cursor_x" => screen(&|v| v.cursor().col.to_string()),
            "cursor_y" => screen(&|v| v.cursor().row.to_string()),
            "alternate_on" => screen(&|v| u8::from(v.alternate()).to_string()),
            "history_size" => screen(&|v| v.history_size().to_string()),
            "history_limit" => self.opts.history.to_string(),
            _ => String::new(),
        }
    }

    /// load-buffer [-b name] path: the file's bytes into a paste buffer.
    fn load_buffer(&mut self, c: Call) -> Outcome {
        let name = c.args.get('b').unwrap_or("buffer0").to_owned();
        let path = c.args.rest.first().ok_or_else(|| CmdError::new("no file\n"))?;
        let path = PathBuf::from(c.cwd).join(path);
        let mut data = vec![];
        std::fs::File::open(&path).and_then(|mut f| f.read_to_end(&mut data)).map_err(|e| CmdError::new(format!("{}: {e}\n", path.display())))?;
        self.buffers.insert(name, data);
        Ok(ok(""))
    }

    /// paste-buffer [-b name] [-d] [-p] [-r] [-s sep] -t target: a paste buffer typed into the session,
    /// line feeds as Enter (or -s), bracketed with -p when the program asked for it.
    fn paste_buffer(&mut self, c: Call) -> Outcome {
        let a = &c.args;
        let name = a.get('b').unwrap_or("buffer0").to_owned();
        let slot = self.find_as(a.get('t'), Lookup::Pane)?;
        let data = if a.has('d') { self.buffers.remove(&name) } else { self.buffers.get(&name).cloned() }
            .ok_or_else(|| CmdError::new(format!("no buffer {name}\n")))?;
        // Without -r, tmux turns LF into CR as a typed Enter would be, or into -s's separator.
        let body: Vec<u8> = if a.has('r') {
            data
        } else {
            let sep = a.get('s').map_or(&b"\r"[..], str::as_bytes);
            let mut b = Vec::with_capacity(data.len());
            for c in data {
                if c == b'\n' { b.extend_from_slice(sep) } else { b.push(c) }
            }
            b
        };
        self.modes_now(slot);
        let bracket = a.has('p') && self.sess(slot).bracketed_paste;
        let mut bytes = vec![];
        if bracket {
            bytes.extend_from_slice(b"\x1b[200~")
        }
        bytes.extend(body);
        if bracket {
            bytes.extend_from_slice(b"\x1b[201~")
        }
        self.type_keys(slot, &bytes)?;
        Ok(ok(""))
    }

    /// list-sessions [-F format]: a line per session, by name; without -F as tmux shows them.
    fn list(&mut self, c: Call) -> Reply {
        let fmt = c.args.get('F');
        if fmt.is_some_and(reads_screen) {
            for slot in self.slots.used().collect::<Vec<_>>() {
                self.drain_now(slot);
            }
        }
        let mut lines: Vec<(&str, String)> = self
            .slots
            .used()
            .map(|i| (i, self.slots.get(i).unwrap()))
            .map(|(i, s)| {
                let line = match fmt {
                    Some(f) => format!("{}\n", self.expand(i, f)),
                    None => format!("{}: 1 windows (created {}){}\n", s.name, ctime(s.created), if s.viewed() { " (attached)" } else { "" }),
                };
                (&*s.name, line)
            })
            .collect();
        lines.sort();
        ok(lines.into_iter().map(|(_, l)| l).collect::<String>())
    }

    /// `mem -t target`: where a session's screen memory is (live: per grid; frozen: its bytes).
    fn mem(&mut self, c: Call) -> Outcome {
        let slot = self.find(c.args.get('t'))?;
        Ok(ok(match &self.sess(slot).term {
            Screenful::Live(t) => format!("live\n{}", t.mem_report()),
            Screenful::Frozen(f) => format!("frozen {} B deep {}\n", f.bytes(), f.is_deep()),
            Screenful::Moving => String::new(),
        }))
    }

    /// pull-sessions / push-sessions -s socket [name...]: sessions from or to another zeropty (see
    /// super::handover).
    fn handover(&mut self, c: Call) -> Outcome {
        match c.args.get('s') {
            Some(sock) if c.name == "pull-sessions" => Ok(super::handover::pull(self, Path::new(sock), &c.args.rest)),
            Some(sock) => Ok(super::handover::push(self, Path::new(sock), &c.args.rest)),
            None => Err(CmdError::new(format!("{}: which server? (-s socket)\n", c.name))),
        }
    }

    fn stats_reply(&self) -> Reply {
        let (mut live, mut frozen, mut bytes) = (0, 0, 0);
        for s in self.slots.iter() {
            match &s.term {
                Screenful::Live(t) => {
                    live += 1;
                    bytes += t.approx_bytes()
                }
                Screenful::Frozen(f) => {
                    frozen += 1;
                    bytes += f.bytes()
                }
                Screenful::Moving => {}
            }
        }
        ok(format!(
            "sessions {} heap_bytes {} screen_bytes {} live {live} frozen {frozen} drains {} reads {} read_bytes {} loops {} events {} sess_struct {}\n",
            live + frozen,
            self.opts.heap_bytes.map_or(0, |f| f()),
            bytes,
            self.stats.drains,
            self.stats.reads,
            self.stats.read_bytes,
            self.stats.loops,
            self.stats.events,
            std::mem::size_of::<Sess>()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::KILL_GRACE_MS;
    use crate::engine::testing::{Fake, advance, close_input, engine, programs, session, session_with_input};
    use crate::engine::{Engine, fail, ok};
    use std::io::{Read as _, Write as _};

    type E = Engine<Fake>;

    fn run(e: &mut E, a: &[&str]) -> crate::engine::Reply {
        e.command(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "/")
    }

    fn typed(theirs: &mut std::os::unix::net::UnixStream) -> Vec<u8> {
        let mut b = vec![0u8; 256];
        let n = theirs.read(&mut b).unwrap_or(0);
        b.truncate(n);
        b
    }

    #[test]
    fn lookups_and_errors() {
        let mut e = engine();
        session(&mut e, "a");
        assert_eq!(run(&mut e, &["has-session", "-t", "=a:"]), ok(""));
        assert_eq!(run(&mut e, &["has", "-t", "b"]), fail("can't find session: b\n"));
        assert_eq!(run(&mut e, &["nope"]), fail("unknown command: nope\n"));
        assert_eq!(run(&mut e, &[]), fail("no command\n"));
        assert_eq!(run(&mut e, &["capture-pane", "-Z"]), fail("capture-pane: unknown flag -Z\n"));
        assert_eq!(run(&mut e, &["wait-pane", "-t", "a"]).code, 1, "the host's to answer");
        assert_eq!(run(&mut e, &["display-message", "-p", "-t", "a", "#{session_name} #{pane_width}x#{pane_height} #{nope}"]), ok("a 20x4 \n"));
        assert_eq!(run(&mut e, &["list-sessions", "-F", "#{session_name}"]), ok("a\n"));
        assert!(e.command_fast(&["new-session".to_string()], "/").is_none(), "not on a host's thread");
        assert_eq!(e.command_fast(&["has".to_string(), "-t".into(), "a".into()], "/"), Some(ok("")));
    }

    #[test]
    fn rename_and_environment() {
        let mut e = engine();
        session(&mut e, "a");
        session(&mut e, "b");
        assert_eq!(run(&mut e, &["rename-session", "-t", "a", "b"]), fail("duplicate session: b\n"));
        assert_eq!(run(&mut e, &["rename-session", "-t", "a", "x:y"]), fail("bad session name: x:y\n"));
        assert_eq!(run(&mut e, &["rename-session", "-t", "a", "c"]), ok(""));
        assert_eq!(run(&mut e, &["list-sessions", "-F", "#{session_name}"]), ok("b\nc\n"));
        assert_eq!(run(&mut e, &["set-environment", "-g", "K", "v"]), ok(""));
        assert_eq!(run(&mut e, &["set-environment", "-gu", "GONE"]), ok(""));
        assert_eq!(run(&mut e, &["show-environment", "-g"]), ok("K=v\n-GONE\n"));
        assert_eq!(run(&mut e, &["show-environment", "-g", "X"]), fail("unknown variable: X\n"));
        assert_eq!(run(&mut e, &["show-environment"]).code, 1);
    }

    #[test]
    fn keys_and_buffers_reach_the_program() {
        let mut e = engine();
        let (key, _out, mut theirs) = session_with_input(&mut e, "a");
        // A raw program (tty 0): keys as they are.
        assert_eq!(run(&mut e, &["send-keys", "-t", "a", "hi", "Enter", "C-c"]), ok(""));
        assert_eq!(typed(&mut theirs), b"hi\r\x03");
        assert_eq!(run(&mut e, &["send-keys", "-t", "a", "-H", "41", "zz"]), fail("send-keys: invalid hex byte: zz\n"));
        assert_eq!(run(&mut e, &["send-keys", "-t", "a", "-N", "0", "x"]), fail("send-keys: repeat count invalid: 0\n"));
        assert_eq!(run(&mut e, &["send-keys", "-t", "a", "-N", "3", "-l", "ab"]), ok(""));
        assert_eq!(typed(&mut theirs), b"ababab");
        // capture-pane without -p keeps the screen in a buffer; paste-buffer types it, LF as CR.
        let path = std::env::temp_dir().join(format!("zeropty-buffer-test-{}", std::process::id()));
        std::fs::write(&path, "one\ntwo").unwrap();
        assert_eq!(run(&mut e, &["load-buffer", "-b", "x", path.to_str().unwrap()]), ok(""));
        let _ = std::fs::remove_file(&path);
        assert_eq!(run(&mut e, &["paste-buffer", "-b", "x", "-t", "a"]), ok(""));
        assert_eq!(typed(&mut theirs), b"one\rtwo");
        assert_eq!(run(&mut e, &["paste-buffer", "-b", "x", "-d", "-r", "-t", "a"]), ok(""));
        assert_eq!(typed(&mut theirs), b"one\ntwo");
        assert_eq!(run(&mut e, &["paste-buffer", "-b", "x", "-t", "a"]), fail("no buffer x\n"), "-d deleted it");
        // The input closed: the error says so. (Not by dropping `theirs`: a process a parallel test
        // is starting holds a copy of it until it execs, which made this fail about one run in six.)
        drop(theirs);
        close_input(&e, key);
        let r = run(&mut e, &["send-keys", "-t", "a", "x"]);
        assert!(r.err.starts_with("write: "), "{r:?}");
    }

    #[test]
    fn capture_reads_what_is_pending() {
        let mut e = engine();
        let (_key, mut out) = session(&mut e, "a");
        out.write_all(b"line one\r\nline two").unwrap();
        // Not read yet (its read is coalesced): a capture reads it first.
        assert_eq!(run(&mut e, &["capture-pane", "-p", "-t", "a"]), ok("line one\nline two\n\n\n"));
        assert_eq!(run(&mut e, &["capture-pane", "-t", "a", "-b", "cap"]), ok(""));
        assert_eq!(run(&mut e, &["grep-panes", "two"]), ok("a: line two\n"));
        assert_eq!(run(&mut e, &["grep-panes", "absent"]).code, 1);
        assert_eq!(run(&mut e, &["resize-window", "-t", "a", "-x", "0"]), fail("resize: -x invalid: 0\n"));
        assert_eq!(run(&mut e, &["kill-session", "-t", "a"]), ok(""));
        assert_eq!(run(&mut e, &["has", "-t", "a"]).code, 1);
    }

    /// kill-session: SIGHUP to the group at once, SIGKILL once KILL_GRACE_MS passed; its size file goes.
    #[test]
    fn kill_session_hangs_up_then_kills() {
        let mut e = engine();
        let (key, _out) = session(&mut e, "a");
        let (other, _o) = session(&mut e, "b");
        let (pid, other_pid) = (e.slots.get(key.slot).unwrap().pid, e.slots.get(other.slot).unwrap().pid);
        let size_file = e.size_file_of(key.slot);
        assert_eq!(run(&mut e, &["kill-session", "-t", "a"]), ok(""));
        let p = programs(&e);
        assert_eq!(p.signals_to(pid), [libc::SIGHUP]);
        assert_eq!(*p.removed.borrow(), [size_file]);
        assert!(e.next_timer_ms() <= KILL_GRACE_MS, "woken for the kill");
        advance(&e, KILL_GRACE_MS - 1);
        e.tick();
        assert_eq!(programs(&e).signals_to(pid), [libc::SIGHUP], "not yet");
        advance(&e, 1);
        e.tick();
        assert_eq!(programs(&e).signals_to(pid), [libc::SIGHUP, libc::SIGKILL]);
        assert!(e.kills.is_empty());
        assert!(programs(&e).signals_to(other_pid).is_empty(), "the other session is left alone");
        // kill-server: every session left is hung up.
        assert_eq!(run(&mut e, &["kill-server"]), ok(""));
        assert_eq!(programs(&e).signals_to(other_pid), [libc::SIGHUP]);
    }

    /// resize: the size file first, then SIGWINCH; the same size again changes nothing.
    #[test]
    fn resize_tells_the_program() {
        let mut e = engine();
        let (key, _out) = session(&mut e, "a");
        let pid = e.slots.get(key.slot).unwrap().pid;
        let size_file = e.size_file_of(key.slot);
        assert_eq!(run(&mut e, &["resize-window", "-t", "a", "-x", "30", "-y", "5"]), ok(""));
        assert_eq!(*programs(&e).sizes.borrow(), [(size_file, 5, 30)]);
        assert_eq!(programs(&e).signals_to(pid), [libc::SIGWINCH]);
        assert_eq!(run(&mut e, &["display", "-p", "-t", "a", "#{pane_width}x#{pane_height}"]), ok("30x5\n"));
        assert_eq!(run(&mut e, &["resize-window", "-t", "a", "-x", "30"]), ok(""));
        assert_eq!(programs(&e).sizes.borrow().len(), 1, "unchanged: not told again");
        assert_eq!(programs(&e).signals_to(pid), [libc::SIGWINCH]);
    }

    /// new-session through `Programs`: its output reaches the screen, keys go through the tty's line
    /// discipline (C-c as SIGINT to its group), and its exit ends the session.
    #[test]
    fn new_session_runs_through_programs() {
        let mut e = engine();
        assert_eq!(run(&mut e, &["new-session", "-d", "-s", "n", "-x", "12", "-y", "3", "fail"]), fail("spawn: refused\n"));
        assert_eq!(run(&mut e, &["new-session", "-d", "-P", "-s", "n", "-x", "12", "-y", "3", "printf", "hi"]), ok("n:\n"));
        let (pid, mut output, mut input) = {
            let mut started = programs(&e).started.borrow_mut();
            let p = started.pop().unwrap();
            assert_eq!(p.command, "printf hi");
            (p.pid, p.output, p.input)
        };
        assert_eq!(run(&mut e, &["display", "-p", "-t", "n", "#{pane_pid} #{pane_current_command} #{pane_current_path}"]), ok(format!("{pid} fake /fake\n")));
        output.write_all(b"hello").unwrap();
        assert_eq!(run(&mut e, &["capture-pane", "-p", "-t", "n"]), ok("hello\n\n\n"));
        // Cooked: the line goes over at Enter, as a line feed (ICRNL).
        assert_eq!(run(&mut e, &["send-keys", "-t", "n", "ab", "Enter"]), ok(""));
        assert_eq!(typed(&mut input), b"ab\n");
        assert_eq!(run(&mut e, &["send-keys", "-t", "n", "C-c"]), ok(""));
        assert_eq!(programs(&e).signals_to(pid), [libc::SIGINT]);
        let slot = e.find(Some("n")).unwrap();
        let tag = e.key(slot).udata();
        e.handle(&[crate::engine::Event::new(crate::engine::Kind::Exited, pid, tag)], &mut Vec::new());
        assert_eq!(run(&mut e, &["has", "-t", "n"]).code, 1, "ended with its program");
        assert_eq!(*programs(&e).reaped.borrow(), [pid]);
    }

    /// pipe-pane: the session's output from then on goes to the command's input; its exit closes the pipe.
    #[test]
    fn pipe_pane_copies_output_to_its_command() {
        let mut e = engine();
        let (key, mut out) = session(&mut e, "a");
        out.write_all(b"before ").unwrap();
        assert_eq!(run(&mut e, &["pipe-pane", "-t", "a", "cat", ">", "log"]), ok(""));
        let (pid, mut piped) = {
            let mut started = programs(&e).started.borrow_mut();
            let p = started.pop().unwrap();
            assert_eq!(p.command, "cat > log");
            (p.pid, p.input)
        };
        out.write_all(b"after").unwrap();
        e.drain_now(key.slot);
        assert_eq!(typed(&mut piped), b"after", "only what came after it started");
        assert!(e.poller.applied.borrow().iter().any(|c| c.ident() == pid), "its exit is watched");
        // No process starts meanwhile: one would hold the engine's end open until it execs (fd::no_spawns).
        let no_spawns = crate::sys::fd::no_spawns();
        e.handle(&[crate::engine::Event::new(crate::engine::Kind::Exited, pid, key.udata())], &mut Vec::new());
        assert!(e.slots.get(key.slot).unwrap().extra.is_none(), "the pipe is gone with its command");
        assert_eq!(*programs(&e).reaped.borrow(), [pid]);
        assert_eq!(piped.read(&mut [0u8; 4]).unwrap(), 0, "and its input closed");
        drop(no_spawns);
    }

    /// Every name answers to one command, and the fast ones are only those that read or type.
    #[test]
    fn the_table() {
        let mut names: Vec<&str> = E::COMMANDS.iter().flat_map(|c| c.names.iter().copied()).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n, "a name used twice");
        let fast: Vec<&str> = E::COMMANDS.iter().filter(|c| c.fast).map(|c| c.names[0]).collect();
        assert_eq!(fast, ["capture-pane", "send-keys", "has-session", "display-message", "paste-buffer", "list-sessions"]);
        assert!(E::lookup("capturep").is_some_and(|c| c.names[0] == "capture-pane"));
        assert!(E::lookup("attach").is_none(), "the host's");
    }
}
