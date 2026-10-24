---
description: 셸 스크립트로 에이전트를 시작하고, 키를 보내고, wait-pane으로 기다리고, 화면을 읽고, 여러 세션을 한꺼번에 찾아요.
---

# 셸에서 에이전트 다루기

이 가이드를 마치면 셸 스크립트로 여러 세션을 다룰 수 있어요. 세션을 시작하고, 입력하고, 각각이 끝날 때까지 기다리고, 조건에 맞는 세션을 찾아요. 명령마다 프로세스를 띄우지 않고 소켓에 바로 말하는 빠른 경로로 스크립트를 바꾸는 방법도 다뤄요.

## 시작하기 전에

- `PATH`에 zeropty가 있는 Mac이 필요해요. 자세한 내용은 [설치와 지원 플랫폼](/ko/guide/install)을 보세요.
- bash builtin을 쓰려면 소스에서 빌드해야 해요(`./build.sh`가 `dist/zeropty_bash.dylib`를 만들어요). 릴리스 아카이브에는 들어 있지 않아요.
- 아래 단계는 각 세션에서 `sh`를 실행하므로 claude가 없어도 돼요. 실제 스크립트에서는 대신 에이전트를 실행해요. 예를 들어 `dist/claude-zeropty`예요([claude와 다른 터미널 프로그램 실행하기](/ko/guide/terminal-programs) 참고).

## 1. 세션 시작하기

**세션**은 **서버**가 계속 실행해 주는 프로그램 하나예요. 서버가 없으면 첫 `new-session`이 서버를 띄워요.

```sh [터미널]
zeropty new-session -d -s build -x 80 -y 6 sh
```

`-d`는 내 터미널을 그대로 두고, `-s build`는 세션 이름을 정하고, `-x`/`-y`는 화면 크기를 정해요. 자세한 내용은 [세션과 서버](/ko/guide/concepts/sessions)를 보세요.

## 2. 키 보내기

모든 명령은 **타깃**(`-t`)으로 세션을 가리켜요. 스크립트에서는 `'=build:'`처럼 따옴표로 감싸서 써요.

```sh [터미널]
zeropty send-keys -t '=build:' -l 'ls / | head -3; echo "exit: $?"'
zeropty send-keys -t '=build:' Enter
```

`send-keys`의 인자는 하나하나가 키 이름이거나 글자 그대로의 텍스트예요.

