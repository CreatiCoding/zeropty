---
description: C로 zeropty 플러그인 라이브러리를 만들어 서버에 붙이고, 서버를 띄울 때마다 같이 시작하게 하고, CLI나 임베딩 호스트에서 그 명령을 불러요.
---

# 플러그인 작성하기

이 가이드를 마치면 C 라이브러리로 작은 **플러그인**을 만들어 zeropty 서버에 붙이고, 플러그인이 더한 명령을 CLI에서 부르고, 서버를 띄울 때마다 같이 실행되게 할 수 있어요. 플러그인은 `tally`예요. 만들어진 세션과 끝난 세션을 세고, 지금 살아 있는 세션이 몇 개인지 알려 줘요.

## 시작하기 전에

- `PATH`에 zeropty가 있는 Mac이 필요해요.
- `bindings/c/include/zeropty.h`와 `examples/plugins/tally.c`가 있는 zeropty 저장소가 필요해요. 명령은 그 폴더에서 실행하세요. 플러그인 라이브러리에는 헤더만 있으면 돼요. `libzeropty_c`에 링크하지 않아요.
- Xcode Command Line Tools의 `cc`가 필요해요.

## 1. 붙이는 방법 고르기

플러그인은 세션 이벤트를 듣고 명령을 더해요. 더한 명령은 내장 명령처럼 CLI, 소켓, 임베디드 호출에서 불러요. 붙이는 방법은 네 가지이고, 모두 같은 등록부, 이벤트, 권한 수준, 격리 규칙을 써요.

| 방법 | 언어 | 어디서 돌아요 | 예제 |
|---|---|---|---|
| 소켓 (프로세스 밖) | 아무 언어 (소켓과 JSON) | 별도 프로세스 | `examples/plugins/hello.py`, `examples/plugins/notify.mjs` |
| 플러그인 라이브러리 (`.dylib`) | C, 또는 C 함수를 내보낼 수 있는 언어 | `zeropty plugin-run`으로 별도 프로세스, 또는 호스트 안 | `examples/plugins/tally.c` |
| C ABI(application binary interface) 콜백 | C, 또는 C 함수를 넘길 수 있는 언어 | 호스트 프로세스, 플러그인 스레드 | `examples/plugins/count.c` |
| Rust 트레이트 | Rust | 호스트 프로세스, 플러그인 스레드 | `examples/plugin_activity.rs` |

이 가이드는 라이브러리를 만들어요. 같은 `.dylib`이 서버 옆에서도, 임베딩 호스트 안에서도 실행되기 때문이에요. 플러그인 코드는 엔진 스레드에서 돌지 않으므로, 느리거나 크래시하는 플러그인이 내 세션을 멈추지 않아요.

## 2. 플러그인 쓰기

라이브러리의 진입점은 `zeropty_plugin_init` 하나예요. 매니페스트(이름, 이벤트, 명령)와 콜백을 채워요. 콜백은 JSON 메시지를 한 번에 하나씩, 플러그인 자신의 스레드에서 받아요.

```c [examples/plugins/tally.c]
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "zeropty.h"

static const zeropty_plugin_api *zp; /* from the host: host_command and free */
static atomic_int created, ended;

/* One JSON message at a time, on the plugin's own thread (a real plugin would use a JSON parser). */
static char *on_message(void *user, const char *msg, zeropty_host *host) {
    (void)user;
    if (strstr(msg, "{\"event\":\"session-created\"") == msg) created++;
    if (strstr(msg, "{\"event\":\"session-exited\"") == msg) ended++;
    if (strncmp(msg, "{\"call\"", 7) != 0) return NULL; /* start, events, stop: nothing to answer */
    /* Asks zeropty, from inside the plugin, for the sessions running now: one "x" per session. */
    char *r = zp->host_command(host, "{\"argv\":[\"list-sessions\",\"-F\",\"x\"],\"cwd\":\"/\"}");
    int live = 0;
    const char *out = r ? strstr(r, "\"out\":\"") : NULL;
    for (const char *p = out ? out + 7 : ""; *p && *p != '"'; p++) live += *p == 'x';
    zp->free(r);
    char *reply = malloc(128); /* zeropty frees it */
    snprintf(reply, 128, "{\"code\":0,\"out\":\"created %d, ended %d, live %d\\n\",\"err\":\"\"}", created, ended, live);
    return reply;
}

int zeropty_plugin_init(const zeropty_plugin_api *api, zeropty_plugin_def *def) {
    if (api->api < 1) return 1;
    zp = api;
    def->manifest_json = "{\"name\":\"tally\",\"events\":[\"session-created\",\"session-exited\"],\"commands\":[\"tally\"]}";
    def->callback = on_message;
    def->user = NULL;
    return 0;
}
```

