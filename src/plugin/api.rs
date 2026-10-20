//! The stable surface for plugin authors: everything a plugin needs, nothing a host's loop uses.
//!
//! [`Plugin`] (the hooks), [`Manifest`] (what it is and asks for), [`Event`] and [`ExitReason`] (what it
//! hears), [`Host`] (what it may ask of the engine), [`PluginError`], the bit sets [`Events`] and
//! [`Access`], [`API_VERSION`], [`reply_wait`], [`is_reserved`], and the reply type ([`Reply`], built with [`ok`], [`fail`]
//! or [`Reply::new`], or a struct literal: its three fields are frozen).
//! Re-exported as `zeropty::plugin::*`. Changes here follow the version policy (docs/PLUGINS.md):
//! additions only within an API_VERSION; hooks have default bodies, and [`Event`], [`ExitReason`],
//! [`Manifest`] and [`PluginError`] are non-exhaustive, so adding to them breaks no plugin. [`Reply`] is
//! the CLI's and the socket's reply and stays exactly `{ code, out, err }`: a field there would break
//! embedders, so a later version that needs more says it beside the reply (a new type), never in it.
//! The machinery behind [`Host`] lives in `registry`, which this module only calls into.
use super::registry::{Shared, State};
use super::wire::{base64, json_str};
use crate::engine::tap::kind;
use crate::engine::{Engine, System};
/// A command's reply, and its two usual forms: what a plugin's `command` returns and `Host::command`
/// gives back. Re-exported so that plugins depend on this module only.
pub use crate::engine::{Reply, fail, ok};
use std::sync::Arc;
use std::time::Duration;

/// The version of this interface: what `Manifest::api` and `plugin-attach -v` are checked against.
pub const API_VERSION: u32 = 1;

/// The common half of the two bit sets ([`Events`], [`Access`]): a `u32` that only ever holds known bits
/// (every constructor checks or masks), so a set can never name something this zeropty does not have.
/// Hand-written rather than the `bitflags` crate: two small types do not justify a dependency.
macro_rules! bit_set {
    ($(#[$doc:meta])* $name:ident, $all:expr) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
        pub struct $name(u32);

        impl $name {
            /// No bit.
            pub const fn empty() -> Self { Self(0) }
            /// Every bit this version knows.
            pub const fn all() -> Self { Self($all) }
            /// The raw bits (for storing or passing on; the wire uses names, see `names`).
            pub const fn bits(self) -> u32 { self.0 }
            /// The set with these bits, or None if one is not known to this version.
            pub const fn from_bits(bits: u32) -> Option<Self> { if bits & !($all) == 0 { Some(Self(bits)) } else { None } }
            /// The set with the known bits of `bits`, the others dropped.
            pub const fn from_bits_truncate(bits: u32) -> Self { Self(bits & ($all)) }
            /// Whether no bit is set.
            pub const fn is_empty(self) -> bool { self.0 == 0 }
            /// Whether every bit of `other` is set here.
            pub const fn contains(self, other: Self) -> bool { self.0 & other.0 == other.0 }
            /// Whether any bit of `other` is set here.
            pub const fn intersects(self, other: Self) -> bool { self.0 & other.0 != 0 }
            /// Both sets' bits (`|`, usable in a const).
            pub const fn union(self, other: Self) -> Self { Self(self.0 | other.0) }
            /// The bits set here but not in `other`.
            pub const fn difference(self, other: Self) -> Self { Self(self.0 & !other.0) }

            /// The names of the bits set, comma-separated in a fixed order: what the wire, `list-plugins`
            /// and `plugin-attach` show ("" for none).
            pub fn names(self) -> String { Self::NAMES.iter().filter(|(_, b)| self.contains(*b)).map(|(n, _)| *n).collect::<Vec<_>>().join(",") }
        }

        impl std::ops::BitOr for $name {
            type Output = Self;
            fn bitor(self, other: Self) -> Self { Self(self.0 | other.0) }
        }
        impl std::ops::BitOrAssign for $name {
            fn bitor_assign(&mut self, other: Self) { self.0 |= other.0 }
        }
        impl std::ops::BitAnd for $name {
            type Output = Self;
            fn bitand(self, other: Self) -> Self { Self(self.0 & other.0) }
        }
        impl std::ops::BitAndAssign for $name {
            fn bitand_assign(&mut self, other: Self) { self.0 &= other.0 }
        }
        impl std::ops::Sub for $name {
            type Output = Self;
            /// The bits set in `self` but not in `other` (`difference`).
            fn sub(self, other: Self) -> Self { Self(self.0 & !other.0) }
        }
        impl std::ops::Not for $name {
            type Output = Self;
            /// Every known bit not set here (never an unknown one).
            fn not(self) -> Self { Self(!self.0 & ($all)) }
        }

        /// `Events(session-created | output)`: the names, as `names` gives them.
        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), if self.is_empty() { "empty".to_string() } else { self.names().replace(',', " | ") })
            }
        }
        /// The names, comma-separated, as on the wire (`names`).
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.names()) }
        }
        /// The same as `parse`.
        impl std::str::FromStr for $name {
            type Err = PluginError;
            fn from_str(list: &str) -> Result<Self, PluginError> { Self::parse(list) }
        }
    };
}

