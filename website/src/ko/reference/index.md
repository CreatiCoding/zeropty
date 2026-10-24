---
description: zeropty 레퍼런스를 영역별로 모았어요. 명령, 키와 포맷, 환경변수, tmux 호환, 소켓 프로토콜, C ABI, Node.js API, 플러그인 API.
---

# 레퍼런스

모든 명령, 옵션, 프로토콜, API 를 빠짐없이 정확하게 적어요. 레퍼런스는 각각이 무엇인지 말해요. 단계와 예제로 쓰는 방법은 [가이드](/ko/guide/introduction)를 보세요.

명령줄 페이지와 소켓·플러그인 프로토콜은 사이트를 빌드할 때마다 저장소의 `docs/` 에서 만들어서, 언제나 `main` 의 코드와 같아요. C ABI 와 Node.js 페이지는 빌드가 내놓는 공개 기호 목록(`api/*.txt`)과 대조해 검사해요.

## 명령줄

| 페이지 | 다루는 것 |
|---|---|
| [명령](/ko/reference/commands) | 모든 명령과 옵션. 세션, 키 보내기, 화면 읽기, 기다리기와 찾기, 버퍼, 보기, 환경, 서버, 플러그인, 진단으로 묶었어요. |
| [키, 포맷, 종료 코드](/ko/reference/keys-and-formats) | `send-keys` 의 키 이름, `-F`·`display-message` 의 포맷 변수, 종료 코드. |
| [환경변수](/ko/reference/environment) | zeropty 가 읽는 모든 변수와 기본값. |
| [tmux 호환](/ko/reference/tmux-compatibility) | tmux 와 비교해 zeropty 가 하는 것과 하지 않는 것. |

## 프로그램에서

| 페이지 | 다루는 것 |
|---|---|
| [소켓 프로토콜](/ko/reference/socket-protocol) | 서버 소켓의 JSON 줄과, 서버 사이에 세션 옮기기. |
| [C ABI](/ko/reference/c-abi) | `libzeropty_c.dylib` 과 `zeropty.h` 의 모든 함수, 타입, 매크로. |
| [Node.js API](/ko/reference/node-api) | `client/zeropty.mjs`, `client/embedded.mjs`, `zeropty.node` 가 내보내는 모든 것. |
| [Rust API](/ko/reference/rust-api) | Rust 크레이트. 내 프로세스 안의 엔진, 서버 플러그인으로 쓰기, Rust 플러그인, rustdoc 만드는 법. |
| [플러그인 API](/ko/reference/plugin-api) | 권한, 플러그인 소켓 프로토콜, 이벤트, 명령, `zeropty::plugin` 의 Rust API. |

## 그 밖에

| 어디 | 무엇 |
|---|---|
| [가이드](/ko/guide/introduction) | 개념과 작업 가이드. |
| [벤치마크](/ko/guide/benchmarks) | 모든 측정값과 잰 방법. |
| [변경 기록](/ko/guide/changelog) | 판마다의 변경. |
| [`llms.txt`](/llms.txt), [`llms-full.txt`](/llms-full.txt) | 이 사이트를 도구가 읽는 글로. |
| `zeropty --help`, `zeropty help COMMAND` | 같은 명령 레퍼런스를 오프라인으로. |