콜백은 `{"start":{"api":1}}`을 가장 먼저 받고, 그다음 이벤트와 호출을 받고, `{"stop":true}`를 마지막으로 받아요. `call`에는 `code`, `out`, `err`가 든 답을 `malloc`해서 돌려주세요. zeropty가 해제해요. 그 밖의 메시지에는 `NULL`을 돌려주세요.

`host_command`는 플러그인 안에서 zeropty 명령을 플러그인의 **권한(access)** 범위에서 실행해요. 위 매니페스트는 권한을 밝히지 않았으므로 기본값인 `read`를 받아요. `list-sessions`와 `capture-pane`은 되지만 `send-keys`는 안 돼요. 키를 보내려면 매니페스트에 `"access":["input"]`을 더하세요.

## 3. 빌드하기

헤더만으로 라이브러리를 빌드해요.

```sh [터미널]
cc -dynamiclib -O2 -Ibindings/c/include -o /tmp/tally.dylib examples/plugins/tally.c
```

## 4. 서버에 붙이기

서버가 없으면 하나 띄우고, `plugin-run`으로 라이브러리를 실행한 다음 그 명령을 불러요.

```sh [터미널]
zeropty new-session -d -s a sh            # starts the server if none is running
zeropty plugin-run /tmp/tally.dylib &
zeropty tally
```

`plugin-run`은 라이브러리를 자기 프로세스에 불러와서 소켓으로 서버에 붙여요. 라이브러리가 크래시하면 그 프로세스만 죽어요. 서버가 끝나면 `plugin-run`도 끝나요.

## 5. 서버를 띄울 때마다 같이 시작하기

라이브러리를 플러그인 디렉터리에 넣고 `ZEROPTY_PLUGIN_DIR`로 그 디렉터리를 가리키세요. 서버는 그 안의 플러그인을 시작하고, 끝나면 다시 시작해요.

```sh [터미널]
export ZEROPTY_PLUGIN_DIR=~/.zeropty/plugins               # put this in ~/.zshrc to keep it always on
mkdir -p ~/.zeropty/plugins
cp /tmp/tally.dylib ~/.zeropty/plugins/                  # a .dylib is started with zeropty plugin-run
zeropty restart-server                                     # takes effect from the next server
```

::: warning 기본은 꺼져 있어요
`ZEROPTY_PLUGIN_DIR`가 없으면 `~/.zeropty/plugins`가 있어도 서버는 아무것도 실행하지 않아요. 이 값을 지정하는 건 그 디렉터리의 코드를 내 사용자 권한으로 실행해도 된다는 동의예요. 다른 사용자가 쓸 수 있는 디렉터리와 파일은 건너뛰어요.
:::

- 서버는 자기를 띄운 셸에서 `ZEROPTY_PLUGIN_DIR`를 가져가요. 서버를 띄우는 `new-session`이나 `restart-server`를 실행한 셸이에요.
- 실행 파일과 `.dylib` 파일은 이름순으로 시작해요. 각 플러그인의 출력은 디렉터리의 `NAME.log`로 가요.
- 금방 끝나는 플러그인은 0.5초 뒤에 다시 시작하고, 이후 두 배씩 늘려 최대 30초까지 기다려요.
- 플러그인은 `restart-server`를 지나도 남아요. 옛 서버의 `plugin-run`은 옛 서버와 함께 끝나고, 새 서버가 소켓을 넘겨받은 뒤 자기 플러그인을 시작해요.