bit_set!(
    /// The events a plugin asks for (`Manifest::events`): a set of the kinds below. Combine with `|`
    /// (`Events::SESSIONS | Events::OUTPUT`); on the wire, a comma list of names (`parse`, `names`).
    Events,
    kind::ALL
);

impl Events {
    /// A session was made (new-session, or taken over from another server).
    pub const SESSION_CREATED: Self = Self(kind::SESSION_CREATED);
    /// A session ended: its program exited, it was killed, or it was handed to another server.
    pub const SESSION_EXITED: Self = Self(kind::SESSION_EXITED);
    /// A session got a new name.
    pub const SESSION_RENAMED: Self = Self(kind::SESSION_RENAMED);
    /// What a session's program printed, as read (coalesced like the screen's reads, ~300 ms when quiet).
    pub const OUTPUT: Self = Self(kind::OUTPUT);
    /// A session's screen changed (one event per read, without the bytes: cheaper than OUTPUT).
    pub const ACTIVITY: Self = Self(kind::ACTIVITY);
    /// The three lifecycle events.
    pub const SESSIONS: Self = Self(kind::SESSION_CREATED | kind::SESSION_EXITED | kind::SESSION_RENAMED);
    /// Every kind.
    pub const ALL: Self = Self(kind::ALL);

    /// Each kind's name on the wire, in bit order.
    const NAMES: [(&'static str, Self); kind::COUNT] = [
        ("session-created", Self::SESSION_CREATED),
        ("session-exited", Self::SESSION_EXITED),
        ("session-renamed", Self::SESSION_RENAMED),
        ("output", Self::OUTPUT),
        ("activity", Self::ACTIVITY),
    ];

    /// "session-created,output", "sessions", "all" or "" -> the set; an unknown name is
    /// `PluginError::UnknownEvent`.
    pub fn parse(list: &str) -> Result<Self, PluginError> {
        let mut set = Self::empty();
        for name in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            set |= match name {
                "all" => Self::ALL,
                "sessions" => Self::SESSIONS,
                _ => Self::NAMES.iter().find(|(n, _)| *n == name).map(|&(_, b)| b).ok_or_else(|| PluginError::UnknownEvent(name.into()))?,
            };
        }
        Ok(set)
    }
}

bit_set!(
    /// What a plugin may do through `Host::command` (`Manifest::access`): a set of the accesses below.
    /// Combine with `|` (`Access::INPUT | Access::CALL`); on the wire, a comma list of names.
    Access,
    ACCESS_ALL
);

const ACCESS_ALL: u32 = 0b1111;
/// The commands `Access::READ` lets through (aliases included); every one a command of the engine or
/// list-plugins (tested).
const READ_COMMANDS: &[&str] = &[
    "has-session",
    "has",
    "capture-pane",
    "capturep",
    "display-message",
    "display",
    "list-sessions",
    "ls",
    "stats",
    "grep-panes",
    "show-environment",
    "showenv",
    "mem",
    "list-plugins",
];
/// The commands `Access::INPUT` lets through.
const INPUT_COMMANDS: &[&str] = &["send-keys", "send", "paste-buffer", "pasteb", "load-buffer", "loadb"];

