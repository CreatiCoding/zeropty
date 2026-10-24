---
layout: page
title: zeropty
titleTemplate: AI 에이전트를 위한 세션 엔진
sidebar: false
github: https://github.com/CreatiCoding/zeropty
badge: v0.1.5 · macOS · Rust · tmux 호환 명령 · C ABI · Node
headline: [에이전트 1000개를, PTY, 없이.]
tagline: zeropty는 AI 에이전트를 위한 세션 엔진이에요. 화면을 읽고, 키를 보내고, 끝날 때까지 기다려요. tmux 명령 그대로, CPU와 메모리는 훨씬 적게 써요.
start: { text: 시작하기, link: /ko/guide/getting-started }
arch:
  label: 구조
  name: zeropty
  left: 에이전트
  programs: [claude, codex, sh / zsh / bash, 어떤 프로그램이든]
  in: 실행
  core: 서버 하나 · 세션마다 socketpair · PTY 없음
  out: 조작
  right: 호스트
  ways: [[CLI, tmux 명령], [소켓, 어떤 언어든], [임베디드, C ABI · Node]]
  more: { text: 세션이 터미널을 얻는 방법, link: /ko/guide/concepts/terminal }
demo:
  label: 실제 화면
  title: 세션을 띄우고, 답을 기다리고, 화면을 읽어요
  dark: https://cdn.creco.dev/zeropty/demo-dark.webp
  light: https://cdn.creco.dev/zeropty/demo-light.webp
  alt: "터미널 데모: zeropty 로 세션을 띄워 키를 보내고, wait-pane 이 1.01초 뒤 done: 42 를 잡고, capture-pane 으로 화면을 읽어요"
  caption: "wait-pane 은 폴링 없이 done: 42 가 뜨는 순간 돌아와요"
stats:
  - { value: "14.1~15.6배", label: "tmux보다 적은 CPU, claude 작업 400개" }
  - { value: "12.0~12.2배", label: "tmux보다 적은 메모리, claude 작업 400개" }
  - { value: "128~159배", label: "Node에서 더 빠른 send-keys, 임베디드" }
  - { value: "1000", label: "세션 검증, PTY 한도 없음" }
statsNote: { text: "같은 Apple M1에서 tmux 3.7c와 번갈아 잰 값이에요. 모든 수치와 측정 방법 →", link: /ko/guide/benchmarks }
features:
  - { tag: 규모, title: 세션 수천 개, body: "PTY를 안 쓰니 PTY 한도가 없어요. tmux는 511개(최대 999개)에서 멈추지만, zeropty는 실제 claude 작업을 재생하는 세션 1000개를 4.65MB로 돌려요.", link: /ko/guide/concepts/terminal }
  - { tag: 속도, title: 명령 하나가 함수 호출 하나, body: "send-keys가 임베디드에서 0.06~0.07ms, 소켓으로 0.24~0.26ms예요. 명령마다 tmux를 실행하면 9.03~9.56ms가 걸려요.", link: /ko/guide/concepts/interfaces }
  - { tag: 기다리기, title: wait-pane, body: "글자가 화면에 뜨는 순간 돌아와요. p50 1ms, 폴링 없음. capture-pane을 반복하며 86~96ms 늦게 알아챌 일이 없어요.", link: /ko/guide/concepts/screens }
  - { tag: 찾기, title: grep-panes, body: "세션 1000개 중 입력을 기다리는 세션을 16~32ms에 찾아요. 화면을 하나하나 캡처하지 않아요.", link: /ko/guide/concepts/screens }
  - { tag: 업그레이드, title: restart-server, body: "돌고 있는 세션을 그대로 넘겨받으며 서버를 바꿔요. tmux는 서버를 다시 띄우면 세션이 끝나요.", link: /ko/guide/moving }
  - { tag: 임베딩, title: 내 프로그램 안에서, body: "C ABI를 링크하거나 Node 모듈을 불러서, 서버 프로세스도 소켓도 없이 세션을 다뤄요.", link: /ko/guide/embed }
platforms:
  label: 지원 환경
  items:
    - { name: macOS 11 이상 }
    - { name: Apple silicon · Intel }
    - { name: CLI · 소켓 }
    - { name: C ABI · Rust · Node.js }
    - { name: Linux · Windows, badge: 미지원 }
  text: 설치와 지원 환경
  link: /ko/guide/install
quick:
  label: 1분 시작
  lines:
    - git clone https://github.com/CreatiCoding/zeropty && cd zeropty && ./build.sh
    - export PATH="$PWD/dist:$PATH"
    - zeropty new-session -d -s demo -x 80 -y 6 sh
    - zeropty send-keys -t demo -l 'echo "done: $((6 * 7))"'
    - zeropty send-keys -t demo Enter
    - zeropty wait-pane -t demo -m 'done: 42' -T 10000
    - zeropty capture-pane -p -t demo
  text: 가이드 읽기
  link: /ko/guide/introduction
links:
  label: 더 보기
  items:
    - { text: 소개, link: /ko/guide/introduction }
    - { text: 레퍼런스, link: /ko/reference/commands }
    - { text: 벤치마크, link: /ko/guide/benchmarks }
    - { text: 변경 기록, link: /ko/guide/changelog }
    - { text: GitHub, link: https://github.com/CreatiCoding/zeropty }
    - { text: llms.txt, link: /llms.txt }
---

<Home />
