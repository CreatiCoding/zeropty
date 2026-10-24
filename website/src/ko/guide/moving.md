---
description: 에이전트를 멈추지 않고 zeropty 서버를 업그레이드하고, 실행 중인 세션을 서버와 임베디드 엔진 사이에서 옮겨요.
---

# 세션 옮기기와 서버 업그레이드

이 가이드를 마치면 프로그램이 계속 실행되는 상태로 zeropty 서버를 업그레이드하고, 실행 중인 세션을 두 서버 사이에서 옮길 수 있어요. 세션과 함께 가는 것과 남는 것도 알게 돼요.

## 시작하기 전에

- zeropty가 설치돼 있고 세션이 하나 이상 있는 서버가 필요해요. 아래 명령은 `work`라는 세션을 써요. `work`는 내 세션 이름으로 바꾸세요.
- 업그레이드하려면 새 `zeropty` 바이너리가 필요해요. 더 새로운 릴리스 폴더나 새로 만든 `./build.sh` 결과예요. 자세한 내용은 [설치와 지원 플랫폼](/ko/guide/install)을 보세요.
- 세션을 옮겨도 프로그램은 계속 살아 있지만, 백업은 아니에요. 세션은 어디에 있든 프로그램이 끝나면 끝나요.

## 1. 확인할 값 기록하기

바꾸기 전에 프로그램의 pid와 서버의 버전을 기록해 두세요.

```sh [터미널]
zeropty display -p -t '=work:' '#{pane_pid}'
zeropty display -p '#{version}'
```

`#{pane_pid}`는 프로그램의 pid이고, `#{version}`은 답하는 서버의 버전이에요. `zeropty -V`는 내가 실행한 CLI의 버전을 출력하므로 다를 수 있어요.

## 2. restart-server로 업그레이드하기

**새** 바이너리로 `restart-server`를 실행하세요.

```sh [터미널]
zeropty restart-server
```

방금 실행한 `zeropty` 바이너리로 새 서버가 시작되어 세션을 모두 넘겨받고, 그다음 소켓을 넘겨받아요. 프로그램은 멈추지 않고 pid도 그대로예요. 전환하는 몇 ms 사이에 들어온 명령은 옛 서버로 가요.

새 바이너리가 `PATH`에서 첫 번째가 아니라면 경로로 실행하세요. 예: `./zeropty-0.1.7-darwin-universal/zeropty restart-server`

::: warning 새 서버는 내 셸의 환경을 가져요
`restart-server`는 그 명령을 실행한 셸에서 새 서버를 띄워요. 이후에 만드는 세션은 그 셸의 환경을 받고, `set-environment -g`로 정한 값은 사라지니 다시 정하세요. 이미 실행 중인 세션은 자기 환경을 그대로 가져요. `ZEROPTY_PLUGIN_DIR`와 `ZEROPTY_SOCK`도 이 셸에서 와요.
:::

## 3. 서버 사이에서 세션 옮기기

`pull-sessions`는 다른 서버의 세션을 내 `zeropty`가 말하는 서버로 가져와요. `push-sessions`는 반대 방향으로 보내요. 다른 서버는 `-s`에 소켓으로 지정해요.

```sh [터미널]
zeropty pull-sessions -s ~/.zeropty/other.sock            # all of the other server's sessions
zeropty pull-sessions -s ~/.zeropty/other.sock work build # only these
zeropty push-sessions -s ~/.zeropty/other.sock work       # this server's work to the other server
```

`~/.zeropty/other.sock`은 다른 서버의 소켓으로 바꾸세요. 내 쪽에 실행 중인 서버가 없으면 `pull-sessions`가 서버를 띄워요.

보내는 쪽은 받는 쪽이 받았다고 확인한 세션만 놓아요. 중간에 실패하면 세션은 원래 쪽에 그대로 있어요.

## 4. 임베디드 앱을 재시작해도 세션 유지하기

임베디드 엔진의 세션은 앱과 함께 끝나요. 같은 명령이 거기서도 되므로, 끝나기 전에 서버에 넘기고 시작한 뒤 다시 가져오세요.