## 6. 프로그램에서 부르기

플러그인 명령은 다른 명령과 똑같이 불러요. 엔진을 내장한 호스트에서는 부른 스레드가 기다릴지 말지 고를 수 있어요.

| 호스트 | 답을 기다려요 | 바로 돌아와요 |
|---|---|---|
| C | `zeropty_command` | `zeropty_command_start` → `zeropty_call_poll` → `zeropty_call_free` |
| Rust `Embedded`, `Polled` | `command(argv, cwd)` | `command_start(argv, cwd) -> Call` |
| Node (`client/embedded.mjs`) | `zeroptySync(argv)` | `await zeropty(argv)` |

엔진을 내장한 호스트는 `zeropty_load_plugin(a, "/tmp/tally.dylib")`로 같은 라이브러리를 자기 프로세스에 불러올 수도 있어요. 그러면 라이브러리의 크래시는 그 프로세스의 크래시가 돼요.

## 확인하기

세션 두 개를 시작하고 하나를 끝낸 다음, 명령을 부르고 플러그인 목록을 보세요.

```sh [터미널]
zeropty new-session -d -s b sh
zeropty new-session -d -s c sh
zeropty kill-session -t '=c:'
zeropty tally
zeropty list-plugins
```

플러그인이 시작된 뒤의 개수가 보여야 해요. `live`는 실행 중인 모든 세션을 세므로, `a`와 `b`만 남았을 때는 이렇게 나와요.

```text
created 2, ended 1, live 2
```

`list-plugins`는 플러그인마다 `name: state (api N) events=... commands=... access=... dropped=N` 형태로 한 줄씩 출력해요. `tally: running`을 찾아보세요.

## 문제가 생기면

| 증상 | 해결 |
|---|---|
| `tally`가 `unknown command`예요 | 플러그인이 붙어 있지 않아요. `zeropty list-plugins`를 확인하세요. `restart-server` 직후에는 수십 ms 안에 플러그인이 다시 붙어요. |
| `failed (stuck in a hook ...)` | 콜백이 마감 시간(10초, `ZEROPTY_PLUGIN_TIMEOUT_MS`)보다 오래 실행됐어요. 먼저 "started"로 답하고 결과는 다른 명령으로 가져가게 하세요. |
| `plugin tally lacks access input` | 매니페스트에 권한을 더하세요. 예: `"access":["input"]` |
| `bad manifest: ...` | JSON 매니페스트에는 `name`, `events`, `commands`, `access`, `api`만 쓸 수 있어요. `"event"` 같은 오타가 없는지 확인하세요. |
| `dropped=`가 계속 늘어요 | 플러그인이 이벤트를 너무 느리게 읽어요. 큐에는 이벤트 1024개나 출력 8MB까지 들어가요. `output`보다 `activity`를 쓰세요. |
| 플러그인 디렉터리가 아무 일도 안 해요 | 서버를 띄운 셸에 `ZEROPTY_PLUGIN_DIR`가 없었어요. 지정한 뒤 `zeropty restart-server`를 실행하세요. |

## 다음 단계

- 모든 [이벤트](/ko/reference/plugin-api#이벤트)와 [권한 수준](/ko/reference/plugin-api#권한-access)은 플러그인 API를 보세요.
- Python이나 Node.js로 플러그인을 쓰려면 [플러그인용 소켓 프로토콜](/ko/reference/plugin-api#1-다른-프로세스에서-소켓-프로토콜-언어-무관)을 보세요.
- 내 프로세스에서 플러그인을 호스팅하려면 [C와 Rust에 zeropty 내장하기](/ko/guide/embed)를 보세요.
