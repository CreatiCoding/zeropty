# Benchmarks

**English** · [한국어](ko/BENCHMARKS.md)

[README](../README.md) · [Docs](README.md) · [Usage](USAGE.md) · [Plugins](PLUGINS.md) · [FAQ](FAQ.md) · [Benchmarks](BENCHMARKS.md) · [Design](DESIGN.md) · [Versioning](VERSIONING.md) · [Releasing](RELEASING.md)

Every measurement is here. Items that fall short of 10x (⛔), the reasons why, and things we tried and did not ship are all recorded as they are. The README lists only the items whose whole range is 10x or better under [a single rule](#how-multipliers-are-computed).

## Summary: all items

**Of the 15 items measured, 10 are 10x or better than tmux across their whole range.** 1 item (5.4MB of output until it is visible on screen, 9.9\~11.7×) is borderline, with the low end of its range just under 10x, and the remaining 4 (⛔) have a low end of 1.8\~8.3x. What sets those lower bounds is described in [Status of the 10x goal](#status-of-the-10x-goal-all-items) below. All values are real measurements taken alternately with tmux on the same machine (Mac mini M1).

| What | tmux | zeropty | Multiplier | Reduction |
|---|---|---|---|---|
| CPU, 400 real claude workloads | 51.5% | 3.55\~3.69% | **13.9\~14.5×** | **−92\~93%** |
| Memory, 400 real claude workloads (standalone server) | 31.9MB | 2.64\~2.72MB | **11.7\~12.0×** | **−91%** |
| Memory, 400 real claude workloads (embedded, polled) | 31.1MB | 1.82MB | **17.0×** | **−94%** |
| Memory, 400 idle (embedded, polled) | 8.0MB | 0.39\~0.41MB | **19.5\~20.5×** | **−94\~95%** |
| Memory, 200 idle (embedded + server plugin, also reachable via CLI and attach) | 5.1MB | 325\~327KB | **15.5\~15.6×** | **−93%** |
| Memory, 400 redrawing screens | 245MB | 1.55\~1.73MB | **141\~158×** | **−99%** |
| Memory, 400 idle (standalone server) | 8.0MB | 1.28\~1.30MB | 6.1\~6.2× ⛔ | −83\~84% |
| `send-keys`, running a tmux command vs zeropty socket (Node) | 6.43\~10.37ms | 0.22\~0.24ms | **26.7\~47.1×** | **−96\~97%** |
| `send-keys`, tmux control mode (`-C`) vs zeropty embedded (Node) | 0.44\~0.53ms | 0.04\~0.06ms | 7.3\~13.2× ⛔ (per run 8\~10× †) | −86\~92% |
| `send-keys`, tmux control mode (`-C`) vs zeropty socket (Node) | 0.44\~0.53ms | 0.22\~0.24ms | 1.8\~2.4× ⛔ (per run 1.9\~2.4× †) | −45\~58% |
| Keystroke → screen, tmux control mode (`-C`) vs zeropty embedded (Node) | 0.67\~1.01ms | 0.06\~0.08ms | 8.3\~16.8× ⛔ (per run 11\~13× †) | −88\~94% |
| `has-session` from the shell, tmux vs zeropty zsh function | 4.35\~4.45ms | 0.11\~0.20ms | **21.7\~40.4×** | **−95\~97%** |
| `send-keys` from the shell, tmux vs zeropty bash builtin | 4.56\~4.80ms | 0.037ms | **123\~129×** | **−99%** |
| 5.4MB of output, until the program finishes | 155\~158ms | 3.2\~3.8ms | **40.7\~49.3×** | **−97%** |
| 5.4MB of output, until it is visible on screen | 159\~164ms | 14\~16ms | 9.9\~11.7× (borderline) | −89\~91% |
| Session count limit | 511 (default PTY limit, max 999) | 1000 verified, no PTY limit | – | – |

### How multipliers are computed

Every multiplier and reduction in this document follows a single rule.

- **They are computed from the ends of the value ranges shown in the tables.** The low end is the fastest (smallest) tmux value ÷ the slowest (largest) zeropty value, and the high end is the reverse. A single value gives a single multiplier. Per-run pairings are not kept, so anyone can recompute the same multipliers from the tables alone.
- **Truncate, never round up.** Below 100x, digits past the first decimal place are dropped; at 100x and above, everything past the decimal point is dropped (13.96 → 13.9, 141.6 → 141). Reductions are also truncated to whole numbers. When units are mixed (MB and KB), 1MB = 1000KB is used and the smaller multiplier is reported.
- **The 10x verdict uses this same rule.** An item is ✅ and goes into the README only if the low end of its range is 10x or more. If the low end is under 10x, it is ⛔ even if the high end exceeds 10x.
- **Values shown for reference only: multipliers printed directly by `bench/latency.mjs` (†).** For each run, the script prints the multiplier between that run's unrounded p50 values (rounded to whole numbers for the embedded column and to one decimal place for the socket column). The three right-hand columns of the Node latency table and the synchronous client multiplier are the range over those three runs. Per-run p50 values were not kept, so these cannot be recomputed from the tables; they are not used for the verdict and are only shown alongside, marked †. For example, embedded `send-keys` (vs running tmux) is 150\~186× per run, and 107\~259× when divided using the ends of the value ranges. Keystroke → screen (vs tmux -C, embedded) was 11\~13× per run, but 8.3\~16.8× using the ends of the value ranges, so it is ⛔.

> The numbers in this document are always the latest measurements. Any change that moves a number updates this document and the README tables in the same commit.

**Measurement environment**

| Item | Value |
|---|---|
| Date | 2026-10-19 (server plugin memory and latency, server memory and CPU, C client latency, polled event loop blocking), 2026-10-18 (bulk output, shell, embedded memory), 2026-10-18 three times (Node latency) |
| Commit | `4151e01` (server memory and CPU, `bench/load.sh` alternated 2 times) |
| Machine | Apple M1 (4 P-cores + 4 E-cores), 16GB |
| OS | macOS 26.6.2 |
| Compared against | tmux 3.7c |

**Measurement conditions**

- Session settings: 220×80, 2000 lines of scrollback. tmux uses the same size, and 2000 lines is tmux's default scrollback.
- Memory: the average of 10 footprint samples taken every 2 seconds over 20 seconds.
- CPU: divided by the actual elapsed time of the same interval.
- Real workload replay: session i starts at the (i × 0.618 mod 1) point of the recording, so the load is the same across runs and servers.
- Repeating the same setup varies memory by about ±1.5% and CPU by about ±5% (the server often runs on an efficiency core).

## Memory and CPU: standalone server (`bench/suite.sh`)

| Load | Sessions | tmux memory | zeropty memory | Multiplier | tmux CPU | zeropty CPU | Multiplier |
|---|---|---|---|---|---|---|---|
| Idle (one claude screen printed, then stopped) | 200 | 5.1MB | 1.23MB | 4.1× ⛔ | 0% | 0% | – |
| | 400 | 8.0MB | 1.28\~1.30MB | 6.1\~6.2× ⛔ | 0% | 0% | – |
| Screen redraw (31 lines redrawn every second) | 200 | 120MB | 1.58\~1.61MB | **74.5\~75.9×** | 17.5% | 1.17\~1.22% | **14.3\~14.9×** |
| | 400 | 245MB | 1.55\~1.73MB | **141\~158×** | 29.3% | 2.09\~2.24% | **13.0\~14.0×** |
| Whole claude transcript dump (15KB, every 2 seconds) | 200 | 9.9MB | 1.72\~1.73MB | 5.7× ⛔ | 17.9% | 1.75\~1.80% | 9.9\~10.2× (borderline) |
| | 400 | 14.1MB | 1.88\~1.92MB | 7.3\~7.5× ⛔ | 36.5% | 3.01\~3.27% | **11.1\~12.1×** |
| Real claude workload replay (`tests/streams/claude2.trace`) | 200 | 17.4MB | 1.92\~2.05MB | 8.4\~9.0× ⛔ | 37.5% | 2.05\~2.48% | **15.1\~18.2×** |
| | 400 | 31.9MB | 2.64\~2.72MB | **11.7\~12.0×** | 51.5% | 3.55\~3.69% | **13.9\~14.5×** |
| Idle | 1000 | Not possible (PTY limit) | 1.39\~1.42MB | – | Not possible | 0% | – |
| Real claude workload replay | 1000 | Not possible (PTY limit) | 4.65\~4.73MB | – | Not possible | 7.5\~8.0% | – |

**Fixed so that all output of 1000 sessions flows (2026-10-18).** The previous measurement (4.66%) was taken while only 59% of the output was flowing. The cause was macOS pipes.
- macOS hands out pipe buffers from a system-wide 16MB. Once about 250 pipes have grown to 64KB, every remaining pipe gets only **512 bytes** (verified with 3000 pipes created by hand).
- A 512-byte pipe cannot be told apart through `fstat` or kevent (both report 16KB). Programs in such sessions wrote only 512 bytes per 300ms read and then waited.
- After session output was switched to **socketpair** (64KB buffer, separate per session), the amount the server read in the 1000-session replay went from 59% to **102%** of the recording rate. For 200 and 400 sessions it was 101% both before and after.
- So the CPU figure for 1000 sessions reflects processing more output.
- **The cost:** when zeropty stops reading, up to 64KB per session accumulates in the kernel socket buffers (mbufs). For 1000 sessions that is 64MB (this machine's mbuf pool is 256MB). With pipes, the system-wide cap was 16MB.

**Items marked ⛔ fall short of 10x because of the lower bound of a server process.** Even a zeropty server with 1 session is 1.09MB (2026-10-19). A C program that does nothing is 0.87MB, a `perl` process 1.36MB, and an empty tmux server 2.2MB.
- Even the 0.865MB of an empty C program is mostly the operating system's share (broken down with `footprint`): 209KB of kernel page tables (XNU counts these in the footprint too), about 200KB of system malloc metadata, about 240KB of data modified by libraries (`__DATA_DIRTY`, `__DATA`, shared cache), plus the stack and malloc regions. Link options (`-no_fixup_chains`, `-dead_strip`, a small stack) moved things only at noise level (±16KB), and Apple's own `/bin/sleep` is 832KB.
- **What could be reduced was the system malloc configuration (applied 2026-10-19).** The server re-executes itself with `MallocNanoZone=0 MallocMaxMagazines=1 MallocSpaceEfficient=1 MallocMediumZone=0` (libmalloc reads these only at startup). Screens are handled by mimalloc and system malloc only serves libSystem's share, so it is fine to shrink the regions and per-CPU caches to suit a single thread. Every load dropped by 144\~240KB (1 session 1233 → 1089KB, 200 idle 1.39\~1.44 → 1.23MB, 400 real workloads 2.96\~3.04 → 2.72\~2.82MB, alternated 2 times). CPU had the same distribution (400 real workloads, alternated 3 times: 1.47/1.53/4.01% vs 1.47/3.85/3.86%; it swings between two values depending on which core the server runs on). These settings are not passed on to programs running in sessions.
- zeropty's own share is the 384KB held by mimalloc. The actual heap is 8KB, but long-lived small objects are scattered across different 16KB pages by size class, and each page is held in full. mimalloc settings (eager commit, arena size) had no effect. `MallocNanoZone=0` alone saves only 16KB; it takes all four settings above together.
For example, for 400 idle sessions to reach 10x, it would need to be 0.80MB or less, which is smaller than an empty process.

**Could a server written in another language get past this?** We built empty servers and measured them (2026-10-18, Mac mini M1).

| Empty server | Memory |
|---|---|
| Static binary using only system calls (no libSystem) | Cannot run: the Apple Silicon macOS kernel SIGKILLs it immediately |
| C, `pause()` only / kqueue and socket server | 0.865MB / 0.865MB |
| Rust, empty `main` | 0.93MB |
| **zeropty empty server** | **1.15MB** |
| Go, socket server | 2.59MB |

Rewriting in C could save about 0.29MB on an empty server. But an empty C process (0.865MB) is already larger than the 10x bar for 400 idle sessions (0.80MB). So **it is impossible with a separate process, in any language.** The way past this lower bound is **embedded mode**, below. With no separate process, there is no process lower bound.

**Memory added per session when going from 200 to 400 sessions (standalone server):**

| Load | tmux | zeropty | Multiplier |
|---|---|---|---|
| Idle | 14.8KB | 0.24KB | **61.6×** |
| Screen redraw | 625KB | 0.8KB | **781×** |
| Whole claude transcript dump | 21.2KB | 1.6KB | **13.2×** |
| Real claude workload | 72.7KB | 3.5KB | **20.7×** |

**PTY usage:** with 400 sessions, tmux uses 438 and zeropty uses 38. The 38 is the system baseline, so it stays the same even at 1000 sessions.

## Memory: embedded (the engine inside your process, `bench/embed_host.c`)

We embedded the engine in a C program that does nothing else, and measured the footprint difference between before the engine is started and after the sessions are up. Hosts like Node, whose own heap moves by several MB, cannot be measured this precisely, so a C host is used.

The engine can run in two modes. In **engine thread** mode (`zeropty_open`, Node `{ polled: false }`), the engine has its own thread. In **polled** mode (`zeropty_open_polled`, the Node default), there is no thread and the host's event loop drives the engine. Polled values are the range of two measurements on 2026-10-18.

| Load | Sessions | tmux server | zeropty engine thread | Multiplier | zeropty polled | Multiplier |
|---|---|---|---|---|---|---|
| Idle | 200 | 5.1MB | 0.66\~0.69MB | 7.3\~7.7× ⛔ | **0.31MB** | **16.4×** |
| | 400 | 8.0MB | 0.72MB | **11.1×** | **0.39\~0.41MB** | **19.5\~20.5×** |
| Real claude workload replay | 200 | 16.9MB | 1.68MB | **10.0×** | **1.22\~1.29MB** | **13.1\~13.8×** |
| | 400 | 31.1MB | 2.13\~2.23MB | **13.9\~14.6×** | **1.82MB** | **17.0×** |

- **Just starting the engine thread costs +293KB.** That is the thread's allocator heap, stack and page tables. Polled is +149KB.
- **Polled also has a smaller per-session share.** All allocations are gathered in a single heap on the host thread (200 idle: 384KB → 160KB).
- **The cost of polled:** the host thread parses session output (\~3.5% of one core with 400 real workloads). All commands must be called from that thread (Node has a single JS thread anyway, so this is no restriction; use `{ polled: false }` only when calling from worker threads).
- **Bulk output is parsed in pieces.** Polled reads output chunks 256KB at a time, at most 2ms per turn, and then gives the turn back to the host loop. With 5.4MB of output coming into 200 busy sessions, the longest the Node event loop was blocked went from 11\~32ms to 3\~6ms (`bench/loop_block.mjs`, alternated 3 times). Time until everything is on screen and memory are unchanged. The engine-thread mode also blocked for up to 30ms, from contention for the CPU.
- Per session: 0.16KB idle, 2.7KB for real workloads (engine-thread mode).
- **Trade-off:** screens that have been quiet for more than 10 seconds are compressed with zstd and kept in an unnamed temporary file. They leave process memory but stay in the OS page cache (the OS reclaims it when needed). `ZEROPTY_COLD=memory` keeps them in memory instead.

**With the server plugin on (`listen`, `bench/embed_host.c ... polled <socket>`, 2026-10-19, on and off alternated 2 times).** We measured the same C host with and without the socket open.

| Load | Sessions | tmux server (table above) | No plugin | Plugin on | vs tmux server (on) |
|---|---|---|---|---|---|
| Engine only | 0 | – | +148\~149KB | +165\~167KB | – |
| Idle | 200 | 5.1MB | 308\~309KB | 325\~327KB | **15.5\~15.6×** |
| Real claude workload replay | 200 | 16.9MB | 1081\~1246KB | 959\~1251KB | **13.5\~17.6×** |
| | 400 | 31.1MB | 1605\~1673KB | 1587\~1888KB | **16.4\~19.5×** |

- **The plugin itself is +16\~18KB** (one socket and the connection list). The differences under the real workload are within noise that flips up and down from run to run.
- **So you can use the CLI, attach and socket clients while keeping embedded-level memory.** The same 200 idle sessions on a standalone server take 1.23MB (4.1× ⛔). The difference is the share of one server process.
- **Caveat:** these values are increments assuming the host program exists anyway (excluding the host itself, \~1.08MB). If you start a host just to hold sessions, that is the same as a standalone server.

## Latency (`node bench/latency.mjs 200`)

We measured four ways a Node program can talk to a session server, side by side.

| Method | Description |
|---|---|
| tmux | Run tmux for each command via Node `execFile` (the common way programs call tmux) |
| tmux -C | Control mode: keep one tmux client open and send commands over stdin (the fastest way to use tmux) |
| zeropty | Keep one connection and send requests directly to the server socket (`client/zeropty.mjs`) |
| zeropty embedded | Embed the engine in the Node process and call it as a function (`client/embedded.mjs`) |

Each server ran 200 busy sessions, and requests were sent **alternately**. 40 times per item; values are the range of p50 (ms) over three runs on 2026-10-18 (after the line discipline was added). The three multiplier columns on the right are the range of the multipliers `bench/latency.mjs` printed per run (†). The 10x verdict uses the next table, with multipliers divided using the ends of the value ranges ([How multipliers are computed](#how-multipliers-are-computed)).

| Item | tmux | tmux -C | zeropty | zeropty embedded | Embedded vs tmux † | Embedded vs tmux -C † | zeropty vs tmux -C † |
|---|---|---|---|---|---|---|---|
| `capture-pane` (busy session) | 9.09\~11.38 | 0.49\~1.18 | 0.23\~0.32 | 0.09\~0.12 | **75\~118×** | 4\~12× ⛔ | 2.2\~3.7× ⛔ |
| `capture-pane -e -S -200` (quiet session) | 9.85\~13.54 | 0.50\~0.78 | 0.20\~0.27 | 0.06\~0.07 | **154\~197×** | 9\~11× ⛔ | 2.5\~2.9× ⛔ |
| `send-keys` | 6.43\~10.37 | 0.44\~0.53 | 0.22\~0.24 | 0.04\~0.06 | **150\~186×** | 8\~10× ⛔ | 1.9\~2.4× ⛔ |
| Keystroke → visible on screen | 16.27\~25.58 | 0.67\~1.01 | 0.26\~0.34 | 0.06\~0.08 | **276\~318×** | 11\~13× ⛔ (8.3\~16.8× by value range) | 2.6\~3.0× ⛔ |

These are multipliers for the same p50 ranges, computed with [the single rule](#how-multipliers-are-computed) (divide using the ends of the value ranges, truncate). The 10x verdict uses this table.

| Item | zeropty vs tmux | Embedded vs tmux | Embedded vs tmux -C | zeropty vs tmux -C |
|---|---|---|---|---|
| `capture-pane` (busy session) | **28.4\~49.4×** | **75.7\~126×** | 4.0\~13.1× ⛔ | 1.5\~5.1× ⛔ |
| `capture-pane -e -S -200` (quiet session) | **36.4\~67.7×** | **140\~225×** | 7.1\~13.0× ⛔ | 1.8\~3.9× ⛔ |
| `send-keys` | **26.7\~47.1×** | **107\~259×** | 7.3\~13.2× ⛔ | 1.8\~2.4× ⛔ |
| Keystroke → visible on screen | **47.8\~98.3×** | **203\~426×** | 8.3\~16.8× ⛔ | 1.9\~3.8× ⛔ |

- **The embedded columns were measured in engine-thread mode.** After the Node default changed to polled (2026-10-19), polled and engine-thread were re-measured alternately, twice each. The p50 values were at the same level: busy-session capture 0.09\~0.13 vs 0.09\~0.16, quiet-session capture 0.05\~0.06 vs 0.05\~0.06, `send-keys` 0.05\~0.06 vs 0.05\~0.06, keystroke→screen 0.08 vs 0.07\~0.09.

- **Embedded `send-keys` (vs tmux -C) went from 10\~11x per run to 8\~10x (†, ⛔).** The measured session (`cat`) is now in cooked mode, like a PTY. That adds one mode-check read and echo handling per key (about 10µs per key). tmux handles the same echo too, but on the server side, after send-keys has run. Raw-mode sessions (claude) get no extra work (`examples/sendkeys_bench.rs`, same as before the change). The mode-check read could be removed by having the shim report the mode through shared memory instead of output, but we did not ship that. The only gain would be about 8µs per key in cooked sessions (25ms interval, cooked 22.9µs vs raw 14.7µs), and claude, the real target, is already raw.

- **We broke down why embedded `capture-pane` (busy session) swings between 5x and 12x (2026-10-18, 200 busy sessions).**
  - Capturing the same screen back to back takes 5.9µs. Called at 25ms intervals, rendering alone takes 27µs. That is due to the clock and caches of a CPU waking up from idle. Measuring with 1 session gave the same result, and raising the QoS of the calling thread made almost no difference.
  - For a busy session, capture parses on the spot the output collected over 300ms. That is 15\~30µs at p50, and up to 0.5ms if 8\~12KB has piled up. If the engine thread is reading another session, capture waits for the lock (p99 up to 0.5ms).
  - Caching the capture result of an unchanged screen would remove rendering even in the cold state, but we did not ship it. It is about 17KB per screen, so 1000 sessions would add 17MB.
  - Reading recently captured sessions immediately would remove this part, but we did not ship it. For a polling host, the polling interval (hundreds of ms) dominates, so there is no perceptible gain, and CPU goes up (immediate reads: +0.8%p for 200 sessions). For cases that need to know quickly, there is `wait-pane`.
- **Why the multipliers swing:** tmux -C is 0.4\~0.6ms and embedded is around 0.05ms, so a 0.01ms difference moves the multiplier by 2\~3. That is why ranges are given rather than a single value. Busy-session capture includes the cost of first reading the output that has piled up, then drawing the screen.
- **The zeropty socket method is 26.7\~98.3x faster than running tmux per command** (first column of the second table above).
- **The socket method (zeropty) is 1.5\~5.1x vs tmux -C (⛔, last column of the second table above; 1.9\~3.7× † per run).** The Unix socket round trip in Node is itself the lower bound (an echo server that does nothing takes 0.33ms).
- **Measured from a C program, the ratio is 1.7\~3.3x (`bench/latency_c.c`, 2026-10-18).** Taking Node out makes tmux -C faster too.

| From a C client (p50) | tmux -C | zeropty socket | Multiplier |
|---|---|---|---|
| `send-keys` | 22\~35µs | 10.5\~12.5µs | 1.7\~3.3× ⛔ |
| `capture-pane` (busy session) | 40µs | 13\~15µs | 2.6\~3.0× ⛔ |

  Both bottom out at one round trip between processes. Reaching 10x vs tmux -C would require 3.5µs or less, but a single socket round trip (two context switches) costs more than that. So changing languages does not help, and only embedded mode, which removes the round trip, gets past 10x.
- **The tables above were measured with requests sent back to back, without pauses.** In practice there are idle gaps between requests. So we measured again with an idle interval before each request (`latency_c ... 300 <interval µs>`, 2026-10-19, against a claude replay session rather than an idle `cat` session). With pauses, both get slower and the multiplier shrinks to 1.7x.

| C client, p50 | tmux -C | zeropty socket | Multiplier |
|---|---|---|---|
| `send-keys`, back to back | 26.1µs | 9.3µs | 2.8× ⛔ |
| `send-keys`, after 1ms idle | 65.5µs | 36.7µs | 1.7× ⛔ |
| `send-keys`, after 25ms idle | 99.2µs | 56.4µs | 1.7× ⛔ |
| `capture-pane`, after 25ms idle | 108.2µs | 62.3µs | 1.7× ⛔ |

- **The server plugin (a socket opened by an embedded host) is as fast as a standalone server.** `bench/listen_host.c` (a polled engine that opens a socket) and `zeropty serve` were measured alternately, 2 times, with the same C client (2026-10-19). The machine was slower that day, so tmux -C also came out about 2x the table above; the multipliers are similar.

| C client, p50 | tmux -C | Standalone server | Server plugin |
|---|---|---|---|
| `send-keys`, back to back | 26.6\~44.6µs | 12.0\~20.4µs | 12.4\~22.2µs |
| `capture-pane`, back to back | 41.5\~74.1µs | 14.6\~27.3µs | 14.2\~28.7µs |
| `send-keys`, after 25ms idle | 224\~252µs | 120\~124µs (1.8\~2.1×) | 126\~130µs (1.7\~2.0×) |
| `capture-pane`, after 25ms idle | 297\~360µs | 143\~149µs (1.9\~2.5×) | 148\~160µs (1.8\~2.4×) |

  With pauses, the plugin is 3\~8% slower. The cause has not been confirmed yet. The polled host waits on the engine fd with `poll` and then reads the kqueue once more in `zeropty_process`, so there is one extra system call.

- **With pauses, the floor is not the "channel" but the cost of waking a sleeping core (2026-10-19, measured directly in C).** We measured a round trip in which two processes exchange 1 byte, for each method. Back to back, the methods differ at 0.7\~10µs, but after even 1ms idle they are all 12\~21µs, and after 25ms idle 28\~34µs. Only spinning, which keeps the core running, gets 0.08µs, and it uses 100% of a core.

| Round-trip method (p50) | Back to back | After 1ms idle | After 25ms idle |
|---|---|---|---|
| socketpair (what zeropty uses) | 9.7µs | 17.6µs | 33.6µs |
| pipe | 5.6µs | 12.2µs | 28.2µs |
| Shared memory + `__ulock` wait | 6.5µs | 20.5µs | 28.2µs |
| Shared memory + 200µs spin, then wait | 0.7µs | 13.3µs | 28.3µs |
| `mach_eventlink` (the kernel hands off directly to the peer thread) | 2.1µs | 14.0µs | – |
| Shared memory + continuous spin | 0.08µs | 0.08µs | (100% of a core) |

  tmux -C takes 99µs after 25ms idle, so 10x would be 9.9µs, which is below this floor (28\~34µs). So 10x under the with-pauses condition is impossible with a separate process.
- **Processing time inside the server also grows when things are cold.** For `has-session` sent after 25ms idle, the time from the server receiving the request to writing the reply was 18.5µs (2.4µs when sent back to back). Socket read, JSON parsing, command execution and reply write all slowed down evenly, by 6\~7x. It is not any particular code but the cold CPU and caches.
- **Things we tried but did not ship (2026-10-19, all measured alternately):**
  - Server thread QoS set to USER_INTERACTIVE: 37.5\~39.9µs vs 39.1\~39.2µs, no difference.
  - LTO + `codegen-units=1` (packing the code together so less of it is touched when cold): 1\~2µs (3%), noise level.
  - Reading only as many bytes as kqueue reported, to skip the "nothing more" check read: no difference.
  - The server spinning for 200µs after replying, waiting for the next request: `send-keys` in back-to-back runs goes from 8.0µs to 4.5µs (5.8x vs tmux -C), but with pauses it is unchanged. In exchange, every request holds a core for 200µs, which eats into the CPU goal. Switching to shared memory would get down to 1.5\~2µs on paper, but again only for back-to-back runs, and a few µs are not perceptible at the speed of a human or of claude.
  - `mach_eventlink`: a thread waits on only one link, so putting it into a server that watches every connection with a single kqueue would need a thread per connection (an allocator heap per thread; +0.8MB in the earlier GCD experiment). Mach messages, which can be used together with kqueue, are only 2µs faster than sockets.
- **We did not ship a screen mirror on the client side (like mosh, or tmux `%output`) (2026-10-19).** If the client receives output through `attach -r` and interprets the screen itself, capture needs no round trip. But mirroring all 200 busy sessions took server CPU from 2.50% to 6.15%, because sessions someone is watching are read and sent immediately instead of being collected over 300ms. The client also has to re-interpret 573KB per second. And unlike a server capture, a mirror's capture does not check for pending output, so it may be as stale as it is fast. Keeping the batched reads leaves CPU unchanged but makes the screen up to 300ms late. `attach -r` already covers continuously receiving the screen, and `wait-pane` covers waiting for changes.
- **The synchronous client (`zeroptySync`, `Client` in `zeropty.node`) is 2.5\~4.5x (⛔, p50 0.11\~0.20ms, measured twice).** It writes and reads with blocking I/O without going through the event loop, so it is 35\~45% faster than the async one. The remaining lower bound is the round trip of two processes, the server and Node, waking each other in turn. When the server is busy, p95 spikes up to 1ms.
- **Embedded has no such lower bound.** The remaining lower bound is the time for the Node thread to sleep and wake (\~40\~50µs). Called back to back, it takes 2.6µs per call.

## Latency: from the shell (`bench/shell.zsh`)

From a shell script, 200 commands each, measured over two alternating rounds (2026-10-18, Mac mini M1). Units are ms (p50 / p95), and ranges are the values of the two rounds.

| Command | tmux | zeropty program | zeropty zsh function | zsh function vs tmux |
|---|---|---|---|---|
| `has-session` | 4.35\~4.45 / 4.94\~5.32 | 2.47 / 2.94\~3.12 | 0.11\~0.20 / 0.12\~0.37 | **21.7\~40.4×** |
| `send-keys -l` | 4.66\~4.78 / 5.27\~5.33 | 2.48\~2.49 / 2.86\~3.12 | 0.11 / 0.12\~0.14 | **42.3\~43.4×** |
| `capture-pane -p -S -2000` | 4.98\~5.02 / 5.71\~6.01 | 2.59\~2.60 / 3.00\~3.03 | 0.45\~0.48 / 0.48\~0.53 | **10.3\~11.1×** |

- **Program vs program is 1.6\~1.8x.** The cost of starting a process for each command (1.3ms even for an empty C program) is the lower bound.
- **The zsh function (`shell/zeropty.zsh`) avoids that lower bound.** It uses the socket module built into zsh (`zsh/net/socket`) to talk to the server directly from inside the shell, without starting a process.
- **bash does the same with a builtin (`shell/zeropty_bash.c` → `dist/zeropty_bash.dylib`).** bash 3.2 has no µs clock, so the values are averages: the total time of a 200-iteration loop minus the time of an empty loop (at the end of the same script).

| Command (bash, average, two rounds) | tmux | zeropty program | zeropty bash builtin | builtin vs tmux |
|---|---|---|---|---|
| `has-session` | 4.35\~4.66ms | 2.47\~2.58ms | 0.035\~0.115ms | **37.8\~133×** |
| `send-keys -l` | 4.56\~4.80ms | – | 0.037ms | **123\~129×** |
| `capture-pane -p -S -2000` | 5.18\~5.20ms | – | 0.095\~0.102ms | **50.7\~54.7×** |

## Bulk output

A program writes 5.4MB (60,000 lines, with colors) as fast as it can (`bench/bulk.sh`, 2026-10-18, Mac mini M1, measured alternately with tmux).

| Item | tmux | zeropty | Multiplier |
|---|---|---|---|
| Until the program finishes writing | 155\~158ms | 3.2\~3.8ms | **40.7\~49.3×** |
| From when the program starts writing until the last line is visible in `capture-pane` | 159\~164ms | 14\~16ms | 9.9\~11.7× (borderline) |

- **When output fills the pipe, zeropty first reads everything until the program stops, and parses afterwards.** So the program never waits on parsing. Previously it read 16KB at a time and the program was blocked while parsing, which took 17ms. The lower bound is 1.6ms, the time the same program takes writing to `cat > /dev/null`.
- **On the screen side, compression of rows scrolling out is deferred until the end of a single write.** Of the 60,000 lines, only the rows that remain in scrollback (2000 lines) are compressed. Parsing 5.4MB went from 16.5ms to about 9ms (`cargo run --release --example parse_bench bulk`).
- Screen time is measured by calling `capture-pane` without pause. Each call starts a process (zeropty \~2.5ms, tmux \~4.5ms), so there is that much error. The previous measurement (31\~41ms) included the cost of `new-session` and of running perl to take timestamps.

## Waiting: `wait-pane` instead of polling (`node bench/wait.mjs 200`)

We compared two ways for a host to find out "whether a session has finished its work". 200 sessions replay a real claude workload (`tests/streams/claude2.trace`) for 5\~25 seconds and then print `TASK-DONE`.
- **Polling:** the host reads the remaining sessions with `capture-pane` every P ms. This is the only option for a tmux host.
- **`wait-pane`:** send `wait-pane -m TASK-DONE` once per session and wait for the reply.

Measured over two alternating rounds on a Mac mini M1, 2026-10-18. "Detection latency" is from the moment the line is printed until the host notices.

| Method | Server CPU | Host (Node) CPU | Detection latency p50 / p95 |
|---|---|---|---|
| 500ms polling | 1.3\~1.4% | 0.5% | 257\~262 / 485\~486ms |
| 200ms polling | 1.7\~1.9% | 0.6\~0.8% | 82\~93 / 191\~196ms |
| **`wait-pane`** | 2.2\~2.7% | **0.3%** | **2 / 2ms** |

- **Time to notice is 41\~131x shorter (p50 82\~262ms → 2ms).** Automation that repeats "send the next input when the prompt appears" gets that much faster at every step. The polling code in the host also goes away.
- **The CPU is not free.** Server CPU rises by 0.5\~0.8%p over 200ms polling (200 sessions). That is because waiting sessions are read immediately rather than having output collected over 300ms. Just reading the same sessions without batching raised server CPU from 2.0% to 2.8%. Once the wait ends, it goes back to normal.
- **Text matching only checks the rows changed by the current read.** The first version converted the whole screen to text and checked it on every read, and server CPU was 4.4\~6.3%. Checking only changed rows brought it to 2.7\~3.2%, and the remaining cost is that of immediate reads. On a screen switch (leaving the alternate screen), old rows become visible again, so the whole screen is checked.
- **We tried waiting while keeping the batched reads.** Server CPU was 15% lower than polling, but detection latency was p50 340ms, slower than 500ms polling. So we chose immediate reads.

## Finding text across sessions: `grep-panes` (`bench/grep.zsh 1000`)

Out of 1000 idle claude screens, 3 were marked, and this is the time it took to find those 3 (2026-10-18, Mac mini M1, two rounds).

| Method | Time | Server memory (1.65MB before) |
|---|---|---|
| `capture-pane` per session via the CLI | 2.9\~3.0 s | 1.8\~2.4MB |
| `capture-pane` per session via the zsh function | 659ms | 2.0\~2.3MB |
| `capture-pane` per session via Node (`client/zeropty.mjs`) | 102\~105ms | 2.7\~2.8MB |
| **`grep-panes`** | **12\~17ms** | **1.78MB** |

- **"Which session is waiting for input?" from the terminal goes from 3 seconds to 12ms.**
- **Screens are not thawed.** `capture-pane` keeps the screens it reads expanded until they freeze again (a little over +1MB for 1000), while `grep-panes` reads a copy of the frozen screen.
- tmux has no command for searching screen contents (`find-window -C` is an interactive chooser).

## Status of the 10x goal (all items)

| Item | Status |
|---|---|
| CPU: screen redraw and real claude workloads at 200 and 400, whole claude transcript dump at 400 | ✅ 11.1\~18.2× |
| CPU: whole claude transcript dump at 200 | Borderline 9.9\~10.2× (not ✅, because the low end of the range is under 10x. The noise of repeated measurements with the same setup is ±5%) |
| Memory: screen redraw, 400 real claude workloads (standalone server) | ✅ 11.7\~158× |
| Memory: embedded 400 idle, 200 and 400 real workloads | ✅ 13.1\~20.5× (polled), 10.0\~14.6× (engine thread) |
| Memory: increment per session (all loads) | ✅ 13.2\~781× |
| Latency: vs running tmux per command | ✅ socket 26.7\~98.3×, embedded 75.7\~426× (per run 75\~318× †) |
| Latency: vs tmux control mode, embedded | ⛔ keystroke→screen 8.3\~16.8× (per run 11\~13× †), send-keys 7.3\~13.2× (echo in cooked sessions), capture 4.0\~13.1× (causes in the latency section above). The low end of the range is under 10x, so these are not in the README |
| Latency: vs tmux control mode, socket | ⛔ 1.5\~5.1× (async, per run 1.9\~3.7× †), 2.5\~4.5× (synchronous client, †), lower bound of an inter-process round trip |
| Absolute memory: standalone server idle, claude output, 200 real workloads | ⛔ 4.1\~9.0×, server process lower bound (empty C process 0.87MB, memory section below) (surpassed with embedded polled: idle 16.4\~20.5×, real workloads 13.1\~17.0×. Even with the server plugin on and sessions handled through the CLI and attach, 200 idle is 15.5\~15.6×) |
| Memory: embedded 200 idle | ✅ 16.4× (polled, the Node default). Engine-thread mode is ⛔ 7.3\~7.7×, thread cost 293KB |
| Bulk output: until the program finishes writing | ✅ 40.7\~49.3× |
| Bulk output: until visible on screen | Borderline 9.9\~11.7× (dividing using the ends of the range gives a low end of 9.9×. Screen time includes the error of starting `capture-pane` as a process; see the bulk output section above) |
| Latency: from the shell, zsh function vs tmux CLI | ✅ 10.3\~43.4× |
| Latency: from the shell, bash builtin vs tmux CLI | ✅ 37.8\~133× (program vs program 1.6\~1.8×, lower bound of process startup cost) |

## Reproducing the measurements

```sh
./build.sh
bash bench/suite.sh            # standalone server memory and CPU, about 15 minutes
clang -O2 -o /tmp/embed_host bench/embed_host.c -Ldist -lzeropty_c -Wl,-rpath,$PWD/dist
/tmp/embed_host 400 trace $PWD # embedded memory (idle | trace)
/tmp/embed_host 200 idle $PWD polled ~/.zeropty/eh.sock   # embedded memory with the server plugin on
clang -O2 -o /tmp/listen_host bench/listen_host.c -Ldist -lzeropty_c -Wl,-rpath,$PWD/dist
/tmp/listen_host ~/.zeropty/lh.sock $PWD &   # server plugin latency: latency_c ~/.zeropty/lh.sock <tmux -L name> 400 [interval µs]
node bench/latency.mjs 200     # latency
bench/bulk.sh zeropty; bench/bulk.sh tmux   # bulk output
bench/shell.zsh                # latency from the shell (zsh function, program, tmux)
node bench/wait.mjs 200        # polling vs wait-pane (CPU, detection latency)
bench/grep.zsh 1000            # finding text across 1000 sessions (grep-panes vs repeated capture)
```

Before measuring 1000 sessions, check the per-user process limit (`kern.maxprocperuid`, 2666 by default). `load.sh` checks it beforehand and stops if it would be exceeded.
