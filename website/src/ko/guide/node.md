---
description: 서버 소켓으로, 또는 Node 프로세스에 엔진을 내장해서 Node.js에서 zeropty 세션을 다뤄요.
---

# Node.js에서 zeropty 쓰기

이 가이드를 마치면 [Node.js](https://nodejs.org)에서 에이전트 호스트를 실행할 수 있어요. 세션을 시작하고, 일을 주고, 기다리고, 화면을 읽어요. 먼저 서버의 소켓으로 하고, 그다음 import 하나만 바꿔서 Node 프로세스 안에 엔진을 넣어요.

## 시작하기 전에

- 소스에서 빌드한 zeropty가 있는 Mac이 필요해요(`git clone`과 `./build.sh`). Node 클라이언트는 `dist/zeropty.node`를 불러오고 `dist/zeropty`를 실행해요. 릴리스 아카이브에는 `zeropty.node`도 클라이언트도 들어 있지 않아요. 자세한 내용은 [설치와 지원 플랫폼](/ko/guide/install)을 보세요.
- N-API 6(`napi6`) 이상을 지원하는 Node.js 가 필요해요. 저장소의 예제는 Node 18 이상으로 돌려요.
- 명령은 zeropty 폴더에서 실행하세요. 아래 import는 `./client/...`를 써요. 내 프로젝트에서는 그 폴더 안의 파일 경로로 import하세요.

## 1. 클라이언트 고르기

두 모듈은 같은 호출을 받고 같은 결과 `{ ok, code, out, err }`를 돌려줘요.

| 모듈 | 세션이 사는 곳 | `send-keys` 한 번 | 이럴 때 써요 |
|---|---|---|---|
| `client/zeropty.mjs` | zeropty 서버 | 0.24~0.26ms | 세션이 앱보다 오래 살아야 하거나, 여러 프로그램이 세션을 함께 쓸 때 |
| `client/embedded.mjs` | 내 Node 프로세스(**임베디드**) | 0.06~0.07ms | 앱이 세션을 직접 가질 때 |

네 가지 인터페이스를 비교한 내용은 [zeropty를 다루는 네 가지 방법](/ko/guide/concepts/interfaces)을 보세요.

## 2. 소켓으로 세션 다루기

`client/zeropty.mjs`는 서버 소켓(`~/.zeropty/mux.sock`, 또는 `ZEROPTY_SOCK`)에 연결을 하나 열어 두고 명령마다 그 연결로 보내요. 서버가 없으면 CLI를 실행하므로 첫 `new-session`이 서버를 띄워요. 아래는 [`examples/node/quickstart.mjs`](https://github.com/CreatiCoding/zeropty/blob/main/examples/node/quickstart.mjs)예요.

```js [examples/node/quickstart.mjs]
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { zeropty } from '../../client/zeropty.mjs'

const sock = path.join(os.tmpdir(), `zeropty-quickstart-${process.pid}.sock`)
const zp = (args, opts) => zeropty(args, { sock, ...opts })

// Start a session; with no server on the socket, new-session starts one (as tmux does).
// A real host runs an agent here: dist/claude-zeropty, or any program.
let r = await zp(['new-session', '-d', '-s', 'job', '-x', '80', '-y', '20', 'sh'])
if (!r.ok) throw new Error(r.err)

// Give it work.
await zp(['send-keys', '-t', '=job:', '-l', 'for i in 1 2 3; do echo step $i; sleep 0.2; done; echo "finished: $?"'])
await zp(['send-keys', '-t', '=job:', 'Enter'])

// Wait until it is done: the text shows up (the typed line holds `$?`, not `0`) and the output
// stays quiet for 300 ms. No polling: the server checks as output comes in.
const t0 = performance.now()
r = await zp(['wait-pane', '-t', '=job', '-m', 'finished: 0', '-q', '300', '-T', '10000'])
console.log(`wait-pane: ${r.ok ? 'done' : r.err.trim()} after ${Math.round(performance.now() - t0)} ms`)

// Read the result.
const { out } = await zp(['capture-pane', '-p', '-t', '=job:'])
console.log(out.trimEnd())

// Which sessions show a given text, across all of them at once.
console.log((await zp(['grep-panes', 'finished: 0'])).out.trimEnd())

await zp(['kill-server'])
fs.rmSync(sock, { force: true })
```

이 예제는 자기 소켓을 쓰므로 내 세션은 건드리지 않아요. 내 앱에서는 `sock`을 빼서 기본 서버를 쓰세요.

- `zeropty(argv, { cwd, sock, timeout })`의 기본 타임아웃은 15초예요.
- `wait-pane`은 연결을 따로 열어서, 기다리는 동안에도 다른 명령이 계속 흘러요. 기본 타임아웃이 없으니 `-T`나 `{ timeout }`으로 정하세요.
- 실패한 명령은 예외를 던지지 않아요. `r.ok`, 또는 `r.code`와 `r.err`를 확인하세요.

## 3. 필요하면 동기로 부르기

`zeroptySync(argv, { cwd, sock })`는 같은 명령을 보내고 답이 올 때까지 스레드를 막아요. 명령당 0.13~0.26ms예요. `dist/zeropty.node`가 필요해요.

```js [app.mjs]
import { zeroptySync } from './client/zeropty.mjs'
const { out } = zeroptySync(['capture-pane', '-p', '-t', '=job:'])
```

`sockPath()`와 `cliPath()`는 클라이언트가 쓰는 소켓(`ZEROPTY_SOCK`)과 CLI(`ZEROPTY_BIN`)를 돌려줘요.

## 4. 대신 엔진을 내장하기

엔진을 내 프로세스 안에 넣으려면 import만 바꾸세요. 아래는 줄여 쓴 [`examples/node/embedded.mjs`](https://github.com/CreatiCoding/zeropty/blob/main/examples/node/embedded.mjs)예요.

```js [examples/node/embedded.mjs]
import { zeropty } from '../../client/embedded.mjs'

const r = await zeropty(['new-session', '-d', '-s', 'w', '-x', '60', '-y', '8', 'exec cat'])
if (!r.ok) throw new Error(r.err)

await zeropty(['send-keys', '-t', '=w:', '-l', 'hello from an embedded engine'])
await zeropty(['send-keys', '-t', '=w:', 'Enter'])

// cat prints the line back once it reads it.
for (let i = 0; i < 100; i++) {
  const { out } = await zeropty(['capture-pane', '-p', '-t', '=w:'])
  if (out.split('hello').length === 3) { console.log(out.trimEnd()); break }
  await new Promise((done) => setTimeout(done, 10))
}

// The sessions end with this process (their programs get EOF); kill-server ends them now.
await zeropty(['kill-server'])
```

엔진을 내장하면 세 가지가 달라져요.

- **`wait-pane`이 없어요.** 함수 호출에는 답을 미룰 루프가 없어요. 위처럼 루프로 화면을 읽거나, 5단계처럼 소켓을 열고 거기서 `wait-pane`을 보내세요.
- **세션이 내 프로세스와 함께 끝나요.** 남기려면 끝나기 전에 서버로 `push-sessions` 하세요. 자세한 내용은 [세션 옮기기와 서버 업그레이드](/ko/guide/moving)를 보세요.
- **옵션은 `open`에 줘요.** 첫 명령 전에 `open({ history, sizeDir, polled })`를 부르세요. 엔진은 기본적으로 **polled**예요. Node의 이벤트 루프가 엔진을 돌리고 엔진은 자기 스레드가 없어요. worker 스레드에서 부르려면 `polled: false`를 줘서 엔진에 스레드를 주세요.

## 5. CLI용 소켓 열기

`listen(path)`는 `zeropty serve`와 같은 서버를 내 앱 안에서 돌려요. 그러면 CLI, `attach`, 다른 스크립트가 내 앱의 세션에 닿아요.

```js [app.mjs]
import { zeropty, listen } from './client/embedded.mjs'
listen(`${process.env.HOME}/.zeropty/myapp.sock`)   // throws if another server already answers on this socket
```

```sh [터미널]
ZEROPTY_SOCK=~/.zeropty/myapp.sock zeropty capture-pane -p -t '=w:'
ZEROPTY_SOCK=~/.zeropty/myapp.sock zeropty attach -t w
```

경로는 103바이트 이하여야 해요.

## 확인하기

소켓 예제를 실행하세요.

```sh [터미널]
node examples/node/quickstart.mjs
```

대기 시간, 화면, `grep-panes` 일치 결과가 보여야 해요. `NNN`은 밀리초이고 실행할 때마다 달라요.

```text
wait-pane: done after NNN ms
for i in 1 2 3; do echo step $i; sleep 0.2; done; echo "finished: $?"
step 1
step 2
step 3
finished: 0
job: finished: 0
```

그다음 `node examples/node/embedded.mjs`를 실행하세요. `capture-pane: … µs each` 시간이 출력되고, 이어서 그 줄이 두 번 나와요. 한 번은 친 대로 에코된 줄이고, 한 번은 `cat`이 되돌려 출력한 줄이에요.

## 문제가 생기면

| 증상 | 해결 |
|---|---|
| `Cannot find module …/dist/zeropty.node` | `./build.sh`를 실행하거나, `ZEROPTY_NODE`를 모듈 경로로 지정하세요. |
| `wait-pane` 호출이 끝나지 않아요 | 기본 타임아웃이 없어요. `-T`나 `{ timeout }`을 주세요. |
| 임베디드 엔진에서 `wait-pane`이 실패해요 | 함수 호출로는 쓸 수 없어요. `capture-pane`을 루프로 돌리거나, `listen`을 열고 소켓 클라이언트를 쓰세요. |
| `listen`이 예외를 던져요 | 그 경로에서 다른 서버가 답하고 있어요. 다른 경로를 고르거나 그 서버를 멈추세요. |
| 내 `listen` 소켓에 붙은 플러그인 명령이 타임아웃돼요 | `await zeropty(...)`로 부르세요. `zeroptySync`는 플러그인이 도는 JS 스레드를 잡고 있어요. |

## 다음 단계

- 어떤 언어에서든 서버와 말하려면 [소켓 프로토콜](/ko/reference/socket-protocol#다른-프로그램에서-소켓)을 보세요.
- Node의 모든 export와 `open` 옵션은 [Node.js API](/ko/reference/node-api)를 보세요.
- Node 밖에서 같은 엔진을 쓰려면 [C와 Rust에 zeropty 내장하기](/ko/guide/embed)를 보세요.
