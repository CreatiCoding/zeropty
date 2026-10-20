# 플러그인 내부

[English](../PLUGINS-INTERNALS.md) · **한국어**

[README](../../README.ko.md) · [문서 목록](README.md) · [사용법](USAGE.md) · [플러그인](PLUGINS.md) · [FAQ](FAQ.md) · [벤치마크](BENCHMARKS.md) · [설계](DESIGN.md) · [버전 정책](VERSIONING.md) · [릴리스](RELEASING.md)

zeropty를 고치거나 호스트 루프를 직접 짜는 사람을 위한 내용이에요. 플러그인을 쓰는 방법과 규칙은 [PLUGINS.md](PLUGINS.md)에 있어요.

## 구조

플러그인 계층은 엔진 위, 소켓 서버 옆에 있어요. 의존은 아래로만 가요(`src/lib.rs`의 계층 표).

```
embed (Embedded, Polled)         ─┐
zeropty serve (src/cli/serve.rs) ─┴→ host::HostLoop ──┬→ Service: server::Server   (소켓, plugin-attach, 플러그인 명령 라우팅)
                                                       └→ Service: plugin::Plugins  (등록부: 이벤트 배달, 플러그인이 시킨 명령, 감시)
                                                                  ↓
                                   engine (engine::tap: 원하는 종류의 세션 이벤트만 큐에 넣어요)
```

- **엔진** (`src/engine/tap.rs`): 세션 생성·종료·이름 변경·출력을 이벤트로 큐에 넣어요. 어떤 플러그인도 원하지 않는 종류는 마스크 분기 하나로 건너뛰고 아무것도 만들지 않아요. `Event`, `ExitReason`은 여기 정의된 순수 데이터라 엔진은 위 계층을 몰라요. `zeropty::plugin`이 같은 타입을 다시 내보내요.
- **등록부** (`Plugins`): 호스트 루프의 `Service`예요. 서버와 똑같은 자리(`host::Service`)에 붙어서 `HostLoop`이 매 바퀴 서버 다음에 돌려요(`after_tick` = 이벤트 배달·플러그인이 시킨 명령 실행·감시). 플러그인 스레드가 루프를 깨우는 이벤트(`Plugins::attach`로 엔진 큐에 단 사용자 이벤트)는 서비스의 `owns`로 걸러져서 호스트 몫으로 새지 않아요. 잠들 시간은 서버의 `wait_timeout`과 함께 `Plugins::watchdog_ms`로 묶여요.
- **서버** (`server::Server::set_plugins`): `list-plugins`, 플러그인 명령 라우팅(답을 기다리는 건 그 연결뿐), `plugin-attach`(연결을 플러그인으로 넘김)를 맡아요.
- 그래서 세 호스트(`zeropty serve`, `Embedded`, `Polled`)가 같은 순서로 플러그인을 돌리고, 순서는 `host.rs`의 단위 테스트(`plugins_are_pumped_after_the_service_and_cap_the_wait`)가 가짜 엔진으로 확인해요.

## 플러그인이 없을 때의 비용

무엇을 더 하는지는 [PLUGINS.md](PLUGINS.md#플러그인이-없을-때의-비용)에 있어요.

플러그인이 없을 때 실제로 재 본 값이에요. 2026-10-20, 같은 Mac mini(M1)에서 플러그인이 없는 `main` 015f4da와 이 브랜치 c7ef81f(`src/embed.rs`의 `Polled::timeout_ms` next_due, `command_start`, `process_within`이 지금 모양인 커밋)를 번갈아 3번씩 돌렸어요(부하 평균 2.5\~5.0이라 절대값보다 두 열을 비교해요). 범위는 3번 실행의 최소\~최대예요(hostloop_bench는 실행마다 마지막 세 라운드라 9개 값).

| 측정 | `main` 015f4da | c7ef81f |
|---|---|---|
| polled 한 바퀴, 조용한 세션(`process()` + `timeout_ms()`, 중앙값) | 12.58\~12.63 us | 12.58\~12.61 us |
| polled 한 바퀴, claude 출력을 재생하는 세션 | 12.54\~13.26 us | 12.60\~12.94 us |
| `Polled::command` display-message | 446\~481 ns | 446\~470 ns |
| `Embedded::command` display-message | 455\~492 ns | 457\~485 ns |
| 임베디드 capture-pane, 조용한 세션, p50 | 4.3\~5.0 us | 3.7\~4.4 us |
| 임베디드 capture-pane -e -S -200, 조용한 세션, p50 | 4.7\~5.5 us | 4.7\~5.4 us |
| 임베디드 capture-pane, 출력 중인 세션(`BUSY=1`), p50 | 5.7\~11.7 us | 6.0\~13.5 us |
| 임베디드 send-keys cooked, p50 | 40.0\~42.5 us | 41.0\~45.0 us |
| 임베디드 send-keys raw, p50 | 30.8\~37.9 us | 32.9\~35.5 us |

모든 차이가 같은 빌드를 다시 돌린 흔들림 안에 있어요(범위가 서로 겹치고, 출력 중인 세션의 capture-pane처럼 흔들림이 큰 줄은 실행마다 순서가 바뀌어요). 다시 재려면 두 빌드에서 번갈아 돌려요.

이 표는 c7ef81f에서 쟀어요. 그 뒤 커밋들이 엔진에서 바꾼 건 `src/engine/session.rs`의 `if stats.tap` 안쪽과, 그 안에서만 불리는 `src/engine/tap.rs`의 `#[cold]` 함수(`copy`, `read_now`: 출력을 원하는 플러그인이 있을 때만 도는 64KB 조각 나누기와 바이트 예산)뿐이라, 플러그인이 없을 때 도는 코드는 c7ef81f와 같아요(`git diff c7ef81f..HEAD -- src/engine`).

```sh
cargo run --release --example hostloop_bench    # polled 한 바퀴와 명령 경로(N, ROUNDS로 조절, 라운드마다 중앙값)
cargo run --release --example capture_bench     # BUSY=1이면 출력 중인 세션
cargo run --release --example sendkeys_bench
```

`main`에서 재려면 `git archive main | tar -x -C <dir>`로 푼 뒤 `examples/hostloop_bench.rs`를 복사해 넣어요(`main`에는 없어요). 위 표는 각 실행의 마지막 세 라운드(앞 두 라운드는 데우기)예요.
