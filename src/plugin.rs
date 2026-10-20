//! Plugins: code outside zeropty that hears what sessions do and adds commands of its own, without
//! being able to slow the engine down. docs/PLUGINS.md is the guide; this is the map.
//!
//! # Where it sits
//!
//! Above the engine and beside the socket server, driven by crate::host like the server is: the
//! engine only queues session events (crate::engine::tap, and only kinds a plugin asked for), a
//! [`Plugins`] registry is a `host::Service` that `host::HostLoop` turns once per turn of the loop
//! (`HostLoop::set_plugins`), and the server routes the plugins' commands and `plugin-attach`. Every
//! host in this crate (`zeropty serve`, `embed::Embedded`, `embed::Polled`) runs it that way.
//!
//! # Stability
//!
//! The boundary is the module boundary:
//!
//! - [`api`] (re-exported here, so `zeropty::plugin::Plugin` and `zeropty::plugin::api::Plugin` are the
//!   same): everything a plugin author uses. [`Plugin`], [`Manifest`], [`Event`], [`ExitReason`],
//!   [`Host`], [`PluginError`], [`Events`], [`Access`], [`API_VERSION`], [`reply_wait`], [`is_reserved`]
//!   and the reply type ([`Reply`], [`ok`], [`fail`]). It only grows within an [`API_VERSION`].
//! - [`Plugins`] and [`Call`]: what an embedding host keeps. Every public method is stable.
//! - `__private`: what zeropty's own binary, bindings and tests use (the socket plugin, plugin
//!   libraries, the plugin directory, the protocol's JSON helpers). `#[doc(hidden)]` and named like
//!   serde's `__private`: public only because the binary and the bindings are other crates, unstable,
//!   outside semver, may change in any release. Nothing in the stable surface names or returns it.
//!
//! # Threading
//!
//! Every plugin runs on a thread of its own, never on the engine's: `Plugin::manifest` first (the thread
//! adding it waits for it, 1 s at most), then the hooks. The engine only queues events (with
//! none registered it queues nothing, and the hot paths pay one branch on a mask). The host's loop hands
//! them to each plugin's bounded queue with `try_send`: a plugin that falls behind loses events (counted)
//! instead of holding the engine. What a plugin asks of the engine (`Host::command`) runs on the engine's
//! thread like any other command. A plugin that panics is stopped and marked failed; one whose hook runs
//! past the deadline (its own time, not time waiting on the engine) is cut off. A command whose caller
//! gave up is either never run or reported "may still take effect", never reported failed and then run.
//!
//! A plugin is a [`Plugin`] (Rust, in the process) registered on a host ([`Plugins::add`], or
//! `Embedded::add_plugin` / `Polled::add_plugin`), or a program in any language that connects to the
//! socket and sends `plugin-attach`, or a C library run by `zeropty plugin-run`: the same registry, the
//! same events, the same isolation, no recompiling.
pub mod api;
mod hosting;
mod registry;
mod wire;

pub use api::*;
pub use hosting::Call;
pub(crate) use hosting::Due;
#[cfg(test)]
pub(crate) use registry::PLUGIN_TAG;
pub use registry::Plugins;
/// The registry's clock (ms), for hosts keeping calls' deadlines.
pub(crate) use registry::now_ms;

/// The implementation behind `__private` (src/plugin/__private/): the socket plugin is the server's
/// (crate::server); plugin libraries and the plugin directory are the binary's and the C binding's.
// Hidden like `__private`, which re-exports it: the protocol structs inside are internal, their fields
// documented by the protocol docs (docs/PLUGINS.md), not one by one.
#[doc(hidden)]
#[path = "plugin/__private"]
pub(crate) mod internal {
    pub mod launch;
    pub mod native;
    pub mod remote;
}

/// zeropty's own uses of the plugin machinery: the `zeropty` binary (plugin-run, the plugin directory),
/// the C binding, and the tests. **Not a stable interface**: anything here may change or go in any
/// release, without a version bump (outside semver). Plugin authors and embedding hosts use the rest of
/// this module: the stable surface is [`api`], [`Plugins`] and [`Call`]. Hidden from the docs, and no
/// stable item names or returns anything in here. It is public, not behind a cargo feature, so that the
/// binary stays in the root package (`cargo install --path .`): a package cannot turn a feature on for
/// its own binary only.
#[doc(hidden)]
pub mod __private {
    use super::Event;
    use crate::engine::{Backend, Engine};

    // Each file's own docs say what it is: the plugin directory, plugin libraries, the socket plugin.
    pub use super::internal::{launch, native, remote};
    pub use super::wire::{base64, call_json, reply_from_json, reply_json};

    /// The engine's session events to queue; none queues none. For tests that drive an engine without
    /// a registry.
    pub fn set_engine_events<B: Backend>(e: &mut Engine<B>, events: super::Events) {
        e.set_tap(events.bits(), 0)
    }

    /// The events the engine queued since the last call.
    pub fn take_engine_events<B: Backend>(e: &mut Engine<B>) -> Vec<Event> {
        e.take_tap().0
    }

    /// Whether the engine queued any (or counted any dropped).
    pub fn has_engine_events<B: Backend>(e: &Engine<B>) -> bool {
        e.has_tap_events()
    }
}
