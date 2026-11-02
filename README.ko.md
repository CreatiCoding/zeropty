<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://cdn.creco.dev/zeropty/logo-dark.webp">
    <img alt="zeropty: AI 에이전트를 위한 세션 엔진" src="https://cdn.creco.dev/zeropty/logo-light.webp">
  </picture>
</p>

<p align="center">
  <b>AI 에이전트를 위한 세션 엔진.</b><br>
  에이전트 1000개를 한 기기에 띄워 두고, 화면을 읽고, 키를 보내고, 끝날 때까지 기다려요.<br>
  tmux 명령을 그대로 받으면서, tmux보다 <b>CPU는 14.1\~15.6배 · 메모리는 12.0\~12.2배</b> 적게 써요.
</p>

<p align="center"><a href="README.md">English</a> · <b>한국어</b></p>

<p align="center">
  <!-- Status badges, each added once what it shows is real (no badge that claims a state it cannot track):
  - Release, once a GitHub release exists:
  <a href="https://github.com/CreatiCoding/zeropty/releases/latest"><img alt="release" src="https://img.shields.io/github/v/release/CreatiCoding/zeropty"></a>
  - crates.io: not published there (built with ./build.sh); no badge until it is. -->
  <img alt="platform: macOS" src="https://img.shields.io/badge/platform-macOS-111827?logo=apple&logoColor=white">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-engine-b7410e?logo=rust&logoColor=white">
  <img alt="embed: C ABI · Node" src="https://img.shields.io/badge/embed-C%20ABI%20%C2%B7%20Node-0891b2">
  <img alt="tmux compatible" src="https://img.shields.io/badge/tmux-compatible%20commands-1bb91f?logo=tmux&logoColor=white">
  <img alt="license: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-7c3aed">
</p>

<p align="center">
  <a href="#1분-시작">1분 시작</a> ·
  <a href="#벤치마크">벤치마크</a> ·
  <a href="#설치">설치</a> ·
  <a href="docs/ko/USAGE.md">사용법</a> ·
  <a href="docs/ko/PLUGINS.md">플러그인</a> ·
  <a href="docs/ko/FAQ.md">FAQ</a> ·
  <a href="docs/ko/DESIGN.md">설계</a> ·
  <a href="docs/ko/README.md">문서 목록</a>
</p>

---

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://cdn.creco.dev/zeropty/demo-dark.webp">
    <img alt="터미널 데모: zeropty로 세션을 띄우고 키를 보낸 뒤, wait-pane이 1.01초 만에 done: 42를 잡고, capture-pane으로 화면을 읽어요" src="https://cdn.creco.dev/zeropty/demo-light.webp" width="880">
  </picture>
</p>

코딩 에이전트(claude, codex ...)를 여러 개 돌리는 프로그램은 보통 tmux로 세션을 띄우고 `capture-pane`을 반복해서 화면을 읽어요. 세션이 수백 개가 되면 PTY 한도에 걸리고, 명령마다 프로세스가 뜨고, 끝났는지는 폴링으로만 알 수 있어요. **zeropty는 처음부터 에이전트 호스트를 위해 만든 세션 엔진이에요.** 이름 그대로 PTY를 하나도 쓰지 않아요(zero PTY).

| 에이전트 호스트에게 필요한 것 | tmux | zeropty |
|---|---|---|
| 세션 수백\~수천 개 | PTY 한도 511개 (최대 999) | **1000개 확인, PTY 한도 없음** |
| 화면 읽기 · 키 보내기 | 명령마다 tmux 실행 (`send-keys` 9.03\~9.56ms) | **함수 호출 한 번 (`send-keys` 0.06\~0.07ms)** |
| 에이전트가 끝났는지 알기 | `capture-pane` 반복 (200ms 폴링이면 p50 86\~96ms 늦게) | **`wait-pane`: 폴링 없이 1ms** |
| 어느 세션이 입력을 기다리나 | 세션마다 `capture-pane` | **`grep-panes`: 1000개를 16\~32ms에** |
| 서버 업그레이드 | 서버를 다시 띄우면 세션이 끝나요 | **`restart-server`: 실행 중인 세션을 넘겨받아요** |
| 내 프로그램 안에 넣기 | ❌ | **C ABI · Node 모듈** |

