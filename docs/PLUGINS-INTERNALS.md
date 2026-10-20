# Plugin internals

**English** · [한국어](ko/PLUGINS-INTERNALS.md)

[README](../README.md) · [Docs](README.md) · [Usage](USAGE.md) · [Plugins](PLUGINS.md) · [FAQ](FAQ.md) · [Benchmarks](BENCHMARKS.md) · [Design](DESIGN.md) · [Versioning](VERSIONING.md) · [Releasing](RELEASING.md)

This page is for people who change zeropty itself or write their own host loop. How to write plugins and the rules they follow are in [PLUGINS.md](PLUGINS.md).

## Structure

The plugin layer sits above the engine, next to the socket server. Dependencies only point downward (see the layer table in `src/lib.rs`).

```
embed (Embedded, Polled)         ─┐
zeropty serve (src/cli/serve.rs) ─┴→ host::HostLoop ──┬→ Service: server::Server   (socket, plugin-attach, plugin command routing)
                                                       └→ Service: plugin::Plugins  (registry: event delivery, commands issued by plugins, watchdog)
                                                                  ↓
                                   engine (engine::tap: queues only the session event kinds someone wants)
```

- **Engine** (`src/engine/tap.rs`): queues session creation, exit, rename and output as events. Event kinds that no plugin wants are skipped with a single mask branch, and nothing is built for them. `Event` and `ExitReason` are plain data defined here, so the engine knows nothing about the layers above it; `zeropty::plugin` re-exports the same types.
- **Registry** (`Plugins`): a `Service` of the host loop. It plugs into exactly the same slot as the server (`host::Service`), and `HostLoop` runs it after the server on every turn (`after_tick` = event delivery, running commands that plugins issued, and the watchdog). The event that plugin threads use to wake the loop (a user event on the engine queue, registered by `Plugins::attach`) is filtered out by the service's `owns`, so it never leaks to the host. The sleep time is bounded by `Plugins::watchdog_ms` together with the server's `wait_timeout`.
- **Server** (`server::Server::set_plugins`): handles `list-plugins`, routing of plugin commands (only the calling connection waits for the reply), and `plugin-attach` (which hands the connection over to become a plugin).
- As a result, all three hosts (`zeropty serve`, `Embedded`, `Polled`) run plugins in the same order. A unit test in `host.rs` (`plugins_are_pumped_after_the_service_and_cap_the_wait`) checks that order with a fake engine.

## Cost with no plugins

What extra work is done is described in [PLUGINS.md](PLUGINS.md#cost-with-no-plugins).

These are measured values with no plugins loaded. On 2026-10-20, on the same Mac mini (M1), `main` 015f4da (no plugins) and this branch at c7ef81f (the commit where `Polled::timeout_ms` next_due, `command_start` and `process_within` in `src/embed.rs` took their current form) were run alternately, 3 times each. The load average was 2.5\~5.0, so compare the two columns rather than the absolute values. Ranges are min\~max over the 3 runs (for hostloop_bench, the last three rounds of each run, so 9 values).

| Measurement | `main` 015f4da | c7ef81f |
|---|---|---|
| One polled turn, idle session (`process()` + `timeout_ms()`, median) | 12.58\~12.63 us | 12.58\~12.61 us |
| One polled turn, session replaying claude output | 12.54\~13.26 us | 12.60\~12.94 us |
| `Polled::command` display-message | 446\~481 ns | 446\~470 ns |
| `Embedded::command` display-message | 455\~492 ns | 457\~485 ns |
| Embedded capture-pane, idle session, p50 | 4.3\~5.0 us | 3.7\~4.4 us |
| Embedded capture-pane -e -S -200, idle session, p50 | 4.7\~5.5 us | 4.7\~5.4 us |
| Embedded capture-pane, session producing output (`BUSY=1`), p50 | 5.7\~11.7 us | 6.0\~13.5 us |
| Embedded send-keys cooked, p50 | 40.0\~42.5 us | 41.0\~45.0 us |
| Embedded send-keys raw, p50 | 30.8\~37.9 us | 32.9\~35.5 us |

Every difference is within the noise of re-running the same build (the ranges overlap, and on noisy rows such as capture-pane on a busy session the order flips from run to run). To measure again, run the two builds alternately.

This table was measured at c7ef81f. The only engine changes in later commits are inside `if stats.tap` in `src/engine/session.rs` and in the `#[cold]` functions of `src/engine/tap.rs` that are called only from there (`copy`, `read_now`: the 64KB chunking and byte budgets that run only when some plugin wants output). So the code that runs with no plugins is the same as at c7ef81f (`git diff c7ef81f..HEAD -- src/engine`).

```sh
cargo run --release --example hostloop_bench    # one polled turn and the command path (tune with N, ROUNDS; median per round)
cargo run --release --example capture_bench     # BUSY=1 for a session producing output
cargo run --release --example sendkeys_bench
```

To measure on `main`, extract it with `git archive main | tar -x -C <dir>` and copy `examples/hostloop_bench.rs` into it (`main` does not have it). The table above uses the last three rounds of each run (the first two rounds are warm-up).