```js [app.mjs]
import { zeropty } from './client/embedded.mjs'

const server = `${process.env.HOME}/.zeropty/mux.sock`
await zeropty(['pull-sessions', '-s', server])     // on start: take the sessions back
// ... run your app ...
await zeropty(['push-sessions', '-s', server])     // before exit: hand them to the server
```

임베딩 쪽은 [Node.js에서 zeropty 쓰기](/ko/guide/node)와 [C와 Rust에 zeropty 내장하기](/ko/guide/embed)를 보세요.

## 5. 무엇이 같이 가는지 알기

| 세션과 함께 가는 것 | 가지 않는 것 |
|---|---|
| 실행 중인 프로그램과 그 pid | `pipe-pane` 파이프: 세션이 옮겨질 때 닫혀요 |
| 화면과 스크롤백(바이트로) | `restart-server` 때 `set-environment -g`로 정한 값 |
| 세션의 fd(fd 전달, SCM_RIGHTS) | 상대편에 이미 같은 이름이 있는 세션 |
| | 옮기는 순간 반쯤 받은 이스케이프 시퀀스의 나머지: 글자로 찍혀요 |

플러그인은 넘어가지 않아요. 플러그인 디렉터리가 있으면 새 서버가 소켓을 넘겨받은 뒤 자기 플러그인을 시작하고, 그동안 수십 ms는 플러그인 명령이 `unknown command`를 돌려줄 수 있어요. 플러그인은 옮김을 이벤트로 봐요. 보내는 쪽에서는 이유가 `handed-over`인 `session-exited`, 받는 쪽에서는 `session-created`예요.

내부적으로 `pull-sessions`, `push-sessions`, `restart-server`는 소켓의 내부 요청 두 개, `export-sessions`와 `import-sessions`를 써요. 직접 보낼 일은 거의 없고, 레코드 형식은 버전마다 바뀔 수 있어요. 자세한 내용은 [소켓 프로토콜: export-sessions, import-sessions](/ko/reference/socket-protocol#소켓-프로토콜-export-sessions-import-sessions)를 보세요.

## 확인하기

1단계의 명령을 다시 실행하세요.

```sh [터미널]
zeropty display -p -t '=work:' '#{pane_pid}'
zeropty display -p '#{version}'
```

이전과 같은 pid와 새 버전이 보여야 해요. 예를 들어 0.1.5로 올린 뒤 pid가 48213인 프로그램은 이렇게 나와요.

```text
48213
0.1.5
```

`zeropty capture-pane -p -t '=work:'`는 업그레이드 전과 같은 화면을 보여 줘요.

## 문제가 생기면

| 증상 | 해결 |
|---|---|
| 옮기기가 `duplicate session: NAME`을 내고 1로 끝나요 | 상대편에 같은 이름의 세션이 있어요. `rename-session`으로 한쪽의 이름을 바꾸고 다시 옮기세요. |
| `session record of another version` | 양쪽의 zeropty 버전이 달라서 세션 레코드가 달라요. 양쪽을 같은 버전으로 실행하세요. |
| `restart-server` 뒤 새 세션에 변수가 없어요 | 새 서버는 그 명령을 실행한 셸의 환경을 가져요. 그 셸에서 변수를 정하거나 `set-environment -g`를 다시 쓰세요. |
| 업그레이드 직후 플러그인 명령이 `unknown command`예요 | 새 서버가 아직 플러그인을 시작하는 중이에요. 잠시 뒤에 다시 해 보세요. |
| 다운그레이드 후 `C-d`가 세션에 닿지 않아요 | 0.1.2 이상에서 시작한 세션은 0.1.1 이하로 돌려보내지 마세요. |

## 다음 단계

- 세션이 프로세스를 바꿀 수 있는 이유는 [세션과 서버](/ko/guide/concepts/sessions)를 보세요.
- 모든 옵션은 [서버와 세션 옮기기](/ko/reference/commands#서버와-세션-옮기기)를 보세요.
- 소켓에서 일어나는 일은 [세션 옮기기](/ko/reference/socket-protocol#세션-옮기기)를 보세요.
