---
description: zeropty-tty와 claude-zeropty로 claude, vim, python REPL, 대화형 셸을 zeropty 세션에서 실행해요.
---

# claude와 다른 터미널 프로그램 실행하기

이 가이드를 마치면 터미널이 필요한 프로그램을 zeropty 세션에서 실행할 수 있어요. python REPL, [Claude Code](https://docs.anthropic.com/en/docs/claude-code)(`claude`), 그리고 `C-c`로 작업을 멈출 수 있는 대화형 셸이에요.

## 시작하기 전에

- zeropty가 설치된 Mac이 필요해요. `zeropty-tty`, `claude-zeropty`, `shim.dylib`이 한 폴더에 있어야 해요. 릴리스 아카이브와 `./build.sh`(`dist/`)는 모두 그 폴더에 넣어 줘요. `cargo install`은 그렇지 않아요. 자세한 내용은 [설치와 지원 플랫폼](/ko/guide/install)을 보세요.
- 셸에서 `C-c`가 작업에 닿으려면(4단계) zeropty 0.1.5 이상이 필요해요. `zeropty -V`로 확인하세요.
- 3단계에는 `PATH`에 `claude`가 있어야 해요.

아래 명령은 `dist/zeropty-tty`를 써요. 릴리스 폴더를 쓴다면 그 폴더의 경로로 바꾸세요.

## 1. 프로그램에 터미널이 필요한 이유 보기

세션의 터미널은 PTY(pseudo-terminal)가 아니라 소켓이에요. `isatty()`로 터미널인지 확인하는 프로그램은 터미널이 아니라고 판단해요. vim, less, python REPL, claude 모두 다르게 동작해요. 자세한 내용은 [세션이 터미널을 얻는 방법](/ko/guide/concepts/terminal)을 보세요.

**`zeropty-tty`**가 이 문제를 풀어요. 프로그램을 **shim**(`shim.dylib`, `DYLD_INSERT_LIBRARIES`로 넣어요)과 함께 시작하고, shim이 터미널처럼 다음 호출에 답해요.

| 호출 | 프로그램이 받는 것 |
|---|---|
| `isatty`, `tcgetattr` | 세션 자신의 소켓 위의 터미널 |
| `TIOCGWINSZ` | 세션의 크기 |
| `tcgetpgrp`, `tcsetpgrp` | 포그라운드 프로세스 그룹 |
| `/dev/tty` | 읽고 쓸 수 있는 세션의 소켓 |
| `readpassphrase`, `getpass` | 에코하지 않는 프롬프트(0.1.4부터) |

세션의 소켓만 터미널로 쳐요. stdout을 파이프로 넘기거나(`zeropty-tty prog | cat`) stdin이 `/dev/null`인 프로그램은 거기서 터미널을 보지 못해요. tmux에서도 마찬가지예요.

## 2. zeropty-tty로 프로그램 시작하기

명령 앞에 `zeropty-tty`를 붙여요.

```sh [터미널]
zeropty new-session -d -s py -x 80 -y 10 "dist/zeropty-tty python3"
zeropty send-keys -t '=py:' -l 'print(6 * 7)'
zeropty send-keys -t '=py:' Enter
zeropty wait-pane -t '=py:' -m '42' -T 10000
```

vim도 같은 방식이에요. `zeropty new-session -d -s v "dist/zeropty-tty vim notes.txt"`로 시작해요. 프로그램이 raw 모드로 바꾸면 shim이 알려 주고, 키는 그대로 전달돼요.

macOS는 SIP로 보호되는 바이너리(`/usr/bin`, `/bin`, `/sbin`, `/System`)와 hardened runtime으로 서명된 바이너리에는 shim을 넣지 못하게 해요. `zeropty-tty`는 이런 프로그램을 서명만 바꾼 사본으로 실행하고, 사본은 `~/.zeropty/tty/`(`ZEROPTY_CACHE`)에 둬요. 원본이 바뀌면 사본을 다시 만들고, 내가 준 이름으로 실행하므로 `ps`와 `pgrep`에서 찾을 수 있어요.

## 3. claude-zeropty로 claude 시작하기

**`claude-zeropty`**는 `zeropty-tty claude`를 실행하는 작은 래퍼예요.

```sh [터미널]
zeropty new-session -d -s fix-1 -x 220 -y 80 -c ~/proj "$PWD/dist/claude-zeropty"
```

`~/proj`는 에이전트가 일할 저장소로 바꾸세요. 다른 claude를 쓰려면 `ZEROPTY_CLAUDE_REAL`에 경로를 주세요. 세션은 서버의 환경을 받으므로 서버가 시작되기 전에 설정하거나, `-e`로 넘겨요.

```sh [터미널]
zeropty new-session -d -s fix-2 -e ZEROPTY_CLAUDE_REAL=/opt/claude/bin/claude "$PWD/dist/claude-zeropty"
```

`/opt/claude/bin/claude`는 내 claude의 경로로 바꾸세요. [셸에서 에이전트 다루기](/ko/guide/shell)처럼 에이전트에 일을 주고 기다리세요.

## 4. 대화형 셸 실행하고 작업 멈추기

세션의 셸에 명령을 직접 치려면 `zeropty-tty`로 셸을 시작하세요.

```sh [터미널]
zeropty new-session -d -s sh1 "dist/zeropty-tty zsh"
zeropty send-keys -t '=sh1:' -l 'sleep 30'
zeropty send-keys -t '=sh1:' Enter
zeropty send-keys -t '=sh1:' C-c
```

`C-c`, `C-\`, `C-z`는 터미널이 보내는 것처럼 시그널로 포그라운드 작업에 전달돼요. 셸의 `tcsetpgrp`가 shim을 거쳐 zeropty에 전달되므로 셸 자체가 shim과 함께 실행돼야 해요. 작업은 `/usr/bin`을 포함해 어떤 프로그램이든 돼요. `C-z`와 `fg`도 파이프라인을 포함해 동작해요. 세션 프로그램 아래에 멤버가 있는 프로세스 그룹만 받아들여요.

::: warning 셸에서 직접 친 프로그램
세션의 셸에서 직접 친 `/usr/bin` 프로그램에는 shim이 들어가지 않아요. 그 프로그램들은 shim 없이 실행돼요. 시그널은 여전히 닿지만, 거기서 친 `ssh`, `ssh-keygen`, `ssh-add`는 암호를 화면과 스크롤백에 그대로 에코해요. 대신 `zeropty-tty ssh-keygen`처럼 별도의 세션으로 시작하세요.
:::

## 확인하기

python 세션을 읽어 보세요.

```sh [터미널]
zeropty capture-pane -p -t '=py:'
```

REPL의 배너가 나오고, 그다음에 프롬프트, 내가 친 줄, 답이 보여야 해요.

```text
>>> print(6 * 7)
42
>>>
```

`>>>` 프롬프트와 줄 편집은 REPL의 터미널 모드예요. REPL은 터미널을 볼 때만 이 모드를 켜요.

4단계의 셸에서는 `zeropty capture-pane -p -t '=sh1:'`가 `sleep 30`이 계속 자고 있는 세션이 아니라, 그 뒤에 새 프롬프트가 나온 화면을 보여 줘요.

## 문제가 생기면

| 증상 | 해결 |
|---|---|
| 프로그램이 "터미널이 아니다"라며 다르게 동작해요 | `zeropty-tty`로 시작하세요. claude는 `claude-zeropty`를 쓰세요. |
| `/usr/bin/env`를 거쳐 시작한 스크립트에서 shim이 사라져요 | SIP가 거기서 shim을 지워요. 인터프리터를 직접 시작하세요: `zeropty-tty python3 script.py`. |
| `top`이나 `ps`가 실행되지 않아요 | 사본에는 원본의 권한(setuid, entitlement)이 없어요. 그 권한이 필요한 프로그램은 동작하지 않아요. |
| `C-c`가 셸의 프롬프트는 끝내지만 `sleep 30`은 못 끝내요 | 0.1.5로 올리기 전에 시작한 셸이라 옛 shim을 그대로 갖고 있어요. 셸을 다시 시작하세요. |
| 암호가 화면에 보여요 | 프로그램을 셸에서 직접 쳐서 shim 없이 실행됐어요. `zeropty-tty`로 시작하세요. |
| `zeropty-tty`가 shim을 찾지 못해요 | 폴더를 한데 두거나, `ZEROPTY_SHIM`이 `shim.dylib`를 가리키게 하세요. |

## 다음 단계

- shim과 라인 디시플린은 [세션이 터미널을 얻는 방법](/ko/guide/concepts/terminal)을 보세요.
- 에이전트에 일을 주고 기다리려면 [셸에서 에이전트 다루기](/ko/guide/shell)를 보세요.
- `ZEROPTY_SHIM`, `ZEROPTY_CACHE`, `ZEROPTY_CLAUDE_REAL`은 [환경변수](/ko/reference/environment)를 보세요.