impl Access {
    /// Look: capture-pane, display-message, has-session, list-sessions, grep-panes, show-environment,
    /// stats, mem, list-plugins. Every plugin has it.
    pub const READ: Self = Self(1 << 0);
    /// Type into sessions: send-keys, paste-buffer, load-buffer.
    pub const INPUT: Self = Self(1 << 1);
    /// Start, end, rename and resize sessions, set their environment, run a program on one's output
    /// (pipe-pane): what changes which sessions there are or runs programs.
    pub const MANAGE: Self = Self(1 << 2);
    /// Run the commands other plugins add.
    pub const CALL: Self = Self(1 << 3);
    /// Every access.
    pub const ALL: Self = Self(ACCESS_ALL);

    /// Each access's name on the wire, in bit order.
    const NAMES: [(&'static str, Self); 4] = [("read", Self::READ), ("input", Self::INPUT), ("manage", Self::MANAGE), ("call", Self::CALL)];

    /// "read,input", "all" or "" -> the set; an unknown name is `PluginError::UnknownAccess`.
    pub fn parse(list: &str) -> Result<Self, PluginError> {
        let mut set = Self::empty();
        for name in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            set |= match name {
                "all" => Self::ALL,
                _ => Self::NAMES.iter().find(|(n, _)| *n == name).map(|&(_, b)| b).ok_or_else(|| PluginError::UnknownAccess(name.into()))?,
            };
        }
        Ok(set)
    }

    /// The access the engine's command `cmd` needs; MANAGE for what is not listed (a command added to the
    /// engine later is refused until it is placed here).
    pub(crate) fn needed(cmd: &str) -> Self {
        if READ_COMMANDS.contains(&cmd) {
            Self::READ
        } else if INPUT_COMMANDS.contains(&cmd) {
            Self::INPUT
        } else {
            Self::MANAGE
        }
    }
}

/// Why a session ended, and what happened to a session: plain data the engine queues
/// (crate::engine::tap), the same types here. Both are non-exhaustive.
pub use crate::engine::tap::{Event, ExitReason};

impl Event {
    /// Its kind, as the one-bit set a manifest asks for it with (`Events::OUTPUT`, ...).
    pub fn kind(&self) -> Events {
        Events(self.bit())
    }

    /// Its name on the wire ("session-created", ...).
    pub fn name(&self) -> &'static str {
        let k = self.kind();
        Events::NAMES.iter().find(|(_, b)| *b == k).map_or("", |(n, _)| n)
    }

    /// The event as a line of the plugin protocol (without the line feed):
    /// `{"event":"output","session":"w","data":"<base64>"}`.
    /// "event" comes first, then "session", whatever else after.
    pub fn to_json(&self) -> String {
        let mut s = format!("{{\"event\":\"{}\",\"session\":{}", self.name(), json_str(self.session()));
        match self {
            Event::SessionExited { reason, .. } => s.push_str(&format!(",\"reason\":\"{}\"", reason.as_str())),
            Event::SessionRenamed { from, .. } => s.push_str(&format!(",\"from\":{}", json_str(from))),
            Event::Output { data, .. } => s.push_str(&format!(",\"data\":\"{}\"", base64(data))),
            _ => {}
        }
        s.push('}');
        s
    }
}

/// What a plugin is: its name, the interface version it was written for, the events it wants, the
/// commands it adds and what it may do.
///
/// Fields are public to read (the registry, `list-plugins`, a host inspecting a plugin); it is built
/// only through `Manifest::new` and its builder methods, which `#[non_exhaustive]` enforces outside the
/// crate, so a field added in a later version (with a default in `new`) breaks no plugin.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Manifest {
    /// Its name: letters, digits, `- _ . :`, at most 64 bytes, unique among running plugins.
    pub name: String,
    /// The API_VERSION it was written for.
    pub api: u32,
    /// What it hears.
    pub events: Events,
    /// The commands it answers.
    pub commands: Vec<String>,
    /// What `Host::command` may run for it (always with `Access::READ`).
    pub access: Access,
}

