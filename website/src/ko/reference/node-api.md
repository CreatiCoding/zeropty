---
description: zeropty Node.js 모듈(client/zeropty.mjs, client/embedded.mjs, 네이티브 zeropty.node)이 내보내는 모든 것을 인자와 결과와 함께 정리해요.
---

# Node.js API

zeropty Node.js 모듈이 내보내는 모든 것이에요. 첫 프로그램과 예제는 [Node.js에서 zeropty 쓰기](/ko/guide/node)를 보세요.

빌드가 이 목록을 검사해요. `api/node.txt`를 모듈과 비교하고, 거기 있는 export가 이 페이지에 없으면 사이트 빌드가 실패해요. 지원하는 Node.js: N-API 6 이상(`napi6`).

| 모듈 | 무엇인가요 | 언제 써요 |
|---|---|---|
| [`client/zeropty.mjs`](#clientzeroptymjs) | 실행 중인 zeropty 서버의 소켓에 붙는 클라이언트 | 세션이 zeropty 서버에 있고 CLI와 함께 써요. |
| [`client/embedded.mjs`](#clientembeddedmjs) | 내 Node 프로세스 안의 엔진 | 내 프로그램이 서버 프로세스 없이 세션을 가져요. |
| [`zeropty.node`](#zeroptynode) | 두 모듈이 함께 쓰는 네이티브 모듈 | JS 모듈이 감싸지 않은 클래스가 필요할 때요. |

두 JS 모듈은 같은 `zeropty`와 `zeroptySync`를 내보내서, 둘 사이를 바꿀 때는 import만 바꾸면 돼요.

## 결과 형태

```ts
{ ok: boolean, code: number, out: string, err: string }
```

명령의 결과로, CLI의 종료 상태와 출력 그대로예요. `code`는 종료 코드, `ok`는 `code === 0`, `out`과 `err`는 표준 출력과 표준 오류예요.

## `client/zeropty.mjs`

zeropty 서버의 클라이언트예요. 서버 소켓과의 연결 하나를 열어 두고, 명령을 한 줄씩 보내면 답이 순서대로 돌아와요. argv가 CLI와 같아서 `tmux(...args)`를 부르던 곳은 `zeropty(args)`가 돼요.

### `zeropty`

```ts
zeropty(argv: string[], options?: { cwd?: string, sock?: string, timeout?: number }): Promise<Result>
```

명령 하나를 실행하고 그 [결과](#결과-형태)로 resolve해요.

| 인자 | 기본값 | 뜻 |
|---|---|---|
| `argv` | | 명령 줄이에요. 명령 이름이 먼저 와요. |
| `options.cwd` | `process.cwd()` | 명령이 보는 작업 디렉터리예요. |
| `options.sock` | [`sockPath()`](#sockpath) | 서버의 소켓이에요. |
| `options.timeout` | `15000` | 답을 기다리는 밀리초예요. `wait-pane`은 여기서 주지 않으면 타임아웃이 없어요(자기 `-T`는 서버 쪽 타임아웃을 정해요). |

서버가 아니라 프로그램이 직접 실행하는 명령(`restart-server`, `serve`, `attach`, `-V`)은 CLI를 실행해요.

### `zeroptySync`

```ts
zeroptySync(argv: string[], options?: { cwd?: string, sock?: string }): Result
```

같은 일을 동기로 해요. 명령을 쓰고 답을 이 스레드에서 블로킹으로 읽고, 그 사이에 이벤트 루프가 돌지 않아요. Node가 달리 할 일이 없을 때는 `zeropty`보다 빠르고, 명령 시간(약 0.1 ms) 동안 스레드를 잡아요. `dist/zeropty.node`가 필요해요. 서버가 없으면 CLI가 실행되고, `new-session`이면 서버를 시작해요.

### `sockPath`

```ts
sockPath(): string
```

서버의 소켓이에요. `$ZEROPTY_SOCK`, 없으면 `~/.zeropty/mux.sock`이에요.

### `cliPath`

```ts
cliPath(): string
```

서버가 실행할 수 없는 명령이 가는 CLI예요. `$ZEROPTY_BIN`, 없으면 모듈 옆의 `dist/zeropty`예요.

### `VERSION`

```ts
VERSION: string
```

이 모듈의 버전이에요. [버전 정책](/ko/guide/versioning)을 따라요.

## `client/embedded.mjs`

서버 프로세스 없이 내 프로세스 안에 있는 엔진이에요. 네이티브 모듈은 `$ZEROPTY_NODE`, 없으면 모듈 옆의 `dist/zeropty.node`에서 불러와요.

### `open`

```ts
open(options?: { history?: number, sizeDir?: string, polled?: boolean }): Engine
```

엔진을 지금 시작해요. 부르지 않으면 첫 명령이 시작해요. [`Engine`](#engine)을 돌려줘요.

| 옵션 | 기본값 | 뜻 |
|---|---|---|
| `history` | | 세션마다 스크롤백 줄 수예요. |
| `sizeDir` | | 크기를 바꾼 세션의 창 크기를 그 프로그램을 위해 써 두는 곳이에요. |
| `polled` | `true` | 엔진에 자기 스레드 없이 Node의 이벤트 루프가 엔진을 돌려요. 약 0.3 MB 덜 쓰고, 출력은 JS 스레드에서 조금씩 나눠 파싱해요. `false`면 엔진에 스레드를 줘서 worker 스레드에서도 명령을 부를 수 있어요. |

### `zeropty`

```ts
zeropty(argv: string[], options?: { cwd?: string }): Promise<Result>
```

명령 하나를 실행하고 그 [결과](#결과-형태)로 resolve해요. 플러그인의 명령은 JS 스레드를 잡지 않고 기다려서, 그동안 다른 일과 `listen`으로 이 프로세스에 붙은 플러그인이 계속 돌아요.

### `zeroptySync`

```ts
zeroptySync(argv: string[], options?: { cwd?: string }): Result
```

같은 일을 바로 돌려줘요. 엔진 명령은 아무것도 기다리지 않아요. 플러그인의 명령은 답할 때까지, 마감까지 JS 스레드를 잡아요. 그런 명령에는 `zeropty`를 써요.

### `listen`

```ts
listen(sockPath: string): void
```

zeropty 서버처럼 `sockPath`의 유닉스 소켓에서도 명령에 답해요. 그러면 zeropty CLI(`ZEROPTY_SOCK=SOCKPATH zeropty send-keys ...`), `attach`, `client/zeropty.mjs`가 서버 프로세스 없이 이 프로세스의 세션에 닿아요. 이미 서버가 거기서 답하고 있으면 예외를 던져요.

### `version`

```ts
version(): string
```

불러온 `zeropty.node`의 버전이에요. `VERSION`과 같아야 해요. 다른 체크아웃에서 빌드한 `dist/`면 다를 수 있어요.

### `VERSION`

```ts
VERSION: string
```

이 모듈의 버전이에요.

## `zeropty.node`

네이티브 모듈이에요. `createRequire`로 불러와요.

```js [app.mjs]
import { createRequire } from 'node:module'
const { Engine, Client, version } = createRequire(import.meta.url)('./dist/zeropty.node')
```

### `version`

```ts
version(): string
```

엔진의 버전이에요. `zeropty --version`, C ABI의 [`zeropty_version()`](/ko/reference/c-abi#zeropty_version)과 같아요.

### `Engine`

이 프로세스 안의 zeropty 엔진이에요. `client/embedded.mjs`가 하나를 감싸요.

```ts
new Engine(options?: { history?: number, sizeDir?: string, polled?: boolean })
```

세션이 없는 엔진이에요. `polled`가 `false`가 아니면 Node의 루프가 돌려요. 옵션은 [`open`](#open)과 같아요.

```js
const engine = new Engine({ history: 2000 })
```

| 메서드 | 돌려주는 값 | 뜻 |
|---|---|---|
| `command(argv: string[], cwd?: string)` | `{ code, out, err }` | 서버 왕복 없이 명령 하나를 동기로 실행해요(Node에서 `send-keys` p50 0.06–0.07 ms, [벤치마크](/ko/guide/benchmarks)에서 측정). 플러그인의 명령은 마감까지 기다려요. |
| `commandStart(argv: string[], cwd?: string)` | `Promise<{ code, out, err }>` | 같은 일을 플러그인이 답하는 동안 JS 스레드를 잡지 않고 해요. 엔진 명령의 promise는 이미 resolve돼 있고, 플러그인 명령은 답이 오면 resolve돼요. |
| `listen(path: string)` | | zeropty 서버처럼 `path`의 유닉스 소켓에서도 명령에 답해요. polled면 연결은 Node의 루프에서 처리해요. |

```js
const { code, out } = engine.command(['capture-pane', '-p', '-t', '=w:'])
engine.listen(`${process.env.HOME}/.zeropty/app.sock`) // then: ZEROPTY_SOCK=... zeropty attach -t w
```

### `Client`

zeropty 서버와의 연결로, 동기로 써요. 명령마다 쓰고 답을 부른 스레드에서 블로킹으로 읽고, 그 사이에 이벤트 루프가 돌지 않아요. `client/zeropty.mjs`의 `zeroptySync`가 하나를 써요.

```ts
new Client(sock: string)
```

`sock`은 서버의 소켓 경로예요. 첫 명령에서 연결해요.

| 메서드 | 돌려주는 값 | 뜻 |
|---|---|---|
| `command(argv: string[], cwd?: string)` | `{ code, out, err }` | 서버에서 명령 하나를 실행해요. 서버가 없거나 사라졌으면 예외를 던져요. |
| `close()` | | 연결을 닫아요. 다음 명령이 새로 열어요. |

```js
const c = new Client(`${process.env.HOME}/.zeropty/mux.sock`)
console.log(c.command(['list-sessions']).out)
c.close()
```