## 1분 시작

macOS, [Rust](https://rustup.rs)(stable), Xcode Command Line Tools(`xcode-select --install`)가 있으면 돼요. claude가 없어도 돼요.

```sh
git clone https://github.com/CreatiCoding/zeropty && cd zeropty
./build.sh                       # dist/ 에 zeropty 와 임베딩용 라이브러리가 생겨요
export PATH="$PWD/dist:$PATH"
```

첫 `./build.sh`는 Apple M1(8코어)에서 23\~27초 걸렸어요. 깨끗한 clone, 빈 cargo 캐시(의존 crate 49개 내려받기 포함)로 세 번 잰 값이에요. 아래 다섯 명령은 `sleep 1`을 포함해 1.07초에 끝났어요.

tmux를 쓰듯 세션을 띄우고, 일을 시키고, 끝날 때까지 기다리고, 화면을 읽어요.

```sh
zeropty new-session -d -s demo -x 80 -y 6 sh                # 서버가 없으면 띄워요
zeropty send-keys -t demo -l 'sleep 1; echo "done: $((6 * 7))"'
zeropty send-keys -t demo Enter
zeropty wait-pane -t demo -m 'done: 42' -T 10000            # 폴링 없이, 글자가 뜨는 순간 돌아와요
zeropty capture-pane -p -t demo
```

```text
sleep 1; echo "done: $((6 * 7))"
done: 42
```

화면이 6줄(`-y 6`)이라 그 아래 빈 줄 4개도 함께 나와요(위에서는 생략). 명령 목록은 `zeropty --help`, 명령 하나는 `zeropty help send-keys`예요.

사람이 직접 보고 싶으면 `zeropty attach -t demo`로 붙고 `Ctrl-b d`로 떼요. 다 썼으면 `zeropty kill-server`. 같은 흐름을 Node, C, Rust로 한 [예제](docs/ko/README.md#예제)도 있어요.

서버는 자신을 띄운 프로세스(서버를 띄우는 첫 명령, 보통 `new-session`)의 환경을 그대로 가져요(tmux와 같아요). 그래서 이후 세션은 `new-session`을 실행한 셸의 환경이 아니라 그 환경을 받아요. 변수를 넘기려면 `new-session -e VAR=value`, 새 세션 모두에 주려면 `set-environment -g`를 쓰세요([환경](docs/ko/USAGE.md#환경)).

## 벤치마크

<!-- bench-v4-*.webp and how-v3-*.webp are docs/assets/ files of the same name, uploaded to the CDN; a redrawn picture gets a new name (docs/assets/webp.sh) so this page never shows numbers older than its tables. -->
<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://cdn.creco.dev/zeropty/bench-v4-dark.webp">
    <img alt="tmux 대비 측정 범위: claude 작업 400개에서 CPU 54.30~54.91% 대 3.51~3.84%(14.1~15.6배), 메모리 32.4~32.5MB 대 2.66~2.70MB(서버, 12.0~12.2배)와 1.63~1.64MB(임베디드, 19.7~19.9배), Node에서 send-keys 한 번 9.03~9.56ms 대 소켓 0.24~0.26ms(34.7~39.8배)·임베디드 0.06~0.07ms(128~159배)" src="https://cdn.creco.dev/zeropty/bench-v4-light.webp">
  </picture>
</p>

tmux 3.7c와 같은 기기(Apple M1)에서 같은 부하로 **번갈아** 쟀어요. 세션 400개는 실제 claude 작업 녹화를 재생해요.

| | tmux | zeropty | |
|---|---|---|---|
| **CPU**, claude 작업 400개 재생 | 54.30\~54.91% | 3.51\~3.84% | **14.1\~15.6×** |
| **메모리**, claude 작업 400개 재생 (서버) | 32.4\~32.5MB | 2.66\~2.70MB | **12.0\~12.2×** |
| **메모리**, claude 작업 400개 재생 (임베디드) | 32.4\~32.5MB | 1.63\~1.64MB | **19.7\~19.9×** |
| **메모리**, 화면 갱신 400개 | 251.2\~251.4MB | 1.71\~1.74MB | **144\~147×** |
| **메모리**, 대기 400개 (임베디드) | 8.0MB | 0.45\~0.46MB | **17.3\~17.7×** |
| **`send-keys`**, Node 프로그램에서 (명령마다 tmux 실행 대비, 소켓) | 9.03\~9.56ms | 0.24\~0.26ms | **34.7\~39.8×** |
| **`send-keys`**, Node 프로그램에서 (명령마다 tmux 실행 대비, 임베디드) | 9.03\~9.56ms | 0.06\~0.07ms | **128\~159×** |
| **`send-keys`**, bash 스크립트에서 | 6.02\~6.31ms | 0.043\~0.048ms | **125\~146×** |
| **5.4MB 출력**이 끝나기까지 | 166\~176ms | 3.0\~4.6ms | **36.0\~58.6×** |

모든 배수는 한 가지 규칙으로 계산했어요. 표에 적힌 값 범위의 양끝(가장 빠른 tmux ÷ 가장 느린 zeropty부터 그 반대까지)으로 나누고, 올리지 않고 버려요. 이 규칙으로 아래 끝까지 10배 이상인 항목만 이 표에 올려요([배수 계산 방법](docs/ko/BENCHMARKS.md#배수-계산-방법)).

세션 1000개가 실제 claude 작업을 재생해도 zeropty는 **메모리 4.65MB, CPU 6.69\~7.10%** 수준이에요. tmux는 PTY 한도 때문에 1000개를 띄우지 못해요.

→ 모든 측정값, 측정 방법, 10배에 못 미친 항목과 그 이유: [docs/BENCHMARKS.md](docs/ko/BENCHMARKS.md)

## 에이전트 하나를 다루는 법

```js
import { zeropty } from './client/zeropty.mjs'   // 서버가 없으면 첫 new-session이 띄워요

// 에이전트 띄우기 (여기선 claude를 런타임 어댑터로: docs/ko/USAGE.md)
await zeropty(['new-session', '-d', '-s', 'fix-1', '-c', '/path/to/repo', "BUN_OPTIONS='--preload /path/to/tty-adapter.cjs' exec claude"])

// 일 시키기
await zeropty(['send-keys', '-t', '=fix-1:', '-l', '실패하는 테스트를 고쳐줘'])
await zeropty(['send-keys', '-t', '=fix-1:', 'Enter'])

// 끝날 때까지 기다리기: 출력이 5초 멈추면 (최대 10분). 폴링 없음
await zeropty(['wait-pane', '-t', '=fix-1:', '-q', '5000', '-T', '600000'])

// 결과 읽기
const { out } = await zeropty(['capture-pane', '-p', '-t', '=fix-1:'])
```

타깃은 tmux 스크립트처럼 `=이름:`으로 써요(`=fix-1:`은 이름이 정확히 `fix-1`인 세션이에요. `=fix-1`, `fix-1`도 같은 세션을 가리켜요). 같은 일을 셸에서는 `zeropty new-session ...`, `zeropty send-keys ...`처럼 tmux 자리에 `zeropty`를 쓰면 돼요. 사람이 보고 싶으면 `zeropty attach -t fix-1`로 붙어요. claude 없이 바로 돌려 볼 수 있는 버전: [`examples/node/quickstart.mjs`](examples/node/quickstart.mjs)

## 내 프로그램 안에 넣기

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://cdn.creco.dev/zeropty/how-v3-dark.webp">
    <img alt="에이전트 앱 안의 zeropty 엔진이 세션 1000개를 다루고, 선택으로 소켓을 열어 사람과 스크립트도 붙어요" src="https://cdn.creco.dev/zeropty/how-v3-light.webp">
  </picture>
</p>

엔진을 내 프로세스에 넣으면 서버 프로세스가 없고, 명령은 함수 호출이에요. 소켓을 열면(`listen`) CLI, `attach`, 다른 프로그램도 같은 세션을 다뤄요.

```js
import { zeropty, listen } from './client/embedded.mjs'   // import만 바꾸면 돼요
await zeropty(['new-session', '-d', '-s', 'w', 'dist/zeropty-tty python3'])
const { out } = await zeropty(['capture-pane', '-p', '-t', '=w:'])   // 0.06~0.07ms (바쁜 세션 0.10~0.17ms)
listen(`${process.env.HOME}/.zeropty/app.sock`)   // 선택: ZEROPTY_SOCK=... zeropty attach -t w
```

```c
zeropty *a = zeropty_open(NULL);   // C ABI: FFI가 되는 어떤 언어든 (Python ctypes, Go cgo, Swift ...)
char *r = zeropty_command(a, "{\"argv\":[\"send-keys\",\"-t\",\"=w:\",\"Enter\"],\"cwd\":\"/\"}");
zeropty_free(r);
```

```rust
let zp = zeropty::embed::Embedded::start(Default::default());   // Rust: 같은 엔진, 같은 명령
let r = zp.command(&["capture-pane", "-p", "-t", "=w:"].map(String::from), "/");
```

→ Node·C·Rust 임베딩, 엔진 스레드 없이 호스트 루프로 돌리기(polled): [docs/USAGE.md](docs/ko/USAGE.md#내-프로세스-안에-임베디드) · 실행되는 예제: [`examples/`](docs/ko/README.md#예제)

## 플러그인

세션 생성·종료·출력 같은 이벤트를 듣고 새 명령을 더하는 코드를 붙일 수 있어요. 아무 언어로 쓴 프로그램이 소켓에 붙거나, `zeropty.h`만으로 만든 C 라이브러리를 `zeropty plugin-run`으로 띄우거나(둘 다 다시 컴파일할 필요 없어요), C 함수나 Rust 트레이트로 내 프로세스 안에 넣어요. 플러그인 디렉터리를 `ZEROPTY_PLUGIN_DIR`로 지정하면 서버가 그 안의 플러그인을 띄우고, 죽거나 `restart-server`를 해도 다시 띄워요. 플러그인은 자기 스레드(또는 프로세스)에서 돌아서, 느리거나 멈추거나 패닉이 나도 엔진은 그대로 돌아요. 무엇을 할 수 있는지는 플러그인이 밝힌 권한(기본은 보기만)까지예요. 플러그인이 없으면 비용도 없어요. 엔진을 넣은 프로그램은 플러그인 명령을 기다리지 않고 시작해 두었다가 답을 가져갈 수도 있어요(`command_start`, `zeropty_command_start`).

```sh
python3 examples/plugins/hello.py &   # 표준 라이브러리만 쓴 68줄짜리 플러그인
zeropty hello you                     # 플러그인이 더한 명령을 내장 명령처럼
zeropty list-plugins
export ZEROPTY_PLUGIN_DIR=~/.zeropty/plugins   # 서버와 함께 띄울 플러그인 디렉터리 (지정할 때만 켜져요)
cc -dynamiclib -Ibindings/c/include -o ~/.zeropty/plugins/tally.dylib examples/plugins/tally.c   # 서버와 함께 뜨는 C 플러그인
```

> **호환성·보안 메모:** `zeropty serve`(그리고 서버를 띄우는 `new-session`)는 `ZEROPTY_PLUGIN_DIR`가 있을 때만 그 디렉터리의 실행 파일과 `.dylib`를 실행해요. 지정하지 않으면 예전과 똑같이 아무것도 실행하지 않아요(`~/.zeropty/plugins`가 있어도요). 디렉터리를 지정하는 것이 그 안의 코드를 이 사용자 권한으로 실행해도 된다는 동의예요. 다른 사용자가 쓸 수 있는 디렉터리·파일은 건너뛰어요.

→ 단계별 안내, 프로토콜, API: [docs/PLUGINS.md](docs/ko/PLUGINS.md)

## 설치

**릴리스에서 받기: Rust가 필요 없어요.** 0.1.3부터 [릴리스](https://github.com/CreatiCoding/zeropty/releases)마다 모든 맥(Apple silicon·Intel, macOS 11 이상. macOS 15와 26에서 확인)용 묶음 하나와 그 `SHA256SUMS`가 붙어 있어요. 묶음 안에는 폴더 하나가 있고, 그 안에 `zeropty`, `shim.dylib`, `zeropty-tty`와 라이선스가 들어 있어요.

```sh
v=0.1.9   # 최신 버전은: v=$(curl -fsSLo /dev/null -w '%{url_effective}' https://github.com/CreatiCoding/zeropty/releases/latest | sed 's|.*/v||')
curl -fLO "https://github.com/CreatiCoding/zeropty/releases/download/v$v/zeropty-$v-darwin-universal.tar.gz"
curl -fLO "https://github.com/CreatiCoding/zeropty/releases/download/v$v/SHA256SUMS"
shasum -a 256 -c --ignore-missing SHA256SUMS   # zeropty-0.1.9-darwin-universal.tar.gz: OK
tar -xzf "zeropty-$v-darwin-universal.tar.gz"
export PATH="$PWD/zeropty-$v-darwin-universal:$PATH"   # 계속 쓰려면 ~/.zshrc에 넣어요
zeropty -V
```

- 폴더는 어디에 풀어도 동작해요. `zeropty-tty`가 자기 옆의 `shim.dylib`를 찾아요. `zeropty-tty`만 다른 곳에 심볼릭 링크하지 말고 폴더를 PATH에 넣거나, `ZEROPTY_SHIM`으로 shim을 가리켜 주세요.
- 스크립트에서 버전을 고정하려면 `v`와 그 릴리스 `SHA256SUMS`의 묶음 해시를 코드에 박고, 풀기 전에 비교하세요.
- **Gatekeeper:** 파일은 ad-hoc 서명이고 공증은 받지 않았어요. `curl`은 quarantine 표시를 붙이지 않아서 위 순서대로면 그대로 실행돼요. 브라우저로 받으면 표시가 붙고, `tar`가 풀린 파일 모두에 그 표시를 옮겨서, macOS가 `zeropty`를 창을 띄워 막아요(명령은 그 창을 기다리며 멈춰요). 처음 실행하기 전에 표시를 지우세요: `xattr -dr com.apple.quarantine zeropty-$v-darwin-universal`.

**Linux(x86_64, glibc 2.29 이상: Ubuntu 20.04·Debian 11 이상)** 는 릴리스의 `zeropty-$v-linux-x86_64.tar.gz`를 받아요. 해시는 같은 `SHA256SUMS`에 있어요. 폴더에는 `shim.dylib` 대신 `shim.so`가 있고 `LD_PRELOAD`로 걸려요. 복사·서명 없이 동적 링크된 프로그램은 모두 터미널을 봐요(정적 링크된 프로그램, 예를 들어 대부분의 Go 바이너리는 못 봐요).

```sh
curl -fLO "https://github.com/CreatiCoding/zeropty/releases/download/v$v/zeropty-$v-linux-x86_64.tar.gz"
curl -fLO "https://github.com/CreatiCoding/zeropty/releases/download/v$v/SHA256SUMS"
sha256sum -c --ignore-missing SHA256SUMS     # zeropty-0.1.9-linux-x86_64.tar.gz: OK
tar -xzf "zeropty-$v-linux-x86_64.tar.gz" && export PATH="$PWD/zeropty-$v-linux-x86_64:$PATH"
```

**Rust가 있고 CLI와 서버만 필요하면 한 줄이에요.** `sh`, `python3`, 빌드 명령처럼 터미널이 없어도 되는 프로그램을 돌리기에는 이걸로 충분해요.

```sh
cargo install --git https://github.com/CreatiCoding/zeropty   # ~/.cargo/bin/zeropty
```

Rust가 없으면 먼저 `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`로 설치해요([rustup](https://rustup.rs)). Homebrew tap은 아직 없어요.

**C·Node 임베딩과 bash builtin까지 전부 쓰려면 소스에서 빌드해요.** 릴리스 묶음에는 이것들이 없고, `cargo install`로는 vim·less처럼 터미널이 필요한 프로그램용 shim과 `zeropty-tty`, 임베딩, builtin이 모두 설치되지 않아요([왜요?](docs/ko/FAQ.md#cargo-install로-설치하면-무엇이-빠지나요)). 빌드는 [1분 시작](#1분-시작)의 `git clone` + `./build.sh` 그대로예요(macOS, Rust stable, Xcode Command Line Tools). 계속 쓰려면 `dist/`를 PATH에 넣어 두세요.

```sh
echo "export PATH=\"$PWD/dist:\$PATH\"" >> ~/.zshrc   # zeropty 폴더 안에서
```

셸 안에서 프로세스 없이 더 빠르게 쓰려면:

```sh
source shell/zeropty.zsh                          # zsh: send-keys 0.12ms (p50)
enable -f "$PWD/dist/zeropty_bash.dylib" zeropty  # bash: send-keys 0.043~0.048ms
```

| `dist/`에 생기는 것 | 용도 |
|---|---|
| `zeropty` | 서버와 CLI |
| `libzeropty_c.dylib`, `zeropty.h` | C ABI로 엔진 넣기 |
| `zeropty.node` | Node로 엔진 넣기 (`client/embedded.mjs`가 불러요) |
| `shim.dylib`, `zeropty-tty` | 터미널이 필요한 프로그램(vim, less) 띄우기 |
| `zeropty_bash.dylib` | bash builtin |

Rust 프로그램에 넣을 때는 `./build.sh` 없이 git 의존성 한 줄이면 돼요(crates.io에는 아직 없어요). 예제: [docs/USAGE.md](docs/ko/USAGE.md#내-프로세스-안에-임베디드)

```toml
[dependencies]
zeropty = { git = "https://github.com/CreatiCoding/zeropty" }
```

## 왜 빠른가

- **PTY 대신 소켓.** 커널의 PTY 한도가 없어요. 터미널이 필요한 프로그램은 `isatty`·`ioctl`을 가로채는 작은 shim으로 속여요.
- **조용한 화면은 얼려 둬요.** 1000개 중 대부분은 가만히 있어요. 250ms 조용한 화면은 약 1KB로, 10초 지나면 zstd로 한 번 더 압축해요.
- **셀 하나가 6바이트.** 화면 해석기를 직접 만들었어요(`vt100` crate는 32바이트). 녹화한 claude 세션과 무작위 입력 수천 세트로 `vt100`과 결과를 대조해요.
- **스레드 하나, kqueue 하나.** 출력은 300ms씩 모아 한 번에 읽어요. 화면을 읽는 쪽은 늘 최신 화면을 받아요(읽기 직전에 밀린 출력부터 처리).
- **프로세스를 띄우지 않아요.** 엔진을 내 프로그램에 넣거나, 셸 안에서 바로 소켓에 말해요(zsh 함수, bash builtin).

→ 구조 그림과 수치 근거: [docs/DESIGN.md](docs/ko/DESIGN.md) · 명령 전체와 프로토콜: [docs/USAGE.md](docs/ko/USAGE.md) · 플러그인: [docs/PLUGINS.md](docs/ko/PLUGINS.md)

## tmux와 다른 점

zeropty는 **프로그램을 띄워 두고 다루는 일**에 집중해요. 창 분할, 상태 표시줄, 복사 모드, 설정 파일은 없고, 세션 하나에 프로그램 하나예요. macOS와 Linux를 지원해요(kqueue 또는 epoll, DYLD 또는 LD_PRELOAD shim). 기능별 비교: [docs/USAGE.md](docs/ko/USAGE.md#tmux와-기능-비교)

## FAQ

<details>
<summary><b>tmux 스크립트를 그대로 쓸 수 있나요?</b></summary>

대부분은 `tmux`를 `zeropty`로 바꾸면 돼요. 창·창 분할이 없고, 타깃 이름은 정확히 일치해야 하고, 포맷 조건식(`#{?...}`)이 없고, 모르는 옵션은 오류라는 점이 달라요. [자세히](docs/ko/FAQ.md#tmux-스크립트를-그대로-쓸-수-있나요)
</details>

<details>
<summary><b>vim이나 claude처럼 터미널이 필요한 프로그램은요?</b></summary>

`zeropty new-session -d -s v "dist/zeropty-tty vim notes.txt"`처럼 `zeropty-tty`로 띄우면 돼요. claude 같은 Node·Bun 프로그램은 바이너리를 그대로 둔 채 [런타임 어댑터](docs/ko/USAGE.md#shim-대신-런타임-어댑터)를 불러올 수도 있어요. [자세히](docs/ko/USAGE.md#터미널이-필요한-프로그램-zeropty-tty)
</details>

<details>
<summary><b><code>capture-pane</code>이 300ms 늦은 화면을 주지 않나요?</b></summary>

아니요. 출력은 CPU를 아끼려고 모아 읽지만, 화면을 읽는 명령은 밀린 출력부터 처리하고 답해요. [자세히](docs/ko/FAQ.md#capture-pane이-300ms-늦은-화면을-주지-않나요)
</details>

<details>
<summary><b>Linux에서도 되나요?</b></summary>

macOS와 Linux(x86_64)에서 돼요. Windows는 안 돼요. Linux에서는 엔진이 epoll을 쓰고 shim이 `LD_PRELOAD`로 걸려요. [자세히](docs/ko/FAQ.md#linux나-windows에서도-되나요)
</details>

→ 더 많은 질문: [docs/FAQ.md](docs/ko/FAQ.md)

## 문서

| | |
|---|---|
| [사용법](docs/ko/USAGE.md) | 모든 명령과 옵션, 키 이름, 포맷 변수, 소켓 프로토콜, 임베디드 API, 환경변수 |
| [플러그인](docs/ko/PLUGINS.md) | 세션 이벤트를 듣고 명령을 더하는 플러그인: 붙이는 방법, 권한, 프로토콜, API |
| [FAQ](docs/ko/FAQ.md) | 자주 묻는 질문과 문제 해결 |
| [벤치마크](docs/ko/BENCHMARKS.md) | 모든 측정값과 재현 방법 |
| [설계](docs/ko/DESIGN.md) | 구조 그림, 왜 빠른가, 코드 지도 |
| [버전 정책](docs/ko/VERSIONING.md) · [변경 기록](docs/ko/CHANGELOG.md) | 호환 범위와 MSRV, 버전마다 바뀐 내용 |
| [API 문서](docs/ko/README.md#rust-api-문서) | Rust 라이브러리와 C·Node 바인딩의 rustdoc. 공개 함수마다 실행되는 예제가 있어요. 저장소에서 `cargo doc --no-deps --workspace --open`으로 열어요 |

## 기여

버그 제보, 측정 결과, 코드 모두 환영해요. [기여 안내](docs/ko/CONTRIBUTING.md)에 빌드·테스트·측정 방법이 있어요. 보안 문제는 [보안 정책](docs/ko/SECURITY.md)을 따라 비공개로 알려 주세요. 커뮤니티는 [행동 강령](docs/ko/CODE_OF_CONDUCT.md)을 따라요.

```sh
cargo test --release
```

호스팅 CI 없이 `scripts/check.sh`가 모든 검사를 돌려요(호스팅 작업은 릴리스 묶음 빌드 하나뿐이에요). 빌드, 테스트(doctest 포함), 예제 7개, `cargo doc -D warnings`, `clippy -D warnings`, `cargo fmt --check`, MSRV(Rust 1.88) 확인, 공개 API 기준선, 커밋 메시지·변경 기록 확인이 들어 있어요. `git config core.hooksPath .githooks`를 해 두면 push할 때마다 `.githooks/pre-push`가 이 스크립트를 돌려요. 자세한 단계는 [기여 안내](docs/ko/CONTRIBUTING.md#pull-request)에 있어요.

화면은 `vt100` crate와 대조하고(녹화한 claude 세션 + 무작위 입력), 서버·임베디드·세션 옮기기는 `cat`, `printf` 같은 평범한 프로그램으로 확인해요. claude 없이 돌아가요.

## 버전과 변경 기록

[Semantic Versioning](docs/ko/VERSIONING.md)을 따라요. CLI, 소켓 프로토콜, C ABI, Node API, 환경변수 중 무엇이 호환 범위인지와 MSRV(Rust 1.88)는 [docs/VERSIONING.md](docs/ko/VERSIONING.md)에, 바뀐 내용은 [CHANGELOG.md](docs/ko/CHANGELOG.md)에 있어요. 버전 확인: `zeropty --version`, C `zeropty_version()`, Node `version()`. 기여 방법(커밋 형식): [CONTRIBUTING.md](docs/ko/CONTRIBUTING.md) · 릴리스 절차: [docs/RELEASING.md](docs/ko/RELEASING.md)

## 라이선스

[MIT](LICENSE-MIT) 또는 [Apache-2.0](LICENSE-APACHE) 중에서 골라 쓰면 돼요.
