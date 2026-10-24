---
description: tmux 스크립트를 zeropty로 바꿔요. 일대일로 대응하는 명령, 없는 기능, 스크립트를 깨뜨리는 차이를 알아봐요.
---

# tmux에서 넘어오기

이 가이드를 마치면 기존 [tmux](https://github.com/tmux/tmux) 스크립트를 zeropty에서 실행하고, 폴링 루프를 `wait-pane`으로 바꾸고, 스크립트가 다르게 동작하게 만드는 몇 가지 차이를 알게 돼요.

## 시작하기 전에

- `PATH`에 zeropty가 있는 Mac이 필요해요. 자세한 내용은 [설치와 지원 플랫폼](/ko/guide/install)을 보세요.
- tmux로 프로그램을 다루는 셸 스크립트가 필요해요. `new-session`, `send-keys`, `capture-pane` 같은 명령을 쓰는 스크립트예요.
- 스크립트가 터미널이 필요한 프로그램(claude, vim)을 시작한다면 릴리스 아카이브나 `./build.sh`의 `zeropty-tty`가 필요해요.

## 1. 일대일로 대응하는 것 확인하기

zeropty는 프로그램을 다루는 데 필요한 tmux의 명령 이름, 별칭, 인자를 그대로 받아요. 포맷 변수는 같은 세션에서 tmux와 대조했고, 키 바이트는 tmux 3과 대조했어요.

| 똑같이 동작하는 것 | 참고 |
|---|---|
| `new-session`, `kill-session`, `has-session`, `list-sessions`, `rename-session` | `new-session`은 `-n -E -D -X`를 받고 무시해요 |
| `send-keys`, `paste-buffer`, `load-buffer` | 같은 [키 이름](/ko/reference/keys-and-formats#키-이름) |
| `capture-pane` | `-p -e -J -N -S -E -b -q`만 돼요. 다른 옵션은 오류예요 |
| `display-message -p`, `list-sessions -F` | [포맷 변수](/ko/reference/keys-and-formats#포맷-변수) 25개 |
| `pipe-pane`, `resize-window`, `set-environment -g` | `pipe-pane -I`는 없어요 |
| `attach` | `Ctrl-b d`로 떼요. `-r`은 읽기 전용이에요 |

## 2. 없는 것 확인하기

zeropty는 프로그램을 계속 실행하고, 화면을 읽고, 키를 보내는 일을 해요. 사람이 터미널을 나눠 쓰는 기능은 없어요.

- **창(window)과 창 분할(pane)이 없어요.** 세션 하나가 프로그램 하나를 실행해요. `split-window`나 `new-window`는 없어요. `window_index`와 `pane_index`는 늘 `0`이에요.
- **상태 표시줄, 복사 모드, 단축키 설정, 설정 파일, 훅이 없어요.**
- **포맷 조건식과 수식어가 없어요**(`#{?...}`, `#{t:...}`). 모르는 변수는 tmux처럼 빈 문자열이 돼요.
- **제어 모드(`-C`)가 없어요.** 프로그램은 대신 JSON 소켓 프로토콜이나 Node 클라이언트를 써요.

전체 표는 [tmux 호환](/ko/reference/tmux-compatibility)를 보세요.

## 3. 스크립트 바꾸기

`tmux`를 `zeropty`로 바꾸세요. 변수 한 줄을 쓰면 변경이 작아져요.

```sh [agents.sh]
#!/bin/sh
set -e
T=zeropty                                   # was: T=tmux
$T new-session -d -s fix-1 -x 220 -y 80 -c ~/proj "$PWD/dist/claude-zeropty"
$T send-keys -t '=fix-1:' -l 'Fix the failing test'
$T send-keys -t '=fix-1:' Enter
```

`~/proj`는 내 저장소로 바꾸세요. 터미널이 필요한 프로그램은 `zeropty-tty`로 시작해야 하고, claude는 `claude-zeropty`가 그 일을 해요. 자세한 내용은 [claude와 다른 터미널 프로그램 실행하기](/ko/guide/terminal-programs)를 보세요.

첫 `new-session`은 tmux처럼, 서버가 없으면 서버를 띄워요.

## 4. 폴링을 wait-pane으로 바꾸기

tmux 스크립트는 `capture-pane`을 루프로 불러서 에이전트가 끝났는지 알아요. zeropty는 일이 일어나는 순간에 답해요.

::: code-group

```sh [이전 (tmux)]
until tmux capture-pane -p -t '=fix-1:' | grep -q 'All tests pass'; do
  sleep 0.2
done
```

```sh [이후 (zeropty)]
zeropty wait-pane -t '=fix-1:' -m 'All tests pass' -T 600000
```

:::

`-m`은 글자를 기다리고, `-q`는 출력이 멈출 때까지 기다리고, `-T`는 타임아웃을 밀리초로 정해요. 서버는 출력이 올 때만 확인해요(감지 p50 1ms, p95 2ms). 여러 세션 중 어느 것이 어떤 글자를 보여 주는지 알려면 세션마다 `capture-pane`을 하지 말고 `grep-panes`를 한 번 쓰세요. 자세한 내용은 [화면, 키, 기다리기](/ko/guide/concepts/screens)를 보세요.

## 5. 스크립트를 깨뜨리는 차이 고치기

| 차이 | 바꿀 것 |
|---|---|
| **타깃은 정확히 일치해야 해요.** 앞부분 일치가 없어서 `-t fix`는 `fix-1`을 찾지 못해요. | 전체 이름을 쓰세요. 타깃은 `'=name:'`으로 쓰세요. |
| **모르는 옵션은 오류예요**(`send-keys: unknown flag -X`). 무시하지 않아요. | zeropty가 목록에 두지 않은 옵션은 지우세요. 예전에는 조용히 무시해서 결과가 달라졌어요. |
| **서버는 자신을 띄운 프로세스의 환경을 가져요.** | 변수는 `new-session -e VAR=value`로 넘기거나, 모든 새 세션에는 `set-environment -g VAR value`를 쓰세요. |
| **zsh에서 `=name:`**은 명령 경로로 바뀌어요(`=` 확장). | 따옴표로 감싸세요: `'=fix-1:'`. tmux도 같아요. |
| **`C-d`는 세션의 입력을 영영 닫아요.** 소켓은 EOF를 한 번만 알릴 수 없어서예요. | 끝낼 생각이 아니면 `C-c`나 프로그램의 종료 명령을 보내세요. |
| **PTY가 없어요.** 직접 실행한 프로그램은 터미널을 보지 못해요. | `zeropty-tty`로 시작하세요. |

세션의 환경에서 `TMUX`와 `TMUX_PANE`은 지워요. 그래서 세션 안의 프로그램은 자기가 tmux 안에서 돈다고 생각하지 않아요. 종료 코드는 tmux를 따라요. 성공은 0, 실패는 1이에요. 자세한 내용은 [종료 코드](/ko/reference/keys-and-formats#종료-코드)를 보세요.

## 확인하기

바꾼 스크립트를 실행하고 에이전트를 기다리거나, claude 대신 `sh`로 이 확인을 해 보세요.

```sh [터미널]
zeropty new-session -d -s demo -x 80 -y 6 sh
zeropty send-keys -t '=demo:' -l 'echo "exit: $?"'
zeropty send-keys -t '=demo:' Enter
zeropty wait-pane -t '=demo:' -m 'exit: 0' -T 10000
zeropty list-sessions
```

tmux의 목록 형식 그대로 세션이 보여야 해요. `...` 자리에는 만든 시각이 들어가요.

```text
demo: 1 windows (created ...)
```

`zeropty capture-pane -p -t '=demo:'`는 친 줄과 `exit: 0`을 보여 줘요.

## 문제가 생기면

| 증상 | 해결 |
|---|---|
| 짧은 이름에 `can't find session`이나 `can't find pane`이 나와요 | 타깃은 정확히 일치해야 해요. 전체 이름을 쓰세요. |
| tmux가 받던 옵션에 `unknown flag`가 나와요 | zeropty에는 없는 옵션이에요. [명령 레퍼런스](/ko/reference/commands#명령-레퍼런스)를 확인하세요. |
| `split-window`나 `new-window`가 실패해요 | 창이 없어요. 프로그램마다 세션을 하나씩 시작하세요. |
| 프로그램이 "터미널이 아니다"라며 다르게 동작해요 | `zeropty-tty`로 시작하세요. |
| 내 셸의 변수가 새 세션에 없어요 | 서버는 시작할 때의 환경을 가져요. `-e`나 `set-environment -g`를 쓰세요. |
| 포맷이 빈 문자열을 출력해요 | 모르는 변수이거나 조건식·수식어를 썼어요. [포맷 변수](/ko/reference/keys-and-formats#포맷-변수)를 보세요. |

## 다음 단계

- 기능별 비교 표는 [tmux 호환](/ko/reference/tmux-compatibility)를 보세요.
- `wait-pane`, `grep-panes`, 빠른 경로는 [셸에서 에이전트 다루기](/ko/guide/shell)를 보세요.
- [FAQ: tmux 스크립트를 그대로 쓸 수 있나요?](/ko/guide/faq#tmux-스크립트를-그대로-쓸-수-있나요)