impl Manifest {
    /// A plugin called `name`, for this API_VERSION, with no events and no commands yet, that may look
    /// (`Access::READ`) but not act.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), api: API_VERSION, events: Events::empty(), commands: Vec::new(), access: Access::READ }
    }
    /// What it may do through `Host::command`, added to what it has (READ at least).
    pub fn access(mut self, access: Access) -> Self {
        self.access |= access;
        self
    }
    /// The events it wants, added to those it has.
    pub fn events(mut self, events: Events) -> Self {
        self.events |= events;
        self
    }
    /// A command it answers (`Plugin::command`), through every way commands come: CLI, socket, embedded.
    pub fn command(mut self, name: impl Into<String>) -> Self {
        self.commands.push(name.into());
        self
    }
    /// The interface version it was written for (API_VERSION by default).
    pub fn api(mut self, version: u32) -> Self {
        self.api = version;
        self
    }

    /// A manifest as plugins outside Rust give it (the C ABI, plugin dylibs):
    /// {"name":"x","events":["session-created"],"commands":["x-do"],"access":["input"],"api":1}, where
    /// each list may also be one comma-separated string. Strict, so a typo is not a silently empty
    /// manifest: a key other than these five, a value of the wrong type or a non-string list item is
    /// `BadManifest`. Checked as `Plugins::add` would only when added.
    pub fn from_json(text: &str) -> Result<Self, PluginError> {
        use serde_json::Value;
        let bad = |why: String| PluginError::BadManifest(why);
        let v: Value = serde_json::from_str(text).map_err(|e| bad(e.to_string()))?;
        let Value::Object(o) = &v else { return Err(bad("not a JSON object".into())) };
        if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "name" | "events" | "commands" | "access" | "api")) {
            return Err(bad(format!("unknown key {k:?} (known: name, events, commands, access, api)")));
        }
        let list = |key: &str| -> Result<String, PluginError> {
            match o.get(key) {
                None | Some(Value::Null) => Ok(String::new()),
                Some(Value::String(s)) => Ok(s.clone()),
                Some(Value::Array(a)) => a
                    .iter()
                    .map(|x| x.as_str().ok_or_else(|| bad(format!("{key:?} holds {x}, not a string"))))
                    .collect::<Result<Vec<_>, _>>()
                    .map(|v| v.join(",")),
                Some(x) => Err(bad(format!("{key:?} is {x}, not a string or a list of strings"))),
            }
        };
        let name = match o.get("name") {
            None => "",
            Some(Value::String(s)) => s,
            Some(x) => return Err(bad(format!("\"name\" is {x}, not a string"))),
        };
        let api = match o.get("api") {
            None | Some(Value::Null) => API_VERSION,
            Some(x) => x.as_u64().ok_or_else(|| bad(format!("\"api\" is {x}, not a version number")))?.min(u32::MAX as u64) as u32,
        };
        let mut m = Manifest::new(name).events(Events::parse(&list("events")?)?).access(Access::parse(&list("access")?)?).api(api);
        for c in list("commands")?.split(',').map(str::trim).filter(|c| !c.is_empty()) {
            m = m.command(c);
        }
        Ok(m)
    }

    /// The `plugin-attach` command that registers this manifest through the socket.
    pub fn attach_argv(&self) -> Vec<String> {
        let mut a = vec!["plugin-attach".into(), "-n".into(), self.name.clone(), "-v".into(), self.api.to_string()];
        if !self.events.is_empty() {
            a.extend(["-e".into(), self.events.names()]);
        }
        if !self.commands.is_empty() {
            a.extend(["-c".into(), self.commands.join(",")]);
        }
        if self.access != Access::READ {
            a.extend(["-a".into(), self.access.names()]);
        }
        a
    }
}

