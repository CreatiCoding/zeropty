# 자주 묻는 질문

[English](../FAQ.md) · **한국어**

[README](../../README.ko.md) · [문서 목록](README.md) · [사용법](USAGE.md) · [플러그인](PLUGINS.md) · [FAQ](FAQ.md) · [벤치마크](BENCHMARKS.md) · [설계](DESIGN.md) · [버전 정책](VERSIONING.md) · [릴리스](RELEASING.md)

**무엇인가요**
- [tmux를 대신하나요?](#tmux를-대신하나요)
- [claude가 없어도 쓸 수 있나요?](#claude가-없어도-쓸-수-있나요)
- [Linux나 Windows에서도 되나요?](#linux나-windows에서도-되나요)
- [왜 README에는 10배 이상인 항목만 있나요?](#왜-readme에는-10배-이상인-항목만-있나요)

**쓰는 법**
- [도움말은 어떻게 보나요?](#도움말은-어떻게-보나요)
- [`cargo install`로 설치하면 무엇이 빠지나요?](#cargo-install로-설치하면-무엇이-빠지나요)
- [tmux 스크립트를 그대로 쓸 수 있나요?](#tmux-스크립트를-그대로-쓸-수-있나요)
- [서버 프로세스 없이 쓸 수 있나요?](#서버-프로세스-없이-쓸-수-있나요)
- [에이전트가 끝났는지 어떻게 알아요?](#에이전트가-끝났는지-어떻게-알아요)
- [`capture-pane`이 300ms 늦은 화면을 주지 않나요?](#capture-pane이-300ms-늦은-화면을-주지-않나요)
- [세션을 잃지 않고 업그레이드할 수 있나요?](#세션을-잃지-않고-업그레이드할-수-있나요)
- [내 앱을 재시작해도 세션을 유지할 수 있나요?](#내-앱을-재시작해도-세션을-유지할-수-있나요)

**문제 해결**
- [프로그램이 "터미널이 아니다"라며 다르게 동작해요](#프로그램이-터미널이-아니다라며-다르게-동작해요)
- [`wait-pane -m`이 바로 끝나요](#wait-pane--m이-바로-끝나요)
- [`wait-pane`이 글자가 나왔는데도 `session ended`로 끝나요](#wait-pane이-글자가-나왔는데도-session-ended로-끝나요)
- [`C-d`를 보낸 뒤로 입력이 안 들어가요](#c-d를-보낸-뒤로-입력이-안-들어가요)
- [zsh에서 `-t =work:`가 이상하게 동작해요](#zsh에서--t-work가-이상하게-동작해요)
- [`no server running on ...`이 나와요](#no-server-running-on-이-나와요)
- [세션을 많이 띄우면 실패해요](#세션을-많이-띄우면-실패해요)
- [서버를 끄려면요?](#서버를-끄려면요)

---

## tmux를 대신하나요?

**사람이 쓰는 터미널 멀티플렉서로서는 아니에요.** zeropty는 프로그램(특히 코딩 에이전트)을 띄워 두고, 화면을 읽고, 키를 보내고, 끝날 때까지 기다리는 일에 집중해요. 창 분할, 상태 표시줄, 복사 모드, 설정 파일은 없어요. 대신 그 일에서는 tmux와 같은 명령으로, 측정한 15개 항목 중 11개에서 메모리·CPU·응답 속도가 범위 전체로 10배 이상 좋고(4개는 10배 미만)([벤치마크](BENCHMARKS.md)), 세션 수에 PTY 한도가 없어요. 사람이 들여다보고 싶을 때는 `zeropty attach`로 붙어요.

## claude가 없어도 쓸 수 있나요?

네. 세션은 아무 프로그램이나 돼요(`sh`, `cat`, `python3`, 빌드 명령...). 테스트와 [예제](README.md#예제)도 모두 claude 없이 돌아가요. zeropty에는 claude 전용 부분이 없어요. claude를 세션에 띄우는 방법은 [USAGE](USAGE.md#shim-대신-런타임-어댑터)에 있어요.

## Linux나 Windows에서도 되나요?

macOS와 Linux(x86_64, glibc 2.29·커널 5.3 이상)에서 돼요. Windows는 안 돼요. 엔진의 큐는 macOS에서 kqueue, Linux에서 epoll이고 같은 타입 뒤에 있어요(`src/sys/kq.rs`, `src/sys/epoll.rs`). 터미널이 필요한 프로그램을 속이는 shim은 macOS에서 `DYLD_INSERT_LIBRARIES`, Linux에서 `LD_PRELOAD`(`shim.so`)로 걸려요. Linux에서 정적 링크된 프로그램은 shim을 불러오지 않아서 터미널을 못 봐요.

## 왜 README에는 10배 이상인 항목만 있나요?

프로젝트의 목표가 "모든 항목에서 tmux보다 10배"이고, README는 그 목표에 도달한 항목만 보여 주기로 했어요. 10배에 못 미친 항목도 숨기지 않아요. [벤치마크](BENCHMARKS.md)에 ⛔ 표시와 함께 모든 측정값, 하한의 이유가 있어요. 숫자는 반올림하지 않고 잰 범위 그대로 적어요.

## 도움말은 어떻게 보나요?

`zeropty --help`가 모든 명령과 옵션을 한 화면에 보여 줘요(`-h`, `help`도 같아요). 명령 하나만 보려면 `zeropty help send-keys`처럼 써요. 서버가 없어도 되고, 서버를 띄우지도 않아요. 옵션마다의 자세한 설명과 키 이름은 [사용법](USAGE.md#명령-레퍼런스)에 있어요. tmux와 같은 이름과 옵션을 쓰니 tmux 문서도 대부분 그대로 통해요([tmux와 기능 비교](USAGE.md#tmux와-기능-비교)).

## `cargo install`로 설치하면 무엇이 빠지나요?

`cargo install --git https://github.com/CreatiCoding/zeropty`는 Rust 바이너리 `zeropty`(서버와 CLI) 하나만 `~/.cargo/bin`에 넣어요. 이것만으로 서버가 뜨고, `sh`·`python3`·빌드 명령처럼 터미널이 없어도 되는 프로그램을 세션으로 돌리고, `send-keys`·`wait-pane`·`capture-pane`·`attach`를 모두 쓸 수 있어요.

빠지는 건 이래요. `cargo install`은 패키지의 실행 파일만 설치하고, 아래 파일은 `./build.sh`가 따로 만들거나 복사하기 때문이에요.

| 빠지는 것 | 왜 빠지나요 | 없으면 |
|---|---|---|
| `shim.dylib` | C 파일(`shim/shim.c`)을 `clang`으로 따로 빌드해요 | 터미널이 필요한 프로그램(vim, less)이 "터미널이 아니다"라고 판단해요 |
| `zeropty-tty` | 셸 스크립트이고, 자기와 같은 폴더의 `shim.dylib`을 찾아요 | 터미널이 필요한 프로그램을 shim으로 띄울 수 없어요 |
| `libzeropty_c.dylib`, `zeropty.h`, `zeropty.node` | 워크스페이스의 다른 crate(`bindings/`)가 만드는 라이브러리예요 | C·Node 임베딩을 못 해요(Rust 임베딩은 git 의존성으로 돼요) |
| `zeropty_bash.dylib` | C 파일(`shell/zeropty_bash.c`)을 따로 빌드해요 | bash builtin이 없어요(일반 CLI는 그대로 돼요) |

이런 것이 필요하면 [1분 시작](../../README.ko.md#1분-시작)처럼 `git clone` 뒤 `./build.sh`로 빌드하고 `dist/`를 써요.

## tmux 스크립트를 그대로 쓸 수 있나요?

대부분은 `tmux`를 `zeropty`로 바꾸면 돼요. `new-session`, `send-keys`, `capture-pane`, `list-sessions -F`, `display-message -p`, `load-buffer`, `paste-buffer`, `pipe-pane`처럼 프로그램을 다루는 명령은 인자가 같아요. 다른 점은 이래요.

- 창과 창 분할이 없어요. 세션 하나에 프로그램 하나예요(`split-window`, `new-window`는 없어요).
- 타깃 이름은 정확히 일치해야 해요(tmux의 앞부분 일치 없음).
- 포맷의 조건식(`#{?...}`)과 수식어(`#{t:...}`)는 안 돼요.
- 모르는 옵션은 무시하지 않고 오류예요.

전체 비교: [사용법: tmux와 기능 비교](USAGE.md#tmux와-기능-비교)

## 서버 프로세스 없이 쓸 수 있나요?

네. 엔진을 내 프로세스 안에 넣으면 돼요(Node 모듈, C ABI, Rust crate). 명령이 함수 호출이 돼서 Node에서 `send-keys` 한 번이 0.06\~0.07ms예요(명령마다 tmux를 실행하면 9.03\~9.56ms). 필요하면 내 프로그램이 소켓도 열어서(`listen`) CLI와 `attach`로도 접근할 수 있어요. [사용법: 임베디드](USAGE.md#내-프로세스-안에-임베디드)

## 에이전트가 끝났는지 어떻게 알아요?

`wait-pane`을 써요. 화면에 글자가 나타날 때(`-m`), 출력이 일정 시간 멈출 때(`-q`), 또는 둘 다일 때 답해요. 서버가 출력이 올 때만, 바뀐 행만 확인해서 폴링이 없어요.

```sh
zeropty wait-pane -t '=fix-1:' -q 5000 -T 600000   # 출력이 5초 멈출 때까지, 최대 10분
```

여러 세션 중 어디가 입력을 기다리는지는 `grep-panes`로 한 번에 찾아요.

## `capture-pane`이 300ms 늦은 화면을 주지 않나요?

아니요. 엔진은 CPU를 아끼려고 출력을 300ms씩 모아 읽지만, `capture-pane`(그리고 `grep-panes`, `wait-pane`)은 밀린 출력을 먼저 읽고 답해요. 읽는 쪽은 늘 최신 화면을 받아요. 사람이 `attach`로 보고 있을 때도 출력이 바로 가요.

## 세션을 잃지 않고 업그레이드할 수 있나요?

네. 새 바이너리로 `zeropty restart-server`를 실행하면, 새 서버가 실행 중인 세션을 전부 넘겨받고 소켓 자리를 차지해요. 프로그램은 멈추지 않고 pid도 같아요. [사용법: 세션 옮기기](USAGE.md#세션-옮기기)

## 내 앱을 재시작해도 세션을 유지할 수 있나요?

임베디드 엔진의 세션은 앱과 같이 끝나요. 앱이 끝나기 전에 `push-sessions -s <서버 소켓>`으로 독립 서버에 넘기고, 다시 시작할 때 `pull-sessions -s <서버 소켓>`으로 가져오면 돼요.

## 프로그램이 "터미널이 아니다"라며 다르게 동작해요

세션의 터미널은 PTY가 아니라 소켓이라서 `isatty()`가 거짓이에요. `zeropty-tty`로 띄우세요.

```sh
zeropty new-session -d -s v "dist/zeropty-tty vim notes.txt"
```

`/usr/bin`의 시스템 프로그램은 서명만 바꾼 사본으로 실행해요. `top`, `ps`처럼 특별한 권한이 필요한 프로그램은 안 돼요. [사용법: zeropty-tty](USAGE.md#터미널이-필요한-프로그램-zeropty-tty)

## `wait-pane -m`이 바로 끝나요

입력한 글자는 PTY처럼 화면에 에코돼요. `send-keys -l 'echo done'`을 보내고 `wait-pane -m done`을 하면, 입력한 줄의 `done`에 바로 맞아요. 입력한 줄에는 없고 결과에만 나오는 글자를 기다리세요. 예를 들어 `echo "exit: $?"`를 보내고 `-m 'exit: 0'`을 기다리면 돼요.

## `wait-pane`이 글자가 나왔는데도 `session ended`로 끝나요

세션이 기다리는 글자를 출력하자마자 끝나면, 엔진이 그 출력을 확인하기 전에 세션 종료를 먼저 알릴 때가 있어요(알려진 문제예요. 20번에 5번쯤 재현돼요). 그러면 `wait-pane: session ended`와 종료 코드 1이 나와요. 기다리는 글자가 나온 뒤에도 세션이 살아 있게 하면 돼요.

```sh
zeropty new-session -d -s build "sh -c 'sleep 1; echo build done; exec cat'"   # 끝에 exec cat
zeropty wait-pane -t build -m 'build done' -T 10000
zeropty kill-session -t build
```

셸(`sh`, `zsh`)이나 claude처럼 계속 도는 프로그램을 세션으로 띄우고 `send-keys`로 일을 시키는 흐름([1분 시작](../../README.ko.md#1분-시작))은 이 문제가 없어요.

## `C-d`를 보낸 뒤로 입력이 안 들어가요

세션의 소켓에는 PTY처럼 "EOF를 한 번만" 알리는 방법이 없어서, 빈 줄의 `C-d`는 입력을 닫아요. 그 뒤로는 그 세션에 입력을 보낼 수 없어요. 프로그램을 끝내려는 게 아니라면 `C-d` 대신 `C-c`나 프로그램의 종료 명령을 보내세요.

## zsh에서 `-t =work:`가 이상하게 동작해요

zsh는 `=단어`를 명령 경로로 바꿔요(`=` 확장). `'=work:'`처럼 따옴표로 감싸세요. tmux도 같아요.

## `no server running on ...`이 나와요

그 소켓에 서버가 없어요. 서버는 첫 `new-session`이 띄워요. 다른 소켓을 쓰는 서버(예: 임베디드 앱의 `listen`)에 말하려면 `ZEROPTY_SOCK=<소켓> zeropty ...`처럼 소켓을 알려 주세요.

## 세션을 많이 띄우면 실패해요

PTY 한도는 없지만 macOS의 사용자당 프로세스 수 한도는 있어요. claude 세션 하나는 프로세스 4개쯤이에요.

```sh
sysctl kern.maxprocperuid     # 사용자당 프로세스 한도
ps -U $(id -u) | wc -l        # 지금 내 프로세스 수
```

남아 있는 테스트·벤치마크 서버가 한도를 다 쓰고 있을 때가 많아요. 그 소켓에 `kill-server`를 보내 정리하세요.

## 서버를 끄려면요?

```sh
zeropty kill-server
```

모든 세션의 프로세스 그룹에 SIGHUP을 보내고 서버가 끝나요. 다른 소켓의 서버라면 `ZEROPTY_SOCK`을 함께 주세요.
