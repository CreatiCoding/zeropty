# 플러그인

[English](../PLUGINS.md) · **한국어**

[README](../../README.ko.md) · [문서 목록](README.md) · [사용법](USAGE.md) · [플러그인](PLUGINS.md) · [FAQ](FAQ.md) · [벤치마크](BENCHMARKS.md) · [설계](DESIGN.md) · [버전 정책](VERSIONING.md) · [릴리스](RELEASING.md)

플러그인은 zeropty 밖의 코드예요. 세션에 일어나는 일(생성, 종료, 이름 변경, 출력)을 듣고, 새 명령을 더해요. 더한 명령은 CLI, 소켓, 임베디드 호출 어디서나 내장 명령처럼 불러요. zeropty를 다시 컴파일하지 않아도 붙일 수 있어요.

```sh
python3 examples/plugins/hello.py &      # 아무 언어로 쓴 프로그램이 소켓에 붙어요
zeropty hello you                        # → hello, you! sessions: ...
zeropty list-plugins                     # → hello: running (api 1) events=session-created,session-exited commands=hello access=read dropped=0
```

## 붙이는 방법

| 방법 | 언어 | 어디서 돌아요 | 다시 컴파일 | 예제 |
|---|---|---|---|---|
| **소켓 (프로세스 밖)** | 아무 언어 (소켓 + JSON) | 별도 프로세스 | 필요 없어요 | [`examples/plugins/hello.py`](../../examples/plugins/hello.py), [`examples/plugins/notify.mjs`](../../examples/plugins/notify.mjs) |
| **플러그인 라이브러리 (`.dylib`)** | C, 또는 C 함수를 내보낼 수 있는 언어 | `zeropty plugin-run`으로 별도 프로세스, 또는 호스트 프로세스 안 | 필요 없어요 (`zeropty.h`만 있으면 돼요) | [`examples/plugins/tally.c`](../../examples/plugins/tally.c) |
| **C ABI 콜백 (프로세스 안)** | C, 또는 C 함수를 넘길 수 있는 언어 | 호스트 프로세스, 플러그인 전용 스레드 | 필요 없어요 (`libzeropty_c.dylib`) | [`examples/plugins/count.c`](../../examples/plugins/count.c) |
| **Rust 트레이트 (프로세스 안)** | Rust | 호스트 프로세스, 플러그인 전용 스레드 | 호스트 프로그램만 | [`examples/plugin_activity.rs`](../../examples/plugin_activity.rs) |

