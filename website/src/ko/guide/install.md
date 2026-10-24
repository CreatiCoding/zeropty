---
description: zeropty가 돌아가는 맥과 Linux, 설치하는 세 가지 방법, 설치되는 파일마다의 용도를 정리했어요.
---

# 설치와 지원 플랫폼

이 페이지는 zeropty가 지원하는 플랫폼을 적고, 설치 방법을 비교하고, 받는 파일마다 무엇인지 설명해요. 모든 명령을 따라 하는 첫 설치는 [시작하기](/ko/guide/getting-started#1-설치하기)를 보세요.

## 지원 플랫폼

| 플랫폼 | 지원 |
|---|---|
| macOS 11 이상, Apple silicon | 지원해요. 릴리스 바이너리가 macOS 11 이상에서 로드돼요 |
| macOS 11 이상, Intel | 릴리스 묶음으로 지원해요(`zeropty`와 `shim.dylib`에 `x86_64` 슬라이스가 있어요) |
| Linux x86_64, glibc 2.29·커널 5.3 이상 | 0.1.6부터 Linux 릴리스 묶음으로 지원해요(Ubuntu 20.04·Debian 11 이상) |
| 확인한 버전 | macOS 15, macOS 26, Ubuntu 22.04, Ubuntu 24.04 |
| Linux arm64 | 아직 없어요 |
| Windows | 지원하지 않아요 |

엔진은 macOS에서 kqueue, Linux에서 epoll을 기다려요. 터미널이 필요한 프로그램을 속이는 **shim**은 macOS에서 `DYLD_INSERT_LIBRARIES`로 거는 `shim.dylib`, Linux에서 `LD_PRELOAD`로 거는 `shim.so`예요. Linux에서 정적 링크된 프로그램(대부분의 Go 바이너리)은 shim을 불러오지 않아서 터미널을 못 봐요.

소스에서 빌드하려면 최소 지원 Rust 버전(MSRV)인 [Rust](https://www.rust-lang.org) 1.88 이상이 필요해요. 자세한 내용은 [버전 정책](/ko/guide/versioning)을 보세요.

## 설치 방법

| 방법 | 필요한 것 | 받는 것 | 빠지는 것 |
|---|---|---|---|
| 릴리스 묶음 | `curl`, `shasum`(Linux는 `sha256sum`) | `zeropty`, shim, `zeropty-tty`, `claude-zeropty` | C·Node 임베딩 라이브러리, bash builtin |
| 소스에서 `./build.sh`(macOS) | Rust stable, Xcode Command Line Tools | 전부, `dist/`에 | 없어요 |
| `cargo install --git` | Rust | `zeropty`만, `~/.cargo/bin`에 | shim, `zeropty-tty`, `claude-zeropty`, 임베딩 라이브러리, bash builtin |

- **릴리스 묶음.** 0.1.3부터 릴리스마다 `zeropty-VERSION-darwin-universal.tar.gz`가, 0.1.6부터는 `zeropty-VERSION-linux-x86_64.tar.gz`도 붙어 있고, `SHA256SUMS`는 둘을 함께 담아요. `VERSION`은 `0.1.8` 같은 릴리스 번호로 바꾸세요. 풀기 전에 묶음을 확인하세요. macOS는 `shasum -a 256 -c --ignore-missing SHA256SUMS`, Linux는 `sha256sum -c --ignore-missing SHA256SUMS`예요(`--ignore-missing`은 받지 않은 묶음을 건너뛰어요). 스크립트에서 버전을 고정하려면 버전과, 그 릴리스 `SHA256SUMS`에 있는 묶음 해시를 고정해 두세요.
- **`./build.sh`.** 저장소를 clone하고 `./build.sh`를 실행해요. 워크스페이스를 release 모드로 빌드하고 결과를 `dist/`에 복사해요. 계속 쓰려면 `dist/`를 `PATH`에 넣어 두세요.
- **`cargo install`.** CLI와 서버만 필요하면 한 줄이에요.

```sh [터미널]
cargo install --git https://github.com/CreatiCoding/zeropty   # ~/.cargo/bin/zeropty
```

`sh`, `python3`, 빌드 명령처럼 터미널이 없어도 되는 프로그램을 돌리기에는 이걸로 충분해요. `cargo install`은 패키지의 실행 파일만 설치해서, `./build.sh`가 따로 빌드하는 파일은 빠져요. 자세한 내용은 [FAQ](/ko/guide/faq#cargo-install로-설치하면-무엇이-빠지나요)를 보세요.

Homebrew tap은 없고, crate는 crates.io에 올라가 있지 않아요. Rust 프로그램은 git 의존성으로 엔진을 쓸 수 있어요. 자세한 내용은 [C와 Rust에 zeropty 내장하기](/ko/guide/embed)를 보세요.

Linux에서는 Linux 묶음으로 같은 순서를 따라요.

```sh [터미널]
v=0.1.8
curl -fLO "https://github.com/CreatiCoding/zeropty/releases/download/v$v/zeropty-$v-linux-x86_64.tar.gz"
curl -fLO "https://github.com/CreatiCoding/zeropty/releases/download/v$v/SHA256SUMS"
sha256sum -c --ignore-missing SHA256SUMS     # zeropty-0.1.8-linux-x86_64.tar.gz: OK
tar -xzf "zeropty-$v-linux-x86_64.tar.gz"
export PATH="$PWD/zeropty-$v-linux-x86_64:$PATH"   # 계속 쓰려면 ~/.bashrc에 넣어요
```

::: warning Gatekeeper (macOS)
릴리스 파일은 ad-hoc 서명이고 공증은 받지 않았어요. 브라우저로 받으면 quarantine 표시가 붙고, macOS가 `zeropty`를 막아요. `xattr -dr com.apple.quarantine FOLDER`로 지우세요. `FOLDER`는 푼 폴더로 바꾸세요. `curl`은 표시를 붙이지 않아요.
:::

## 파일마다의 용도

| 파일 | 묶음에 | `dist/`에 | 용도 |
|---|---|---|---|
| `zeropty` | 있어요 (arm64, x86_64) | 있어요 | 서버와 CLI |
| `shim.dylib`(macOS) | 있어요 (arm64, arm64e, x86_64) | 있어요 (arm64, arm64e) | 세션의 소켓을 프로그램에게 터미널처럼 보이게 해요 |
| `shim.so`(Linux) | 있어요 (x86_64) | 없어요 | 같은 일을 `LD_PRELOAD`로 해요 |
| `zeropty-tty` | 있어요 | 있어요 | shim을 넣어 프로그램을 띄워요(vim, less, claude) |
| `claude-zeropty` | 있어요 | 있어요 | `zeropty-tty claude`를 실행해요 |
| `libzeropty_c.dylib`, `zeropty.h` | 없어요 | 있어요 | C ABI(애플리케이션 바이너리 인터페이스)로 엔진 넣기 |
| `zeropty.node` | 없어요 | 있어요 | Node.js에 엔진 넣기, `client/embedded.mjs`가 불러요 |
| `zeropty_bash.dylib` | 없어요 | 있어요 | bash builtin: `enable -f dist/zeropty_bash.dylib zeropty` |
| `LICENSE-MIT`, `LICENSE-APACHE` | 있어요 | 없어요 | 라이선스 |

이 파일들은 한곳에 두세요. `zeropty-tty`는 자기 옆의 shim을 찾고, `claude-zeropty`도 같은 방식으로 `zeropty-tty`를 찾아요. `zeropty-tty`나 `claude-zeropty`만 다른 곳에 심볼릭 링크하지 말고 폴더를 `PATH`에 넣거나, `ZEROPTY_SHIM`으로 shim을 가리켜 주세요.

zsh 함수 `shell/zeropty.zsh`는 `dist/`가 아니라 저장소에 있어요. `source shell/zeropty.zsh`로 불러요.

## zeropty가 만드는 파일

| 경로 | 무엇 |
|---|---|
| `~/.zeropty/mux.sock` | 서버 소켓(`ZEROPTY_SOCK`으로 바꿔요) |
| `~/.zeropty/tty/` | macOS만: `zeropty-tty`가 실행하는 프로그램(`/usr/bin`에 있는 것 등)의 서명을 바꾼 사본(상위 폴더는 `ZEROPTY_CACHE`로 바꿔요). Linux는 프로그램을 그대로 실행해요 |

## 끄기와 지우기

1. 서버와 그 안의 모든 세션을 꺼요.

   ```sh [터미널]
   zeropty kill-server
   ```

   서버는 모든 세션의 프로세스 그룹에 SIGHUP을 보내고 끝나요. 다른 소켓의 서버라면 `ZEROPTY_SOCK`도 함께 주세요.

2. 풀었거나 clone한 폴더를 지우고, `~/.zshrc`에 넣은 `PATH` 줄도 지워요.
3. `cargo install`로 설치했다면 `cargo uninstall zeropty`를 실행해요.
4. `~/.zeropty`를 지워서 소켓 파일과 `zeropty-tty`가 만든 사본을 없애요.

## 다음 단계

- [시작하기](/ko/guide/getting-started): 첫 세션을 돌려 봐요.
- [세션이 터미널을 얻는 방법](/ko/guide/concepts/terminal): shim과 `zeropty-tty`가 왜 있는지.
- [C와 Rust에 zeropty 내장하기](/ko/guide/embed): `./build.sh`가 `dist/`에 만드는 라이브러리를 써요.
