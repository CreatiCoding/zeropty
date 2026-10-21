# Benchmarks

**English** · [한국어](ko/BENCHMARKS.md)

[README](../README.md) · [Docs](README.md) · [Usage](USAGE.md) · [Plugins](PLUGINS.md) · [FAQ](FAQ.md) · [Benchmarks](BENCHMARKS.md) · [Design](DESIGN.md) · [Versioning](VERSIONING.md) · [Releasing](RELEASING.md)

Every measurement is here. Items that fall short of 10x (⛔), the reasons why, and things we tried and did not ship are all recorded as they are. The README lists only the items whose whole range is 10x or better under [a single rule](#how-multipliers-are-computed).

## Summary: all items

**Of the 15 items measured, 11 are 10x or better than tmux across their whole range.** The remaining 4 (⛔) have a low end of 1.6\~8.9x. What sets those lower bounds is described in [Status of the 10x goal](#status-of-the-10x-goal-all-items) below. All values are real measurements taken alternately with tmux on the same machine (Mac mini M1).

One of the 11 crossed 10x only because tmux was slower: 5.4MB of output until it is visible on screen was borderline (9.9\~11.7×) on 2026-10-18 and is 10.5\~12.5× on 2026-10-21. zeropty took the same time both days (14\~16ms, then 15\~16ms), and tmux went from 159\~164ms to 169\~188ms on a busier machine. It is ✅ under [the rule](#how-multipliers-are-computed) but is not in the README.

| What | tmux | zeropty | Multiplier | Reduction |
|---|---|---|---|---|
| CPU, 400 real claude workloads | 54.30\~54.91% | 3.51\~3.84% | **14.1\~15.6×** | **−92\~93%** |
| Memory, 400 real claude workloads (standalone server) | 32.4\~32.5MB | 2.66\~2.70MB | **12.0\~12.2×** | **−91%** |
| Memory, 400 real claude workloads (embedded, polled) | 32.4\~32.5MB | 1.63\~1.64MB | **19.7\~19.9×** | **−94%** |
| Memory, 400 idle (embedded, polled) | 8.0MB | 0.45\~0.46MB | **17.3\~17.7×** | **−94%** |
| Memory, 200 idle (embedded + server plugin, also reachable via CLI and attach) | 5.0\~5.1MB | 388\~389KB | **12.8\~13.1×** | **−92%** |
| Memory, 400 redrawing screens | 251.2\~251.4MB | 1.71\~1.74MB | **144\~147×** | **−99%** |
| Memory, 400 idle (standalone server) | 8.0MB | 1.25\~1.27MB | 6.2\~6.4× ⛔ | −84% |
| `send-keys`, running a tmux command vs zeropty socket (Node) | 9.03\~9.56ms | 0.24\~0.26ms | **34.7\~39.8×** | **−97%** |
| `send-keys`, tmux control mode (`-C`) vs zeropty embedded (Node) | 0.42\~0.49ms | 0.06\~0.07ms | 5.9\~8.1× ⛔ (per run 7\~8× †) | −83\~87% |
| `send-keys`, tmux control mode (`-C`) vs zeropty socket (Node) | 0.42\~0.49ms | 0.24\~0.26ms | 1.6\~2.0× ⛔ (per run 1.7\~2.0× †) | −38\~51% |
| Keystroke → screen, tmux control mode (`-C`) vs zeropty embedded (Node) | 0.89\~0.98ms | 0.09\~0.10ms | 8.9\~10.8× ⛔ (per run 9\~10× †) | −88\~90% |
| `has-session` from the shell, tmux vs zeropty zsh function | 6.04\~6.19ms | 0.11\~0.17ms | **35.5\~56.2×** | **−97\~98%** |
| `send-keys` from the shell, tmux vs zeropty bash builtin | 6.02\~6.31ms | 0.043\~0.048ms | **125\~146×** | **−99%** |
| 5.4MB of output, until the program finishes | 166\~176ms | 3.0\~4.6ms | **36.0\~58.6×** | **−97\~98%** |
| 5.4MB of output, until it is visible on screen | 169\~188ms | 15\~16ms | **10.5\~12.5×** | **−90\~92%** |
| Session count limit | 511 (default PTY limit, max 999) | 1000 verified, no PTY limit | – | – |

### How multipliers are computed

Every multiplier and reduction in this document follows a single rule.

- **They are computed from the ends of the value ranges shown in the tables.** The low end is the fastest (smallest) tmux value ÷ the slowest (largest) zeropty value, and the high end is the reverse. A single value gives a single multiplier. Per-run pairings are not kept, so anyone can recompute the same multipliers from the tables alone.
- **Truncate, never round up.** Below 100x, digits past the first decimal place are dropped; at 100x and above, everything past the decimal point is dropped (13.96 → 13.9, 141.6 → 141). Reductions are also truncated to whole numbers. When units are mixed (MB and KB), 1MB = 1000KB is used and the smaller multiplier is reported.
- **The 10x verdict uses this same rule.** An item is ✅ and goes into the README only if the low end of its range is 10x or more. If the low end is under 10x, it is ⛔ even if the high end exceeds 10x.
- **Values shown for reference only: multipliers printed directly by `bench/latency.mjs` (†).** For each run, the script prints the multiplier between that run's unrounded p50 values (rounded to whole numbers for the embedded column and to one decimal place for the socket column). The three right-hand columns of the Node latency table and the synchronous client multiplier are the range over those three runs. Per-run p50 values were not kept, so these cannot be recomputed from the tables; they are not used for the verdict and are only shown alongside, marked †. For example, embedded `send-keys` (vs running tmux) is 140\~151× per run, and 128\~159× when divided using the ends of the value ranges. Keystroke → screen (vs tmux -C, embedded) reached 10× in some runs (9\~10× per run), but the ends of the value ranges give 8.9\~10.8×, so it is ⛔.

> The numbers in this document are always the latest measurements. Any change that moves a number updates this document and the README tables in the same commit.

**Measurement environment**

| Item | Value |
|---|---|
| Date | 2026-10-21 (server memory and CPU, embedded memory with and without the server plugin, Node latency three times, shell, bulk output, `wait-pane`, `grep-panes`), 2026-10-19 (C client and server plugin latency, polled event loop blocking) |
| Commit | `9694aa3` with the `bench/` scripts of this change (2026-10-21 measurements) |
| Machine | Apple M1 (4 P-cores + 4 E-cores), 16GB |
| OS | macOS 26.6.2 |
| Compared against | tmux 3.7c |

**Measurement conditions**

- Session settings: 220×80, 2000 lines of scrollback. tmux uses the same size, and 2000 lines is tmux's default scrollback.
- Memory: the average of 10 footprint samples taken every 2 seconds over 20 seconds.
- CPU: divided by the actual elapsed time of the same interval.
- Real workload replay: session i starts at the (i × 0.618 mod 1) point of the recording, so the load is the same across runs and servers.
- Repeating the same setup varies memory by about ±1.5% and CPU by about ±5% (the server often runs on an efficiency core).
- On 2026-10-21 other programs were running on the machine (load average about 2.9), so tmux and zeropty were both slower than on earlier dates in the latency and shell measurements. Every pair was still measured alternately under the same load, and the multipliers compare those pairs.

## Memory and CPU: standalone server (`bench/suite.sh`)

| Load | Sessions | tmux memory | zeropty memory | Multiplier | tmux CPU | zeropty CPU | Multiplier |
|---|---|---|---|---|---|---|---|
| Idle (one claude screen printed, then stopped) | 200 | 5.0\~5.1MB | 1.20MB | 4.1\~4.2× ⛔ | 0% | 0% | – |
| | 400 | 8.0MB | 1.25\~1.27MB | 6.2\~6.4× ⛔ | 0% | 0% | – |
| Screen redraw (31 lines redrawn every second) | 200 | 121.0\~121.3MB | 1.57\~1.61MB | **75.1\~77.2×** | 18.21\~18.31% | 1.02\~1.12% | **16.2\~17.9×** |
| | 400 | 251.2\~251.4MB | 1.71\~1.74MB | **144\~147×** | 28.71\~29.79% | 1.85\~2.09% | **13.7\~16.1×** |
| Whole claude transcript dump (15KB, every 2 seconds) | 200 | 7.3\~7.7MB | 1.71\~1.73MB | 4.2\~4.5× ⛔ | 26.74\~26.83% | 1.51\~1.61% | **16.6\~17.7×** |
| | 400 | 12.5\~13.6MB | 1.95\~2.30MB | 5.4\~6.9× ⛔ | 42.29\~42.82% | 2.96\~2.97% | **14.2\~14.4×** |
| Real claude workload replay (`tests/streams/claude2.trace`) | 200 | 17.4MB | 1.65\~1.66MB | **10.4\~10.5×** | 36.04\~36.67% | 2.24\~2.29% | **15.7\~16.3×** |
| | 400 | 32.4\~32.5MB | 2.66\~2.70MB | **12.0\~12.2×** | 54.30\~54.91% | 3.51\~3.84% | **14.1\~15.6×** |
| Idle | 1000 | Not possible (PTY limit) | 1.38\~1.39MB | – | Not possible | 0% | – |
| Real claude workload replay | 1000 | Not possible (PTY limit) | 4.65MB | – | Not possible | 6.69\~7.10% | – |

Measured on 2026-10-21, `bench/suite.sh` twice; ranges are the two runs. MB here is footprint KB ÷ 1000.
- **The 200-session real workload now passes 10x (8.4\~9.0× on 2026-10-19).** zeropty went from 1.92\~2.05MB to 1.65\~1.66MB, while tmux stayed at 17.4MB. The CPU of the 200-session transcript dump went from borderline (9.9\~10.2×) to 16.6\~17.7×, as tmux used more CPU (17.9% → 26.74\~26.83%) and zeropty less (1.75\~1.80% → 1.51\~1.61%). We did not look for the cause of either.

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
| Idle | 14.9\~15.0KB | 0.24\~0.32KB | **46.5\~62.5×** |
| Screen redraw | 649\~652KB | 0.54\~0.83KB | **781\~1207×** |
| Whole claude transcript dump | 23.8\~31.8KB | 1.1\~3.0KB | 7.9\~28.9× ⛔ |
| Real claude workload | 74.8\~75.3KB | 5.0\~5.3KB | **14.1\~15.0×** |

Computed from the footprints in KB of the two runs (2026-10-21), pairing the ends of the ranges as everywhere else. The transcript dump is ⛔ because zeropty's 400-session footprint differed by 352KB between the two runs (1948KB and 2300KB); it was 13.2× on 2026-10-19.

**PTY usage:** with 400 sessions, tmux uses 435 and zeropty uses 35. The 35 is the system baseline, so it stays the same even at 1000 sessions.

## Memory: embedded (the engine inside your process, `bench/embed_host.c`)

We embedded the engine in a C program that does nothing else, and measured the footprint difference between before the engine is started and after the sessions are up. Hosts like Node, whose own heap moves by several MB, cannot be measured this precisely, so a C host is used.

The engine can run in two modes. In **engine thread** mode (`zeropty_open`, Node `{ polled: false }`), the engine has its own thread. In **polled** mode (`zeropty_open_polled`, the Node default), there is no thread and the host's event loop drives the engine. Values are the range of two measurements of each mode on 2026-10-21, and the tmux server column is the standalone server table above.

| Load | Sessions | tmux server | zeropty engine thread | Multiplier | zeropty polled | Multiplier |
|---|---|---|---|---|---|---|
| Idle | 200 | 5.0\~5.1MB | 0.68\~0.69MB | 7.2\~7.5× ⛔ | **0.39MB** | **12.8\~13.0×** |
| | 400 | 8.0MB | 0.74MB | **10.8×** | **0.45\~0.46MB** | **17.3\~17.7×** |
| Real claude workload replay | 200 | 17.4MB | 1.65\~1.66MB | **10.4\~10.5×** | **0.95\~1.32MB** | **13.1\~18.3×** |
| | 400 | 32.4\~32.5MB | 2.25\~2.33MB | **13.9\~14.4×** | **1.63\~1.64MB** | **19.7\~19.9×** |

- **Just starting the engine thread costs +308\~309KB.** That is the thread's allocator heap, stack and page tables. Polled is +180\~181KB.
- **Polled 200 idle went from 0.31MB (16.4×, 2026-10-18) to 0.39MB (12.8\~13.0×).** Starting the engine went from +149KB to +180\~181KB, and the 200 sessions from +160KB to +208KB. We have not found which change since then added it.
- **Polled also has a smaller per-session share.** All allocations are gathered in a single heap on the host thread (200 idle: 368\~384KB → 208KB).
- **The cost of polled:** the host thread parses session output (\~3.5% of one core with 400 real workloads). All commands must be called from that thread (Node has a single JS thread anyway, so this is no restriction; use `{ polled: false }` only when calling from worker threads).
- **Bulk output is parsed in pieces.** Polled reads output chunks 256KB at a time, at most 2ms per turn, and then gives the turn back to the host loop. With 5.4MB of output coming into 200 busy sessions, the longest the Node event loop was blocked went from 11\~32ms to 3\~6ms (`bench/loop_block.mjs`, alternated 3 times). Time until everything is on screen and memory are unchanged. The engine-thread mode also blocked for up to 30ms, from contention for the CPU.
- Per session: 0.24\~0.32KB idle, 3.0\~3.4KB for real workloads (engine-thread mode, 200 → 400 sessions).
- **Trade-off:** screens that have been quiet for more than 10 seconds are compressed with zstd and kept in an unnamed temporary file. They leave process memory but stay in the OS page cache (the OS reclaims it when needed). `ZEROPTY_COLD=memory` keeps them in memory instead.

**With the server plugin on (`listen`, `bench/embed_host.c ... polled <socket>`, 2026-10-21, on and off alternated 2 times).** We measured the same C host with and without the socket open.

| Load | Sessions | tmux server (table above) | No plugin | Plugin on | vs tmux server (on) |
|---|---|---|---|---|---|
| Engine only | 0 | – | +180\~181KB | +180\~181KB | – |
| Idle | 200 | 5.0\~5.1MB | 388KB | 388\~389KB | **12.8\~13.1×** |
| Real claude workload replay | 200 | 17.4MB | 949\~1322KB | 973\~1248KB | **13.9\~17.8×** |
| | 400 | 32.4\~32.5MB | 1630\~1636KB | 1395\~1740KB | **18.6\~23.2×** |

- **The plugin itself now adds 0\~1KB** (+16\~18KB on 2026-10-19: one socket and the connection list). In the same period polled's start cost rose by about 31KB, so the engine may now set up at start what `listen` used to add; we have not checked. The differences under the real workload are within noise that flips up and down from run to run.
- **So you can use the CLI, attach and socket clients while keeping embedded-level memory.** The same 200 idle sessions on a standalone server take 1.20MB (4.1\~4.2× ⛔). The difference is the share of one server process.
- **Caveat:** these values are increments assuming the host program exists anyway (excluding the host itself, \~1.08MB). If you start a host just to hold sessions, that is the same as a standalone server.

## Latency (`node bench/latency.mjs 200`)

We measured four ways a Node program can talk to a session server, side by side.

| Method | Description |
|---|---|
| tmux | Run tmux for each command via Node `execFile` (the common way programs call tmux) |
| tmux -C | Control mode: keep one tmux client open and send commands over stdin (the fastest way to use tmux) |
| zeropty | Keep one connection and send requests directly to the server socket (`client/zeropty.mjs`) |
| zeropty embedded | Embed the engine in the Node process and call it as a function (`client/embedded.mjs`) |

Each server ran 200 busy sessions, and requests were sent **alternately**. 40 times per item; values are the range of p50 (ms) over three runs on 2026-10-21. The embedded column is polled mode, the Node default. The three multiplier columns on the right are the range of the multipliers `bench/latency.mjs` printed per run (†). The 10x verdict uses the next table, with multipliers divided using the ends of the value ranges ([How multipliers are computed](#how-multipliers-are-computed)).

| Item | tmux | tmux -C | zeropty | zeropty embedded | Embedded vs tmux † | Embedded vs tmux -C † | zeropty vs tmux -C † |
|---|---|---|---|---|---|---|---|
| `capture-pane` (busy session) | 9.31\~9.90 | 0.55\~0.63 | 0.25\~0.30 | 0.10\~0.17 | **57\~93×** | 3\~5× ⛔ | 1.8\~2.5× ⛔ |
| `capture-pane -e -S -200` (quiet session) | 9.37\~10.14 | 0.45\~0.60 | 0.23\~0.27 | 0.06\~0.07 | **134\~172×** | 7\~10× ⛔ | 2.0\~2.3× ⛔ |
| `send-keys` | 9.03\~9.56 | 0.42\~0.49 | 0.24\~0.26 | 0.06\~0.07 | **140\~151×** | 7\~8× ⛔ | 1.7\~2.0× ⛔ |
| Keystroke → visible on screen | 24.62\~25.24 | 0.89\~0.98 | 0.35\~0.43 | 0.09\~0.10 | **250\~268×** | 9\~10× ⛔ (8.9\~10.8× by value range) | 2.1\~2.8× ⛔ |

These are multipliers for the same p50 ranges, computed with [the single rule](#how-multipliers-are-computed) (divide using the ends of the value ranges, truncate). The 10x verdict uses this table.

| Item | zeropty vs tmux | Embedded vs tmux | Embedded vs tmux -C | zeropty vs tmux -C |
|---|---|---|---|---|
| `capture-pane` (busy session) | **31.0\~39.6×** | **54.7\~99.0×** | 3.2\~6.3× ⛔ | 1.8\~2.5× ⛔ |
| `capture-pane -e -S -200` (quiet session) | **34.7\~44.0×** | **133\~169×** | 6.4\~10.0× ⛔ | 1.6\~2.6× ⛔ |
| `send-keys` | **34.7\~39.8×** | **128\~159×** | 5.9\~8.1× ⛔ | 1.6\~2.0× ⛔ |
| Keystroke → visible on screen | **57.2\~72.1×** | **246\~280×** | 8.9\~10.8× ⛔ | 2.0\~2.8× ⛔ |

- **The embedded columns are polled mode; engine-thread mode is at the same level.** Measured alternately, twice each, on 2026-10-21, polled vs engine thread p50: busy-session capture 0.08\~0.09 vs 0.08\~0.11, quiet-session capture 0.06\~0.07 vs 0.05\~0.07, `send-keys` 0.06 vs 0.06, keystroke→screen 0.10 vs 0.08\~0.09. So the mode does not explain why embedded `send-keys` went from 0.04\~0.06ms (2026-10-18, engine thread) to 0.06\~0.07ms; the machine was busier, and we have not checked further.

- **Embedded `send-keys` (vs tmux -C) went from 10\~11x per run to 8\~10x (†, ⛔) when the line discipline was added (2026-10-18), and is 7\~8x per run on 2026-10-21.** The measured session (`cat`) is now in cooked mode, like a PTY. That adds one mode-check read and echo handling per key (about 10µs per key). tmux handles the same echo too, but on the server side, after send-keys has run. Raw-mode sessions (claude) get no extra work (`examples/sendkeys_bench.rs`, same as before the change). The mode-check read could be removed by having the shim report the mode through shared memory instead of output, but we did not ship that. The only gain would be about 8µs per key in cooked sessions (25ms interval, cooked 22.9µs vs raw 14.7µs), and claude, the real target, is already raw.

- **We broke down why embedded `capture-pane` (busy session) swings so much (5\~12x per run on 2026-10-18, 3\~5x on 2026-10-21; broken down on 2026-10-18, 200 busy sessions).**
  - Capturing the same screen back to back takes 5.9µs. Called at 25ms intervals, rendering alone takes 27µs. That is due to the clock and caches of a CPU waking up from idle. Measuring with 1 session gave the same result, and raising the QoS of the calling thread made almost no difference.
  - For a busy session, capture parses on the spot the output collected over 300ms. That is 15\~30µs at p50, and up to 0.5ms if 8\~12KB has piled up. If the engine thread is reading another session, capture waits for the lock (p99 up to 0.5ms).
  - Caching the capture result of an unchanged screen would remove rendering even in the cold state, but we did not ship it. It is about 17KB per screen, so 1000 sessions would add 17MB.
  - Reading recently captured sessions immediately would remove this part, but we did not ship it. For a polling host, the polling interval (hundreds of ms) dominates, so there is no perceptible gain, and CPU goes up (immediate reads: +0.8%p for 200 sessions). For cases that need to know quickly, there is `wait-pane`.
- **Why the multipliers swing:** tmux -C is 0.4\~0.6ms and embedded is around 0.05ms, so a 0.01ms difference moves the multiplier by 2\~3. That is why ranges are given rather than a single value. Busy-session capture includes the cost of first reading the output that has piled up, then drawing the screen.
- **The zeropty socket method is 31.0\~72.1x faster than running tmux per command** (first column of the second table above).
- **The socket method (zeropty) is 1.6\~2.8x vs tmux -C (⛔, last column of the second table above; 1.7\~2.8× † per run).** The Unix socket round trip in Node is itself the lower bound (an echo server that does nothing takes 0.33ms).
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
- **The synchronous client (`zeroptySync`, `Client` in `zeropty.node`) is 3.0\~4.1x per run (⛔ †, p50 0.13\~0.26ms, the same three runs).** It writes and reads with blocking I/O without going through the event loop, so its p50 is about 30\~46% lower than the async one's. The remaining lower bound is the round trip of two processes, the server and Node, waking each other in turn. When the server is busy, p95 spikes up to 1ms.
- **Embedded has no such lower bound.** The remaining lower bound is the time for the Node thread to sleep and wake (\~40\~50µs). Called back to back, it takes 2.6µs per call.

## Latency: from the shell (`bench/shell.zsh`)

From a shell script, 200 commands each, in alternating rounds: `bench/shell.zsh` run three times, two rounds each, six rounds in all (2026-10-21, Mac mini M1). Units are ms (p50 / p95), and ranges are the values of the six rounds.

| Command | tmux | zeropty program | zeropty zsh function | zsh function vs tmux |
|---|---|---|---|---|
| `has-session` | 6.04\~6.19 / 6.74\~7.20 | 3.59\~3.95 / 4.22\~4.85 | 0.11\~0.17 / 0.14\~0.38 | **35.5\~56.2×** |
| `send-keys -l` | 6.11\~6.33 / 6.92\~9.86 | 3.68\~3.92 / 4.25\~5.39 | 0.12 / 0.15\~0.20 | **50.9\~52.7×** |
| `capture-pane -p -S -2000` | 6.67\~6.75 / 7.41\~7.76 | 3.86\~4.23 / 4.47\~5.52 | 0.50\~0.55 / 0.57\~1.13 | **12.1\~13.5×** |

- **Both programs took longer than on 2026-10-18** (tmux 4.35\~5.02ms → 6.04\~6.75ms, the zeropty program 2.47\~2.60ms → 3.59\~4.23ms), on a busier machine. The zsh function changed little (0.11\~0.48ms → 0.11\~0.55ms), so its multipliers went up.
- **Program vs program is 1.5\~1.7x.** The cost of starting a process for each command (1.3ms even for an empty C program) is the lower bound.
- **The zsh function (`shell/zeropty.zsh`) avoids that lower bound.** It uses the socket module built into zsh (`zsh/net/socket`) to talk to the server directly from inside the shell, without starting a process.
- **bash does the same with a builtin (`shell/zeropty_bash.c` → `dist/zeropty_bash.dylib`).** bash 3.2 has no µs clock, so the values are averages: the total time of a 200-iteration loop minus the time of an empty loop (at the end of the same script).

| Command (bash, average, six rounds) | tmux | zeropty program | zeropty bash builtin | builtin vs tmux |
|---|---|---|---|---|
| `has-session` | 5.968\~6.470ms | 3.630\~3.860ms | 0.041\~0.075ms | **79.5\~157×** |
| `send-keys -l` | 6.022\~6.313ms | – | 0.043\~0.048ms | **125\~146×** |
| `capture-pane -p -S -2000` | 6.733\~7.066ms | – | 0.114\~0.138ms | **48.7\~61.9×** |

Six rounds, as above. One round of the builtin's `has-session` came out at 2.448ms, against 0.041\~0.075ms in the other five; the other commands in that round were normal, so we take it for a stall of the machine (not checked) and leave it out of the range. Counting it, the low end would be 2.4×.

## Bulk output

A program writes 5.4MB (60,000 lines, with colors) as fast as it can (`bench/bulk.sh`, 2026-10-21, Mac mini M1, alternately with tmux: 3 runs each, twice).

| Item | tmux | zeropty | Multiplier |
|---|---|---|---|
| Until the program finishes writing | 166\~176ms | 3.0\~4.6ms | **36.0\~58.6×** |
| From when the program starts writing until the last line is visible in `capture-pane` | 169\~188ms | 15\~16ms | **10.5\~12.5×** |

- **The screen time passes 10x only because tmux was slower.** On 2026-10-18 it was 159\~164ms against zeropty's 14\~16ms (9.9\~11.7×, borderline). zeropty took the same time on both days.

- **When output fills the pipe, zeropty first reads everything until the program stops, and parses afterwards.** So the program never waits on parsing. Previously it read 16KB at a time and the program was blocked while parsing, which took 17ms. The lower bound is 1.6ms, the time the same program takes writing to `cat > /dev/null`.
- **On the screen side, compression of rows scrolling out is deferred until the end of a single write.** Of the 60,000 lines, only the rows that remain in scrollback (2000 lines) are compressed. Parsing 5.4MB went from 16.5ms to about 9ms (`cargo run --release --example parse_bench bulk`).
- Screen time is measured by calling `capture-pane` without pause. Each call starts a process (zeropty \~2.5ms, tmux \~4.5ms), so there is that much error. The previous measurement (31\~41ms) included the cost of `new-session` and of running perl to take timestamps.

## Waiting: `wait-pane` instead of polling (`node bench/wait.mjs 200`)

We compared two ways for a host to find out "whether a session has finished its work". 200 sessions replay a real claude workload (`tests/streams/claude2.trace`) for 5\~25 seconds and then print `TASK-DONE`.
- **Polling:** the host reads the remaining sessions with `capture-pane` every P ms. This is the only option for a tmux host.
- **`wait-pane`:** send `wait-pane -m TASK-DONE` once per session and wait for the reply.

Measured over two alternating rounds on a Mac mini M1, 2026-10-21. "Detection latency" is from the moment the line is printed until the host notices.

| Method | Server CPU | Host (Node) CPU | Detection latency p50 / p95 |
|---|---|---|---|
| 500ms polling | 2.0% | 0.6% | 253\~262 / 479\~489ms |
| 200ms polling | 2.9\~3.0% | 1.1\~1.3% | 86\~96 / 173\~196ms |
| **`wait-pane`** | 3.8\~3.9% | **0.3\~0.4%** | **1 / 2ms** |

- **Time to notice is 86\~262x shorter (p50 86\~262ms → 1ms).** Automation that repeats "send the next input when the prompt appears" gets that much faster at every step. The polling code in the host also goes away.
- **The CPU is not free.** Server CPU rises by 0.8\~1.0%p over 200ms polling (200 sessions). That is because waiting sessions are read immediately rather than having output collected over 300ms. Just reading the same sessions without batching raised server CPU from 2.0% to 2.8%. Once the wait ends, it goes back to normal.
- **Text matching only checks the rows changed by the current read.** The first version converted the whole screen to text and checked it on every read, and server CPU was 4.4\~6.3%. Checking only changed rows brought it to 2.7\~3.2%, and the remaining cost is that of immediate reads. On a screen switch (leaving the alternate screen), old rows become visible again, so the whole screen is checked.
- **We tried waiting while keeping the batched reads.** Server CPU was 15% lower than polling, but detection latency was p50 340ms, slower than 500ms polling. So we chose immediate reads.

## Finding text across sessions: `grep-panes` (`bench/grep.zsh 1000`)

Out of 1000 idle claude screens, 3 were marked, and this is the time it took to find those 3 (2026-10-21, Mac mini M1, `bench/grep.zsh` twice, two rounds each).

| Method | Time | Server memory (1.41MB before) |
|---|---|---|
| `capture-pane` per session via the CLI | 4.2\~4.3 s | 1.70\~2.05MB |
| `capture-pane` per session via the zsh function | 0.88\~1.02 s | 1.71\~1.81MB |
| `capture-pane` per session via Node (`client/zeropty.mjs`) | 92\~118ms | 2.11\~2.48MB |
| **`grep-panes`** | **16\~32ms** | **1.60\~1.76MB** |

- **"Which session is waiting for input?" from the terminal goes from about 4 seconds to 16\~32ms.**
- The memory after each method includes screens left expanded by the rounds before it; `grep-panes` runs first in each round, and its first-round value is 1.60\~1.63MB.
- **Screens are not thawed.** `capture-pane` keeps the screens it reads expanded until they freeze again (a little over +1MB for 1000), while `grep-panes` reads a copy of the frozen screen.
- tmux has no command for searching screen contents (`find-window -C` is an interactive chooser).

## Status of the 10x goal (all items)

| Item | Status |
|---|---|
| CPU: every load at 200 and 400 (screen redraw, whole claude transcript dump, real claude workloads) | ✅ 13.7\~17.9× |
| Memory: screen redraw, real claude workloads at 200 and 400 (standalone server) | ✅ 10.4\~147× |
| Memory: embedded 400 idle, 200 and 400 real workloads | ✅ 13.1\~19.9× (polled), 10.4\~14.4× (engine thread, 400 idle 10.8×) |
| Memory: increment per session | ✅ idle, screen redraw and real workloads 14.1\~1207×. ⛔ whole claude transcript dump 7.9\~28.9× (a 352KB difference between two runs, see the standalone server section) |
| Latency: vs running tmux per command | ✅ socket 31.0\~72.1×, embedded 54.7\~280× (per run 57\~268× †) |
| Latency: vs tmux control mode, embedded | ⛔ keystroke→screen 8.9\~10.8× (per run 9\~10× †), send-keys 5.9\~8.1× (echo in cooked sessions), capture 3.2\~10.0× (causes in the latency section above). The low end of the range is under 10x, so these are not in the README |
| Latency: vs tmux control mode, socket | ⛔ 1.6\~2.8× (async, per run 1.7\~2.8× †), 3.0\~4.1× (synchronous client, †), lower bound of an inter-process round trip |
| Absolute memory: standalone server idle, whole claude transcript dump | ⛔ 4.1\~6.9×, server process lower bound (empty C process 0.87MB, memory section below) (surpassed with embedded polled: idle 12.8\~17.7×, real workloads 13.1\~19.9×. Even with the server plugin on and sessions handled through the CLI and attach, 200 idle is 12.8\~13.1×) |
| Memory: embedded 200 idle | ✅ 12.8\~13.0× (polled, the Node default). Engine-thread mode is ⛔ 7.2\~7.5×, thread cost 308\~309KB |
| Bulk output: until the program finishes writing | ✅ 36.0\~58.6× |
| Bulk output: until visible on screen | ✅ 10.5\~12.5× (borderline 9.9\~11.7× on 2026-10-18; tmux was slower this time and zeropty the same. Screen time includes the error of starting `capture-pane` as a process; see the bulk output section above) |
| Latency: from the shell, zsh function vs tmux CLI | ✅ 12.1\~56.2× |
| Latency: from the shell, bash builtin vs tmux CLI | ✅ 48.7\~157× (program vs program 1.5\~1.7×, lower bound of process startup cost) |

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