/// Why a plugin was not registered (`Plugins::add`, `Plugins::check`) or a manifest not read. Non-exhaustive:
/// later versions may add kinds. `Display` gives the text `list-plugins`, the socket and the C ABI
/// show; `String::from` adds the line feed a reply's `err` ends with.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
// The fields name what they hold: `plugin` the plugin's name, `command` the command, `owner` the
// plugin that has it, `api`/`supported` versions.
#[allow(missing_docs)]
pub enum PluginError {
    /// The name is empty, longer than 64 bytes, starts with `-` or has characters other than letters,
    /// digits and `- _ . :`.
    BadName(String),
    /// The same for a command name.
    BadCommand { plugin: String, command: String },
    /// The manifest was written for an interface version this zeropty does not have (0, or newer than
    /// `supported`).
    Api { plugin: String, api: u32, supported: u32 },
    /// A name `Events::parse` does not know.
    UnknownEvent(String),
    /// A name `Access::parse` does not know.
    UnknownAccess(String),
    /// A command name the engine or its hosts answer (`is_reserved`).
    BuiltinCommand { plugin: String, command: String },
    /// The manifest lists a command twice.
    DuplicateCommand { plugin: String, command: String },
    /// A running plugin has that name.
    NameTaken(String),
    /// A running plugin (`owner`) answers that command.
    CommandTaken { plugin: String, command: String, owner: String },
    /// A JSON manifest (C ABI, plugin libraries) that does not parse.
    BadManifest(String),
    /// `Plugin::manifest` panicked.
    ManifestPanicked(String),
    /// `Plugin::manifest` did not return within the manifest deadline (ms): the plugin is not added.
    ManifestTimeout(u64),
    /// Its thread could not be started.
    Thread(String),
    /// This front end does not run plugins (the default `embed::Frontend::add_plugin_boxed`).
    Unsupported,
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginError::BadName(n) => write!(f, "bad plugin name: {n:?} (letters, digits, - _ . :, at most 64)"),
            PluginError::BadCommand { plugin, command } => write!(f, "plugin {plugin}: bad command name {command:?}"),
            PluginError::Api { plugin, api, supported } => write!(f, "plugin {plugin}: api {api} not supported (this zeropty: 1 to {supported})"),
            PluginError::UnknownEvent(n) => write!(f, "unknown event: {n}"),
            PluginError::UnknownAccess(n) => write!(f, "unknown access: {n} (read, input, manage, call, all)"),
            PluginError::BuiltinCommand { plugin, command } => write!(f, "plugin {plugin}: {command} is a built-in command"),
            PluginError::DuplicateCommand { plugin, command } => write!(f, "plugin {plugin}: {command} twice"),
            PluginError::NameTaken(n) => write!(f, "plugin {n} is already running"),
            PluginError::CommandTaken { plugin, command, owner } => write!(f, "plugin {plugin}: command {command} is plugin {owner}'s"),
            PluginError::BadManifest(why) => write!(f, "bad manifest: {why}"),
            PluginError::ManifestPanicked(why) => write!(f, "plugin manifest panicked: {why}"),
            PluginError::ManifestTimeout(ms) => write!(f, "plugin manifest took longer than {ms} ms"),
            PluginError::Thread(why) => write!(f, "plugin thread: {why}"),
            PluginError::Unsupported => write!(f, "this front end does not run plugins"),
        }
    }
}

impl std::error::Error for PluginError {}

/// As a reply's `err`: the text and a line feed.
impl From<PluginError> for String {
    fn from(e: PluginError) -> Self {
        format!("{e}\n")
    }
}

/// A plugin. Every hook (`start`, `event`, `command`, `stop`) runs on the plugin's own thread, one at a
/// time, never on the engine's: a hook may block (`Host::command` does), but a hook still running when
/// the next command for it is due holds that command up, and one running past the deadline gets the
/// plugin cut off. `manifest` runs there too, first, under a deadline of its own: see there.
///
/// A hook that never returns keeps its thread for good (Rust cannot end a thread from outside). The
/// engine is unaffected all the same: the watchdog marks the plugin failed and closes its queue, so no
/// more events or commands go to it (the engine never waits on a plugin's queue or thread), callers of
/// its commands get an error at their deadline, and `shutdown` leaves the thread detached after its
/// wait. The cost is that one parked thread and what the plugin holds, until the process ends.
pub trait Plugin: Send + 'static {
    /// What it is and asks for. Called once, as it is added, on the plugin's own new thread while
    /// `add` waits for it: build it from constants and do slow set-up in `start`. `add` waits at most
    /// the manifest deadline (the registry's deadline, at most 1 s) and then fails with
    /// `PluginError::ManifestTimeout` (the plugin is not added, and is dropped on its thread once this
    /// returns), so a slow or stuck one holds the caller of `add` that long at most (for
    /// `Polled::add_plugin`, the host's loop) and never the engine. A panic here is caught and
    /// reported as `PluginError::ManifestPanicked`.
    fn manifest(&self) -> Manifest;
    /// Once, first.
    fn start(&mut self, _host: &Host) {}
    /// One of the events the manifest asked for.
    fn event(&mut self, _event: &Event, _host: &Host) {}
    /// One of the manifest's commands (`argv[0]` is its name); the reply goes back as a built-in one's.
    fn command(&mut self, argv: &[String], _cwd: &str, _host: &Host) -> Reply {
        fail(format!("{}: not answered\n", argv.first().map_or("", String::as_str)))
    }
    /// Once, last, when the plugin is removed or its host stops (not after a panic).
    fn stop(&mut self, _host: &Host) {}
}

