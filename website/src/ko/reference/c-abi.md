---
description: zeropty C ABI(libzeropty_c.dylib와 zeropty.h)의 모든 함수, 타입, 매크로를 인자, 반환값, 누가 무엇을 해제하는지와 함께 정리해요.
---

# C ABI

`libzeropty_c.dylib`의 헤더 `zeropty.h`에 있는 모든 함수, 타입, 매크로예요. 빌드, 링크, 첫 프로그램은 [C와 Rust에 zeropty 내장하기](/ko/guide/embed)를 보세요.

빌드가 이 목록을 검사해요. `api/c-abi.txt`를 라이브러리, 헤더와 비교하고, 거기 있는 심볼이 이 페이지에 없으면 사이트 빌드가 실패해요. 원본은 [`bindings/c/include/zeropty.h`](https://github.com/CreatiCoding/zeropty/blob/main/bindings/c/include/zeropty.h)예요.

## 모든 호출의 규칙

| 규칙 | 내용 |
|---|---|
| 요청과 답은 JSON | 요청은 `{"argv": [...], "cwd": "/"}`이고 argv는 CLI와 같아요. 답은 `{"code": 0, "out": "...", "err": ""}`예요. |
| 답은 누가 해제해요 | 라이브러리가 돌려주는 모든 `char *`는 `zeropty_free`로 정확히 한 번 해제해요. `free()`로 해제하지 않아요. 예외는 `zeropty_version`이에요. 정적 문자열이에요. |
| 스레드 | `zeropty_open`이면 명령은 어느 스레드에서 와도 되고, 각 명령은 부른 쪽 스레드에서 돌아요. `zeropty_open_polled`면 모든 호출이 루프를 돌리는 한 스레드에서 와요. |
| 닫은 뒤 | `zeropty_close` 뒤에는 핸들이 무효예요. 그 도중이나 뒤에 핸들로 어떤 호출도 돌면 안 돼요. |

## 버전

### `ZEROPTY_VERSION_*`

```c
#define ZEROPTY_VERSION_MAJOR 0
#define ZEROPTY_VERSION_MINOR 1
#define ZEROPTY_VERSION_PATCH 5
#define ZEROPTY_VERSION_PRERELEASE ""
#define ZEROPTY_VERSION_STRING "0.1.5"
#define ZEROPTY_VERSION_NUMBER (ZEROPTY_VERSION_MAJOR * 1000000 + ZEROPTY_VERSION_MINOR * 1000 + ZEROPTY_VERSION_PATCH)
#define ZEROPTY_VERSION_IS_PRERELEASE 0
```

이 헤더가 속한 버전이에요. [버전 정책](/ko/guide/versioning)을 따라요. 위 값은 현재 릴리스의 값이에요.

| 매크로 | 뜻 |
|---|---|
| `ZEROPTY_VERSION_MAJOR`, `ZEROPTY_VERSION_MINOR`, `ZEROPTY_VERSION_PATCH` | 세 숫자예요. |
| `ZEROPTY_VERSION_PRERELEASE` | 프리릴리스 부분(0.2.0-rc.1의 `"rc.1"`), 릴리스면 `""`예요. |
| `ZEROPTY_VERSION_STRING` | 버전 전체를 문자열로요. |
| `ZEROPTY_VERSION_NUMBER` | `#if`용 숫자 하나: MAJOR × 1000000 + MINOR × 1000 + PATCH. 프리릴리스는 빠지므로 0.2.0-rc.1과 0.2.0은 숫자가 같아요. |
| `ZEROPTY_VERSION_IS_PRERELEASE` | 프리릴리스면 `1`, 릴리스면 `0`. `#if`로 검사할 수 있어요. |

실행할 때 헤더와 라이브러리를 비교하세요. MAJOR가 다른(1.0 전에는 MINOR가 다른) 라이브러리는 이 선언과 맞지 않을 수 있어요.

### `zeropty_version`

```c
const char *zeropty_version(void);
```

링크된 라이브러리의 버전이에요. `ZEROPTY_VERSION_STRING`과 같은 형태예요. 정적 문자열이라 해제하지 마세요.

## 자기 스레드를 가진 엔진

### `zeropty`

```c
typedef struct zeropty zeropty;
```

엔진 핸들이에요. 불투명 타입이라 이 페이지의 함수로만 써요.

### `zeropty_open`

```c
zeropty *zeropty_open(const char *options_json);
```

이 프로세스 안에서 엔진을 시작해요.

| 인자 | 뜻 |
|---|---|
| `options_json` | `NULL`, 또는 `{"history": ROWS, "size_dir": "DIR"}`: 세션마다 스크롤백 줄 수, 크기를 바꾼 세션의 창 크기를 그 프로그램을 위해 써 두는 곳이에요. |

핸들을 돌려주고, 실패하면 `NULL`이에요. `zeropty_close`로 닫아요.

```c
zeropty *zp = zeropty_open("{\"history\": 2000}");
if (!zp) return 1;
```

### `zeropty_command`

```c
char *zeropty_command(zeropty *a, const char *request_json);
```

CLI처럼 명령 하나를 실행해요.

| 인자 | 뜻 |
|---|---|
| `a` | 엔진이에요. |
| `request_json` | `{"argv": ["capture-pane", "-p", "-t", "=x:"], "cwd": "/"}`. |

답 `{"code", "out", "err"}`를 돌려줘요. 유효한 엔진이면 `NULL`은 없어요. `zeropty_free`로 해제해요. 플러그인의 명령은 플러그인을 기다려요. 스레드를 잡지 않고 기다리려면 [`zeropty_command_start`](#zeropty_command_start)를 써요.

### `zeropty_free`

```c
void zeropty_free(char *reply);
```

이 라이브러리의 답을 해제해요. 각각 정확히 한 번, `free()`로는 해제하지 않아요. `NULL`이면 아무것도 하지 않아요.

### `zeropty_listen`

```c
int zeropty_listen(zeropty *a, const char *path);
```

zeropty 서버처럼 `path`의 유닉스 소켓에서도 명령에 답해요. 그러면 `ZEROPTY_SOCK=PATH zeropty send-keys ...`, `attach`, 소켓 클라이언트가 서버 프로세스 없이 이 엔진의 세션에 닿아요.

`0`을 돌려주고, 거기서 listen할 수 없으면(이미 서버가 답하고 있거나, 경로가 너무 길거나, ...) `-1`이에요. polled 엔진은 `zeropty_process`에서 소켓을 처리해요.

### `zeropty_close`

```c
void zeropty_close(zeropty *a);
```

엔진을 멈춰요. 세션은 프로그램이 끝날 때까지 계속 돌아요. 세션을 끝내려면 먼저 `kill-server`를 보내요. 그 뒤로 핸들은 무효이고, `zeropty_close` 도중이나 뒤에 핸들로 어떤 호출도 돌면 안 돼요.

## 내 이벤트 루프가 돌리는 엔진

이 네 함수는 자기 스레드가 없는 엔진을 줘요. 내 루프가 엔진을 돌려요. 모든 호출은 그 한 스레드에서 와요.

```c
zeropty *a = zeropty_open_polled(NULL);
for (;;) { poll({zeropty_fd(a), POLLIN}, zeropty_timeout_ms(a)); zeropty_process(a); }
```

### `zeropty_open_polled`

```c
zeropty *zeropty_open_polled(const char *options_json);
```

[`zeropty_open`](#zeropty_open)과 같고 `options_json`도 같지만, 엔진에 스레드가 없어요. 핸들의 다른 호출(`zeropty_command`, `zeropty_listen`, `zeropty_close`, ...)은 루프의 스레드에서 전과 똑같이 동작해요.

### `zeropty_fd`

```c
int zeropty_fd(zeropty *a);
```

읽기를 기다릴 파일 디스크립터예요. 읽을 수 있게 되면 `zeropty_process`를 불러요. 자기 스레드를 가진 엔진이면 `-1`이에요.

### `zeropty_timeout_ms`

```c
long long zeropty_timeout_ms(zeropty *a);
```

`zeropty_fd`를 읽을 수 없어도 `zeropty_process`를 불러야 할 때까지 남은 밀리초예요.

### `zeropty_process`

```c
void zeropty_process(zeropty *a);
```

기다리고 있는 일을 블로킹 없이 처리해요. `zeropty_fd`를 읽을 수 있거나 `zeropty_timeout_ms` 밀리초가 지나면 불러요.

## 플러그인을 기다리지 않는 명령

플러그인의 명령은 시간이 걸릴 수 있어요. 이 호출들은 명령을 시작하고 답이 올 때까지 루프가 계속 돌게 해 줘요. 두 종류의 엔진 모두에서 동작해요.

```c
zeropty_call *c = zeropty_command_start(a, "{\"argv\":[\"my-plugin-cmd\"],\"cwd\":\"/\"}");
for (;;) { poll({zeropty_fd(a), POLLIN}, zeropty_timeout_ms(a)); zeropty_process(a);
           char *r = zeropty_call_poll(c); if (r) { ...; zeropty_free(r); break; } }
zeropty_call_free(c);
```

플러그인의 답이 `zeropty_fd`를 읽을 수 있게 만들고, `zeropty_timeout_ms`가 호출의 마감을 셈에 넣어서, 루프가 헛돌지 않아요.

### `zeropty_call`

```c
typedef struct zeropty_call zeropty_call;
```

진행 중인 명령이에요. 불투명 타입이에요.

### `zeropty_command_start`

```c
zeropty_call *zeropty_command_start(zeropty *a, const char *request_json);
```

[`zeropty_command`](#zeropty_command)와 같은 `request_json`으로 명령을 시작해요. 플러그인의 명령은 진행 중인 상태로 돌아오고, 그 밖의 명령(과 잘못된 요청)은 이미 답이 들어 있어요. 호출은 `zeropty_call_free`로 해제해요.

### `zeropty_call_poll`

```c
char *zeropty_call_poll(zeropty_call *c);
```

호출이 기다리는 중이면 `NULL`이에요. 아니면 답 JSON을 한 번 줘요. 플러그인의 답, 또는 플러그인이 멈췄거나 마감이 지났을 때의 실패예요. `zeropty_free`로 해제해요.

### `zeropty_call_due_ms`

```c
long long zeropty_call_due_ms(zeropty_call *c);
```

호출의 마감, 곧 `zeropty_call_poll`이 플러그인을 포기할 때까지 남은 밀리초예요. `NULL`이면 `-1`이에요.

### `zeropty_call_free`

```c
void zeropty_call_free(zeropty_call *c);
```

호출을 해제해요. 답 전에 해제하면, 플러그인이 아직 시작하지 않은 호출은 실행되지 않아요.

## 이 프로세스 안의 플러그인

C 함수가 이 엔진의 세션을 듣고, 자기가 더한 명령에 답할 수 있어요. 작성 방법은 [플러그인 작성하기](/ko/guide/plugins)를, 메시지와 권한 규칙은 [플러그인 API](/ko/reference/plugin-api)를 보세요.

### `zeropty_host`

```c
typedef struct zeropty_host zeropty_host;
```

플러그인의 콜백이 명령을 실행할 때 쓰는 거예요. 넘겨받은 그 호출 동안만 유효해요.

### `zeropty_plugin_fn`

```c
typedef char *(*zeropty_plugin_fn)(void *user, const char *message_json, zeropty_host *host);
```

플러그인의 콜백이에요. 엔진 스레드가 아닌 플러그인 전용 스레드에서 돌고, 매번 JSON 메시지 하나를 받아요.

| 메시지 | 언제 |
|---|---|
| `{"start":{"api":1}}` | 맨 처음. |
| `{"event":"session-exited","session":"w","reason":"killed"}` 및 다른 이벤트 | 매니페스트가 요청한 이벤트마다. |
| `{"call":1,"argv":["hello","x"],"cwd":"/"}` | 자기 명령 하나가 불렸을 때. |
| `{"stop":true}` | 맨 마지막. |

`call`이면 답 `{"code":0,"out":"...","err":""}`를 `malloc`한 메모리로 돌려주고(zeropty가 해제해요), 그 밖에는 `NULL`을 돌려줘요. 마감(10초)을 넘겨 멈춘 콜백은 잘리고, 느린 콜백은 이벤트를 잃어요. 엔진은 콜백 때문에 멈추지 않아요.

### `zeropty_add_plugin`

```c
char *zeropty_add_plugin(zeropty *a, const char *manifest_json, zeropty_plugin_fn callback, void *user);
```

이 엔진에 플러그인을 더해요.

| 인자 | 뜻 |
|---|---|
| `manifest_json` | `{"name":"hello","events":["session-created","output"],"commands":["hello"],"access":["input"],"api":1}`. `events`는 목록이나 쉼표로 구분한 문자열(`"session-created,output"`)이에요. `access`는 `zeropty_host_command`가 실행할 수 있는 범위예요: 기본 `"read"`, `"input"`, `"manage"`, `"call"`, `"all"`. 빼도 돼요. |
| `callback` | [`zeropty_plugin_fn`](#zeropty_plugin_fn)이에요. |
| `user` | `callback`을 부를 때마다 다시 넘겨줘요. |

더해지면 `NULL`, 아니면 이유를 돌려줘요. `zeropty_free`로 해제해요.

### `zeropty_host_command`

```c
char *zeropty_host_command(zeropty_host *host, const char *request_json);
```

콜백 안에서 [`zeropty_command`](#zeropty_command)와 같은 `request_json`으로 명령을 실행해요. 엔진 루프를 거쳐 실행되어서 두 종류의 엔진 모두에서 안전해요. 답 JSON을 돌려주고, `zeropty_free`로 해제해요.

## 플러그인 라이브러리

플러그인 라이브러리는 이 헤더 하나만 가지고(`-lzeropty_c` 없이) 빌드한 `.dylib`이고, 진입점은 하나예요.

```c
int zeropty_plugin_init(const zeropty_plugin_api *api, zeropty_plugin_def *def);
```

`def`를 채우고 `0`을 돌려주고(그 밖의 값이면 불러오지 않아요), `api`는 보관해 둬요. 같은 라이브러리가 서버에 붙은 별도 프로세스(`zeropty plugin-run plugin.dylib`, 또는 `ZEROPTY_PLUGIN_DIR`로 시작한 서버의 플러그인 디렉터리)에서 돌 수도 있고, [`zeropty_load_plugin`](#zeropty_load_plugin)으로 이 프로세스 안에서 돌 수도 있어요.

### `ZEROPTY_PLUGIN_API`

```c
#define ZEROPTY_PLUGIN_API 1
```

이 헤더가 설명하는 플러그인 API 버전이에요.

### `zeropty_plugin_api`

```c
typedef struct zeropty_plugin_api {
    unsigned int api;
    unsigned int size;
    char *(*host_command)(zeropty_host *host, const char *request_json);
    void (*free)(char *s);
} zeropty_plugin_api;
```

| 필드 | 뜻 |
|---|---|
| `api` | 호스트의 플러그인 API 버전이에요. |
| `size` | 호스트가 넘긴 것의 `sizeof`예요. 나중 버전은 끝에만 필드를 더해요. |
| `host_command` | 이 플러그인용 [`zeropty_host_command`](#zeropty_host_command)예요. |
| `free` | `host_command`가 돌려준 것을 해제해요. |

### `zeropty_plugin_def`

```c
typedef struct zeropty_plugin_def {
    const char *manifest_json;
    zeropty_plugin_fn callback;
    void *user;
} zeropty_plugin_def;
```

| 필드 | 뜻 |
|---|---|
| `manifest_json` | [`zeropty_add_plugin`](#zeropty_add_plugin)과 같아요. 라이브러리가 불려 있는 동안 유효해야 해요. |
| `callback` | 플러그인의 [`zeropty_plugin_fn`](#zeropty_plugin_fn)이에요. |
| `user` | `callback`을 부를 때마다 다시 넘겨줘요. |

### `zeropty_plugin_init_fn`

```c
typedef int (*zeropty_plugin_init_fn)(const zeropty_plugin_api *api, zeropty_plugin_def *def);
```

진입점 `zeropty_plugin_init`의 타입이에요.

### `zeropty_load_plugin`

```c
char *zeropty_load_plugin(zeropty *a, const char *path);
```

플러그인 라이브러리를 이 엔진의 프로세스에 불러와요. 라이브러리의 크래시는 이 프로세스의 크래시예요. 더해지면 `NULL`, 아니면 이유를 돌려줘요. `zeropty_free`로 해제해요.
