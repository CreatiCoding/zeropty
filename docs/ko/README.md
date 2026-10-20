# zeropty 문서

[English](../README.md) · **한국어**

[README](../../README.ko.md) · [문서 목록](README.md) · [사용법](USAGE.md) · [플러그인](PLUGINS.md) · [FAQ](FAQ.md) · [벤치마크](BENCHMARKS.md) · [설계](DESIGN.md) · [버전 정책](VERSIONING.md) · [릴리스](RELEASING.md)

## 문서

| 문서 | 내용 |
|---|---|
| [README](../../README.ko.md) | zeropty가 무엇이고 왜 빠른지, 1분 시작, 설치 |
| [사용법](USAGE.md) | 명령·옵션 전체 레퍼런스, 키 이름, 포맷 변수, 소켓 프로토콜, 임베디드(Node·C·Rust), 세션 옮기기, 환경변수, tmux와 기능 비교 |
| [플러그인](PLUGINS.md) | 세션 이벤트를 듣고 명령을 더하는 플러그인: 소켓 프로토콜, C 라이브러리(`plugin-run`), 호스트 안 콜백, 권한, API 레퍼런스 |
| [플러그인 내부](PLUGINS-INTERNALS.md) | zeropty를 고치거나 호스트 루프를 직접 짜는 사람용: 계층 구조, 플러그인이 없을 때의 비용 측정 |
| [FAQ](FAQ.md) | 자주 묻는 질문과 문제 해결 |
| [벤치마크](BENCHMARKS.md) | 모든 측정값(10배에 못 미친 항목 포함), 측정 방법, 재현하는 법 |
| [설계](DESIGN.md) | 구조 그림, 메모리·CPU·응답 속도를 줄인 방법과 그 수치 근거, 코드 지도 |
| [버전 정책](VERSIONING.md) | Semantic Versioning, 호환 범위(CLI, 소켓 프로토콜, 세션 레코드, C ABI, Node API, 환경변수), MSRV |
| [릴리스](RELEASING.md) | 버전 올리기와 릴리스 절차(`scripts/release.mjs`) |
| [변경 기록](CHANGELOG.md) | 버전마다 사용자에게 보이는 변경 |
| [기여 안내](CONTRIBUTING.md) | 빌드, 테스트, 측정, PR 규칙 |
| [보안 정책](SECURITY.md) | 취약점을 비공개로 알리는 방법 |
| [행동 강령](CODE_OF_CONDUCT.md) | 커뮤니티 규칙 |

## 하고 싶은 일로 찾기

