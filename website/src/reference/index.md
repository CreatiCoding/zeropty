---
description: "The zeropty reference, by area: commands, keys and formats, environment variables, tmux compatibility, the socket protocol, the C ABI, the Node.js API and the plugin API."
---

# Reference

Exact, complete descriptions of every command, option, protocol and API. The reference says what each thing is. For how to use it, with steps and examples, see the [Guide](/guide/introduction).

The command-line pages and the socket and plugin protocols are built from the repository's `docs/` each time the site is built, so they always match the code on `main`. The C ABI and Node.js pages are checked against the build's list of public symbols (`api/*.txt`).

## Command line

| Page | What it covers |
|---|---|
| [Commands](/reference/commands) | Every command and option, grouped as sessions, sending keys, reading the screen, waiting and searching, buffers, viewing, environment, server, plugins and diagnostics. |
| [Keys, formats and exit codes](/reference/keys-and-formats) | Key names for `send-keys`, format variables for `-F` and `display-message`, and exit codes. |
| [Environment variables](/reference/environment) | Every variable zeropty reads, with its default. |
| [tmux compatibility](/reference/tmux-compatibility) | What zeropty does and does not do compared with tmux. |

## From programs

| Page | What it covers |
|---|---|
| [Socket protocol](/reference/socket-protocol) | The JSON lines on the server's socket, and moving sessions between servers. |
| [C ABI](/reference/c-abi) | Every function, type and macro of `libzeropty_c.dylib` and `zeropty.h`. |
| [Node.js API](/reference/node-api) | Every export of `client/zeropty.mjs`, `client/embedded.mjs` and `zeropty.node`. |
| [Rust API](/reference/rust-api) | The Rust crate: the engine in your process, the server-plugin variant, plugins in Rust, and how to build the rustdoc. |
| [Plugin API](/reference/plugin-api) | Access rules, the plugin socket protocol, events, commands and the Rust API of `zeropty::plugin`. |

## Elsewhere

| Where | What |
|---|---|
| [Guide](/guide/introduction) | Concepts and task guides. |
| [Benchmarks](/guide/benchmarks) | Every measurement and how it was taken. |
| [Changelog](/guide/changelog) | Changes, release by release. |
| [`llms.txt`](/llms.txt), [`llms-full.txt`](/llms-full.txt) | This site as plain text, for tools. |
| `zeropty --help`, `zeropty help COMMAND` | The same command reference, offline. |