/// Whether `command` is a name the engine or a host (the server, the CLI) answers itself, which no
/// plugin may take (`PluginError::BuiltinCommand`). The engine's part is read from its command table
/// (every name and alias there), so it can never fall behind it. The list behind it grows with
/// zeropty's own commands and is not part of this interface: a name free today may be reserved by a
/// later zeropty (pick a prefix of your own, `myplugin-...`, to stay clear).
pub fn is_reserved(command: &str) -> bool {
    Engine::<System>::is_command(command) || HOST_COMMANDS.contains(&command)
}

/// What hosts (the server, the CLI) answer before the engine: reserved as well (`is_reserved`).
pub(crate) const HOST_COMMANDS: &[&str] = &[
    "attach-session",
    "attach",
    "a",
    "export-sessions",
    "import-sessions",
    "mstats",
    "serve",
    "restart-server",
    "-V",
    "list-plugins",
    "plugin-attach",
    "plugin-run",
];

/// How long a hook may wait on something outside zeropty (a plugin program's reply, say) and still come
/// back clearly inside the deadline: the deadline less an eighth (at least 20 ms, at most half), so that
/// one late answer is reported as late and never gets the plugin cut off as stuck (the watchdog checks
/// every quarter deadline, and fires only past the full one).
pub fn reply_wait(deadline: Duration) -> Duration {
    let margin = (deadline / 8).max(Duration::from_millis(20)).min(deadline / 2);
    deadline - margin
}

/// What a plugin holds of its host. Cloning gives another handle for the same plugin (a thread the
/// plugin starts may keep one); every method may be called from any thread.
#[derive(Clone)]
pub struct Host {
    pub(super) shared: Arc<Shared>,
    pub(super) state: Arc<State>,
}

impl Host {
    /// The plugin's name.
    pub fn name(&self) -> &str {
        &self.state.name
    }

    /// The deadline: how long a hook may run (the plugin's own time) before the plugin is cut off, and
    /// how long a command's caller waits for its reply (ZEROPTY_PLUGIN_TIMEOUT_MS, Plugins::set_timeout).
    /// A hook that waits on something outside zeropty bounds the wait by `reply_wait(deadline)`.
    pub fn deadline(&self) -> Duration {
        self.shared.timeout()
    }

    /// Runs one tmux-style command, as a client would (another plugin's command included, but not this
    /// plugin's own, nor kill-server, pull-sessions, push-sessions, attach or plugin-attach), on the
    /// engine's thread, and blocks for its reply up to half the deadline (the other half is left to
    /// answer whoever is waiting on this plugin). The wait does not count as the plugin's time in the
    /// hook. What the manifest's `access` does not cover is refused with the reason; a call of a plugin
    /// that waits for this one is refused at once ("call cycle"). wait-pane is the socket's: use the
    /// events instead.
    ///
    /// Past half the deadline the command is dropped if it was not started yet (the error ends in
    /// "not run"); one already started ends in "started, may still take effect".
    pub fn command<S: AsRef<str>>(&self, argv: &[S], cwd: &str) -> Reply {
        let argv: Vec<String> = argv.iter().map(|a| a.as_ref().to_owned()).collect();
        self.shared.command_for(&self.state, argv, cwd)
    }

    /// Takes this plugin out of the registry (a remote plugin whose connection closed); its `stop` runs.
    pub fn detach(&self) {
        self.shared.detach(&self.state)
    }

