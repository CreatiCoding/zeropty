---
description: zeropty는 tmux 명령을 받아 PTY 없이 세션을 돌리는 AI 에이전트용 세션 엔진이에요.
---

# 소개

zeropty는 AI 에이전트를 위한 **세션 엔진**이에요. 코딩 에이전트 같은 프로그램을 띄우고, 화면을 읽고, 키를 보내고, 끝나면 알려 줘요. [tmux](https://github.com/tmux/tmux)와 같은 명령을 받지만 PTY(가상 터미널)는 하나도 쓰지 않아요. 이름도 여기서 왔어요.

zeropty는 [Rust](https://www.rust-lang.org)로 만들었고 macOS와 Linux(x86_64)에서 돌아요. 명령줄 도구로 쓰거나, 어떤 언어에서든 소켓으로 말하거나, 엔진을 내 프로그램 안에 넣을 수 있어요.

## 어떻게 동작하나요?

### 세션과 서버

**세션**은 프로그램 하나와 그 화면 하나, 스크롤백이에요. 창이나 창 분할은 없어요. `zeropty` CLI는 모든 세션을 가진 **서버**에 명령을 보내고, 첫 `new-session`이 tmux처럼 서버를 띄워 줘요. 명령은 **타깃**(`-t =name:`)으로 세션을 가리키고, 이름은 정확히 일치해야 해요.

자세한 내용은 [세션과 서버](/ko/guide/concepts/sessions)를 보세요.

### 세션이 터미널을 얻는 방법

tmux는 창마다 PTY를 하나씩 주는데, macOS는 기본으로 511개(최대 999개)까지만 허용해요. zeropty는 세션마다 socketpair로 연결해서 이런 한도가 없어요. vim이나 [Claude Code](https://docs.anthropic.com/en/docs/claude-code)처럼 터미널인지 확인하는 프로그램은 `zeropty-tty`로 띄워요. `zeropty-tty`는 터미널 호출에 답하는 작은 라이브러리를 넣어 줘요. PTY가 하던 일은 엔진이 직접 해요. Enter는 줄바꿈이 되고, `C-c`는 시그널이 되고, 입력한 글자는 에코돼요.

자세한 내용은 [세션이 터미널을 얻는 방법](/ko/guide/concepts/terminal)을 보세요.

### 화면, 키, 기다리기

엔진은 세션마다 출력을 셀 하나에 6바이트인 자체 화면 구조로 해석해요. 조용한 화면은 약 1KB로 접어 둬요. `send-keys`와 `capture-pane`은 tmux와 똑같이 동작해요. `wait-pane`은 글자가 나타나거나 출력이 멈추는 순간 답하고, `grep-panes`는 모든 화면을 한 번에 찾아서 폴링할 일이 없어요.

자세한 내용은 [화면, 키, 기다리기](/ko/guide/concepts/screens)를 보세요.

### zeropty를 다루는 네 가지 방법

명령은 어디서나 같은 형태예요. `["send-keys", "-t", "=work:", "Enter"]` 같은 인자 목록이에요. CLI에서 보내도 되고, 서버 소켓에 JSON 한 줄로 보내도 되고, 임베디드 엔진의 함수 호출(C ABI, Node.js 모듈, Rust crate)로 보내도 돼요. 플러그인은 세션 이벤트를 듣고 자기 명령을 더해요.

자세한 내용은 [zeropty를 다루는 네 가지 방법](/ko/guide/concepts/interfaces)을 보세요.

## 왜 zeropty 인가요?

코딩 에이전트를 여러 개 돌리는 프로그램은 보통 tmux 세션으로 띄우고 `capture-pane`을 반복해서 화면을 읽어요. 세션이 수백 개가 되면 이 방식은 아래 문제에 부딪혀요. zeropty는 처음부터 에이전트 호스트를 위해 만들었어요.

| 에이전트 호스트에게 필요한 것 | tmux | zeropty |
|---|---|---|
| 세션 수백~수천 개 | PTY 한도 511개 (최대 999) | 1000개 확인, PTY 한도 없음 |
| 화면 읽기 · 키 보내기 | 명령마다 tmux 실행 (Node에서 `send-keys` 9.03~9.56ms) | 함수 호출 한 번 (임베디드 `send-keys` 0.06~0.07ms) |
| 에이전트가 끝났는지 알기 | `capture-pane` 반복 (200ms 폴링이면 p50 86~96ms 늦게) | `wait-pane`: p50 1ms, 폴링 없음 |
| 어느 세션이 입력을 기다리나 | 세션마다 `capture-pane` | `grep-panes`: 1000개를 16~32ms에 |
| 서버 업그레이드 | 서버를 다시 띄우면 세션이 끝나요 | `restart-server`가 실행 중인 세션을 넘겨받아요 |
| 내 프로그램 안에 넣기 | 안 돼요 | C ABI, Node 모듈, Rust crate |

- **PTY 한도.** tmux는 세션마다 PTY가 하나 필요해요. zeropty는 하나도 쓰지 않고, 실제 claude 작업을 재생하는 세션 1000개에서 메모리 4.65MB, CPU 6.69~7.10%로 쟀어요. tmux는 1000개를 띄우지 못해요.
- **명령마다 프로세스.** 명령마다 tmux를 실행하면 매번 프로세스 시작 비용이 들어요. zeropty는 소켓 연결 하나로(Node에서 `send-keys` 한 번에 0.24~0.26ms) 또는 함수 호출로(0.06~0.07ms) 답해요.
- **끝났는지 알려면 폴링.** 세션 200개에서 200ms 폴링은 끝난 작업을 p50 86~96ms 늦게 알아챘어요. `wait-pane`은 1ms에 알아챘어요.
- **누가 입력을 기다리는지 찾기.** CLI로 화면 1000개를 하나씩 읽으면 4.2~4.3초 걸렸어요. `grep-panes`는 16~32ms였어요.
- **재시작하면 세션이 끝나요.** `restart-server`는 실행 중인 세션을 모두 새 서버에 넘겨요. 프로그램은 계속 돌고 pid도 그대로예요.
- **임베딩.** 엔진을 내 프로세스 안에 넣으면 서버 프로세스가 없고, 명령은 함수 호출이에요.

비용도 적어요. 실제 claude 작업 녹화를 재생하는 세션 400개로 Apple M1에서 tmux 3.7c와 비교하면, zeropty는 CPU 3.51~3.84%(tmux 54.30~54.91%, 14.1~15.6배 적음), 서버 메모리 2.66~2.70MB(tmux 32.4~32.5MB, 12.0~12.2배 적음)를 썼어요. 모든 측정값과 측정 방법은 [벤치마크](/ko/guide/benchmarks)를 보세요.

zeropty는 사람이 쓰는 터미널 멀티플렉서로서 tmux를 대신하지 않아요. 창 분할, 상태 표시줄, 복사 모드, 설정 파일이 없어요. 세션을 들여다보고 싶으면 `zeropty attach`로 내 터미널을 붙여요.

## 다음 단계

- [시작하기](/ko/guide/getting-started): zeropty를 빌드하고, 세션을 띄우고, 출력을 기다린 뒤 화면을 읽어요.
- [설치와 지원 플랫폼](/ko/guide/install): 릴리스 묶음, 파일마다의 용도, 지원하는 맥.
- [세션과 서버](/ko/guide/concepts/sessions): 첫 번째 개념 페이지.
- [tmux에서 넘어오기](/ko/guide/from-tmux): 기존 스크립트에서 무엇이 달라지는지.