넷 다 같은 등록부, 같은 이벤트, 같은 권한, 같은 격리 규칙을 써요. 소켓 플러그인은 내부에서 Rust 플러그인 하나(`src/plugin/__private/remote.rs`의 `Remote`)로 감싸져서, 큐·마감 시간·실패 처리가 똑같이 적용돼요. 서버와 함께 늘 떠 있어야 하는 플러그인은 [플러그인 디렉터리](#자동-시작-플러그인-디렉터리)에 넣어요.

## 작성자가 알아야 할 것

플러그인을 쓰는 데 필요한 규칙은 이것뿐이에요. 숫자는 기본값이고, 마감 시간(`ZEROPTY_PLUGIN_TIMEOUT_MS`, Rust는 `Plugins::set_timeout`, 기본 10초)을 바꾸면 비율대로 바뀌어요. 왜 그렇게 동작하는지는 [동작 모델](#동작-모델)에 있어요.

| 규칙 | 기본값 | 넘으면 |
|---|---|---|
| 플러그인 코드는 엔진 스레드에서 돌지 않아요. 플러그인마다 전용 스레드(프로세스 밖이면 그 프로세스)가 있고 훅은 한 번에 하나씩 불려요. 훅은 블로킹해도 돼요 | | |
| Rust `manifest()`는 등록할 때 한 번, 플러그인의 새 스레드에서 맨 먼저 불려요. `add`는 그동안 기다려요. 상수로 바로 만들고 느린 준비는 `start`에서 해요 | 1초 (마감이 더 짧으면 마감) | `add`가 `ManifestTimeout` 오류로 돌아오고 플러그인은 등록되지 않아요. `add`를 부른 쪽(`Polled`면 호스트 루프)이 그만큼만 기다리고, 엔진은 기다리지 않아요. 패닉은 `ManifestPanicked` 오류 |
| 이벤트는 플러그인마다 큐로 와요 | 1024개, 출력 8MB | 그 이벤트는 버리고 `list-plugins`의 `dropped=`에 세요. 엔진은 기다리지 않아요 |
| 더한 명령(`call`)에 답하기 | 8.75초 (마감의 8분의 7) | 그 호출만 `plugin timed out`, 늦은 답은 버려요. 플러그인은 `running`으로 남아요 |
| 훅 하나가 자기 시간으로 도는 시간 | 10초 (마감) | `failed (stuck in a hook ...)`, 더는 이벤트·명령을 받지 않아요 |
| 플러그인이 엔진에 시키는 명령(`Host::command`, 소켓 `id`, C `zeropty_host_command`) | 5초 (마감의 절반), 권한 안에서만 | 그 명령만 오류로 답해요. 기다리는 시간은 훅 시간에 치지 않아요 |
| 소켓 플러그인은 zeropty가 보내는 줄을 읽어야 해요 | 8.75초 | `failed (connection: not read ...)` |
| 훅의 패닉, 프로세스 밖 플러그인의 크래시 | | 그 플러그인만 `failed`가 되거나 빠져요. 엔진과 다른 플러그인은 그대로예요. 같은 이름으로 다시 붙을 수 있어요 |
| 플러그인끼리 서로를 기다리는 호출(A → B → A) | | 바로 `call cycle` 오류로 거절해요 |

- 오래 걸리는 일은 먼저 "시작했다"고 답하고 결과는 다른 명령으로 가져가게 만들어요.
- 부른 쪽이 포기한 명령은 나중에 실행되지 않아요(오류 끝에 `not run`).
- 종료 때는 `stop`이 와요. 프로세스 밖 플러그인은 서버가 끝나면 EOF를 받아요.
- 플러그인이 하나도 없으면 비용이 없어요: 출력 경로는 분기 하나, 명령은 원자 변수 하나를 더 읽어요([측정](#플러그인이-없을-때의-비용)).

## 호스트에서 플러그인 명령 부르기

플러그인이 더한 명령은 내장 명령과 같은 자리에서 불러요. 다만 플러그인이 일하는 동안 호출한 스레드를 잡을지 말지를 고를 수 있어요.

| 호스트 | 기다리기 (답까지 돌아오지 않아요) | 기다리지 않기 (바로 돌아와요) |
|---|---|---|
| Rust `Embedded` | `command(argv, cwd)` | `command_start(argv, cwd) -> Call` |
| Rust `Polled` | `command(argv, cwd)` (기다리는 동안에도 엔진을 돌려요) | `command_start(argv, cwd) -> Call` |
| C | `zeropty_command` | `zeropty_command_start` → `zeropty_call_poll` → `zeropty_call_free` |
| Node (`client/embedded.mjs`) | `zeroptySync(argv)` | `await zeropty(argv)` (`Engine.commandStart`: 늘 `Promise<Reply>`) |

- 엔진 명령(플러그인 것이 아닌 명령)은 `command_start`에서도 바로 실행돼서 답이 이미 들어 있는 `Call`로 와요. 그래서 어떤 명령이든 같은 코드로 다뤄요.
- `Call::poll()`은 기다리는 중이면 `None`, 답이 왔거나 플러그인이 멈췄거나 마감이 지났으면 `Some(Reply)`를 한 번 돌려줘요. 블로킹해도 되는 스레드는 `Call::wait()`를 써요. `Call::due_ms()`는 마감까지 남은 시간이에요.
- polled 엔진에서는 플러그인의 답이 `fd()`를 읽을 수 있게 만들고, `timeout_ms()`가 시작한 호출의 마감을 셈에 넣어요. 그래서 `fd()`와 `timeout_ms()`로 잠드는 루프는 답이나 마감을 바로 알아채고, 따로 깨울 필요가 없어요.

```rust
let call = polled.command_start(&argv, "/");       // 바로 돌아와요
loop {                                              // 호스트의 이벤트 루프
    poll_readable(polled.fd(), polled.timeout_ms()); // 호스트가 쓰는 poll/kqueue/libuv
    polled.process();
    if let Some(reply) = call.poll() { break reply; }
}
```

```c
zeropty_call *c = zeropty_command_start(a, "{\"argv\":[\"hello\",\"you\"],\"cwd\":\"/\"}");
for (;;) {
    struct pollfd p = { zeropty_fd(a), POLLIN, 0 };
    poll(&p, 1, (int)zeropty_timeout_ms(a));
    zeropty_process(a);
    char *r = zeropty_call_poll(c);                 // 기다리는 중이면 NULL
    if (r) { puts(r); zeropty_free(r); break; }
}
zeropty_call_free(c);                               // 답 전에 풀면 아직 시작 안 한 명령은 실행되지 않아요
```

- Node에서 `listen`으로 연 소켓에 **같은 프로세스의** 플러그인(`attachPlugin`)을 붙였다면 반드시 `await zeropty(...)`로 불러요. `zeroptySync`는 JS 스레드를 잡고 기다리는데, 그 플러그인도 같은 JS 스레드에서 돌아서 마감 시간까지 답하지 못해요.

## 권한 (access)

플러그인이 `Host::command`(소켓은 `id` 명령, C는 `zeropty_host_command`)로 실행할 수 있는 명령은 플러그인이 등록할 때 밝힌 권한까지예요. 기본은 보기(`read`)뿐이에요.

| 권한 | Rust | 허용하는 명령 |
|---|---|---|
| `read` (기본) | `Access::READ` | `capture-pane`, `display-message`, `has-session`, `list-sessions`, `grep-panes`, `show-environment`, `stats`, `mem`, `list-plugins` |
| `input` | `Access::INPUT` | `send-keys`, `paste-buffer`, `load-buffer` |
| `manage` | `Access::MANAGE` | `new-session`, `kill-session`, `rename-session`, `resize-*`, `set-environment`, `pipe-pane`(프로그램을 실행해요), 그 밖에 위 목록에 없는 명령 |
| `call` | `Access::CALL` | 다른 플러그인이 더한 명령 |
| `all` | `Access::ALL` | 위 전부 |

- 밝히는 곳: Rust `Manifest::new("x").access(Access::INPUT)`, 소켓 `plugin-attach ... -a input,call`, C·라이브러리 매니페스트 `"access":["input"]`, Node `attachPlugin({ access: ['input'] })`.
- 권한 밖의 명령은 실행하지 않고 `send-keys: plugin x lacks access input (...)`로 답해요. `list-plugins`의 `access=`에 각 플러그인의 권한이 보여요.
- 어떤 권한으로도 안 되는 것: `kill-server`, `pull-sessions`, `push-sessions`(세션 넘기기는 엔진 스레드를 잡아요), `attach`, `plugin-attach`, 자기 자신의 명령.
- 권한은 플러그인이 무엇을 하려는지 드러내고 실수를 막는 경계예요. 같은 사용자로 도는 프로세스 밖 플러그인은 소켓에 일반 클라이언트로 붙을 수도 있어서, 보안 경계는 소켓(이 사용자만 접근)이에요.

## 1. 다른 프로세스에서: 소켓 프로토콜 (언어 무관)

`zeropty serve`든, `listen`을 부른 임베디드 엔진이든 소켓이 열려 있으면 붙을 수 있어요. 연결 하나가 플러그인 하나예요. 메시지는 모두 JSON 한 줄(끝에 `\n`)이에요.

**1단계: 연결하기.** `ZEROPTY_SOCK`(없으면 `~/.zeropty/mux.sock`)에 유닉스 소켓으로 연결해요. CLI와 같은 규칙이에요.

**2단계: 붙기.** 첫 명령으로 `plugin-attach`를 보내요.

```json
{"argv":["plugin-attach","-n","hello","-e","session-created,session-exited","-c","hello"],"cwd":"/"}
```

| 옵션 | 뜻 |
|---|---|
| `-n 이름` | 플러그인 이름 (필수). 영문자, 숫자, `- _ . :`, 64자까지 |
| `-e 목록` | 받을 이벤트, 쉼표로 구분. `sessions`(생성·종료·이름 변경), `all`도 돼요. 없으면 이벤트를 받지 않아요 |
| `-c 목록` | 더할 명령 이름, 쉼표로 구분. 내장 명령 이름과 다른 플러그인이 가진 이름은 쓸 수 없어요 |
| `-a 목록` | 권한(위 표), 쉼표로 구분. 없으면 `read` |
| `-v 버전` | 이 플러그인이 쓰인 API 버전 (기본 1). zeropty보다 높으면 거절해요 |

답은 보통 명령 답과 같은 줄이에요. 성공하면 `{"code":0,"out":"zeropty-plugin 1\n","err":""}`이고, 이 답이 올 때는 이미 등록이 끝나 있어요. 실패하면 `code` 1과 이유예요(연결은 클라이언트로 남아서 고쳐서 다시 보내도 돼요).

**3단계: 메시지 주고받기.** 붙은 뒤로는 이 연결로 아래 메시지가 오가요. 첫 키가 메시지 종류예요.

| 방향 | 메시지 | 할 일 |
|---|---|---|
| zeropty → 플러그인 | `{"event":"session-created","session":"w"}` | 이벤트 (아래 표) |
| zeropty → 플러그인 | `{"call":7,"argv":["hello","you"],"cwd":"/"}` | 더한 명령이 불렸어요. 같은 번호로 답해요 |
| 플러그인 → zeropty | `{"reply":7,"code":0,"out":"hello\n","err":""}` | `call`에 대한 답 |
| 플러그인 → zeropty | `{"id":3,"argv":["capture-pane","-p","-t","=w:"],"cwd":"/"}` | zeropty 명령 실행 (권한 안에서) |
| zeropty → 플러그인 | `{"id":3,"code":0,"out":"...","err":""}` | 그 명령의 답 (보낸 순서대로) |

- `call`에 답하는 중에 `id` 명령을 보내도 돼요(예: `hello.py`는 `list-sessions`로 세션 목록을 받아서 답해요). 그 답을 기다리는 동안 이벤트나 다른 `call`이 먼저 올 수 있으니, 받은 순서대로 쌓아 뒀다가 처리해요.
- `wait-pane`은 플러그인에서 쓸 수 없어요. 기다리는 일은 이벤트로 해요.
- 연결을 닫으면 플러그인이 등록부에서 빠지고, 그 명령은 다시 `unknown command`가 돼요. 서버가 끝나면 플러그인은 EOF를 받아요.
- zeropty가 쓰는 소켓 쓰기에는 마감 시간(`reply_wait`, 마감의 8분의 7)이 있어요. 플러그인이 읽지 않고 버티면 그 플러그인만 실패 처리돼요.
- `call`에는 마감의 8분의 7(기본 8.75초) 안에 답해요. 늦으면 그 호출만 `plugin timed out`으로 실패하고 늦은 `reply`는 버려져요. 플러그인은 계속 붙어 있어요. 오래 걸리는 일이면 먼저 "시작했다"고 답하고 결과는 다른 명령으로 가져가게 만드세요.

**Python, 표준 라이브러리만:** [`examples/plugins/hello.py`](../../examples/plugins/hello.py)가 프로토콜 전체예요(68줄).

```sh
python3 examples/plugins/hello.py &
zeropty hello you
```

**Node:** [`client/plugin.mjs`](../../client/plugin.mjs)의 `attachPlugin`이 연결, 줄 나누기, 번호 맞추기를 해 줘요. CLI처럼, 다른 사용자(root 제외)가 띄운 서버에는 아무것도
보내기 전에 거절해요. 그런 서버는 플러그인의 명령을 불러 그 결과를 읽을 수 있기 때문이에요.

```js
import { attachPlugin } from './client/plugin.mjs'
const plugin = await attachPlugin({
  name: 'notes',
  events: ['session-exited'],
  access: [],                       // 보기만 해요. 키 입력이 필요하면 ['input']
  onEvent: (ev) => console.log(`${ev.session} ${ev.reason}`),
  commands: {
    // 문자열을 돌려주면 출력, { code, out, err }도 돼요. 예외는 code 1과 메시지가 돼요.
    'notes-count': async (argv, { plugin }) => {
      const { out } = await plugin.command(['list-sessions', '-F', '#{session_name}'])
      return `${out.split('\n').filter(Boolean).length}\n`
    },
  },
})
```

[`examples/plugins/notify.mjs`](../../examples/plugins/notify.mjs)는 세션 생성·종료 기록(`zeropty notify-log`)과 조용한 세션 찾기(`zeropty notify-idle 10`)를 더하고, `--desktop`이면 세션이 끝날 때 macOS 알림을 띄워요.

## 2. 플러그인 라이브러리 (`.dylib`)

C로 쓴 플러그인을 `zeropty.h` 하나만 가지고 라이브러리로 만들어요. `libzeropty_c`에 링크하지 않아요. 진입점은 하나예요.

```c
#include "zeropty.h"

static const zeropty_plugin_api *zp;   // 호스트가 준 함수: host_command, free

static char *on_message(void *user, const char *msg, zeropty_host *host) {
    if (strncmp(msg, "{\"call\"", 7) != 0) return NULL;          // start, 이벤트, stop
    char *r = zp->host_command(host, "{\"argv\":[\"list-sessions\"],\"cwd\":\"/\"}");
    zp->free(r);
    return strdup("{\"code\":0,\"out\":\"hi\\n\",\"err\":\"\"}");  // malloc한 답: zeropty가 free해요
}

int zeropty_plugin_init(const zeropty_plugin_api *api, zeropty_plugin_def *def) {
    if (api->api < 1) return 1;
    zp = api;
    def->manifest_json = "{\"name\":\"hi\",\"events\":[\"session-created\"],\"commands\":[\"hi\"]}";
    def->callback = on_message;
    def->user = NULL;
    return 0;                                                    // 0이 아니면 불러오지 않아요
}
```

1. 빌드: `cc -dynamiclib -O2 -Ibindings/c/include -o tally.dylib examples/plugins/tally.c`
2. 서버에 붙이기: `zeropty plugin-run ./tally.dylib &`. `plugin-run`은 라이브러리를 자기 프로세스에 불러와 소켓 프로토콜로 서버에 붙어요. 라이브러리가 죽어도 그 프로세스만 죽고 서버는 연결이 끊긴 것만 봐요. 서버가 끝나면 `plugin-run`도 끝나요.
3. 늘 켜 두기: 라이브러리를 [플러그인 디렉터리](#자동-시작-플러그인-디렉터리)(`ZEROPTY_PLUGIN_DIR`로 지정)에 복사하면 서버가 시작할 때 `plugin-run`으로 띄우고, 죽으면 다시 띄워요.
4. 내 프로세스에 넣기: 엔진을 넣은 프로그램은 `zeropty_load_plugin(a, "tally.dylib")`로 같은 라이브러리를 불러와요(이 경우 라이브러리의 크래시는 그 프로세스의 크래시예요).

- `zeropty_plugin_api`의 `size`는 호스트가 넘긴 구조체 크기예요. 나중 버전은 끝에만 필드를 더해요.
- 콜백이 받는 메시지는 소켓 프로토콜과 같아요. 처음에 `{"start":{"api":1}}`, 마지막에 `{"stop":true}`가 더 와요. `call`이면 `{"code","out","err"}` 답을 malloc해서 돌려줘요.
- 콜백은 한 번에 하나씩만 불려요(한 스레드). `host`는 그 호출 동안만 유효해요.

## 3. 내 프로세스 안에서: C ABI 콜백

`zeropty_open`(또는 `zeropty_open_polled`)으로 엔진을 넣은 프로그램은 함수 하나로 플러그인을 더해요. 콜백과 메시지는 2번과 같고, 명령은 `zeropty_host_command`로 실행해요.

```c
#include "zeropty.h"   // cc app.c -Ldist -lzeropty_c

static char *on_message(void *user, const char *msg, zeropty_host *host) {
    if (strncmp(msg, "{\"call\"", 7) == 0) {
        char *r = zeropty_host_command(host, "{\"argv\":[\"list-sessions\"],\"cwd\":\"/\"}");  // 엔진에 묻기
        zeropty_free(r);
        return strdup("{\"code\":0,\"out\":\"hi\\n\",\"err\":\"\"}");   // malloc한 답: zeropty가 free해요
    }
    return NULL;   // 이벤트, start, stop에는 NULL
}

char *err = zeropty_add_plugin(a, "{\"name\":\"hi\",\"events\":[\"session-created\"],\"commands\":[\"hi\"]}", on_message, NULL);
if (err) { fprintf(stderr, "%s", err); zeropty_free(err); }   // NULL이면 등록됐어요
```

- `zeropty_host_command`는 콜백 안에서만 써요(`host`는 그동안만 유효해요). 엔진 스레드 방식이든 polled 방식이든 안전해요(엔진 루프를 거쳐 실행돼요).
- 전체 예제: [`examples/plugins/count.c`](../../examples/plugins/count.c) (`./build.sh` 후 `cc -O2 -o /tmp/count examples/plugins/count.c -Idist -Ldist -lzeropty_c -Wl,-rpath,"$PWD/dist"`).
- Node 임베디드(`client/embedded.mjs`)는 `listen`으로 소켓을 열고 1번 방법으로 붙여요. 그 플러그인 명령은 `await zeropty(...)`로 불러요.

## 4. 내 프로세스 안에서: Rust

플러그인은 `zeropty::plugin`만 써요(`Reply`, `ok`, `fail`도 여기서 다시 내보내요). 엔진 모듈 구조가 바뀌어도 플러그인 코드는 그대로예요.

```rust
use zeropty::embed::Embedded;
use zeropty::engine::Options;
use zeropty::plugin::{ok, Access, Event, Events, Host, Manifest, Plugin, Reply};

struct Bell;

impl Plugin for Bell {
    fn manifest(&self) -> Manifest {
        Manifest::new("bell").events(Events::SESSION_EXITED).command("bell-test").access(Access::READ)
    }
    fn event(&mut self, event: &Event, _host: &Host) {
        if let Event::SessionExited { session, reason } = event {
            eprintln!("\x07{session} {}", reason.as_str());
        }
    }
    fn command(&mut self, _argv: &[String], _cwd: &str, host: &Host) -> Reply {
        let r = host.command(&["list-sessions"], "/");   // 엔진 명령, 엔진 스레드에서 실행돼요
        ok(format!("{} sessions\n", r.out.lines().count()))
    }
}

let e = Embedded::start(Options::default());
e.add_plugin(Bell)?;                        // 이 줄이 돌아온 순간부터 이벤트를 받아요
e.command(&["bell-test".into()], "/");      // 소켓(listen)과 CLI에서도 똑같이 불려요
```

- `Polled::add_plugin`도 같아요. 플러그인 명령을 기다리는 동안에도 polled 엔진은 돌아가요. 호스트 스레드를 잡지 않으려면 `command_start`를 써요([호스트에서 플러그인 명령 부르기](#호스트에서-플러그인-명령-부르기)).
- 훅에서 zeropty 밖의 무언가(다른 프로세스, 네트워크)를 기다린다면 `zeropty::plugin::reply_wait(host.deadline())`까지만 기다리세요. 그러면 늦을 때 그 호출만 실패하고 플러그인은 감시에 잘리지 않아요(소켓 플러그인의 서버 쪽이 이렇게 해요).
- 실행해 보기: `cargo run --release --example plugin_activity` (세션마다 출력한 바이트 수를 세는 `activity` 명령). 인자로 소켓 경로를 주면 계속 떠 있어서 `ZEROPTY_SOCK=경로 zeropty activity`로 불러 볼 수 있어요.
- 자기 루프를 가진 호스트(`Engine`을 직접 돌리는 프로그램)는 `host::HostLoop`에 등록부를 서비스로 붙여요. 세 줄이고, `src/cli/serve.rs`가 이렇게 해요.

  ```rust
  let plugins = Plugins::new();
  plugins.attach(&engine)?;                  // 플러그인이 루프를 깨울 수 있게 (엔진 큐에 사용자 이벤트 하나)
  server.set_plugins(plugins.clone());       // 소켓: list-plugins, 플러그인 명령, plugin-attach
  let mut host = HostLoop::new(Some(server), Waiting::WithEngine);
  host.set_plugins(plugins.clone());         // 매 바퀴 서버 다음에 pump, 잠들 시간에 감시 상한
  loop { host.turn(&mut engine, Source::Wait(None), Yield::Never, |_, _, _| {})?; }
  ```

  소켓 없이 쓰는 호스트는 `Plugins::add`로 붙이고, 플러그인 명령은 `Plugins::list`와 `Embedded`·`Polled`의 `command`/`command_start`처럼 부르면 돼요.
- 등록이 실패하면 `PluginError`로 이유를 구분해요. 예: `Err(PluginError::CommandTaken { owner, .. })`면 다른 이름으로 다시 시도하고, `Err(PluginError::Api { supported, .. })`면 `.api(supported)`로 낮춰요. 나머지는 `Err(e) => eprintln!("{e}")`처럼 `_` 갈래로 받아요.

## 자동 시작: 플러그인 디렉터리

`ZEROPTY_PLUGIN_DIR`로 디렉터리를 지정하면 `zeropty serve`가 시작할 때 그 안의 플러그인을 띄워요. **지정하지 않으면 아무것도 띄우지 않아요(기본).**

> **보안·호환성:** 디렉터리를 지정한다는 건 그 안의 코드를 서버와 같은 사용자 권한으로 실행해도 된다는 동의예요. 이미 있던 명령(`serve`, 서버를 띄우는 `new-session`)이 잘 알려진 경로의 파일을 몰래 실행하지 않도록 기본은 꺼 뒀어요. 그래서 이 기능이 생기기 전과 기본 동작이 같아요.

```sh
export ZEROPTY_PLUGIN_DIR=~/.zeropty/plugins               # 셸 설정(~/.zshrc 등)에 넣어 두면 늘 켜져요
mkdir -p ~/.zeropty/plugins
cp /tmp/tally.dylib ~/.zeropty/plugins/                  # .dylib는 zeropty plugin-run으로 떠요
printf '#!/bin/sh\nexec python3 /path/to/hello.py\n' > ~/.zeropty/plugins/hello
chmod +x ~/.zeropty/plugins/hello                          # 실행 파일은 그대로 떠요
zeropty restart-server                                     # 다음 서버부터 적용돼요
```

- 서버는 자기를 띄운 명령의 환경을 받아요. `new-session`이 서버를 띄울 때, `restart-server`가 새 서버를 띄울 때 그 셸에 `ZEROPTY_PLUGIN_DIR`가 있어야 해요(`ZEROPTY_SOCK`과 같은 규칙). 상대 경로는 서버가 시작한 디렉터리 기준으로 절대 경로가 돼요.

- 실행 권한이 있는 파일과 `.dylib`를 이름순으로 띄워요. `.`으로 시작하는 파일, 실행 권한 없는 파일은 건너뛰어요.
- 각 프로그램은 `ZEROPTY_SOCK`에 서버 소켓을 받고, 디렉터리를 작업 디렉터리로, 출력은 `<이름>.log`(1MB를 넘으면 새로 써요)로 가요. 스스로 `plugin-attach`해요.
- 끝나면 다시 띄워요. 금방 끝나면 0.5초에서 두 배씩 30초까지 기다렸다 띄우고, 30초 이상 돌았으면 0.5초 뒤에 띄워요. 디렉터리에서 지운 파일은 다시 띄우지 않아요.
- `kill-server`면 서버가 띄운 프로그램에 SIGTERM을 보내요(각자 자기 프로세스 그룹이에요).
- **`restart-server`를 지나도 남아요.** 옛 서버가 끝나면 그 플러그인들은 EOF를 받고 끝나고, 새 서버는 소켓 자리를 넘겨받은 뒤(`mux.sock.new`가 사라진 뒤) 자기 플러그인을 띄워요. 플러그인 쪽에서 다시 연결할 코드가 필요 없어요. 그 사이 잠깐(수십 ms) 플러그인 명령이 `unknown command`일 수 있어요.
- 디렉터리나 파일을 다른 사용자가 쓸 수 있으면(그룹·기타 쓰기 권한, 다른 소유자) 건너뛰고 서버 stderr에 이유를 남겨요. 다시 띄울 때마다 또 확인해요: 그 사이에 디렉터리나 파일을 다른 사람이 쓸 수 있게 됐거나 다른 사용자의 것으로 바뀌었으면 다시 띄우지 않아요. `<이름>.log`는 0600으로 만들고 심볼릭 링크를 따라 열지 않아요.
- 플러그인 디렉터리는 `zeropty serve`의 기능이에요. 구현(`zeropty::plugin::__private::launch`)은 zeropty 내부용이라 안정 API가 아니에요(아래 API 레퍼런스).

## 이벤트

| 이름 (`-e`) | Rust | 필드 | 언제 |
|---|---|---|---|
| `session-created` | `Event::SessionCreated` | `session` | `new-session`, 다른 서버에서 넘겨받았을 때 |
| `session-exited` | `Event::SessionExited` | `session`, `reason`: `exited` · `killed` · `handed-over` | 프로그램이 끝났을 때(`kill-server`도 `exited`), `kill-session`, 다른 서버로 넘겼을 때 |
| `session-renamed` | `Event::SessionRenamed` | `session`(새 이름), `from` | `rename-session` |
| `output` | `Event::Output` | `session`, `data`(소켓·C: base64) | 프로그램이 출력한 바이트를 읽을 때마다. 화면과 같이 모아 읽어서 조용한 세션은 \~300ms 단위예요. 이벤트 하나는 64KB까지라서 한꺼번에 많이 읽으면 여러 이벤트로 와요. UTF-8 문자가 두 이벤트에 나뉠 수 있어요 |
| `activity` | `Event::Activity` | `session` | 출력을 읽어 화면이 바뀔 때마다. 바이트를 복사하지 않아서 `output`보다 싸요 |
| `sessions` | `Events::SESSIONS` | | 위 세 생애 이벤트 |
| `all` | `Events::ALL` | | 전부 |

`output`을 원하는 플러그인이 있을 때만 엔진이 읽은 바이트를 복사해요. 화면 변화만 알면 되면 `activity`를 쓰고, 필요할 때 `capture-pane`으로 읽어요.

- **`output`은 손실이 있을 수 있어요.** 메모리 예산(엔진이 넘김 한 번에 16MB, 플러그인 큐마다 8MB) 때문에, 제때 읽는 플러그인도 한꺼번에 아주 큰 출력(수십 MB 이상)이 오면 일부를 잃어요. 출력 전체가 빠짐없이 필요하면 플러그인 대신 `pipe-pane`(세션 출력을 파일이나 명령으로 그대로 보내요)을 써요.
- **`dropped=`는 하나의 합계예요.** 큐가 차서(1024개나 출력 8MB) 버린 이벤트, 엔진이 16MB 예산을 넘어 버린 출력, 엔진 쪽 큐가 넘쳐 버린 이벤트를 모두 같은 숫자에 더해요. 이유별로 나눠 세지는 않아요.

## 명령

- `list-plugins`: 플러그인마다 한 줄. `이름: 상태 (api N) events=... commands=... access=... dropped=N`. 소켓, CLI, 임베디드 호출 모두 돼요. 상태는 셋이에요.
  - `running`: 이벤트와 명령을 받고 있어요.
  - `stopped`: 빠지는 중이에요. 연결이 닫혔거나 `remove`됐는데 호스트 루프가 아직 목록에서 지우지 않은 잠깐 동안 보여요. 명령은 이미 가지 않아요.
  - `failed (이유)`: 패닉(`panicked in 훅: 메시지`), 마감 시간을 넘긴 훅(`stuck in a hook for more than N ms`), 연결 오류(`connection: ...`). 목록에 남아서 이유를 볼 수 있고, 같은 이름으로 다시 붙으면 자리를 내줘요. 실패한 항목은 최근 16개까지만 남기고 오래된 것부터 지워요(이름을 바꿔 가며 다시 붙는 프로그램이 등록부를 끝없이 늘리지 않도록).
- `plugin-attach`: 위 1번. 소켓 연결에서만 의미가 있어요(CLI로 보내면 답을 받고 바로 끊겨서 플러그인도 바로 빠져요).
- `zeropty plugin-run 라이브러리.dylib`: 위 2번. 서버가 끝나면 끝나요.

## API 레퍼런스 (Rust, `zeropty::plugin`)

안정성 경계는 코드의 모듈·파일 경계와 같아요.

| 파일 | 경로 | 누가 써요 | 안정성 |
|---|---|---|---|
| `src/plugin/api.rs` | `zeropty::plugin::api` (= `zeropty::plugin::*`) | 플러그인 작성자 | 안정 API (아래 버전 정책) |
| `src/plugin/registry.rs`, `src/plugin/hosting.rs` | `Plugins`, `Call` | 엔진을 넣는 호스트 | 공개 메서드는 안정 API |
| `src/host.rs` | `HostLoop::set_plugins`, `impl Service for Plugins` | 자기 루프를 가진 호스트 | 호스트 루프 API(`zeropty::host`)와 같아요 |
| `src/plugin/__private/{remote,native,launch}.rs`, `src/plugin/wire.rs` | `zeropty::plugin::__private` (`#[doc(hidden)]`, 문서에 나오지 않아요) | zeropty 바이너리·C 바인딩·테스트 | **불안정**, semver 밖 |
| `src/engine/tap.rs` | (`Event`, `ExitReason`만 `zeropty::plugin`으로 공개) | 엔진 | 큐와 메서드는 크레이트 안에서만 |

**안정 API.** 플러그인 작성자와 엔진을 넣는 호스트가 쓰는 부분이에요. 같은 `API_VERSION` 안에서는 더하기만 해요(아래 버전 정책).

| 항목 | 설명 |
|---|---|
| `API_VERSION` | 이 인터페이스의 버전(지금 1). `Manifest::api`와 `plugin-attach -v`를 이것과 비교해요 |
| `trait Plugin: Send + 'static` | `manifest()`(필수), `start`, `event`, `command`, `stop`(기본 구현 있음). 훅 넷(`start`, `event`, `command`, `stop`)은 플러그인 스레드에서 한 번에 하나씩 불려요. `manifest()`는 등록할 때 한 번, 플러그인의 새 스레드에서 맨 먼저 불리고 `add`는 그 답을 매니페스트 마감(등록부 마감, 최대 1초)까지만 기다려요. 넘으면 `PluginError::ManifestTimeout(ms)`로 등록을 거절하고, 플러그인은 `manifest()`가 끝나는 대로 자기 스레드에서 버려져요. 그래서 멈춘 `manifest()`도 `add`를 부른 쪽을 최대 1초만 붙잡고 엔진은 붙잡지 않아요. 상수로 바로 만들고 느린 준비는 마감 시간 안에서 도는 `start`에서 해요. 패닉은 잡혀서 `PluginError::ManifestPanicked`가 돼요 |
| `Manifest::new(name).events(Events).command(name).access(Access).api(v)` | 이름, 받을 이벤트, 더할 명령, 권한, 버전. `events`·`access`는 이미 있는 것에 더해요. 필드 `events: Events`, `access: Access`(늘 `READ` 포함)는 읽을 수 있어요. `#[non_exhaustive]` |
| `Manifest::from_json(text)`, `attach_argv()` | C·라이브러리의 JSON 매니페스트 읽기, 같은 매니페스트의 `plugin-attach` 명령 |
| `Event` | `#[non_exhaustive]` enum. `kind() -> Events`(그 이벤트 하나의 비트), `name()`, `session()`, `to_json()` |
| `ExitReason` | `Exited`, `Killed`, `HandedOver`. `as_str()`. `#[non_exhaustive]`라 `match`에는 `_` 갈래가 필요해요 |
| `PluginError` | 등록·매니페스트 오류. `#[non_exhaustive]` enum: `BadName`, `BadCommand`, `Api { api, supported }`, `UnknownEvent`, `UnknownAccess`, `BuiltinCommand`, `DuplicateCommand`, `NameTaken`, `CommandTaken { owner }`, `BadManifest`, `ManifestPanicked`, `ManifestTimeout(ms)`, `Thread`, `Unsupported`(플러그인을 돌리지 않는 프런트엔드). `Display`는 사람이 읽는 이유, `String::from`은 답의 `err`처럼 줄바꿈을 붙여요. `std::error::Error` 구현 |
| `Events` | 받을 이벤트의 집합(타입이 있는 비트 플래그). 상수 `Events::{SESSION_CREATED, SESSION_EXITED, SESSION_RENAMED, OUTPUT, ACTIVITY, SESSIONS, ALL}`. `Copy`, `Eq`, `Hash`, `Debug`(`Events(session-created \| output)`), `Default`(빈 집합), `Display`(소켓과 같은 쉼표 목록). 연산 `\|`, `\|=`, `&`, `&=`, `-`(차집합), `!`(아는 비트 안에서의 여집합). 메서드 `empty()`, `all()`, `is_empty()`, `contains(other)`, `intersects(other)`, `union`, `difference`, `bits() -> u32`, `from_bits(u32) -> Option<Events>`(모르는 비트가 있으면 `None`), `from_bits_truncate`, `parse("a,b") -> Result<Events, PluginError>`(`FromStr`도 같아요), `names() -> String` |
| `Access` | 권한의 집합. 상수 `Access::{READ, INPUT, MANAGE, CALL, ALL}`. 트레이트와 메서드는 `Events`와 같아요(`parse("read,input")`, `names()`) |
| `Host::command(argv, cwd) -> Reply` | 엔진 명령을 권한 안에서 실행하고 답을 기다려요(마감 시간의 절반까지, 기다리는 시간은 훅 시간에 안 쳐요). 다른 플러그인의 명령은 `CALL`이 있어야 하고, 고리를 만드는 호출은 거절돼요 |
| `Host::deadline() -> Duration` | 마감 시간(`set_timeout`, `ZEROPTY_PLUGIN_TIMEOUT_MS`). 훅이 자기 시간으로 이보다 오래 돌면 잘려요 |
| `reply_wait(deadline) -> Duration` | 훅이 zeropty 밖을 기다려도 되는 시간: 마감에서 8분의 1(최소 20ms, 최대 절반)을 뺀 값. 감시보다 분명히 먼저 포기하게 해요 |
| `Host::name()`, `Host::detach()`, `Host::fail(why)` | 이름, 스스로 빠지기, 실패로 표시하고 빠지기(그 순간부터 명령이 오지 않아요) |
| `Reply`, `ok(out)`, `fail(err)`, `Reply::new(code, out, err)` | 명령의 답(`code`, `out`, `err` 필드를 읽어요). 엔진 모듈 것을 다시 내보내요. CLI와 소켓의 답 그대로라 세 필드는 고정이에요(`Reply { code, out, err }`로 직접 만들어도 돼요) |
| `is_reserved(name) -> bool` | 엔진이나 호스트(서버, CLI)가 직접 답하는 이름이라 플러그인이 가질 수 없는지. 엔진 쪽은 엔진의 명령 표(`COMMANDS`, 별칭 포함)를 그대로 읽어서 엔진에 명령이 생기면 함께 예약돼요. 그 목록 자체는 API가 아니에요: zeropty가 명령을 더하면 늘어나니 `myplugin-...`처럼 자기 접두사를 쓰세요 |
| `Plugins` | 등록부. `add`·`add_boxed`·`check`(`Result<(), PluginError>`), `remove`(없으면 `false`), `names`, `list`, `set_timeout`, `timeout`, `set_shutdown_wait`, `shutdown`, `attach(&engine)`(플러그인이 그 엔진의 루프를 깨우게 해요. 엔진을 받아서 큐 타입(지금은 kqueue)이 API에 드러나지 않아요), `watchdog_ms()`(플러그인이 있을 때 루프가 잠들 수 있는 최대 시간). `host::Service`를 구현해요. 공개 메서드는 모두 안정 API예요 |
| `HostLoop::set_plugins(plugins)`, `HostLoop::plugins()` | 호스트 루프가 등록부를 서비스로 돌리게 해요(`zeropty serve`, `Embedded`, `Polled`가 이걸 써요) |
| `Embedded::add_plugin`·`add_plugin_boxed`, `Embedded::plugins()`, `Polled::add_plugin`·`add_plugin_boxed`, `Polled::plugins()` | 임베디드 엔진에 붙이기(`Result<(), PluginError>`). 바인딩이 쓰는 `embed::Frontend`에도 `add_plugin_boxed`, `command_start`가 있어요. 둘 다 기본 구현이 있어서 플러그인 이전에 `Frontend`를 구현한 코드도 그대로 컴파일돼요(`add_plugin_boxed` 기본값은 `Err(PluginError::Unsupported)`, `command_start` 기본값은 `command`를 바로 답해요) |
| `Embedded::command_start`, `Polled::command_start` `-> Call` | 기다리지 않고 명령 부르기. 엔진 명령은 답이 든 `Call`로 와요 |
| `Call` | 진행 중인 명령. `poll() -> Option<Reply>`(답을 한 번 줘요), `wait() -> Reply`, `due_ms()`, `answered()`, `Call::ready(reply)`. 답 전에 drop하면 아직 시작 안 한 명령은 실행되지 않아요. `Polled::timeout_ms()`는 아직 답을 기다리는 `Call`의 마감만 세요. 답을 받았거나 drop한 `Call`은 루프를 깨우지 않아요 |

**`zeropty::plugin::__private` (안정 API 아님).** zeropty 자신의 바이너리(`plugin-run`, 플러그인 디렉터리)·C 바인딩·테스트가 쓰는 부분을 이 모듈 하나에 모았어요. **불안정하고 semver 밖**이에요: 버전을 올리지 않고도 언제든 바뀌거나 없어질 수 있어요. 안정 플러그인 API는 `api`(= `zeropty::plugin::*`), `Plugins`, `Call`뿐이고, 플러그인과 호스트는 `__private`를 쓰지 않아요. serde의 `__private`와 같은 관례예요: `#[doc(hidden)]`이라 API 문서에 나오지 않고, 안정 API의 어떤 항목도 이 모듈의 타입을 받거나 돌려주지 않아요. 이름에 `__`가 붙은 경로를 직접 쓰는 것이 곧 불안정한 것을 쓰겠다는 표시예요.

cargo 기능으로 숨기지 않은 이유: 바이너리(`src/main.rs`)와 바인딩은 라이브러리와 다른 크레이트라서 공개된 경로가 필요하고, cargo에는 패키지가 자기 바이너리에만 기능을 켜는 방법이 없어요. 기능 뒤에 두면 바이너리를 다른 패키지로 옮기거나(`cargo install --path .`가 깨져요) `required-features`를 걸어야 해서(`cargo install`이 바이너리를 건너뛰어요), 설치 방법을 그대로 두는 쪽을 골랐어요. 그래서 바이너리와 그 테스트는 예전처럼 루트 패키지 `zeropty`에 있어요:

```sh
cargo install --path .              # zeropty 바이너리 설치
cargo build --release -p zeropty    # target/release/zeropty
```

| 항목 | 설명 |
|---|---|
| `remote` (`Remote`, `parse_attach`) | 소켓 플러그인(`plugin-attach`)의 서버 쪽 |
| `launch::{Autostart, configured_dir, programs}` | 플러그인 디렉터리(`ZEROPTY_PLUGIN_DIR`) |
| `unsafe native::{load, run}`, `unsafe native::Library::from_parts`, `native::{Api, Def, ENTRY}` | 플러그인 라이브러리 불러오기(`zeropty_plugin_init`), `plugin-run`. 라이브러리를 불러오면 그 코드가 실행되므로 `unsafe`예요(`# Safety`에 조건). 다른 언어에서는 C ABI(`zeropty_load_plugin`)나 `zeropty plugin-run`을 써요 |
| `call_json`, `reply_json`, `reply_from_json`, `base64` | 프로토콜 구현용 |
| `set_engine_events`, `take_engine_events`, `has_engine_events` | 등록부 없이 엔진 쪽 이벤트 큐를 보는 테스트용 함수 |

크레이트 밖에서는 보이지 않는 것: 엔진 쪽 큐(`Engine::set_tap`, `take_tap`, `has_tap_events`), 등록부의 `pump`·`call`·`owns`, 깨우기 이벤트 태그(`PLUGIN_TAG`)는 `pub(crate)`예요.

C ABI (`bindings/c/include/zeropty.h`): `zeropty_add_plugin`, `zeropty_host_command`, `zeropty_load_plugin`, 라이브러리 진입점 `zeropty_plugin_init`과 `zeropty_plugin_api`·`zeropty_plugin_def`, 기다리지 않는 호출 `zeropty_command_start`·`zeropty_call_poll`·`zeropty_call_due_ms`·`zeropty_call_free`. Node(`zeropty.node`): `Engine.commandStart(argv, cwd)`는 늘 `Promise<Reply>`를 돌려줘요. 엔진 명령이면 이미 풀린 Promise예요. `Reply`와 Promise가 섞이지 않아서 늘 `await`하면 돼요.

등록 규칙: 이름과 명령 이름은 영문자·숫자·`- _ . :`(64자까지, `-`로 시작 못 해요), 내장 명령(`is_reserved`가 참인 이름)과 실행 중인 다른 플러그인의 이름·명령은 쓸 수 없어요. 실패하거나 멈춘 플러그인의 이름은 다시 쓸 수 있어요(재연결).

동작 한계값: 플러그인 큐 1024개·출력 8MB(256개와 2MB 아래로 비면 출력 복사 재개), `output` 이벤트 하나 64KB, 엔진 쪽 큐 이벤트 16384개·출력 16MB(한 번 읽은 출력도 16MB까지 복사), 마감 시간 10초(`Host::command`는 5초, 소켓 플러그인의 `call` 답과 소켓 쓰기는 8.75초), 감시 주기 마감 시간의 4분의 1(10ms\~1초), 종료 대기 1초, 남기는 실패 항목 16개.

## 버전 정책

- 플러그인은 자기가 쓰인 버전을 밝혀요(`Manifest::api`, `-v`, 매니페스트의 `api`). zeropty보다 새 버전이면 등록을 거절해요. 옛 버전은 계속 받아요.
- 같은 버전 안에서는 더하기만 해요: 새 훅은 기본 구현을 달고 오고, `Event`, `ExitReason`, `PluginError`, `Manifest`는 `#[non_exhaustive]`라 새 이벤트·종료 이유·오류 종류·필드가 생겨도 기존 플러그인이 컴파일돼요(그래서 이 enum들을 `match`할 때는 `_` 갈래를 둬요). `Reply`는 CLI·소켓의 답이자 임베더가 구조체 리터럴로 만드는 타입이라 `code`·`out`·`err` 세 필드로 고정이에요. 더 알려야 할 게 생기면 `Reply`에 필드를 더하지 않고 옆에 새 타입으로 더해요. 소켓 플러그인은 모르는 키와 모르는 `event`를 무시하면 돼요. C의 `zeropty_plugin_api`는 끝에만 필드를 더하고 `size`로 알려요.
- 바이트 단위 호환이 필요한 것(메시지의 키 순서 `event`·`call`·`reply`·`id`가 맨 앞)은 테스트로 고정해 뒀어요.
- **`zeropty::plugin::__private`는 이 정책 밖이에요.** zeropty 자신의 바이너리·바인딩·테스트용이라 semver를 따르지 않고, 버전을 올리지 않고도 바뀌거나 없어질 수 있어요. 쓰면 다음 릴리스에서 깨질 수 있어요. 컴파일러가 막지는 않아서(serde의 `__private`와 같은 관례), 이 이름을 쓰는 코드는 스스로 그 위험을 지는 거예요.
- 이벤트와 권한은 날 `u32` 비트가 아니라 `Events`·`Access` 타입이에요. 이 타입은 이 버전이 아는 비트만 담아서(`from_bits`는 모르는 비트에 `None`을 돌려줘요) 모르는 비트를 가진 매니페스트를 만들 수 없어요. 그래서 그런 매니페스트를 거절하던 `PluginError::UnknownEventBits`·`UnknownAccessBits`는 첫 릴리스 전에 없앴어요(`#[non_exhaustive]` enum이라 이 변경으로 깨지는 `match`는 없어요). 소켓·C·JSON의 형식(이름, 쉼표 목록)은 그대로예요.
- C·dylib의 JSON 매니페스트(`Manifest::from_json`)는 엄격해요. 다섯 키(`name`, `events`, `commands`, `access`, `api`) 밖의 키, 문자열이 아닌 목록 값, 타입이 틀린 값은 등록 오류(`bad manifest: ...`)예요. `"event"`처럼 철자를 틀려서 이벤트 0개로 조용히 등록되는 일이 없게 하려는 거예요.

## 소켓 서버와의 관계

소켓 서버(`src/server.rs`, `listen`)와 플러그인 등록부는 같은 메커니즘에 붙어 있어요. 둘 다 `host::Service`이고, 호스트 루프(`HostLoop`)가 한 바퀴마다 서버, 등록부 순서로 돌려요. 다만 서버를 `Plugin` 트레이트로 만들지는 않았어요.

- 서버는 엔진과 같은 스레드, 같은 kqueue에서 연결을 처리해서 명령 하나에 스레드를 건너지 않아요(그게 \~0.1ms 차이예요). `Plugin`은 정의상 엔진과 떨어진 스레드에서 돌아요.
- 서버는 attach, wait-pane, 세션 넘기기처럼 엔진 내부(`Engine<B>`)에 직접 닿아야 해요. `Plugin`은 공개된 명령(`Host::command`)과 권한(`access`) 안에서만 움직이도록 만든 경계예요.

그래서 서버를 플러그인으로 만들면 지연과 경계 둘 중 하나를 잃어요. 대신 "엔진 옆에서 같은 루프를 타는 것"(`Service`)과 "엔진과 떨어져 이벤트를 듣고 명령을 더하는 것"(`Plugin`)을 나눴고, 서버는 프로세스 밖 플러그인이 들어오는 입구예요(`plugin-attach`). 서버에 붙은 플러그인 명령은 서버의 모든 클라이언트가 써요.

## 테스트

```sh
cargo test --release --workspace                        # 전부: 루트 패키지, C 바인딩, Node 모듈
cargo test --release --test plugin                      # 등록, 이벤트, 명령 확장(임베디드·소켓·CLI), 예제, 격리, 마감
cargo test --release --lib -- plugin tap                # 엔진 쪽 큐, 매니페스트·프로토콜, 호스트 루프가 등록부를 도는 순서
cargo test --release -p zeropty-c                       # C 콜백 플러그인, 플러그인 라이브러리, polled 엔진에서 부르기
cargo test --release -p zeropty-node                    # Node 모듈: await zeropty()가 플러그인을 기다리는 동안 이벤트 루프를 막지 않음
ZEROPTY_REQUIRE_ALL=1 cargo test --release --workspace  # 릴리스 점검: 건너뛰는 테스트 없이 전부
```

- **훅과 명령 확장**: 이벤트 순서와 종류, 원하지 않은 이벤트는 오지 않음, 권한 밖 명령 거절, 붙기 성공 답을 받으면 이미 등록돼 있음, 플러그인 디렉터리.
- **함께 들어 있는 예제**: 모두 테스트에서 돌아요. `hello.py`는 실제 `zeropty serve`에 붙여 CLI로 부르고, `tally.c`는 `plugin-run`으로 붙여요. `count.c`는 빌드된 C 라이브러리(`libzeropty_c.dylib`)에 링크해 실행하고 출력(`created 2, ended 1, live 1`)을 확인해요. `notify.mjs`는 실제 서버에 붙여 세션 생성·이름 변경·종료 기록과 `notify-idle`(플러그인 안에서 서버에 묻는 명령)을 CLI로 확인하고, 서버가 끝나면 스스로 끝나는지 봐요. `--desktop` 알림은 따옴표(`"`)와 `&`가 든 세션 이름이 AppleScript 코드가 아니라 글자 그대로 가는지 확인해요(가짜 `osascript`가 받은 인자를 보고, 진짜 `osascript`로 같은 인자를 돌려 봐요).
- **예약 이름**: 엔진의 모든 명령 이름과 별칭이 예약돼 있고, 예약 이름 중 호스트 것이 아닌 것은 모두 엔진이 답함(양방향, `plugin::api` 단위 테스트).
- **격리**: 훅의 패닉, 읽지 않는 `output` 플러그인과 200MB 출력에도 서버 최대 메모리가 예산 안, 멈춘 훅(그동안 엔진 명령은 100ms 안에 답해요), 멈춘 `manifest()`(`add`는 1초 안에 `ManifestTimeout`), 큐가 찬 `output` 플러그인, 답하지 않거나 읽지 않는 소켓 플러그인, 호출 고리, 포기하거나 버린 명령은 실행되지 않음, 할 일 없는 엔진에서도 감시가 돎.
- **호스트 쪽 대기**: polled 엔진이 플러그인 명령을 기다리는 동안 다른 클라이언트에 답하고 CPU를 돌리지 않음, 답을 받았거나 버린 `Call`의 마감이 `timeout_ms()`를 앞당기지 않음(`embed` 단위 테스트), Node `await zeropty()`가 이벤트 루프를 막지 않음(`bindings/node/tests/plugin.rs`).
- **비용**: 플러그인이 없으면 엔진이 이벤트를 하나도 큐에 넣지 않음(`engine::tap` 단위 테스트). 지연은 [플러그인이 없을 때의 비용](#플러그인이-없을-때의-비용)에 있어요.
- **C 라이브러리와 Node 모듈**: `count.c` 테스트는 C 라이브러리를, Node 모듈 테스트는 `zeropty.node`를 써요. `cargo test --workspace`가 테스트보다 먼저 빌드해요(두 패키지의 lib이 `cdylib`+`rlib`이라서예요). 그냥 `cargo test --release`는 루트 패키지만 빌드하므로, `count.c` 테스트는 `./build.sh`나 `cargo build --release --workspace`가 만든 라이브러리를 써요. 어느 쪽이든 라이브러리 소스보다 오래된 것은 옛 코드를 시험하지 않게 쓰지 않아요. 테스트는 cargo를 직접 부르지 않아요.
- **건너뛴 테스트**: `python3`, `node`, `cc`, 빌드된 C 라이브러리가 없으면 그것이 필요한 테스트는 `SKIPPED <테스트 이름>: <이유>`를 찍고 넘어가요. 이 줄은 테스트 하네스가 가로채는 `eprintln!`이 아니라 프로세스의 stderr로 바로 가서, 그냥 `cargo test`의 결과 옆에도 보여요. `ZEROPTY_REQUIRE_ALL=1`이면 대신 실패해요. 기계 부하에 흔들리는 메모리 측정(`a_burst_to_a_plugin_that_never_reads_stays_within_the_budgets`)도 이때만 돌아요.

## 동작 모델

[작성자가 알아야 할 것](#작성자가-알아야-할-것)의 규칙을 자세히 적었어요. 각 규칙을 확인하는 테스트는 [테스트](#테스트)에 있어요. 등록부·서버·엔진이 어떻게 이어져 있는지는 [PLUGINS-INTERNALS.md](PLUGINS-INTERNALS.md)에 있어요.

**스레드**

- 플러그인 코드는 엔진 스레드에서 돌지 않아요. 플러그인마다 전용 스레드가 하나 있고, 훅(`start`, `event`, `command`, `stop`)은 그 스레드에서 한 번에 하나씩 불려요. 훅은 블로킹해도 돼요.
- 플러그인이 엔진에 시키는 명령(`Host::command`, 소켓은 `id` 명령, C는 `zeropty_host_command`)은 엔진 스레드에서 다른 명령과 똑같이 실행돼요.
- 엔진 스레드는 플러그인 소켓에 블로킹하지 않아요. `plugin-attach`의 성공 답과 그 연결에 밀린 답은 플러그인 스레드가 보내고, 성공 답이 갈 때는 이미 등록이 끝나 있어요.

**이벤트**

- 엔진은 어떤 플러그인도 원하지 않는 종류의 이벤트는 만들지 않아요.
- 이벤트는 플러그인마다 큐(1024개)로 가요. 호스트 루프가 한 바퀴 돌 때(`pump`) `try_send`로 넣고, 큐가 차 있으면 그 이벤트는 버리고 `list-plugins`의 `dropped=`에 세요. 느린 플러그인은 이벤트를 잃을 뿐 엔진을 붙잡지 못해요.
- 큐는 개수만이 아니라 바이트로도 막혀요. `output` 이벤트는 64KB 이하 조각으로 나뉘고, 플러그인 큐마다 출력 8MB까지만 들어가요(넘는 조각은 버리고 `dropped=`에 세요). 엔진도 한 번 읽은 출력을 16MB까지만 복사하고 나머지는 이벤트 하나를 버린 것으로 세요. 그래서 읽지 않는 플러그인이 있어도 서버가 더 쓰는 메모리는 이 예산들(합쳐서 수십 MB)을 넘지 않아요. 예전에는 이벤트 하나가 한 번에 읽은 출력 전부라서, 200MB `yes`와 읽지 않는 `output` 플러그인 하나로 서버 메모리가 141MB에서 656MB가 됐어요. `a_burst_to_a_plugin_that_never_reads_stays_within_the_budgets`가 서버의 최대 메모리(`ri_lifetime_max_phys_footprint`)를 플러그인 없을 때와 비교해 64MB 안으로 고정해요(이 기계에서 101MB 대 131MB). 이 측정은 기계 부하에 흔들릴 수 있어서 `ZEROPTY_REQUIRE_ALL=1`일 때만 돌고, 예산 자체는 결정적인 단위 테스트(`engine::tap`의 `bursts_come_in_bounded_pieces`, 등록부의 바이트 예산 테스트)가 늘 확인해요.
- `output`(또는 `activity`)을 원하는 플러그인이 모두 큐가 가득 찼으면, 엔진은 출력을 복사하지도 이벤트를 만들지도 않고 `dropped=`만 세요. 그중 하나의 큐가 4분의 1(256개) 아래로 비면 다시 복사해요. 생애 이벤트(`session-*`)는 이렇게 끄지 않고 큐에서 하나씩 세요.
- 엔진 쪽 큐에도 상한(이벤트 16384개, 출력 16MB)이 있어요. 호스트가 `pump`를 드물게 불러서 넘친 이벤트는 그 종류를 원한 플러그인의 `dropped=`에 더해요.

**명령과 마감 시간**

- 플러그인 명령을 부른 쪽은 마감 시간 안에 답을 못 받으면 `plugin timed out` 오류를 받아요.
- 소켓 서버는 플러그인 명령의 답을 비동기로 기다려요. 기다리는 건 그 연결뿐이고 다른 클라이언트는 그대로 처리돼요. 같은 연결의 다음 명령은 순서를 지키려고 그 뒤에 처리돼요.
- 엔진을 넣은 호스트는 두 가지로 불러요. `command`는 답이나 마감까지 돌아오지 않아요. `command_start`(C `zeropty_command_start`, Node `commandStart`는 늘 `Promise<Reply>`)는 바로 돌아오고, 답은 나중에 `Call::poll`로 가져가요. 자세한 건 [호스트에서 플러그인 명령 부르기](#호스트에서-플러그인-명령-부르기)에 있어요.
- polled 엔진의 `Polled::command`는 기다리는 동안에도 `process`가 하는 일(세션 출력 읽기, `listen`한 소켓의 클라이언트, 플러그인이 시킨 엔진 명령, 감시)을 계속 해요. 그 사이에는 엔진의 kqueue에서 잠들고, 플러그인의 답·세션 출력·클라이언트·엔진 타이머가 깨워요. CPU를 돌리며 기다리지 않아요.
- 소켓 플러그인과 `plugin-run` 라이브러리의 `call` 답은 `reply_wait(마감)`(마감의 8분의 7, 기본 8.75초)까지 기다려요. 넘으면 그 호출만 `plugin timed out (no reply within N ms; a later reply is ignored)`로 실패하고, 뒤늦게 온 `reply`는 버려요. 플러그인은 `running`으로 남아요. 플러그인에 한 줄(이벤트, 호출, 답)을 쓰는 데도 같은 시간을 통틀어 한 번만 줘요. 소켓 버퍼가 차서 그 안에 다 못 쓰면 그 플러그인은 `failed (connection: not read for N ms)`가 돼요. 마감보다 먼저 끝나므로 감시가 `stuck in a hook`으로 자르지 않아요. (`write`마다 시간 제한을 걸면 macOS는 몇 바이트라도 보낸 `write`를 성공으로 돌려줘서 `write_all`이 제한을 여러 번 기다렸고, 감시가 먼저 잘랐어요.)
- `Host::command`는 마감 시간의 절반까지 기다려요. 넘으면 `명령: no answer within N ms (half the plugin deadline)`를 받아요. 남은 절반은 이 플러그인을 부른 쪽에 이유를 담아 답하는 데 써요.
- 부른 쪽이 포기한 명령은 나중에 실행되지 않아요. 아직 시작하지 않은 명령은 버리고 오류 끝에 `not run`(플러그인 큐에서 기다리기만 했으면 `busy; not run`)을 붙여요. 이미 시작한 명령이면 `started, may still take effect`(플러그인 명령은 `plugin timed out`)를 붙여요. 시작과 포기는 원자 변수 하나로 한 번만 정해지므로, 실패로 알린 명령이 뒤에 실행되는 일은 없어요. 답을 받기 전에 버린 `Call`(C `zeropty_call_free`)도 같아요.
- 고리를 만드는 호출은 바로 거절해요. A가 B의 명령을 기다리는 동안 B가 A의 명령을 부르면(`call` 권한) `명령: call cycle (b -> a -> b): plugin a is waiting for b`를 받아요. 더 긴 고리도 같아요. 두 플러그인은 `running`으로 남아요.

**실패 격리**

- 훅에서 패닉이 나면 그 플러그인만 `failed (panicked in 훅: 메시지)`가 되고, 부른 쪽은 오류를 받아요. 실패한 순간부터 명령이 그 플러그인으로 가지 않고, 같은 이름으로 다시 등록할 수 있어요.
- 훅 하나가 자기 시간으로 마감 시간보다 오래 돌면 `failed (stuck in a hook for more than N ms)`가 되고 더는 이벤트와 명령을 받지 않아요. Rust 스레드는 밖에서 끝낼 수 없어서, 그 스레드는 훅에서 돌아올 때 끝나요.
- 영영 돌아오지 않는 훅은 그 스레드를 끝까지 붙잡아요. 이건 피할 수 없어요(Rust에는 스레드를 밖에서 죽이는 방법이 없어요). 그래도 엔진은 영향을 받지 않아요. 감시가 그 플러그인을 `failed`로 표시하고 큐를 닫으니 이벤트도 명령도 더 가지 않고, 엔진은 원래 플러그인 큐나 스레드를 기다리는 일이 없어요(`try_send`만 해요). 그 플러그인 명령을 부른 쪽은 마감 시간에 오류를 받고, `shutdown`은 기다림(기본 1초)이 끝나면 그 스레드를 떼어 놓고 돌아와요. 남는 비용은 멈춘 스레드 하나와 그 플러그인이 쥔 메모리뿐이고, 프로세스가 끝날 때 사라져요. 플러그인을 다시 시작해야 하면 같은 이름으로 새로 등록할 수 있어요. 이게 걱정되는 플러그인은 프로세스 밖(소켓 플러그인, `plugin-run`)으로 돌리면 멈춘 프로세스를 OS가 끝낼 수 있어요.
- `manifest()`도 플러그인 스레드에서 돌아요. `add`는 플러그인 스레드를 먼저 띄우고 거기서 `manifest()`를 불러서, 그 답을 매니페스트 마감(등록부 마감과 1초 중 짧은 쪽)까지만 기다려요. 넘으면 `ManifestTimeout`으로 거절하고 이름도 비워 둬요. 그 스레드는 `manifest()`가 돌아오면 플러그인을 버리고 끝나요(영영 안 돌아오면 위의 멈춘 훅처럼 스레드 하나가 남아요). 등록부 잠금은 `manifest()`의 답을 받은 뒤에만 잡으니, 기다리는 동안 다른 플러그인 등록·명령·엔진은 막히지 않아요.
- 자기 시간만 세요. 훅 안에서 `Host::command`로 엔진이나 다른 플러그인을 기다리는 시간은 빼요. 느린 쪽은 자기 훅 시간으로 판정돼요.
- 감시는 호스트 루프의 `pump`에서 돌아요. 플러그인이 하나라도 있으면 호스트 루프는 마감 시간의 4분의 1(10ms\~1초)보다 오래 잠들지 않아요(`Plugins::watchdog_ms`를 `HostLoop::timeout_ms`가 셈에 넣어요. polled 엔진은 `timeout_ms()`, 서버와 `Embedded`는 루프가 잠들 시간에 들어 있어요). 그래서 할 일이 없는 엔진에서도 멈춘 훅은 마감 시간의 1.25배 안에 잘려요. 플러그인이 없으면 이 상한도 없어요.
- 프로세스 밖 플러그인이 죽으면 서버는 연결이 끊긴 것만 봐요. 그 플러그인은 등록부에서 빠져요.

**종료**

- `Plugins::shutdown`(그리고 `Embedded`·`Polled`의 drop)은 플러그인마다 `stop`을 보내고 스레드가 끝나기를 `set_shutdown_wait`(기본 1초, 0이면 기다리지 않음)까지 기다려요. 그때도 훅 안에 멈춰 있는 스레드는 떼어 놓고(detach) 돌아와요. 그 스레드는 훅에서 돌아오면 `stop` 없이 끝나요.
- 프로세스 밖 플러그인은 서버가 끝나면 EOF를 받아요.

## 플러그인이 없을 때의 비용

- 플러그인이 없으면 엔진의 출력 읽기 경로는 정수 하나를 읽는 분기 하나만 더 해요(세션 생성·종료·이름 변경도 같아요). 출력 바이트는 `output`을 원하는 플러그인이 있을 때만 복사해요.
- 호스트 루프 한 바퀴: 등록부의 `after_tick`(= `pump`)은 원자 변수 세 개를 읽고 돌아오고, 잠들 시간 계산(`watchdog_ms`)은 원자 변수 하나를 읽어요. 플러그인 깨우기 이벤트 확인은 호스트 이벤트가 있을 때만 해요.
- 명령 한 번: `Embedded::command`, `Polled::command`, 서버 모두 플러그인 명령인지 먼저 원자 변수 하나(명령을 더한 플러그인이 있는지)로 확인해요. 없으면 이름 비교도 하지 않아요. `list-plugins` 이름 비교는 엔진이 모르는 명령일 때만 해요(`Embedded`는 엔진 스레드로 넘기는 느린 길, `Polled`는 엔진이 실패로 답한 뒤).
- 서버 루프는 답을 기다리는 플러그인 명령이 있을 때만 클라이언트 목록을 훑어요. polled 엔진의 `timeout_ms()`는 `command_start`로 시작한 호출이 없으면 빈 목록 하나를 확인해요.
- 등록부는 플러그인이 붙을 때까지 fd도 스레드도 갖지 않아요(`Embedded`·`Polled`는 첫 `add_plugin`이나 `listen` 때 엔진 큐에 `attach`해요).

같은 기계에서 플러그인이 없는 `main`과 번갈아 잰 표(polled 한 바퀴, 명령, capture-pane, send-keys)와 다시 재는 방법은 [PLUGINS-INTERNALS.md](PLUGINS-INTERNALS.md#플러그인이-없을-때의-비용)에 있어요. 모든 차이가 같은 빌드를 다시 돌린 흔들림 안이에요.