    /// Marks the plugin failed (shown by list-plugins) and takes it out of the registry's routing.
    pub fn fail(&self, why: impl Into<String>) {
        self.shared.fail(&self.state, why.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn event_lists() {
        assert_eq!(Events::parse("session-created,output").unwrap(), Events::SESSION_CREATED | Events::OUTPUT);
        assert_eq!(Events::parse(" all ").unwrap(), Events::ALL);
        assert_eq!("sessions".parse::<Events>().unwrap(), Events::SESSIONS);
        assert_eq!(Events::parse("").unwrap(), Events::empty());
        assert_eq!(Events::parse("nope"), Err(PluginError::UnknownEvent("nope".into())));
        assert_eq!(Events::SESSIONS.names(), "session-created,session-exited,session-renamed");
        assert_eq!(Events::ALL.to_string(), "session-created,session-exited,session-renamed,output,activity");
        assert_eq!(Access::parse("input, call").unwrap(), Access::INPUT | Access::CALL);
        assert_eq!(Access::parse("root"), Err(PluginError::UnknownAccess("root".into())));
        assert_eq!(Access::ALL.names(), "read,input,manage,call");
        // Every name parses back to its bit, and the names cover every bit.
        for (n, b) in Events::NAMES {
            assert_eq!(Events::parse(n).unwrap(), b);
        }
        assert_eq!(Events::NAMES.iter().fold(Events::empty(), |a, (_, b)| a | *b), Events::ALL);
        assert_eq!(Access::NAMES.iter().fold(Access::empty(), |a, (_, b)| a | *b), Access::ALL);
    }

    /// The bit sets hold only bits this version knows: `from_bits` refuses others, and no operator makes one.
    #[test]
    fn bit_sets_hold_only_known_bits() {
        assert_eq!(Events::from_bits(Events::OUTPUT.bits()), Some(Events::OUTPUT));
        assert_eq!(Events::from_bits(1 << 30), None);
        assert_eq!(Events::from_bits(Events::ALL.bits() + 1), None);
        assert_eq!(Events::from_bits_truncate(u32::MAX), Events::ALL);
        assert_eq!(Access::from_bits(0b1_0000), None);
        assert_eq!(Access::from_bits(0), Some(Access::empty()));
        assert_eq!(!Access::READ, Access::INPUT | Access::MANAGE | Access::CALL);
        assert_eq!(!Events::empty(), Events::ALL);
        assert_eq!(Events::all(), Events::ALL);
        assert_eq!(Events::ALL - Events::SESSIONS, Events::OUTPUT | Events::ACTIVITY);
        assert_eq!(Events::ALL.difference(Events::OUTPUT) & Events::OUTPUT, Events::empty());
        let mut e = Events::SESSION_CREATED;
        e |= Events::OUTPUT;
        e &= Events::OUTPUT | Events::ACTIVITY;
        assert_eq!(e, Events::OUTPUT);
        assert!(Events::SESSIONS.contains(Events::SESSION_EXITED) && !Events::SESSIONS.contains(Events::SESSION_EXITED | Events::OUTPUT));
        assert!(Events::SESSIONS.intersects(Events::SESSION_EXITED | Events::OUTPUT) && !Events::SESSIONS.intersects(Events::OUTPUT));
        assert!(Events::default().is_empty() && Access::default().is_empty());
        assert_eq!(format!("{:?}", Events::SESSION_CREATED | Events::OUTPUT), "Events(session-created | output)");
        assert_eq!(format!("{:?}", Access::empty()), "Access(empty)");
        // Hash + Eq: usable as keys.
        assert_eq!([Events::OUTPUT, Events::OUTPUT, Events::ACTIVITY].into_iter().collect::<HashSet<_>>().len(), 2);
        // An event's kind is its own bit, and its name the bit's.
        let ev = Event::SessionRenamed { session: "b".into(), from: "a".into() };
        assert_eq!((ev.kind(), ev.name()), (Events::SESSION_RENAMED, "session-renamed"));
        // The engine's plain masks and the typed sets are the same bits.
        assert_eq!(Events::ALL.bits(), kind::ALL);
    }

    #[test]
    fn manifests_from_json_and_back() {
        let m = Manifest::from_json(r#"{"name":"x","events":["session-created","output"],"commands":"x-a, x-b","access":["input"]}"#).unwrap();
        assert_eq!(
            (m.events, m.access, m.commands.clone()),
            (Events::SESSION_CREATED | Events::OUTPUT, Access::READ | Access::INPUT, vec!["x-a".to_string(), "x-b".into()])
        );
        let back = crate::plugin::internal::remote::parse_attach(&m.attach_argv()[1..]).unwrap();
        assert_eq!((back.name, back.events, back.access, back.commands, back.api), (m.name, m.events, m.access, m.commands, m.api));
        assert_eq!(Manifest::from_json(r#"{"name":"x","access":"root"}"#).unwrap_err(), PluginError::UnknownAccess("root".into()));
        assert!(matches!(Manifest::from_json("{"), Err(PluginError::BadManifest(_))));
        assert_eq!(Manifest::from_json(r#"{"name":"x","events":null,"api":1}"#).unwrap().events, Events::empty());
        // The wire form is the names, as before the typed sets.
        let full = Manifest::new("y").events(Events::SESSIONS | Events::ACTIVITY).access(Access::INPUT | Access::CALL).command("y-go");
        assert_eq!(
            full.attach_argv(),
            ["plugin-attach", "-n", "y", "-v", "1", "-e", "session-created,session-exited,session-renamed,activity", "-c", "y-go", "-a", "read,input,call"]
        );
    }
    #[test]
    fn manifest_typos_are_errors_not_empty_manifests() {
        for bad in [
            r#"{"name":"x","event":["output"]}"#,    // misspelt key
            r#"{"name":"x","events":["output",1]}"#, // non-string item
            r#"{"name":"x","commands":{"a":1}}"#,    // wrong type
            r#"{"name":7}"#,
            r#"{"name":"x","api":"1"}"#,
            r#"["x"]"#,
        ] {
            assert!(matches!(Manifest::from_json(bad), Err(PluginError::BadManifest(_))), "{bad}");
        }
        let e = Manifest::from_json(r#"{"name":"x","event":["output"]}"#).unwrap_err().to_string();
        assert!(e.contains("\"event\""), "{e}");
    }
    #[test]
    fn access_needed_by_command() {
        assert_eq!(Access::needed("send-keys"), Access::INPUT);
        assert_eq!(Access::needed("capture-pane"), Access::READ);
        assert_eq!(Access::needed("some-future-command"), Access::MANAGE);
        // Every name the access lists let through is a real command: no stale name widens a list.
        for c in READ_COMMANDS.iter().chain(INPUT_COMMANDS) {
            assert!(Engine::<System>::is_command(c) || *c == "list-plugins", "{c}");
        }
    }

    /// The reserved names and the engine's command table, both ways: every engine command and alias is
    /// reserved, and the engine answers every reserved name that is not a host's.
    #[test]
    fn reserved_names_are_the_engines_and_the_hosts() {
        let engine: Vec<&str> = Engine::<System>::command_names().collect();
        assert!(engine.len() >= 36 && engine.contains(&"resizep") && engine.contains(&"kill-server"), "{engine:?}");
        for c in &engine {
            assert!(is_reserved(c), "engine command {c} is not reserved");
        }
        let mut e = Engine::new(crate::engine::Options::default());
        for c in &engine {
            // Arguments the command refuses, so nothing happens (kill-server with none would hang up nothing).
            let r = e.command(&[c.to_string(), "-Z".into()], "/");
            assert!(!r.err.starts_with("unknown command"), "{c}: {}", r.err);
        }
        assert!(e.command(&["no-such".into()], "/").err.starts_with("unknown command"));
        // The hosts' names are their own: none of them reaches the engine.
        for c in HOST_COMMANDS {
            assert!(is_reserved(c) && !engine.contains(c), "{c}");
        }
    }

    #[test]
    fn reserved_names() {
        assert!(is_reserved("new-session") && is_reserved("ls") && is_reserved("list-plugins") && is_reserved("-V"));
        assert!(!is_reserved("hello") && !is_reserved("new-session-x") && !is_reserved(""));
    }
}