| 하고 싶은 일 | 볼 곳 |
|---|---|
| 명령과 옵션을 한눈에 보기 | `zeropty --help`, `zeropty help <명령>`, [사용법: 명령 레퍼런스](USAGE.md#명령-레퍼런스) |
| 셸에서 tmux처럼 써 보기 | [README: 1분 시작](../../README.ko.md#1분-시작), [`examples/shell/quickstart.sh`](../../examples/shell/quickstart.sh) |
| Node 프로그램에서 에이전트 여러 개 다루기 | [사용법: Node 클라이언트](USAGE.md#node-클라이언트), [`examples/node/quickstart.mjs`](../../examples/node/quickstart.mjs) |
| 서버 없이 내 프로그램 안에 넣기 | [사용법: 임베디드](USAGE.md#내-프로세스-안에-임베디드), [`examples/node/embedded.mjs`](../../examples/node/embedded.mjs), [`examples/c/hello.c`](../../examples/c/hello.c), [`examples/embed.rs`](../../examples/embed.rs) |
| 호스트 이벤트 루프로 엔진 돌리기 | [사용법: polled](USAGE.md#내-프로세스-안에-임베디드), [`examples/polled.rs`](../../examples/polled.rs) |
| 에이전트가 끝날 때까지 기다리기 | [사용법: 기다리기와 찾기](USAGE.md#기다리기와-찾기) |
| vim, claude처럼 터미널이 필요한 프로그램 띄우기 | [사용법: zeropty-tty](USAGE.md#터미널이-필요한-프로그램-zeropty-tty) |
| 세션을 잃지 않고 서버 업그레이드 | [사용법: 세션 옮기기](USAGE.md#세션-옮기기) |
| 기존 tmux 스크립트 옮기기 | [사용법: tmux와 기능 비교](USAGE.md#tmux와-기능-비교), [FAQ](FAQ.md#tmux-스크립트를-그대로-쓸-수-있나요) |
| 세션 이벤트 듣기, 내 명령 더하기 (플러그인) | [플러그인](PLUGINS.md), [사용법: 플러그인](USAGE.md#플러그인) |
| 버전 확인, 무엇이 바뀌었는지 보기 | `zeropty --version`, [변경 기록](CHANGELOG.md), [버전 정책](VERSIONING.md) |
| 벤치마크 재현하기 | [벤치마크](BENCHMARKS.md), [기여 안내: 측정](CONTRIBUTING.md#성능을-바꾸는-변경) |

## 예제

모두 `./build.sh` 뒤에 저장소 루트에서 실행해요. claude가 없어도 돌아가요.

| 예제 | 실행 | 보여 주는 것 |
|---|---|---|
| [`examples/shell/quickstart.sh`](../../examples/shell/quickstart.sh) | `sh examples/shell/quickstart.sh` | CLI로 세션 만들기, 키 보내기, `wait-pane`, 화면 읽기 |
| [`examples/node/quickstart.mjs`](../../examples/node/quickstart.mjs) | `node examples/node/quickstart.mjs` | Node 클라이언트로 같은 일 + `grep-panes` |
| [`examples/node/embedded.mjs`](../../examples/node/embedded.mjs) | `node examples/node/embedded.mjs` | 서버 없이 Node 안의 엔진, 명령 하나의 시간 |
| [`examples/c/hello.c`](../../examples/c/hello.c) | 파일 맨 위 주석 참고 | C ABI |
| [`examples/embed.rs`](../../examples/embed.rs) | `cargo run --release --example embed` | Rust에서 엔진 넣기 |
| [`examples/polled.rs`](../../examples/polled.rs) | `cargo run --release --example polled` | 엔진 스레드 없이 호스트 루프로, 폴링 없는 `wait-pane` |

`examples/`의 나머지 `*_bench.rs`, `sizes.rs`, `zstd_ratio.rs`, `claude_loop.rs`는 측정·프로파일용이에요.

## Rust API 문서

`zeropty`, `zeropty_c`, `zeropty_node` 세 crate의 rustdoc이에요. crates.io에는 올리지 않아요(macOS 전용이고 `./build.sh`로 빌드해요). 웹에 올린 주소는 없고, 저장소에서 만들어 열어요.

```sh
cargo doc --no-deps --workspace --open
```

crate 첫 페이지에 구조와 실행되는 예제가 있고, 공개 함수·타입마다 예제가 붙어 있어요(옛 경로로 남겨 둔 `#[deprecated]` 항목은 빼고). Rust 예제 200여 개는 doctest라서 `cargo test --release --doc`이 모두 실제로 돌려 봐요. 공개 항목은 모두 문서가 있고(세 crate 모두 `#![warn(missing_docs)]`), `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace`가 경고 없이 끝나요. `scripts/check.sh`가 둘 다 확인해요(rustdoc은 `--quick`이 아닐 때).

어디서부터 읽을지:

| 하고 싶은 일 | rustdoc에서 볼 곳 |
|---|---|
| 내 프로세스에 엔진 넣기, 여러 스레드에서 명령 | `embed::Embedded`, `Embedded::command`, `Embedded::with` |
| 스레드 없이 호스트 이벤트 루프로 돌리기 | `embed::Polled`, `Polled::fd`, `Polled::timeout_ms`, `Polled::process` |
| 글자가 나올 때까지 폴링 없이 기다리기 | `engine::Engine::wait_start`, `wait_check`, `wait_due_ms`, `wait_end` |
| CLI와 소켓 클라이언트가 내 세션에 닿게 하기 | `Embedded::listen`, `Polled::listen`, `server` 모듈 |
| 실행 중인 세션을 다른 프로세스로 옮기기 | `engine::handover::pull`, `push`, `Engine::export_session` |
| 엔진만으로 루프 짜기, 내 fd도 같은 큐에서 기다리기 | `engine::Engine`, `Engine::try_poll`, `Engine::watch`, `host::HostLoop` |
| 내 터미널을 세션에 붙이기 | `Engine::attach`, `viewer_events`, `zeropty::tty` |
| 소켓 클라이언트를 직접 짜기 | `protocol` 모듈(`Request`, `reply_line`, `ClientFrame`, `ServerFrame`) |
| 엔진을 가짜 OS로 테스트하기 | `engine::Backend`, `Poller`, `Clock`, `Programs`, `Engine::with_backend` |
| 플러그인 만들기, 호스트에 붙이기 | `plugin` 모듈(`plugin::api`), `Embedded::add_plugin`, `Polled::command_start` |
| 터미널 에뮬레이터만 쓰기 | `term::Term`, `Term::freeze`, `Term::export` |
| C, Node에서 쓰기 | `zeropty_c`, `zeropty_node` crate 첫 페이지 |
