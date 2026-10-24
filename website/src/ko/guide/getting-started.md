---
description: zeropty를 설치하고, 세션을 띄우고, 키를 보내고, wait-pane으로 출력을 기다린 뒤 화면을 읽어요.
---

# 시작하기

이 안내를 끝내면 zeropty가 설치되어 있고, 셸을 돌리는 세션이 하나 떠 있고, 그 세션의 출력을 기다린 뒤 화면을 읽어 온 상태가 돼요. 전체 흐름은 명령 다섯 개, 1초쯤이면 끝나요.

## 시작하기 전에

- 맥(macOS 11 이상, Apple silicon·Intel)이나 Linux(x86_64, glibc 2.29 이상)가 필요해요. 자세한 내용은 [설치와 지원 플랫폼](/ko/guide/install)을 보세요.
- 아래 중 하나가 필요해요.
  - **릴리스 묶음.** macOS에 기본으로 있는 `curl`과 `shasum`만 있으면 돼요. Rust는 필요 없어요.
  - **소스에서 빌드(macOS).** [Rust](https://rustup.rs)(stable)와 Xcode Command Line Tools(`xcode-select --install`)가 필요해요.
- claude나 다른 에이전트는 없어도 돼요. 이 안내에서는 `sh`를 돌려요.

## 1. 설치하기

아래 두 방법 중 하나를 고르세요.

### 방법 A: 릴리스 내려받기

0.1.3부터 [릴리스](https://github.com/CreatiCoding/zeropty/releases)마다 모든 맥용 묶음 하나와 그 `SHA256SUMS`가 붙어 있어요. 둘 다 내려받아 해시를 확인한 뒤 풀어요.

```sh [터미널]
v=0.1.8   # or the latest: v=$(curl -fsSLo /dev/null -w '%{url_effective}' https://github.com/CreatiCoding/zeropty/releases/latest | sed 's|.*/v||')
curl -fLO "https://github.com/CreatiCoding/zeropty/releases/download/v$v/zeropty-$v-darwin-universal.tar.gz"
curl -fLO "https://github.com/CreatiCoding/zeropty/releases/download/v$v/SHA256SUMS"
shasum -a 256 -c --ignore-missing SHA256SUMS   # zeropty-0.1.8-darwin-universal.tar.gz: OK
tar -xzf "zeropty-$v-darwin-universal.tar.gz"
export PATH="$PWD/zeropty-$v-darwin-universal:$PATH"   # add it to ~/.zshrc to keep it
zeropty -V
```

`v`에 버전이 들어 있어요. 다른 버전으로 고정하려면 `0.1.8`을 바꾸세요. 마지막 명령은 버전을 출력해요. 예를 들면 이래요.

```text
zeropty 0.1.8
```

::: warning 브라우저로 받았나요?
파일은 ad-hoc 서명이고 공증은 받지 않았어요. `curl`은 quarantine 표시를 붙이지 않아서 위 순서대로면 그대로 실행돼요. 브라우저로 받으면 표시가 붙고, macOS가 창을 띄워 `zeropty`를 막아요. 처음 실행하기 전에 표시를 지우세요: `xattr -dr com.apple.quarantine zeropty-$v-darwin-universal`.
:::

Linux(x86_64)에서는 `zeropty-$v-linux-x86_64.tar.gz`를 받아 `sha256sum -c --ignore-missing SHA256SUMS`로 확인하고, `zeropty-$v-linux-x86_64`를 `PATH`에 넣어요. 명령 전체는 [설치와 지원 플랫폼](/ko/guide/install#설치-방법)을 보세요.

### 방법 B: 소스에서 빌드하기

빌드하면 임베딩용 라이브러리와 bash builtin까지 전부 생겨요.

```sh [터미널]
git clone https://github.com/CreatiCoding/zeropty && cd zeropty
./build.sh                       # puts zeropty and the embedding libraries in dist/
export PATH="$PWD/dist:$PATH"
```

첫 `./build.sh`는 깨끗한 clone과 빈 cargo 캐시로 Apple M1(8코어)에서 23~27초 걸렸어요.

## 2. 세션 띄우기

80열 6행 화면에서 `sh`를 돌리는 `demo` 세션을 띄워요.

```sh [터미널]
zeropty new-session -d -s demo -x 80 -y 6 sh                # starts the server if none is running
```

`-d`는 내 터미널을 비워 두고, `-s demo`는 세션 이름을 정해요. 돌고 있는 서버가 없었으니 이 명령이 서버도 띄워요. 자세한 내용은 [세션과 서버](/ko/guide/concepts/sessions)를 보세요.

## 3. 키 보내기

세션에 명령을 입력하고 Enter를 눌러요.

```sh [터미널]
zeropty send-keys -t demo -l 'sleep 1; echo "done: $((6 * 7))"'
zeropty send-keys -t demo Enter
```

`-t demo`는 **타깃**, 즉 키를 보낼 세션이에요. `-l`은 글자를 그대로 보내요. `Enter`만 따로 쓰면 키 이름이라서 Enter 키를 보내요.

## 4. wait-pane으로 출력 기다리기

화면에 `done: 42`가 나타날 때까지 최대 10초 기다려요.

```sh [터미널]
zeropty wait-pane -t demo -m 'done: 42' -T 10000            # no polling: returns the moment the text appears
```

셸이 그 줄을 출력하자마자, 1초쯤 뒤에 명령이 돌아와요. `-T`는 밀리초 단위 제한 시간이에요. 시간이 지나면 `wait-pane`은 종료 코드 1로 끝나요.

::: tip 입력한 줄이 아니라 결과를 기다리세요
실제 터미널처럼 입력한 글자는 화면에 에코돼요. 결과에만 나오는 글자를 기다리세요. 여기서 입력한 줄에는 `42`가 아니라 `$((6 * 7))`이 들어 있어요.
:::

## 5. 화면 읽기

세션의 지금 화면을 출력해요.

```sh [터미널]
zeropty capture-pane -p -t demo
```

`-p`는 화면을 표준 출력으로 내요. 없으면 tmux처럼 화면이 버퍼에 들어가요.

## 확인하기

입력한 줄과 그 결과가 보여야 해요.

```text
sleep 1; echo "done: $((6 * 7))"
done: 42
```

화면이 6줄(`-y 6`)이라 그 아래 빈 줄 4개가 이어지는데, 위에서는 생략했어요. `capture-pane`이 아무것도 출력하지 않으면 `zeropty ls`로 세션이 있는지 확인하세요.

## 6. 붙었다 떼고, 서버 끄기

세션을 직접 보려면 내 터미널을 붙여요.

```sh [터미널]
zeropty attach -t demo
```

`Ctrl-b d`로 떼요. 세션은 계속 돌아요. 다 썼으면 서버와 그 안의 모든 세션을 꺼요.

```sh [터미널]
zeropty kill-server
```

명령 목록은 `zeropty --help`, 명령 하나는 `zeropty help send-keys`로 봐요.

## 다음 단계

- [셸에서 에이전트 다루기](/ko/guide/shell): 실제 에이전트로 같은 흐름을 해 보고, 더 빠른 zsh 함수와 bash builtin도 써 봐요.
- [claude와 다른 터미널 프로그램 실행하기](/ko/guide/terminal-programs): 터미널이 필요한 프로그램을 `zeropty-tty`로 띄워요.
- [Node.js에서 zeropty 쓰기](/ko/guide/node): 프로그램에서 소켓이나 임베디드로 세션을 다뤄요.
- [화면, 키, 기다리기](/ko/guide/concepts/screens): `wait-pane`이 폴링 없이 아는 방법.
