---
layout: page
title: zeropty
titleTemplate: The session engine for AI agents
sidebar: false
github: https://github.com/CreatiCoding/zeropty
badge: v0.1.5 · macOS · Rust · tmux-compatible commands · C ABI · Node
headline: [Run 1000 agents, without, a PTY.]
tagline: zeropty is the session engine for AI agents. Read their screens, send them keys and wait until they finish, with tmux's commands and a fraction of its CPU and memory.
start: { text: Get started, link: /guide/getting-started }
arch:
  label: Architecture
  name: zeropty
  left: Your agents
  programs: [claude, codex, sh / zsh / bash, any program]
  in: run
  core: one server · a socketpair per session · no PTY
  out: drive
  right: Your host
  ways: [[CLI, tmux commands], [Socket, any language], [Embedded, C ABI · Node]]
  more: { text: How a session gets a terminal, link: /guide/concepts/terminal }
demo:
  label: In action
  title: Start a session, wait for the answer, read the screen
  dark: https://cdn.creco.dev/zeropty/demo-en-dark.webp
  light: https://cdn.creco.dev/zeropty/demo-en-light.webp
  alt: "Terminal demo: start a session with zeropty and send keys; wait-pane catches done: 42 after 1.01 seconds, and capture-pane reads the screen"
  caption: "wait-pane returns the moment done: 42 appears, with no polling"
stats:
  - { value: "14.1~15.6×", label: "less CPU than tmux, 400 claude jobs" }
  - { value: "12.0~12.2×", label: "less memory than tmux, 400 claude jobs" }
  - { value: "128~159×", label: "faster send-keys from Node, embedded" }
  - { value: "1000", label: "sessions verified, no PTY limit" }
statsNote: { text: "Measured against tmux 3.7c on an Apple M1, interleaved. Every number and how it was measured →", link: /guide/benchmarks }
features:
  - { tag: Scale, title: Thousands of sessions, body: "No PTY means no PTY limit. tmux stops at 511 (999 at most); zeropty runs 1000 replaying real claude work in 4.65MB.", link: /guide/concepts/terminal }
  - { tag: Speed, title: One call per command, body: "send-keys is 0.06~0.07ms embedded and 0.24~0.26ms over the socket, where running tmux per command takes 9.03~9.56ms.", link: /guide/concepts/interfaces }
  - { tag: Waiting, title: wait-pane, body: "Returns the moment the text appears on the screen: p50 1ms, no polling. No capture-pane loop arriving 86~96ms late.", link: /guide/concepts/screens }
  - { tag: Search, title: grep-panes, body: "Finds which of 1000 sessions is waiting for input in 16~32ms, without capturing every screen.", link: /guide/concepts/screens }
  - { tag: Upgrades, title: restart-server, body: "Upgrade the server while it keeps the running sessions. Restarting tmux ends them.", link: /guide/moving }
  - { tag: Embedding, title: Inside your program, body: "Link the C ABI or load the Node module and drive sessions with no server process and no socket.", link: /guide/embed }
platforms:
  label: Platform support
  items:
    - { name: macOS 11+ }
    - { name: Apple silicon · Intel }
    - { name: CLI · socket }
    - { name: C ABI · Rust · Node.js }
    - { name: Linux · Windows, badge: not supported }
  text: Install and platform support
  link: /guide/install
quick:
  label: Quick start
  lines:
    - git clone https://github.com/CreatiCoding/zeropty && cd zeropty && ./build.sh
    - export PATH="$PWD/dist:$PATH"
    - zeropty new-session -d -s demo -x 80 -y 6 sh
    - zeropty send-keys -t demo -l 'echo "done: $((6 * 7))"'
    - zeropty send-keys -t demo Enter
    - zeropty wait-pane -t demo -m 'done: 42' -T 10000
    - zeropty capture-pane -p -t demo
  text: Read the guide
  link: /guide/introduction
links:
  label: More
  items:
    - { text: Introduction, link: /guide/introduction }
    - { text: Reference, link: /reference/commands }
    - { text: Benchmarks, link: /guide/benchmarks }
    - { text: Changelog, link: /guide/changelog }
    - { text: GitHub, link: https://github.com/CreatiCoding/zeropty }
    - { text: llms.txt, link: /llms.txt }
---

<Home />