| 이렇게 쓰면 | 이렇게 보내요 |
|---|---|
| `Enter`, `C-c`, `Up`, `F5` | 그 키의 바이트([키 이름](/ko/reference/keys-and-formats#키-이름)) |
| `-l 'Enter'` | `Enter`라는 다섯 글자. 키가 아니에요 |
| `-H 41 42 0a` | 16진수 바이트 그대로(`AB`와 줄바꿈) |
| `-N 3 Down` | 인자 전체를 세 번 |

프롬프트나 변수에서 온 텍스트에는 항상 `-l`을 쓰세요. 그러면 그 안의 `Enter`나 `Space` 같은 단어가 키로 해석되지 않아요. `Enter`는 `-l` 없이 별도 명령으로 보내요.

::: warning zsh에서는 타깃에 따옴표를 쓰세요
zsh는 `=word`를 명령의 경로로 바꿔요(`=` 확장). `=build:`가 아니라 `'=build:'`로 쓰세요. tmux 타깃도 같은 문제가 있어요.
:::

## 3. 결과 기다리기

`wait-pane`은 글자가 나타나거나(`-m`), 출력이 멈추거나(`-q`), 둘 다 만족할 때 답해요. `-T`는 타임아웃이에요.

```sh [터미널]
zeropty wait-pane -t '=build:' -m 'exit: 0' -T 10000
```

서버는 출력이 올 때만 확인하므로 폴링이 없어요. 친 키는 화면에 에코되므로, 결과에서만 출력되는 글자를 기다리세요. 위에서 친 줄에는 `0`이 아니라 `$?`가 들어 있어서, `exit: 0`은 출력에만 일치해요.

마지막에 어떤 글자가 나올지 알 수 없는 에이전트라면 대신 조용해질 때까지 기다려요.

```sh [터미널]
zeropty wait-pane -t '=fix-1:' -q 5000 -T 600000   # until output pauses for 5 s, at most 10 minutes
```

`wait-pane`은 시간이 초과되거나(`wait-pane: timed out`) 세션이 끝나면 종료 코드 1로 끝나요. 자세한 내용은 [화면, 키, 기다리기](/ko/guide/concepts/screens)를 보세요.

## 4. 화면 읽기

`capture-pane -p`는 화면을 출력해요. 밀린 출력을 먼저 처리하므로 화면은 늘 최신이에요.

```sh [터미널]
zeropty capture-pane -p -t '=build:'                 # the visible screen
zeropty capture-pane -p -e -S -200 -t '=build:'      # plus 200 lines of scrollback, with colors
zeropty capture-pane -p -J -S - -E - -t '=build:'    # everything, wrapped lines joined
```

| 옵션 | 효과 |
|---|---|
| `-p` | 표준 출력으로 내보내요(없으면 화면이 버퍼에 들어가요) |
| `-e` | 색·글꼴 이스케이프를 남겨요 |
| `-J` | 줄바꿈으로 나뉜 줄을 이어 붙여요 |
| `-N` | 줄 끝 공백을 남겨요 |
| `-S`, `-E` | 첫 줄과 마지막 줄. 0이 화면 첫 줄, 음수가 스크롤백, `-`는 끝까지예요 |

모든 옵션은 [화면 읽기](/ko/reference/commands#화면-읽기)를 보세요.

## 5. 여러 세션 다루기

작업마다 세션을 하나씩 시작한 다음, 각각을 기다리고 `grep-panes`로 한꺼번에 찾아요. 아래 내용을 `jobs.sh`로 저장하세요.

```sh [jobs.sh]
#!/bin/sh
set -e
for n in 1 2 3; do
  zeropty new-session -d -s "job-$n" -x 80 -y 10 sh
  zeropty send-keys -t "=job-$n:" -l "sleep $n; echo \"finished: \$?\""
  zeropty send-keys -t "=job-$n:" Enter
done
for n in 1 2 3; do
  zeropty wait-pane -t "=job-$n:" -m 'finished: 0' -T 30000
done
zeropty grep-panes 'finished: 0'
```

`grep-panes`는 일치한 줄을 `세션: 줄` 형태로 세션 이름순으로 출력하고, 일치하는 게 없으면 종료 코드 1로 끝나요. `-S -100`을 더하면 스크롤백까지 찾아요. 세션 1000개에서 16~32ms 걸렸어요. 화면을 하나씩 읽지 않고도 입력을 기다리는 에이전트를 찾을 때 써요.

## 6. 빠른 경로 쓰기

`zeropty` 프로그램을 실행하면 명령마다 프로세스가 떠요(3.59~4.23ms). zsh 함수와 bash builtin은 대신 셸 안에서 소켓으로 명령을 바로 보내요.

::: code-group

```sh [zsh (~/.zshrc)]
source /path/to/zeropty/shell/zeropty.zsh
```

```sh [bash]
enable -f /path/to/zeropty/dist/zeropty_bash.dylib zeropty
```

:::

`/path/to/zeropty`는 clone한 폴더로 바꾸세요. 스크립트는 그대로 두면 돼요. 이제 `zeropty`가 함수 또는 builtin이 될 뿐이에요.

| 경로 | 측정값 |
|---|---|
| zsh 함수 | `send-keys -l` p50 0.12ms(tmux CLI보다 50.9~52.7배 빨라요), `has-session` 0.11~0.17ms, 2000줄 `capture-pane` 0.50~0.55ms |
| bash builtin | 명령당 0.041~0.138ms(tmux CLI보다 48.7~157배 빨라요), macOS의 `/bin/bash` 3.2에서 확인했어요 |

둘 다 `attach`, `serve`, `restart-server`, `-V`, `--help`, 인자 없는 실행, 서버가 없을 때의 모든 명령은 여전히 프로그램을 실행해요. `ZEROPTY_BIN`을 지정하면 다른 프로그램을 가리킬 수 있어요.

## 확인하기

스크립트를 실행한 뒤 화면 하나를 읽어 보세요.

```sh [터미널]
sh jobs.sh
zeropty capture-pane -p -t '=job-2:'
```

`grep-panes`가 세션마다 한 줄씩 보여 줘야 해요.

```text
job-1: finished: 0
job-2: finished: 0
job-3: finished: 0
```

`job-2`의 화면에는 친 줄이 먼저 나오고, 그다음에 `finished: 0`이 나와요.

```text
sleep 2; echo "finished: $?"
finished: 0
```

`zeropty kill-server`로 정리하세요.

## 문제가 생기면

| 증상 | 해결 |
|---|---|
| `wait-pane -m`이 바로 끝나요 | 글자가 내가 친 줄에 들어 있어요. 결과에서만 출력되는 글자를 기다리세요. |
| 글자가 나왔는데도 `wait-pane: session ended`가 나와요 | 프로그램이 출력 직후에 끝났어요. 살아 있게 하세요. 예: `sh -c 'make; echo build done; exec cat'` |
| 보이는 세션인데 `can't find session: NAME` 또는 `can't find pane: NAME`이 나와요 | 타깃은 정확히 일치해야 해요. 앞부분 일치는 없어요. `zeropty ls`로 확인하세요. |
| `no server running on ...` | 그 소켓에서 듣는 서버가 없어요. `new-session`으로 서버를 띄우거나 `ZEROPTY_SOCK`을 지정하세요. |
| `send-keys: unknown flag -X` | 모르는 옵션은 오류예요. [키 보내기](/ko/reference/commands#키-보내기)를 보세요. |
| `C-d` 뒤로 입력이 안 들어가요 | 빈 줄의 `C-d`는 세션의 입력을 영영 닫아요. 대신 `C-c`나 프로그램의 종료 명령을 보내세요. |

## 다음 단계

- [claude와 다른 터미널 프로그램 실행하기](/ko/guide/terminal-programs)에서 이 세션에 에이전트를 넣어 보세요.
- [Node.js에서 zeropty 쓰기](/ko/guide/node)에서 명령마다 프로세스를 띄우지 않고 같은 흐름을 만들어 보세요.
- [기다리기와 찾기](/ko/reference/commands#기다리기와-찾기)에서 `wait-pane`과 `grep-panes`의 모든 옵션을, [종료 코드](/ko/reference/keys-and-formats#종료-코드)에서 종료 코드를 확인하세요.
