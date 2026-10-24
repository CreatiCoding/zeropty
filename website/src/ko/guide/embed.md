---
description: C나 Rust 프로그램에 zeropty 엔진을 넣어요. 엔진 스레드를 두거나 내 이벤트 루프로 돌리고, 필요하면 CLI용 소켓도 열어요.
---

# C와 Rust에 zeropty 내장하기

이 가이드를 마치면 서버 프로세스 없이 내 C 또는 [Rust](https://www.rust-lang.org) 프로그램 안에서 세션을 실행할 수 있어요. CLI와 같은 명령을 보내고, 엔진 스레드를 쓸지 내 루프를 쓸지 고르고, CLI가 내 세션에 닿도록 소켓을 열어요.

## 시작하기 전에

- Xcode Command Line Tools(`xcode-select --install`)가 있는 Mac이 필요해요.
- **C:** 소스에서 빌드해야 해요. `./build.sh`가 `dist/`에 `libzeropty_c.dylib`와 `zeropty.h`를 넣어요. 릴리스 아카이브에는 들어 있지 않아요.
- **Rust:** Rust 1.88 이상이 필요해요. crate가 crates.io에 없으므로 git 의존성으로 넣어요. `./build.sh`는 필요 없어요.
- 터미널이 필요한 프로그램(claude, vim)을 띄우려면 `dist/zeropty-tty`도 필요해요. crate에는 들어 있지 않아요. 자세한 내용은 [claude와 다른 터미널 프로그램 실행하기](/ko/guide/terminal-programs)를 보세요.

## 1. 빌드에 zeropty 추가하기

::: code-group

```sh [C (터미널)]
git clone https://github.com/CreatiCoding/zeropty && cd zeropty
./build.sh                       # dist/libzeropty_c.dylib and dist/zeropty.h
```

```toml [Rust (Cargo.toml)]
[dependencies]
zeropty = { git = "https://github.com/CreatiCoding/zeropty" }
```

:::

Rust에서 한 커밋에 머물려면 `rev = "COMMIT"`을 더하세요. `COMMIT`은 커밋 해시로 바꾸세요.

## 2. 엔진 시작하고 명령 보내기

**임베디드** 엔진은 CLI와 같은 명령을 받고 같은 `code`, `out`, `err`를 돌려줘요. C에서 요청과 답은 소켓 프로토콜의 JSON 줄이에요. Rust에서는 `argv`와 `Reply`예요.

::: code-group

```c [examples/c/hello.c]
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include "zeropty.h"

/* Runs one command and prints its reply. */
static void run(zeropty *zp, const char *request) {
  char *reply = zeropty_command(zp, request);
  printf("%s\n  -> %s\n", request, reply);
  zeropty_free(reply);
}

int main(void) {
  zeropty *zp = zeropty_open(NULL); /* or "{\"history\": 2000}" */
  if (!zp) return 1;
  run(zp, "{\"argv\":[\"new-session\",\"-d\",\"-s\",\"c\",\"-x\",\"40\",\"-y\",\"4\",\"exec cat\"],\"cwd\":\"/\"}");
  run(zp, "{\"argv\":[\"send-keys\",\"-t\",\"=c:\",\"-l\",\"hi from C\"],\"cwd\":\"/\"}");
  run(zp, "{\"argv\":[\"send-keys\",\"-t\",\"=c:\",\"Enter\"],\"cwd\":\"/\"}");
  usleep(100 * 1000); /* time for cat to print the line back */
  run(zp, "{\"argv\":[\"capture-pane\",\"-p\",\"-t\",\"=c:\"],\"cwd\":\"/\"}");
  run(zp, "{\"argv\":[\"kill-server\"],\"cwd\":\"/\"}");
  zeropty_close(zp);
  return 0;
}
```

```rust [examples/embed.rs]
use std::time::{Duration, Instant};
use zeropty::embed::Embedded;
use zeropty::engine::{Options, Reply};

fn main() {
    let zp = Embedded::start(Options::default());
    let run = |args: &[&str]| -> Reply { zp.command(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "/") };

    // A session is a program on pipes: `sh` here, any program works (`exec cat`, a coding agent...).
    let r = run(&["new-session", "-d", "-s", "demo", "-x", "60", "-y", "10", "sh"]);
    assert_eq!(r.code, 0, "{}", r.err);

    // Keys go through the engine's line discipline, as through a PTY: Enter ends the line.
    run(&["send-keys", "-t", "=demo:", "-l", "echo \"answer: $((6 * 7))\""]);
    run(&["send-keys", "-t", "=demo:", "Enter"]);

    // capture-pane reads the output pending first, so the screen is never stale.
    let start = Instant::now();
    let screen = loop {
        let screen = run(&["capture-pane", "-p", "-t", "=demo:"]).out;
        if screen.contains("answer: 42") || start.elapsed() > Duration::from_secs(5) {
            break screen;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    print!("{}", screen.trim_end_matches('\n'));
    println!();

    // Every session's name and size, in tmux's format language.
    print!("{}", run(&["list-sessions", "-F", "#{session_name} #{window_width}x#{window_height}"]).out);

    // kill-server ends the sessions; dropping `zp` stops the engine's thread.
    run(&["kill-server"]);
}
```

:::

`zeropty_open`(Rust는 `Embedded::start`)은 엔진에 자기 **엔진 스레드**를 줘요. 명령은 어느 스레드에서든 불러도 돼요. 화면 읽기, 키 보내기, 조회는 부른 스레드에서 실행되고, 나머지는 모두 엔진 스레드로 넘어가요. C에서는 모든 답을 `free()`가 아니라 `zeropty_free`로 풀어야 해요.

함수 호출에는 `wait-pane`이 없으므로 이 예제들은 화면을 루프로 읽어요. polled 엔진에서 기다리는 방법은 3단계에서 보여 줘요.

## 3. 내 루프로 엔진 돌리기

**polled** 엔진은 자기 스레드가 없어요. 내 이벤트 루프가 엔진의 fd를 기다리다가, 읽을 수 있게 되거나 타임아웃이 지나면 `process`를 불러요. 모든 호출은 그 루프의 스레드에서 해야 해요. 대기 세션 400개에서 polled 모드는 메모리를 0.74MB에서 0.45~0.46MB로 줄였어요.

::: code-group

```c [app.c]
zeropty *a = zeropty_open_polled(NULL);
for (;;) {   // host loop: call zeropty_process when the engine fd is readable or the timeout expires
  struct pollfd p = { zeropty_fd(a), POLLIN, 0 };
  poll(&p, 1, (int)zeropty_timeout_ms(a));
  zeropty_process(a);
}
```

```rust [examples/polled.rs]
let mut zp = Polled::start(Options::default());
let r = zp.command(&argv(&["new-session", "-d", "-s", "job", "sh -c 'sleep 0.5; echo build done; exec cat'"]), "/");
assert_eq!(r.code, 0, "{}", r.err);

// wait-pane -t job -m 'build done' -T 5000, without the command name.
let wait = zp.engine().wait_start(&argv(&["-t", "=job", "-m", "build done", "-T", "5000"])).unwrap_or_else(|r| panic!("{}", r.err));
let reply = loop {
    if let Some(reply) = zp.engine().wait_check(&wait) {
        break reply;
    }
    let mut p = libc::pollfd { fd: zp.fd(), events: libc::POLLIN, revents: 0 };
    unsafe { libc::poll(&mut p, 1, zp.timeout_ms().min(i32::MAX as u64) as i32) };
    zp.process();
};
zp.engine().wait_end(&wait);
```

:::

Rust에서는 `Engine::wait_start`와 `wait_check`로 화면을 폴링하지 않고 루프 안에서 `wait-pane`을 쓸 수 있어요. 세션을 `exec cat`으로 끝내서 출력한 뒤에도 살아 있게 했어요. 마지막 출력 직후에 끝나는 세션은 그 출력이 일치하기 전에 끝난 것으로 보고될 수 있어요.

## 4. CLI용 소켓 열기

**서버 플러그인**은 `zeropty serve`와 같은 서버를 내 프로그램 안에서 돌려요. 그러면 CLI, `attach`, 셸 함수, Node 클라이언트가 내 세션에 닿고, 그 소켓에서 `wait-pane`도 동작해요.

::: code-group

```c [app.c]
if (zeropty_listen(a, "/tmp/myapp.sock") != 0) { /* could not open it */ }
```

```sh [터미널]
ZEROPTY_SOCK=/tmp/myapp.sock zeropty capture-pane -p -t '=w:'
ZEROPTY_SOCK=/tmp/myapp.sock zeropty attach -t w
```

:::

- 경로는 103바이트 이하여야 해요. 그 경로에서 이미 서버가 답하면 `zeropty_listen`은 -1을 돌려줘요.
- 소켓에서 보낸 `kill-server`는 세션만 끝내요. 내 프로그램은 계속 실행돼요.
- 세션은 여전히 내 프로그램과 함께 끝나요. 남기려면 끝나기 전에 독립 서버로 `push-sessions` 하세요([세션 옮기기](/ko/guide/moving)).

## 확인하기

zeropty 폴더에서 각 예제를 빌드하고 실행하세요(3단계의 Rust 발췌는 `examples/polled.rs`를 줄인 거예요).

::: code-group

```sh [C]
cc examples/c/hello.c -Idist -Ldist -lzeropty_c -o /tmp/zeropty-hello
DYLD_LIBRARY_PATH=dist /tmp/zeropty-hello
```

```sh [Rust]
cargo run --release --example embed
cargo run --release --example polled
```

:::

C 프로그램은 요청마다 그 답을 이어서 출력해요. 처음 세 개는 `{"code":0,"out":"","err":""}`가 보여야 하고, `capture-pane`의 답에는 `hi from C`가 두 번 들어 있어요. 한 번은 친 대로 에코된 것이고, 한 번은 `cat`이 되돌려 출력한 거예요.

`embed`는 친 줄, 답, 세션 목록을 출력해요.

```text
echo "answer: $((6 * 7))"
answer: 42
demo 60x10
```

`polled`는 대기 결과(`NNN`은 밀리초)와 화면을 출력해요.

```text
wait-pane: code 0 after NNN ms
build done
```

## 문제가 생기면

| 증상 | 해결 |
|---|---|
| `Library not loaded: … libzeropty_c.dylib` | `DYLD_LIBRARY_PATH=dist`를 지정하거나, `-Wl,-rpath,"$PWD/dist"`로 링크하세요. |
| `zstd-sys` 빌드가 실패해요 | Xcode Command Line Tools를 설치하세요. |
| 프로그램이 끝나면 세션이 사라져요 | 세션은 내 프로세스 안에 있어요. 먼저 서버로 `push-sessions` 하세요. |
| polled 엔진에서 다른 스레드의 호출이 이상하게 동작해요 | 모든 호출은 루프의 스레드에서 해야 해요. 여러 스레드에서 부르려면 `zeropty_open`을 쓰세요. |
| 플러그인 명령이 내 루프를 잡고 있어요 | `zeropty_command_start`로 시작하고 `zeropty_call_poll`로 받으세요. [플러그인 작성하기](/ko/guide/plugins)를 보세요. |

## 다음 단계

- C ABI 의 모든 함수, 타입, 매크로는 [C ABI](/ko/reference/c-abi)를 보세요.
- Rust 크레이트와 rustdoc 만드는 법은 [Rust API](/ko/reference/rust-api)를 보세요.
- 임베딩을 소켓·CLI와 비교하려면 [zeropty를 다루는 네 가지 방법](/ko/guide/concepts/interfaces)을 보세요.
- 임베디드 엔진에 명령을 더하려면 [플러그인 작성하기](/ko/guide/plugins)를 보세요.
