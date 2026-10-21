# 사용법

[English](../USAGE.md) · **한국어**

[README](../../README.ko.md) · [문서 목록](README.md) · [사용법](USAGE.md) · [플러그인](PLUGINS.md) · [FAQ](FAQ.md) · [벤치마크](BENCHMARKS.md) · [설계](DESIGN.md) · [버전 정책](VERSIONING.md) · [릴리스](RELEASING.md)

zeropty의 명령, 옵션, 프로토콜을 모두 적은 레퍼런스예요. 처음이라면 [README의 1분 시작](../../README.ko.md#1분-시작)부터 보세요.

- [기본 개념](#기본-개념)
- [CLI 빠르게 보기](#cli-빠르게-보기)
- [명령 레퍼런스](#명령-레퍼런스)
  - [세션](#세션) · [키 보내기](#키-보내기) · [화면 읽기](#화면-읽기) · [기다리기와 찾기](#기다리기와-찾기) · [버퍼](#버퍼) · [보기](#보기-attach) · [환경](#환경) · [서버와 세션 옮기기](#서버와-세션-옮기기) · [진단](#진단)
- [키 이름](#키-이름) · [포맷 변수](#포맷-변수) · [종료 코드](#종료-코드)
- [셸에서 빠르게: zsh 함수, bash builtin](#셸에서-빠르게-zsh-함수-bash-builtin)
- [터미널이 필요한 프로그램: zeropty-tty](#터미널이-필요한-프로그램-zeropty-tty)
- [키 입력과 라인 디시플린](#키-입력과-라인-디시플린)
- [다른 프로그램에서: 소켓](#다른-프로그램에서-소켓)
- [내 프로세스 안에: 임베디드](#내-프로세스-안에-임베디드)
- [세션 옮기기](#세션-옮기기)
- [환경변수](#환경변수)
- [tmux와 기능 비교](#tmux와-기능-비교)

## 기본 개념

- **세션 = 프로그램 하나.** `new-session`은 명령 하나를 `/bin/sh -c`로 띄워요. 창(window)이나 창 분할(pane)은 없고, 세션마다 화면 하나와 스크롤백이 있어요.
- **서버.** `zeropty` CLI는 서버(`~/.zeropty/mux.sock`)에 명령을 보내요. 서버가 없으면 `new-session`(그리고 `pull-sessions`)이 tmux처럼 띄워요. 같은 엔진을 내 프로세스 안에 넣을 수도 있어요([임베디드](#내-프로세스-안에-임베디드)).
- **타깃(`-t`).** 세션 이름으로 가리켜요. `=work`, `=work:`, `work`, `work:0`은 모두 세션 `work`예요. 이름은 **정확히 일치**해야 해요(tmux의 앞부분 일치는 없어요). 스크립트에서는 tmux처럼 `=이름:`을 권해요.
- **명령 형식은 어디서나 같아요.** CLI 인자, 소켓의 JSON `argv`, 임베디드 함수 호출, C ABI가 모두 같은 `["send-keys", "-t", "=work:", "Enter"]`을 받고 같은 결과(`code`, `out`, `err`)를 내요.
- **모르는 옵션은 오류예요**(`send-keys: unknown flag -X`). 예전에는 조용히 무시해서 `send-keys -N 3`이 한 번만 가는 식으로 결과가 달라졌어요.

## CLI 빠르게 보기

```sh
zeropty new-session -d -s work -x 220 -y 80 -c ~/proj "$PWD/dist/claude-zeropty"   # 서버가 없으면 띄워요
zeropty capture-pane -p -e -S -200 -t '=work:'    # 화면 + 스크롤백 200줄, 색 포함
zeropty wait-pane -t '=work:' -m '>' -q 1000 -T 60000   # '>'가 보이고 1초 조용할 때까지 (최대 60초)
zeropty send-keys -t '=work:' -l 'hello' && zeropty send-keys -t '=work:' Enter
zeropty attach -t work                            # 사람이 붙어서 보기. Ctrl-b d로 떼기
zeropty attach -r -t work                         # 읽기 전용 (q 또는 Ctrl-c로 떼기)
zeropty kill-session -t '=work:'
```

**도움말과 버전.** `zeropty --help`(또는 `-h`, `help`)는 명령과 옵션 목록을, `zeropty help <명령>`은 그 명령의 사용법과 하는 일, 이 문서의 해당 절 링크를 표준 출력에 내고 종료 코드 0으로 끝나요. 줄임말(`help send`)도 돼요. 프로그램이 직접 답해서 서버가 없어도 되고, 서버를 띄우지도 않아요. zsh 함수와 bash builtin에서도 같아요. `zeropty -V`(또는 `--version`)는 CLI 자신의 버전을 출력해요(실행 중인 서버의 버전은 `zeropty display -p '#{version}'`). 무엇이 호환 범위인지는 [VERSIONING.md](VERSIONING.md), 바뀐 내용은 [CHANGELOG.md](CHANGELOG.md)에 있어요. 옵션별 설명은 아래 [명령 레퍼런스](#명령-레퍼런스)에 있어요.

```sh
zeropty --help     # 명령 목록 (종료 코드 0)
zeropty help send-keys   # 명령 하나: 사용법, 하는 일, 문서 링크 (없는 명령이면 종료 코드 2)
zeropty -V         # zeropty X.Y.Z (--version도 같아요)
zeropty            # usage: zeropty serve | <tmux command> ...  (표준 에러, 종료 코드 2)
```

## 명령 레퍼런스

괄호 안은 tmux와 같은 줄임말이에요. 옵션 뒤의 `<값>`은 값을 받는 옵션이에요.

### 세션

| 명령 | 옵션 | 하는 일 |
|---|---|---|
| `new-session` (`new`) `[명령]` | `-d` `-s <이름>` `-x <열>` `-y <행>` `-c <폴더>` `-e <변수=값>` `-A` `-P` `-F <포맷>` | 세션을 만들어요. 명령이 없으면 `$SHELL`(없으면 `/bin/sh`)이에요. 크기 기본값은 80×24이고, 이름이 없으면 지금까지 만든 세션 수(`0`, `1`, ...)를 써요. `-A`는 같은 이름이 있으면 그대로 두고 성공해요. `-P`는 새 세션 정보를 출력해요(`-F`로 형식 지정, 기본 `이름:`). `-e`는 여러 번 쓸 수 있어요. `-n -E -D -X`는 창·클라이언트용이라 받아만 둬요 |
| `has-session` (`has`) | `-t` | 세션이 있으면 0, 없으면 1 (`can't find session: 이름`) |
| `kill-session` | `-t` `-a` | 세션의 프로세스 그룹에 SIGHUP을 보내고, 2초 뒤에도 남은 프로세스는 SIGKILL해요. `-a`는 `-t` 세션을 뺀 나머지 전부예요. `-C`는 받아만 둬요 |
| `list-sessions` (`ls`) | `-F <포맷>` | 세션 목록. 기본 출력은 tmux와 같은 `이름: 1 windows (created ...)`예요 |
| `rename-session` (`rename`) `<새 이름>` | `-t` | 이름을 바꿔요. 빈 이름, `:`·`.`이 든 이름, 이미 있는 이름은 오류예요 |
| `resize-window` (`resizew`), `resize-pane` (`resizep`) | `-t` `-x <열>` `-y <행>` | 화면 크기를 바꿔요. 프로그램은 SIGWINCH를 받아요 |
| `pipe-pane` (`pipep`) `[명령]` | `-t` `-o` `-O` | 세션 출력을 명령의 입력으로 계속 보내요(tmux의 `-O`가 기본). 명령 없이 부르면 닫혀요. `-o`는 이미 열려 있으면 닫기만 해요. 명령이 못 받는 출력은 8MB까지 기다리고 넘치면 버려요. 세션을 다른 프로세스로 옮기면 닫혀요 |
| `kill-server` | | 모든 세션에 SIGHUP을 보내요. 독립 서버는 프로세스도 끝나요. 임베디드 엔진은 세션만 끝나고 계속 돌아요 |

프로그램이 끝나면 세션도 끝나요(tmux의 `remain-on-exit` 꺼짐과 같아요).

서버는 자신을 띄운 프로세스(서버를 띄우는 첫 명령: `new-session`, `pull-sessions`, `zeropty serve`)의 환경을 그대로 가져요(tmux와 같아요). 세션은 `new-session`을 실행한 셸의 환경이 아니라 그 환경에 `set-environment -g`와 `-e`를 적용한 것을 받아요. `restart-server`는 그 명령을 실행한 셸에서 새 서버를 띄워서, 이후 세션은 그 셸의 환경을 받고 `set-environment -g`로 넣은 값은 사라져요(다시 넣어야 해요). 실행 중인 세션은 자기 환경을 그대로 가져요. 세션 안의 프로그램이 받는 환경: `TERM=xterm-256color`, `COLUMNS`/`LINES`, `ZEROPTY_SESSION`(세션 이름), `ZEROPTY_COLS`/`ZEROPTY_ROWS`, `ZEROPTY_SIZE_FILE`(shim이 읽는 창 크기 파일), `ZEROPTY_TTY_ID`(어느 파일이 세션의 터미널인지, shim용). `TMUX`, `TMUX_PANE`은 지워요(tmux 안에서 서버를 띄웠어도 세션은 tmux 안이 아니에요).

### 키 보내기

| 명령 | 옵션 | 하는 일 |
|---|---|---|
| `send-keys` (`send`) `<키>...` | `-t` `-l` `-H` `-N <횟수>` | 키를 보내요. 인자마다 [키 이름](#키-이름)이면 그 키의 바이트를, 아니면 글자 그대로를 보내요. `-l`은 키 이름을 해석하지 않고 글자 그대로 보내요. `-H`는 인자를 16진수 바이트로 읽어요(`-H 41 42 0a`는 `AB` + 줄바꿈). `-N`은 전체를 그 횟수만큼 반복해요 |
| `paste-buffer` (`pasteb`) | `-t` `-b <버퍼>` `-d` `-p` `-r` `-s <구분자>` | 버퍼 내용을 키처럼 보내요. 줄바꿈은 `\r`로 바꿔 보내요(`-r`이면 그대로, `-s`로 구분자 지정). `-p`는 프로그램이 bracketed paste를 켰으면 감싸서 보내요. `-d`는 보낸 뒤 버퍼를 지워요 |

키는 PTY처럼 [라인 디시플린](#키-입력과-라인-디시플린)을 거쳐요.

### 화면 읽기

| 명령 | 옵션 | 하는 일 |
|---|---|---|
| `capture-pane` (`capturep`) | `-t` `-p` `-e` `-J` `-N` `-S <줄>` `-E <줄>` `-b <버퍼>` `-q` | 화면을 텍스트로 읽어요. 읽기 직전에 밀린 출력부터 처리해서 늘 최신 화면이에요. `-e`는 색·글꼴 이스케이프 포함, `-J`는 줄바꿈으로 나뉜 줄을 이어 붙이고, `-N`은 줄 끝 공백을 남겨요. `-S`/`-E`는 tmux와 같은 줄 번호예요(0이 화면 첫 줄, 음수가 스크롤백, `-`는 끝까지). `-p`가 없으면 tmux처럼 화면을 버퍼(`-b`, 없으면 `buffer0`)에 넣고 아무것도 출력하지 않아요 |
| `display-message` (`display`) `[포맷]` | `-t` `-p` `-F <포맷>` | [포맷 변수](#포맷-변수)를 채워서 출력해요. 예: `display-message -p -t '=work:' '#{cursor_x},#{cursor_y}'` |

### 기다리기와 찾기

tmux에는 없는 zeropty 명령이에요. 에이전트가 끝났는지, 어느 세션이 입력을 기다리는지를 폴링 없이 알아요.

| 명령 | 옵션 | 하는 일 |
|---|---|---|
| `wait-pane` | `-t` `-m <글자>` `-q <ms>` `-T <ms>` | `-m`은 그 글자가 화면에 나타날 때까지(이미 있으면 바로), `-q`는 세션이 그 시간 동안 아무것도 출력하지 않을 때까지 기다려요. 둘 다 주면 둘 다 만족할 때 답해요. `-T`가 지나면 종료 코드 1(`wait-pane: timed out`), 기다리는 중에 세션이 끝나도 1(`wait-pane: session ended`)이에요 |
| `grep-panes <글자>` | `-S <줄>` | 모든 세션의 화면에서 글자를 찾아 `세션: 줄`로 보여요. `-S`는 `capture-pane -S`처럼 스크롤백까지 찾아요(`-S -100`). 세션 이름순이고, 없으면 종료 코드 1이에요(grep처럼) |

- `wait-pane`은 서버가 출력이 올 때만, 그 출력이 바꾼 행만 확인해요. 글자는 행 단위로 찾아요(줄바꿈으로 나뉜 글자는 못 찾아요).
- 같은 연결의 다음 명령은 `wait-pane`의 답 뒤에 처리돼요. `client/zeropty.mjs`는 `wait-pane`마다 연결을 따로 열어서 다른 명령이 막히지 않아요.
- 세션이 마지막 출력을 내자마자 끝나면, 엔진이 그 출력을 확인하기 전에 세션 종료를 먼저 알려 `wait-pane: session ended`(종료 코드 1)로 끝날 수 있어요(알려진 문제). 기다리는 글자가 나온 뒤에도 세션이 살아 있게 하세요. 예: `sh -c 'make; echo build done; exec cat'`
- 임베디드 엔진의 함수 호출로는 `wait-pane`을 쓸 수 없어요(답을 미룰 루프가 없어서). [서버 플러그인](#서버-플러그인-내-프로그램이-소켓도-열기)을 켜면 소켓으로는 돼요. Rust 호스트는 `Engine::wait_start`·`wait_check`를 직접 써요([`examples/polled.rs`](../../examples/polled.rs)).

### 버퍼

| 명령 | 옵션 | 하는 일 |
|---|---|---|
| `load-buffer` (`loadb`) `<파일>` | `-b <버퍼>` | 파일 내용을 버퍼에 넣어요(기본 `buffer0`). 상대 경로는 명령을 보낸 쪽의 현재 폴더 기준이에요 |
| `paste-buffer` | | [키 보내기](#키-보내기)를 보세요 |
| `capture-pane -b` | | `-p` 없이 쓰면 화면을 버퍼에 넣어요 |

### 보기 (attach)

| 명령 | 옵션 | 하는 일 |
|---|---|---|
| `attach-session` (`attach`, `a`) | `-t` `-r` | 지금 터미널로 세션에 붙어요. `Ctrl-b d`로 떼요. `-r`은 읽기 전용이고 `q`나 `Ctrl-c`로 떼요. `-t`가 없으면 세션이 하나일 때 그 세션에, 여럿이면 번호를 물어봐요 |

붙어 있는 동안 세션은 내 터미널 크기를 따르고, 떼면 원래 크기로 돌아와요. 여러 터미널이 한 세션에 붙을 수 있고, 크기는 마지막에 붙은 터미널을 따라요.

### 환경

| 명령 | 옵션 | 하는 일 |
|---|---|---|
| `set-environment` (`setenv`) `<변수> [값]` | `-g` `-u` | 이후에 만드는 세션에 줄 환경변수를 정해요. `-u`는 지워요. `-g` 없이는 tmux에서도 새 창에만 적용되는데, 세션에 새 창이 생기지 않으니 아무 일도 하지 않아요 |
| `show-environment` (`showenv`) | `-g` | `-g`로 정한 변수를 보여요. 지운 변수는 `-변수`로 보여요 |

### 서버와 세션 옮기기

| 명령 | 옵션 | 하는 일 |
|---|---|---|
| `serve` | `--bind <소켓>` | 독립 서버를 직접 띄워요(보통은 `new-session`이 알아서 띄워요) |
| `pull-sessions` `[이름...]` | `-s <소켓>` | 다른 서버의 세션을 이 서버로 가져와요(이름 없으면 전부) |
| `push-sessions` `[이름...]` | `-s <소켓>` | 이 서버의 세션을 다른 서버로 보내요 |
| `restart-server` | | 새 서버가 세션을 전부 넘겨받고 소켓 자리를 차지해요. 업그레이드에 써요 |

자세한 동작: [세션 옮기기](#세션-옮기기). 서버끼리 쓰는 내부 요청 `export-sessions`, `import-sessions`: [소켓 프로토콜](#소켓-프로토콜-export-sessions-import-sessions)

### 플러그인

| 명령 | 옵션 | 하는 일 |
|---|---|---|
| `list-plugins` | | 붙은 플러그인마다 한 줄: `이름: 상태 (api N) events=... commands=... access=... dropped=N`. 상태는 `running`, `stopped`, `failed (이유)`예요 |
| `plugin-attach` | | 소켓 연결을 플러그인으로 바꿔요. 소켓에서만 의미가 있어요 |
| `plugin-run` `<라이브러리.dylib>` | | C 플러그인 라이브러리를 별도 프로세스로 불러와 서버에 붙여요. 서버가 끝나면 같이 끝나요 |

플러그인이 더한 명령은 내장 명령처럼 CLI·소켓·임베디드에서 불러요. `ZEROPTY_PLUGIN_DIR`를 지정하면 `zeropty serve`가 그 디렉터리의 플러그인을 띄우고 끝나면 다시 띄워요. **지정하지 않으면(기본) 아무것도 실행하지 않아요.** 디렉터리 지정이 그 안의 코드를 서버와 같은 사용자로 실행해도 된다는 동의라서 기본은 꺼 뒀어요(이 기능 전과 같은 동작). 붙이는 법과 프로토콜: [PLUGINS.md](PLUGINS.md)

### 진단

| 명령 | 하는 일 |
|---|---|
| `stats` | 엔진 카운터 한 줄: `sessions`, `heap_bytes`(할당기에 요청한 바이트), `screen_bytes`, `live`/`frozen` 화면 수, 읽기 횟수 등 |
| `mstats` | 할당기(mimalloc) 통계. 독립 서버에만 있어요(그 프로세스의 할당기를 세는 명령이라서요) |
| `mem -t <세션>` | 화면 하나의 메모리 내역. 얼린 화면이면 `frozen <바이트> B deep <true/false>` |

## 키 이름

`Enter`, `Escape`, `Tab`, `BTab`, `BSpace`, `Space`, `Up`, `Down`, `Left`, `Right`, `Home`, `End`, `PageUp`(`PgUp`, `PPage`), `PageDown`(`PgDn`, `NPage`), `IC`(`Insert`), `DC`(`Delete`), `F1`\~`F12`.
앞에 `C-`(Ctrl), `M-`(Alt), `S-`(Shift)를 붙여 조합해요(`C-c`, `M-x`, `S-Up`, `^c`도 돼요). 이름은 대소문자를 가리지 않아요. tmux 3과 같은 바이트를 보내는지 대조했어요. 키 이름이 아닌 인자는 글자 그대로 가요.

프로그램이 application cursor 모드를 켜면(vim 등) 방향키는 `ESC O A` 형태로 바뀌어요. tmux와 같아요.

## 포맷 변수

`list-sessions -F`, `new-session -P -F`, `display-message`에서 써요.

| 변수 | 뜻 |
|---|---|
| `session_name`, `session_id`, `session_created`, `session_attached`, `session_windows` | 세션 이름, `$`로 시작하는 id(`pane_id`는 `%`), 만든 시각(유닉스 초), 붙은 터미널 수, 창 수(늘 1) |
| `window_index`, `window_id`, `window_name`, `window_width`, `window_height`, `window_panes` | 창 정보(창은 늘 하나) |
| `pane_id`, `pane_index`, `pane_pid`, `pane_width`, `pane_height` | 패널 정보, 프로그램의 pid |
| `pane_current_command`, `pane_current_path` | 지금 실행 중인 명령 이름과 폴더 |
| `pane_dead`, `pane_in_mode` | 늘 `0`(tmux 스크립트 호환용. `window_index`, `pane_index`도 늘 `0`) |
| `cursor_x`, `cursor_y`, `alternate_on` | 커서 위치(0부터), 대체 화면 사용 여부 |
| `history_size`, `history_limit` | 스크롤백 줄 수와 한도 |
| `version` | 답하는 서버(또는 임베디드 엔진)의 zeropty 버전 |

tmux와 같은 세션에서 대조했어요. `#{version}`만 쓰는 포맷은 세션 없이도 돼요(`zeropty display -p '#{version}'`는 실행 중인 서버의 버전, `zeropty -V`는 CLI 자신의 버전). `-t`가 없는 세션을 가리키면 `display-message`는 tmux처럼 세션 변수를 비운 포맷을 출력하고 종료 코드 0이에요. 세션이 있는지 확인하려면 `has-session`을 쓰세요. 없는 변수는 tmux처럼 빈 문자열이고, 조건식(`#{?...}`)과 수식어(`#{t:...}`)는 안 돼요.

## 종료 코드

| 코드 | 뜻 |
|---|---|
| 0 | 성공 |
| 1 | 실패: 없는 세션(`display-message -t`는 예외, [포맷 변수](#포맷-변수) 참고), 모르는 옵션, `wait-pane` 시간 초과·세션 종료, `grep-panes`·`has-session`의 "없음", 서버 없음(`no server running on ...`) |
| 2 | 사용법 오류: 인자 없이 실행, `zeropty help <모르는 명령>`, `attach`의 모르는 인자 |

## 셸에서 빠르게: zsh 함수, bash builtin

`zeropty` 프로그램을 실행하면 명령마다 프로세스가 떠요(2.47\~2.60ms). 셸 안에서 소켓에 바로 말하면 그 비용이 없어요.

- **zsh:** `.zshrc`에 `source /path/to/zeropty/shell/zeropty.zsh`를 넣으면 `zeropty`가 셸 함수가 돼요(p50 `send-keys -l` 0.11ms로 tmux CLI보다 42.3\~43.4배, `has-session` 0.11\~0.20ms로 21.7\~40.4배, 2000줄 `capture-pane` 0.45\~0.48ms로 10.3\~11.1배 빨라요). zsh에 기본으로 있는 `zsh/net/socket` 모듈을 써요.
- **bash:** `enable -f /path/to/zeropty/dist/zeropty_bash.dylib zeropty`를 한 번 하면 `zeropty`가 builtin이 돼요(명령당 0.035\~0.115ms, tmux CLI보다 37.8\~133배 빨라요). macOS 기본 `/bin/bash`(3.2)에서 확인했어요.
- 둘 다 인자 없는 실행, `attach`(`attach-session`, `a`), `serve`, `restart-server`, `-V`·`--version`, `-h`·`--help`·`help`, 그리고 서버가 없을 때의 명령(`new-session`이 서버를 띄워야 하니까요)은 프로그램을 실행해요. 프로그램 위치는 `ZEROPTY_BIN`으로 바꿀 수 있어요.
- zsh에서는 `=work:`가 명령 경로로 바뀌어요(`=` 확장). `'=work:'`처럼 따옴표로 감싸요. tmux도 같아요.

## 터미널이 필요한 프로그램: zeropty-tty

세션의 터미널은 PTY가 아니라 소켓이라서, `isatty()`로 터미널인지 확인하는 프로그램(vim, less, python REPL, claude)은 그냥 띄우면 터미널이 아니라고 판단해요. `zeropty-tty`로 띄우면 돼요.

```sh
zeropty new-session -d -s v "dist/zeropty-tty vim notes.txt"
```

- shim(`dist/shim.dylib`)이 `isatty`, `tcgetattr`, 창 크기(`TIOCGWINSZ`), `tcgetpgrp`, `/dev/tty`에 터미널처럼 답해요. `DYLD_INSERT_LIBRARIES`로 넣어요.
- 암호 입력(`readpassphrase`, `getpass`: `ssh`, `ssh-keygen`, `ssh-add`)은 친 내용을 화면에도 스크롤백에도 보여 주지 않아요. libc는 거기서 에코를 자기 내부 호출로 끄는데 shim이 그 호출에 닿지 못해서, 세션 터미널에서는 shim이 이 두 함수를 직접 처리해요. 세션 밖에서는 libc의 함수가 그대로 돌아요. 치는 도중 C-c를 누르면 에코를 되돌린 뒤 예전처럼 프로그램이 끝나요. shim이 들어간 프로그램에서만 그래요: `zeropty-tty ssh-keygen`처럼 띄웠거나, shim이 든 셸에서 띄운 직접 만든 프로그램이요. `ssh`와 그 도구는 `/usr/bin`에 있어서, 세션 안 셸에서 치면 shim 없이 돌고(다음 항목) 여전히 보여요. 이럴 땐 `zeropty-tty`로 띄워요.
- 터미널로 치는 건 세션 자신의 소켓뿐이에요(어느 fd 번호에 있든). stdin, stdout, stderr가 모두 그 소켓 하나예요(tmux에서 PTY 하나인 것처럼). 읽기, 쓰기, 읽기+쓰기 어느 모드로 연 `/dev/tty`도 그 소켓이라서, 그걸로 키를 읽고 화면을 그리는 프로그램(stdin을 파이프로 받은 `fzf`)도 돌아요. stdout을 파이프로 넘기거나(`zeropty-tty prog | cat`) 파일로 돌린 프로그램, stdin이 `/dev/null`인 프로그램은 거기서 터미널을 보지 못해요(tmux와 같아요). 모드 변경 신호도 그 출력에 섞이지 않아요. 자식 프로세스도 `DYLD_INSERT_LIBRARIES`를 물려받지만 같은 규칙을 따르고, 세션 밖에서는 shim이 아무것도 터미널로 답하지 않아요. 예전 서버가 시작해서 `restart-server`로 이어진 세션은 입력이 아직 파이프예요. shim이 그 파이프와 소켓을 찾고(`ZEROPTY_TTY_ID`에서, 그게 없으면 프로그램이 시작할 때 가진 fd에서), 거기서 읽기로 연 `/dev/tty`는 그 파이프라서 쓸 수 없어요.
- macOS는 `/usr/bin` 같은 SIP 보호 바이너리와 hardened runtime 서명 바이너리에 shim을 넣지 못하게 해요. 그래서 `zeropty-tty`는 이런 프로그램을 서명만 바꾼 사본(`~/.zeropty/tty/`)으로 실행하고, 원본이 바뀌면 다시 만들어요. 사본이든 아니든 프로그램은 실행할 때 쓴 이름으로 돌아요(`claude.exe`를 가리키는 `claude` 링크는 `claude`로 실행돼요). 그래서 `ps`, `pgrep`에서 그 이름으로 찾을 수 있어요.
- 사본에는 원본의 권한이 없어서 `top`, `ps`는 안 돼요. `/usr/bin/env`를 거치는 스크립트는 SIP가 shim을 지워요. 인터프리터를 직접 `zeropty-tty`로 띄워요.
- `dist/claude-zeropty`는 `zeropty-tty claude`를 하는 작은 래퍼예요. 다른 claude를 쓰려면 `ZEROPTY_CLAUDE_REAL`로 경로를 줘요.

## 키 입력과 라인 디시플린

키는 PTY처럼 라인 디시플린을 거쳐요. 새 세션은 새 PTY와 같은 cooked 모드예요.

- Enter가 줄바꿈이 돼서 `read`가 끝나요.
- `C-c`, `C-\`, `C-z`는 프로세스 그룹에 시그널로 가요.
- 줄 단위로 모으면서 `BSpace`, `C-u`, `C-w`, `C-v`로 편집하고, 빈 줄의 `C-d`는 입력 끝(EOF)이에요.
- 입력한 글자가 화면에 에코돼요. 그래서 `send-keys -l 'echo hi'`를 보내면 화면에 `echo hi`가 먼저 보여요. `wait-pane -m`으로 결과를 기다릴 때는 입력한 줄에 없는 글자를 고르세요.
- 프로그램이 raw 모드로 바꾸면(claude, vim, python REPL: shim이 알려요) 키가 그대로 가요.
- shim 없는 프로그램이 raw 모드를 원하면 `printf '\033]zeropty-tty;0\007'`를 출력해요(`stty raw -echo`에 해당).
- **PTY와 다른 점 하나:** 세션의 소켓에는 EOF를 한 번만 알리는 방법이 없어서, `C-d`의 EOF는 입력 쪽을 닫아요. 그 뒤로는 입력을 받지 못해요(출력은 계속 나와요).
- PTY처럼 stdin, stdout, stderr가 파일 하나라서, 프로그램이 그중 하나에 `O_NONBLOCK`을 걸면 셋 모두에, 세션에서 그 뒤에 실행하는 명령에도 걸려요(그 뒤 큰 파일을 `cat`하면 `Resource temporarily unavailable`로 실패할 수 있어요). 0.1.1 이하에서는 stdin이 따로 된 파이프여서 그런 변경이 stdin에만 남았어요.

## 다른 프로그램에서: 소켓

서버 소켓(`~/.zeropty/mux.sock`, `ZEROPTY_SOCK`으로 바꿔요)에 JSON 한 줄을 보내면 한 줄로 답해요. 연결 하나로 여러 명령을 보낼 수 있고, 답은 순서대로 와요. 소켓은 0600 권한이라 같은 사용자(와 root)만 접속할 수 있어요.

```
→ {"argv":["capture-pane","-p","-t","=work:"],"cwd":"/"}
← {"code":0,"out":"...화면...","err":""}
```

- `cwd`는 상대 경로(`new-session -c`, `load-buffer`)의 기준 폴더예요.
- 요청에 `"raw":true`를 넣으면 답이 JSON 대신 `<code> <out 바이트 수> <err 바이트 수>` 한 줄과 그 뒤의 out, err 바이트 그대로 와요. JSON을 풀기 어려운 클라이언트(zsh 함수)용이에요.
- `attach` 연결은 첫 답 뒤로 프레임을 주고받아요: `[종류 u8][길이 u32 LE][바이트]`. 서버 → 클라이언트는 0(출력), 1(세션 끝), 클라이언트 → 서버는 0(키), 1(크기: 열·행 u16 LE), 2(떼기)예요.
- 첫 요청이 `export-sessions`나 `import-sessions`인 연결은 세션을 넘기는 데 써요(내부용): [소켓 프로토콜](#소켓-프로토콜-export-sessions-import-sessions).

```sh
# 아무 언어에서나: 예를 들어 nc로
printf '{"argv":["list-sessions"],"cwd":"/"}\n' | nc -U ~/.zeropty/mux.sock
```

### Node 클라이언트

`client/zeropty.mjs`가 소켓 연결을 하나 열어 두고 명령을 보내요. tmux 호출을 `zeropty(args)`로 바꾸면 돼요.

```js
import { zeropty, zeroptySync } from './client/zeropty.mjs'
const r = await zeropty(['capture-pane', '-p', '-t', '=work:'])   // { ok, code, out, err }
```

| 함수 | 하는 일 |
|---|---|
| `zeropty(argv, { cwd, sock, timeout })` | 명령 하나를 보내고 `{ ok, code, out, err }`로 답해요. 서버가 없으면 CLI를 실행해요(`new-session`이 서버를 띄워요). 기본 타임아웃 15초 |
| `zeropty(['wait-pane', ...])` | 연결을 따로 열어서, 기다리는 동안에도 다른 명령은 공유 연결로 바로 처리돼요. 기본 타임아웃 없음(`-T`나 `{ timeout }`으로 줘요) |
| `zeroptySync(argv, { cwd, sock })` | 같은 일을 동기로 해요. `dist/zeropty.node` 필요, 명령당 0.11\~0.20ms 동안 스레드를 막아요 |
| `sockPath()`, `cliPath()` | 쓰는 소켓(`ZEROPTY_SOCK`)과 CLI(`ZEROPTY_BIN`) 경로 |

실행해 볼 수 있는 예제: [`examples/node/quickstart.mjs`](../../examples/node/quickstart.mjs)

## 내 프로세스 안에: 임베디드

서버 없이 엔진을 프로세스 안에 넣어요. 명령 형식과 결과는 소켓과 같아요. `wait-pane`만 예외로 함수 호출로는 안 돼요([위](#기다리기와-찾기) 참고). 세션은 이 프로세스가 살아 있는 동안만 살아요(프로세스가 끝나면 프로그램들은 EOF를 받아요). 남기고 싶으면 끝나기 전에 서버로 넘겨요(아래 `push-sessions`).

**Node:** import만 바꾸면 돼요. 예제: [`examples/node/embedded.mjs`](../../examples/node/embedded.mjs)

```js
import { zeropty, open } from './client/embedded.mjs'   // './client/zeropty.mjs' 대신
await zeropty(['new-session', '-d', '-s', 'w', 'exec cat'])
```

| 함수 | 하는 일 |
|---|---|
| `zeropty(argv, { cwd })`, `zeroptySync(argv, { cwd })` | 서버 클라이언트와 같은 호출, 같은 결과. `send-keys` p50 0.04\~0.06ms, `capture-pane` 0.06\~0.07ms(바쁜 세션 0.09\~0.12ms) ([측정](BENCHMARKS.md)) |
| `open({ history, sizeDir, polled })` | 첫 명령 전에 옵션을 주고 엔진을 시작해요(안 부르면 첫 명령이 기본값으로 시작해요) |
| `listen(소켓)` | [서버 플러그인](#서버-플러그인-내-프로그램이-소켓도-열기) |

**C (또는 FFI가 되는 어떤 언어든):** 헤더는 [`bindings/c/include/zeropty.h`](../../bindings/c/include/zeropty.h), 예제는 [`examples/c/hello.c`](../../examples/c/hello.c)예요.

```c
#include "zeropty.h"   // cc app.c -Idist -Ldist -lzeropty_c
zeropty *a = zeropty_open(NULL);   // 또는 {"history": 2000, "size_dir": "/tmp"}
char *r = zeropty_command(a, "{\"argv\":[\"new-session\",\"-d\",\"-s\",\"x\",\"exec cat\"],\"cwd\":\"/\"}");
zeropty_free(r);                // r은 {"code":0,"out":"","err":""}
zeropty_close(a);
```

| 함수 | 하는 일 |
|---|---|
| `zeropty_open(options_json)` | 엔진 스레드가 있는 엔진. 옵션은 `NULL`이나 `{"history": 줄 수, "size_dir": "폴더"}` |
| `zeropty_command(a, request_json)` | 명령 하나. 답은 JSON 문자열이고 `zeropty_free`로 풀어요 |
| `zeropty_listen(a, path)` | 소켓도 열기. 0, 실패하면 -1 |
| `zeropty_close(a)` | 엔진을 멈춰요. 세션은 프로그램이 끝날 때까지 돌아요(먼저 `kill-server`를 보내면 끝나요) |
| `zeropty_open_polled`, `zeropty_fd`, `zeropty_timeout_ms`, `zeropty_process` | 엔진 스레드 없이 호스트 루프로 돌리기(아래) |

`zeropty_open`의 명령은 어느 스레드에서 불러도 돼요. 화면 읽기, 키 보내기, 조회는 부른 스레드에서 바로 처리하고, 나머지는 엔진 스레드에 넘겨요.

**Rust:** crates.io에는 아직 없어서 git 의존성으로 넣어요. 같은 커밋에 머물고 싶으면 `rev = "<커밋>"`을 더해요.

```toml
# Cargo.toml
[dependencies]
zeropty = { git = "https://github.com/CreatiCoding/zeropty" }
```

그리고 `zeropty::embed::Embedded`(엔진 스레드)나 `zeropty::embed::Polled`(호스트 루프)를 써요. API 문서는 이 저장소에서 `cargo doc --no-deps --workspace --open`, 예제는 `cargo run --release --example embed`, `cargo run --release --example polled`예요.

```rust
use zeropty::{embed::Embedded, engine::Options};

fn main() {
    let zp = Embedded::start(Options::default());
    let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let r = zp.command(&argv(&["new-session", "-d", "-s", "w", "exec cat"]), "/");
    assert_eq!(r.code, 0, "{}", r.err);
    zp.command(&argv(&["send-keys", "-t", "=w:", "-l", "hello"]), "/");
    print!("{}", zp.command(&argv(&["capture-pane", "-p", "-t", "=w:"]), "/").out);   // hello
    zp.command(&argv(&["kill-server"]), "/");
}
```

빌드에는 macOS와 Xcode Command Line Tools가 필요해요(`zstd-sys`가 C 코드를 컴파일해요). 터미널이 필요한 프로그램을 띄우는 shim은 crate에 들어 있지 않아요. 그럴 때는 이 저장소의 `./build.sh`로 만든 `dist/zeropty-tty`를 써요.

**polled: 엔진 스레드 없이 (Node 기본값, 대기 400개에서 0.72MB → 0.39\~0.41MB).** 호스트의 이벤트 루프가 엔진을 돌려요. 모든 호출은 그 루프 스레드에서 해요. Node에서는 libuv가 엔진 fd를 지켜봐서 따로 할 일이 없어요.

```js
open({ polled: false })   // Node에서 엔진 스레드를 쓰려면 (worker 스레드에서 부를 때). 첫 명령 전에 불러요
```

```c
zeropty *a = zeropty_open_polled(NULL);
for (;;) {   // 호스트 루프: 엔진 fd가 읽을 수 있거나 타임아웃이 지나면 zeropty_process
  struct pollfd p = { zeropty_fd(a), POLLIN, 0 };
  poll(&p, 1, (int)zeropty_timeout_ms(a));
  zeropty_process(a);
}
```

플러그인이 더한 명령은 답을 기다리는 동안 부른 스레드를 잡을 수 있어요(마감 기본 10초). 호스트 루프를 잡지 않으려면 `zeropty_command_start`로 시작하고 `zeropty_process` 뒤에 `zeropty_call_poll`로 답을 가져가요(Rust는 `Polled::command_start`, Node는 `await zeropty(...)`). 엔진 명령은 이 방식으로 불러도 바로 답이 들어 있어요. 자세한 건 [PLUGINS.md](PLUGINS.md#호스트에서-플러그인-명령-부르기)에 있어요.

### 서버 플러그인: 내 프로그램이 소켓도 열기

`listen(소켓)`을 부르면 `zeropty serve`와 같은 서버가 내 프로그램 안에서 돌아요. 그러면 `zeropty` CLI, `zeropty attach`, zsh 함수, bash builtin, `client/zeropty.mjs`가 별도 서버 프로세스 없이 이 프로그램의 세션을 다뤄요. 연결은 엔진과 같은 루프에서 처리해요(polled면 호스트 루프, 엔진 스레드 방식이면 엔진 스레드).

```js
import { zeropty, listen } from './client/embedded.mjs'
listen(`${process.env.HOME}/.zeropty/myapp.sock`)   // 이미 다른 서버가 답하는 소켓이면 예외
```

```sh
ZEROPTY_SOCK=~/.zeropty/myapp.sock zeropty capture-pane -p -t '=w:'   # 다른 터미널에서
ZEROPTY_SOCK=~/.zeropty/myapp.sock zeropty attach -t w
```

```c
if (zeropty_listen(a, "/tmp/myapp.sock") != 0) { /* 못 열었어요 */ }
```

- 소켓 경로는 103바이트 이하예요. 소켓은 0600, 새로 만든 폴더는 0700 권한이에요.
- 그 경로에서 이미 서버가 답하면 `listen`은 실패해요(Node는 예외, C는 -1). 아무도 답하지 않는 소켓 파일(끝난 서버가 남긴 것)은 넘겨받고, 소켓이 아닌 파일이 있으면 지우지 않고 실패해요.
- 소켓에서 온 `kill-server`는 세션만 끝내요. 내 프로그램은 계속 돌고 소켓도 계속 열려 있어요(독립 서버는 프로세스가 끝나요).
- 엔진을 닫으면(`zeropty_close`, Rust의 `Embedded`·`Polled` 해제) 소켓 파일도 지워요. 그사이 다른 서버가 같은 경로를 차지했으면 그 소켓은 건드리지 않아요.
- 세션은 여전히 내 프로그램과 같이 끝나요. 프로그램보다 오래 두려면 끝나기 전에 독립 서버로 `push-sessions` 해요.
- 엔진을 멈추거나 프로세스가 끝나도 소켓 파일은 남아요. 아무도 답하지 않는 소켓이라 다음 `listen`이 넘겨받아요.
- `mstats`는 독립 서버에만 있어요.

## 세션 옮기기

세션을 다른 프로세스로 옮겨요. **실행 중인 프로그램은 그대로**예요(pid도 같아요). 화면·스크롤백은 바이트로, 세션의 fd는 fd 전달(SCM_RIGHTS)로 넘겨요.

```sh
zeropty pull-sessions -s ~/.zeropty/other.sock [이름...]   # 다른 서버의 세션을 이 서버로 (이름 없으면 전부)
zeropty push-sessions -s ~/.zeropty/other.sock [이름...]   # 이 서버의 세션을 다른 서버로
zeropty restart-server                                  # 새 서버가 세션을 전부 넘겨받고 소켓 자리를 차지
```

- 임베디드 엔진에서도 같은 명령이 돼요. 예를 들어 Node 앱이 시작할 때 서버의 세션을 `pull-sessions`로 가져오고, 끝날 때 `push-sessions`로 돌려주면 앱을 재시작해도 세션이 안 끊겨요.
- `restart-server`는 지금 실행한 `zeropty` 바이너리로 새 서버를 띄워요. 그래서 **세션을 잃지 않고 업그레이드**하는 방법이기도 해요. 전환하는 몇 ms 사이에 들어온 명령은 옛 서버로 가요.
- 받는 쪽이 "받았다"고 답한 세션만 보낸 쪽이 놓아요. 중간에 실패하면 세션은 원래 쪽에 그대로 있어요. 이름이 이미 있는 세션은 옮기지 않아요.
- **한계:** 옮기는 순간 이스케이프 시퀀스가 반쯤 들어와 있었다면, 그 나머지는 글자로 찍혀요.

### 소켓 프로토콜: export-sessions, import-sessions

`pull-sessions`, `push-sessions`, `restart-server`가 서버 소켓에서 쓰는 **내부용** 요청이에요. 직접 보낼 일은 거의 없고, 세션 레코드 형식은 버전마다 바뀔 수 있어요(첫 바이트가 레코드 버전이라, 다른 버전의 레코드는 `session record of another version`으로 거절해요). 사람이 읽을 레퍼런스로 남겨 둬요. 구현: `src/engine/handover.rs`.

- 연결의 **첫 요청 줄**이 `{"argv":["export-sessions", 이름...],"cwd":""}` 또는 `{"argv":["import-sessions"],"cwd":""}`이면, 그 연결은 세션을 넘기는 데만 쓰고 끝나면 닫혀요. 그동안 서버는 다른 일을 하지 않아요(보통 몇 ms).
- 양쪽 모두 한 단계에 10초까지 기다려요.
- 세션 하나는 `[레코드 길이 u32 LE][레코드]`로 가요. 세션의 fd(입력, 출력 중 열려 있는 것, 0\~2개)는 길이 4바이트에 SCM_RIGHTS로 붙여 보내요. 길이 0이 끝 표시예요. 레코드는 256MB를 넘을 수 없어요.
- 나머지 줄은 모두 JSON 한 줄이에요.

```
export-sessions (가져가는 쪽 → 서버)
→ {"argv":["export-sessions","a","b"],"cwd":""}     이름이 없으면 전부
← [길이][레코드] ... [0]                              세션마다 fd와 함께
← ["b"]                                             서버에 없는 이름
→ {"taken":["a"],"failed":[]}                       받은 세션. 서버는 이것만 놓아요

import-sessions (주는 쪽 → 서버)
→ {"argv":["import-sessions"],"cwd":""}
← "ready"
→ [길이][레코드] ... [0]
← {"taken":["a"],"failed":["duplicate session: b\n"]}   주는 쪽은 taken만 놓아요
```

`failed`에는 받지 못한 이유가 들어가요(같은 이름이 이미 있음, 레코드 버전이 다름 등). 어느 단계에서든 연결이 끊기거나 형식이 틀리면 받은 쪽이 받은 세션을 놓고, 세션은 원래 쪽에 남아요.

## 환경변수

| 변수 | 기본값 | 의미 |
|---|---|---|
| `ZEROPTY_SOCK` | `~/.zeropty/mux.sock` | 서버 소켓 (경로는 103바이트 이하) |
| `ZEROPTY_HISTORY` | 500 | 스크롤백 줄 수 |
| `ZEROPTY_COALESCE_MS` | 300 | 출력을 묶어 읽는 간격. 16KB 이상 쌓이면 즉시 읽고, 화면 읽기는 항상 밀린 출력부터 읽어요. 붙어 있는 터미널에는 바로 보내요 |
| `ZEROPTY_FREEZE_MS` | 250 | 이 시간만큼 조용하면 화면을 통째로 얼림 |
| `ZEROPTY_COLD` | 파일 | 오래 조용한 화면의 압축본을 둘 곳. `memory`면 프로세스 메모리에 |
| `ZEROPTY_COLD_DIR` | 임시 폴더 | 압축본 파일을 만들 폴더 (만들자마자 이름을 지워요) |
| `ZEROPTY_RECORD` | – | 세션 출력을 `<이름>.raw`/`.trace`로 기록할 폴더 |
| `ZEROPTY_PURGE_MS` | 0 | 독립 서버: 해제한 메모리를 OS에 돌려주기 전 기다리는 시간 |
| `ZEROPTY_BIN` | `dist/zeropty` | Node 클라이언트·zsh 함수·bash builtin이 실행할 프로그램 |
| `ZEROPTY_NODE` | `dist/zeropty.node` | Node 클라이언트가 불러올 네이티브 모듈 |
| `ZEROPTY_SHIM` | `dist/shim.dylib` | `zeropty-tty`가 넣을 shim |
| `ZEROPTY_CACHE` | `~/.zeropty` | `zeropty-tty`가 서명을 바꾼 사본을 둘 곳(`tty/` 아래) |
| `ZEROPTY_CLAUDE_REAL` | `PATH`의 `claude` | `claude-zeropty`가 띄울 claude |
| `ZEROPTY_PLUGIN_DIR` | (없음: 끔) | 서버가 띄울 플러그인 디렉터리. 지정할 때만 그 안의 실행 파일과 `.dylib`를 실행해요. `restart-server`·`new-session`이 띄우는 서버는 그 명령을 실행한 셸의 값을 받아요 ([PLUGINS.md](PLUGINS.md#자동-시작-플러그인-디렉터리)) |
| `ZEROPTY_PLUGIN_TIMEOUT_MS` | 10000 | 플러그인 명령과 훅의 마감 시간. 플러그인의 `Host::command`는 그 절반까지 기다려요 |

## tmux와 기능 비교

zeropty는 **프로그램을 띄워 두고 화면을 읽고 키를 보내는 일**에 집중해요. 사람이 터미널을 나눠 쓰는 기능은 대부분 없어요.

| 기능 | tmux | zeropty |
|---|---|---|
| 세션 만들기·끝내기·목록, 키 보내기, 화면 읽기 | ✅ | ✅ 같은 인자 |
| `capture-pane` 옵션 | 많음 | `-p -e -J -N -S -E -b -q` (나머지는 오류) |
| 버퍼 | `load/paste/set/show/save-buffer` 등 | `load-buffer`, `paste-buffer`, `capture-pane -b` |
| `pipe-pane`, `rename-session`, `resize-window`, `set-environment -g` | ✅ | ✅ (`pipe-pane -I`는 없음) |
| 포맷 변수 `#{...}` | 수백 개 | 세션·창·패널·커서·히스토리·프로세스 변수 25개 (조건식, 수식어 없음) |
| 사람이 붙어서 보기 (`attach`) | ✅ | ✅ 읽기 전용(`-r`), Ctrl-b d, 터미널 크기 맞춤 |
| 창(window)·창 분할(pane) | ✅ | ❌ 세션 하나에 프로그램 하나 |
| 상태 표시줄, 복사 모드, 단축키 설정, 설정 파일, 훅 | ✅ | ❌ |
| 여러 클라이언트가 한 세션에 붙기 | ✅ | ✅ (크기는 마지막에 붙은 터미널을 따라요) |
| PTY | 세션마다 1개 (macOS 한도 511\~999개) | 안 씀, 세션 1000개 이상 |
| 전체화면 프로그램 (vim 등) | ✅ | `zeropty-tty <프로그램>`으로 띄우면 돼요. vim(편집·저장), less(페이지 넘기기·종료), python REPL(줄 편집·히스토리), claude를 확인했어요. `top`·`ps`처럼 권한(setuid, entitlement)이 필요한 시스템 프로그램은 안 돼요 |
| 프로그램용 접속 | 제어 모드(`-C`) | JSON 소켓 프로토콜, Node 클라이언트 |
| 모든 세션 화면에서 글자 찾기 | ❌ 세션마다 `capture-pane` | ✅ `grep-panes` (1000개 12\~17ms) |
| 키 입력의 라인 디시플린 (Enter, `C-c` 시그널, 줄 편집, 에코, `C-d`) | ✅ PTY | ✅ 엔진이 흉내 내요(같은 키를 tmux와 대조). `C-d`의 EOF는 입력을 닫아요 |
| 화면에 글자가 뜨거나 출력이 멈출 때까지 기다리기 | ❌ `capture-pane`을 반복해서 확인 | ✅ `wait-pane` (서버가 출력이 올 때만 확인, 감지 2ms) |
| 라이브러리로 내장 | ❌ | ✅ C ABI, Node 모듈, Rust crate |
| 실행 중인 세션을 다른 프로세스로 옮기기 | ❌ | ✅ `pull-sessions`, `push-sessions` |
| 서버 재시작·업그레이드 후에도 세션 유지 | ❌ | ✅ `restart-server` |
| 플랫폼 | 대부분의 Unix | macOS만 (kqueue, DYLD shim) |
