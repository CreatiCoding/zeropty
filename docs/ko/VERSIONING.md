# 버전 정책

[English](../VERSIONING.md) · **한국어**

[README](../../README.ko.md) · [문서 목록](README.md) · [사용법](USAGE.md) · [플러그인](PLUGINS.md) · [FAQ](FAQ.md) · [벤치마크](BENCHMARKS.md) · [설계](DESIGN.md) · [버전 정책](VERSIONING.md) · [릴리스](RELEASING.md)

zeropty는 [Semantic Versioning 2.0.0](https://semver.org/lang/ko/)을 따라요. 이 문서는 **무엇이 공개 인터페이스이고, 어떤 변경이 호환을 깨는지**를 정해요. 변경 내역은 [CHANGELOG.md](CHANGELOG.md), 릴리스 절차는 [RELEASING.md](RELEASING.md)에 있어요.

## 버전 하나

- 저장소 전체가 **버전 하나**를 써요. `zeropty`, `zeropty-c`, `zeropty-node` 크레이트는 `Cargo.toml`의 `[workspace.package] version`을 물려받고, C 헤더의 `ZEROPTY_VERSION_*`, Node 클라이언트의 `VERSION`, `CHANGELOG.md`도 같은 값이에요.
- 버전은 손으로 고치지 않고 `node scripts/release.mjs bump ...`로만 바꿔요. 어긋나면 `cargo test`(`tests/version.rs`)와 `scripts/check.sh`가 실패해요.
- 태그는 `v` + 버전이에요(`v0.1.0`). 프리릴리스는 `0.2.0-rc.1`처럼 써요.

실행 중에 버전을 확인하는 방법:

| 어디서 | 방법 | 결과 |
|---|---|---|
| CLI | `zeropty --version` 또는 `zeropty -V` | `zeropty 0.1.0` (CLI 프로그램 자신) |
| 실행 중인 서버 | `zeropty display-message -p '#{version}'` (tmux와 같은 포맷 변수. `#{version}`이 있고 세션 변수가 없는 포맷만 세션 없이 답하고, `-t` 없이 세션도 없으면 다른 포맷은 전처럼 종료 코드 1이에요) | `0.1.0` |
| C ABI | `zeropty_version()` (런타임), `ZEROPTY_VERSION_STRING`·`_MAJOR`·`_MINOR`·`_PATCH`·`_PRERELEASE`·`_IS_PRERELEASE`·`_NUMBER` (컴파일 타임) | `"0.1.0"`. `ZEROPTY_VERSION_NUMBER`는 `MAJOR*1000000 + MINOR*1000 + PATCH`라서 **프리릴리스를 담지 않아요**: `0.2.0-rc.1`과 `0.2.0`이 같은 값이에요. 구별은 `ZEROPTY_VERSION_PRERELEASE`(`"rc.1"`, 정식은 `""`)나 `#if ZEROPTY_VERSION_IS_PRERELEASE`로 해요 |
| Node (임베디드) | `version()` (`zeropty.node`, `client/embedded.mjs`) | `'0.1.0'` |
| Node (클라이언트 모듈) | `VERSION` (`client/zeropty.mjs`, `client/embedded.mjs`) | `'0.1.0'` |

헤더와 라이브러리는 같은 빌드에서 나와야 해요. 다른 MAJOR(1.0 이전에는 다른 MINOR)의 라이브러리는 헤더 선언과 맞지 않을 수 있으니, 임베드하는 쪽은 시작할 때 `zeropty_version()`을 `ZEROPTY_VERSION_STRING`과 비교하면 좋아요.

## 버전을 올리는 규칙

| 변경 | 1.0 이전 (지금, `0.MINOR.PATCH`) | 1.0 이후 |
|---|---|---|
| 아래 공개 인터페이스의 호환을 깨는 변경 | **MINOR** (`0.1.x` → `0.2.0`) | **MAJOR** |
| 기능 추가 (새 명령·플래그·함수·환경변수), 하위 호환 | PATCH | MINOR |
| 버그 수정, 성능 개선, 문서 | PATCH | PATCH |
| MSRV 올림, 지원 macOS 버전 변경 | MINOR | MINOR |

- 1.0 이전에는 Cargo와 같은 해석을 써요. `0.1.x`끼리는 서로 호환되고, `0.2.0`은 깨질 수 있어요.
- 커밋 메시지로 올릴 단계를 계산해요(`node scripts/release.mjs next`). `!`나 `BREAKING CHANGE:` 꼬리말이 있으면 깨는 변경, `feat`는 기능 추가, `fix`·`perf`·`revert`는 수정이에요. 자세한 형식은 [CONTRIBUTING.md](CONTRIBUTING.md)에 있어요.
- 깨는 변경은 CHANGELOG에 `**BREAKING**`으로 시작하는 줄과, 사용자가 무엇을 바꿔야 하는지를 적어요. `scripts/check.sh`가 확인해요.

## 공개 인터페이스 (호환을 지키는 범위)

아래를 바꾸거나 없애면 **깨는 변경**이에요. 새로 더하는 건(새 명령, 새 플래그, 새 함수, 새 JSON 필드, 새 환경변수) 깨는 변경이 아니에요.

### 1. CLI

- 명령 이름과 별칭(`new-session`/`new`, `attach-session`/`attach`/`a` ...), 플래그와 그 의미, tmux와 같은 인자 해석(`-t =이름:` 타깃 형식 포함).
- 종료 코드: 성공 0, 실패 1(`has-session` 없음, `wait-pane` 시간 초과, `grep-panes` 못 찾음, 모르는 옵션 ...), 인자 없이 실행하면 2.
- 프로그램이 읽는 출력 형식: `list-sessions` 기본 출력과 `-F`, `display-message -p` 포맷 변수, `new-session -P`, `grep-panes`의 `세션: 줄`, `zeropty --version`의 `zeropty X.Y.Z`.
- `capture-pane` 출력 바이트. 같은 화면이면 같은 바이트여야 해요. tmux와 다르게 나오던 걸 맞추는 건 버그 수정(`fix`)이고, 의도해서 형식을 바꾸는 건 깨는 변경이에요.
- 기본 소켓 경로(`~/.zeropty/mux.sock`)와 첫 `new-session`이 서버를 띄우는 동작.
- 사람이 보는 오류 문구(`err`)는 공개 인터페이스가 아니에요. 종료 코드로 판단해요.

### 2. 소켓 프로토콜

- 요청 한 줄: `{"argv": [...], "cwd": "...", "raw": bool}`. 응답 한 줄: `{"code", "out", "err"}`. 한 연결에 여러 요청, 답은 보낸 순서대로.
- `"raw": true` 응답 형식(`<code> <out 바이트 수> <err 바이트 수>\n` + 바이트).
- `attach`의 프레임 형식과 종류 값(`FRAME_OUTPUT`, `FRAME_ENDED`, `FRAME_INPUT`, `FRAME_RESIZE`, `FRAME_DETACH`).
- 새 요청 필드는 더할 수 있어요. 옛 클라이언트가 보내지 않아도 지금과 똑같이 동작해야 해요.

### 3. 세션 옮기기 레코드

- `pull-sessions`·`push-sessions`·`restart-server`가 주고받는 세션 레코드(`RECORD_VERSION`, 지금 1). **`restart-server`가 업그레이드 경로**라서, 새 버전 서버가 이전 버전 레코드를 못 읽으면 실행 중인 세션을 잃지 않고는 업그레이드할 수 없어요.
- 그래서 `RECORD_VERSION`을 올리면 깨는 변경이에요. 올려야 한다면 이전 레코드도 읽도록 하고, CHANGELOG에 어느 버전부터 어느 버전으로 `restart-server`가 되는지 적어요.

### 4. C ABI (`bindings/c/include/zeropty.h`, `libzeropty_c.dylib`)

- 함수 이름, 인자와 반환 타입, 의미, 스레드 규칙(`zeropty_open`은 아무 스레드, polled는 루프 스레드 하나).
- 메모리 소유권: `zeropty_command`의 결과는 `zeropty_free`로, `zeropty_version()`의 문자열은 해제하지 않음.
- 옵션 JSON의 키(`history`, `size_dir`)와 요청·응답 JSON(소켓 프로토콜과 같음).
- `zeropty`는 불투명 타입이에요. 크기와 내부 배치는 공개 인터페이스가 아니에요.
- 헤더의 `ZEROPTY_VERSION_*` 매크로 이름과 의미.
- 함수를 더하는 건 깨는 변경이 아니에요. 있는 함수의 시그니처를 바꾸는 대신 새 이름으로 더해요.

### 5. Node API

- `zeropty.node`의 내보내기: `Engine`(생성자 옵션 `history`, `sizeDir`, `polled`, `command`, `listen`), `Client`(`command`, `close`), `version()`.
- `client/zeropty.mjs`: `zeropty`, `zeroptySync`, `sockPath`, `cliPath`, `VERSION`와 결과 형태 `{ ok, code, out, err }`, 옵션(`cwd`, `sock`, `timeout`).
- `client/embedded.mjs`: `zeropty`, `zeroptySync`, `open`, `listen`, `version`, `VERSION`. 두 모듈은 import만 바꿔서 서로 바꿔 쓸 수 있어야 해요.
- 지원하는 Node: N-API 6 이상(`napi6`).

### 6. 환경변수

| 공개 (이름·의미·기본값을 지켜요) | 내부 (예고 없이 바뀔 수 있어요) |
|---|---|
| `ZEROPTY_SOCK`, `ZEROPTY_HISTORY`, `ZEROPTY_COALESCE_MS`, `ZEROPTY_FREEZE_MS`, `ZEROPTY_COLD`, `ZEROPTY_COLD_DIR`, `ZEROPTY_RECORD` ([USAGE.md](USAGE.md#환경변수)) | `ZEROPTY_COLS`, `ZEROPTY_ROWS`, `ZEROPTY_SIZE_FILE`, `ZEROPTY_TTY_ID` (엔진이 shim에 넘기는 값, 같은 빌드끼리만 써요) |
| `ZEROPTY_BIN`, `ZEROPTY_NODE` (클라이언트가 쓸 바이너리·모듈), `ZEROPTY_SHIM`, `ZEROPTY_CACHE`, `ZEROPTY_CLAUDE_REAL` (`zeropty-tty`, `claude-zeropty`) | `ZEROPTY_MALLOC_ADDED` (서버 재실행용), `ZEROPTY_PURGE_MS` (측정용) |
| `ZEROPTY_SESSION` (세션 안 프로그램이 읽는 세션 이름) | 테스트·벤치마크에서만 쓰는 나머지 `ZEROPTY_*` |

기본값을 바꾸는 것도 동작이 달라지면 깨는 변경이에요. 단, 성능 조정용 기본값(`ZEROPTY_COALESCE_MS`, `ZEROPTY_FREEZE_MS`)은 화면을 읽는 결과가 같다면 수정으로 봐요. 이때도 CHANGELOG의 Changed에 적어요.

### 7. 빌드 결과물과 셸 연동

- `./build.sh`가 `dist/`에 만드는 파일 이름: `zeropty`, `libzeropty_c.dylib`, `zeropty.h`, `zeropty.node`, `shim.dylib`, `zeropty_bash.dylib`, `claude-zeropty`, `zeropty-tty`.
- zsh 함수(`shell/zeropty.zsh`)와 bash builtin(`enable -f ... zeropty`)의 이름과 동작(CLI와 같아야 해요).

## 기계 검사

규칙을 사람의 눈에만 맡기지 않아요.

| 무엇을 | 어떻게 | 어디서 |
|---|---|---|
| C ABI (4절) | `nm -gU dist/libzeropty_c.dylib`의 `zeropty_*` 심볼과 `zeropty.h`의 선언(매개변수 이름·주석·줄바꿈은 빼고)·매크로 이름을 `api/c-abi.txt`와 비교 | `node scripts/check-api.mjs` (`scripts/check.sh`의 public API 단계) |
| Node API (5절) | `zeropty.node`, `client/zeropty.mjs`, `client/embedded.mjs`의 내보내기 이름과 종류, 클래스의 메서드를 `api/node.txt`와 비교 | 같음 |
| 깨는 변경 표시 | PR에서 `api/*.txt`의 줄이 사라지거나 바뀌면, 커밋(또는 PR 제목)에 `!`·`BREAKING CHANGE:`가 있고 `[Unreleased]`에 `- **BREAKING**` 줄이 새로 있어야 통과 | `node scripts/check-api.mjs --base <base>` (`scripts/check.sh`의 public API 단계) |
| 버전 일치 | 크레이트, `Cargo.lock`, 헤더 매크로, Node `VERSION`, `--version`, `#{version}`, CHANGELOG | `tests/version.rs`, `node scripts/release.mjs check` |
| MSRV | Rust 1.88.0으로 `cargo check --workspace --all-targets --locked` | `scripts/check.sh`의 MSRV 단계 |
| CLI·소켓 프로토콜·레코드 | 동작을 `tests/server.rs`, `tests/handover.rs`, `tests/embed.rs`가 실제 서버와 클라이언트로 확인 | `cargo test` |

새 함수나 내보내기는 기준선에 줄이 생길 뿐이라 깨는 변경이 아니지만, 빌드와 기준선이 다르면 `scripts/check.sh`가 실패하니 `node scripts/check-api.mjs --update`로 갱신한 `api/`를 같은 커밋에 넣어요. Rust 라이브러리 API는 공개 인터페이스가 아니라서(아래) `cargo-semver-checks`는 쓰지 않아요. 공개하기로 하면 그때 `scripts/check.sh`에 더해요.

## 공개 인터페이스가 아닌 것

- **Rust 라이브러리 API**(`zeropty::engine`, `zeropty::term`, `zeropty::server` ...). crates.io에 올리지 않았고, 바인딩과 서버가 같은 저장소에서 함께 바뀌어요. 1.0 전에 공개 여부를 따로 정해요.
- 성능 수치. 느려지는 건 호환 문제가 아니라 회귀로 다뤄요([AGENTS.md](../../AGENTS.md)의 10배 목표, [BENCHMARKS.md](BENCHMARKS.md)).
- `bench/`, `examples/`, `tests/`, `scripts/`의 내용과 진단 명령(`stats`, `mstats`, `mem`)의 출력 형식.
- 오류 문구, 로그, 레코드 파일(`ZEROPTY_RECORD`)의 내부 형식.

## 폐기(deprecation)

- 1.0 이후에는 없애기 전에 최소 한 번의 MINOR 릴리스 동안 폐기 예정으로 두어요. 그동안 계속 동작하고, CHANGELOG의 **Deprecated**에 대체 방법을 적어요. 다음 MAJOR에서 **Removed**로 없애요.
- 1.0 이전에는 MINOR 릴리스에서 바로 없앨 수 있지만, 할 수 있으면 같은 절차를 따라요.

## MSRV (최소 지원 Rust 버전)

- **Rust 1.88** (`Cargo.toml`의 `rust-version`). 의존성 `napi` 3이 1.88을 요구해요. edition은 2024예요.
- `scripts/check.sh`의 MSRV 단계가 정확히 1.88.0으로(`rustup run 1.88.0`) 워크스페이스 전체를 `cargo check --locked` 해요.
- MSRV는 MINOR 릴리스에서만 올리고, CHANGELOG의 Changed에 적어요. 올릴 때는 `Cargo.toml`의 `rust-version`, `scripts/check.sh`의 MSRV 단계, 이 문서를 같은 커밋에서 바꿔요.
- 플랫폼: macOS, 그리고 Linux x86_64(glibc 2.29 이상). 검사(`scripts/check.sh`)는 관리자의 Apple Silicon Mac에서 돌고, Linux 테스트는 CI(ci.creco.dev 파이프라인 `zeropty-release`의 `test-linux`)에서 돌아요.

## 판단 예시

| 변경 | 분류 | 커밋 |
|---|---|---|
| `capture-pane`에 `-C` 플래그 추가 | 기능 추가 | `feat(cli): support capture-pane -C` |
| `wait-pane` 시간 초과 종료 코드를 1에서 2로 | 깨는 변경 | `fix(cli)!: exit 2 when wait-pane times out` |
| 세션 없을 때 실패(1)하던 `display -p '#{version}'`이 서버 버전을 출력(0). 다른 포맷은 여전히 1 | 기능 추가 (전에 오류였던 입력만 새로 동작해요. 성공하던 입력의 결과는 그대로예요) | `feat(cli): add the #{version} format variable` |
| 실패(1)하던 `display -p -t =gone: '...'`이 tmux처럼 세션 변수를 비운 포맷을 출력(0) | 버그 수정 (오류였던 입력이 tmux처럼 동작해요. 성공하던 것은 그대로예요) | `fix(cli): answer display-message on a missing target as tmux does` |
| 이미 성공하던 명령의 종료 코드나 출력 형식이 바뀜 | 깨는 변경 | `feat(cli)!: ...` |
| C ABI에 `zeropty_command_len()` 추가 | 기능 추가 | `feat(c-abi): add zeropty_command_len` |
| `zeropty_open` 인자 추가 | 깨는 변경 (대신 새 함수 추가를 권해요) | `feat(c-abi)!: ...` |
| 응답 JSON에 `"pid"` 필드 추가 | 기능 추가 | `feat(protocol): ...` |
| `RECORD_VERSION` 2로, 1도 계속 읽음 | 기능 추가 | `feat(handover): ...` |
| `RECORD_VERSION` 2로, 1은 못 읽음 | 깨는 변경 | `feat(handover)!: ...` |
| 같은 화면에서 `capture-pane -e` 바이트가 tmux와 다르던 것 맞춤 | 수정 | `fix(term): ...` |
| 화면 해석 속도 2배 | 수정 (성능) | `perf(term): ...` |
| `ZEROPTY_SOCK` 이름 변경 | 깨는 변경 | `refactor!: ...` 또는 `feat!: ...` |
| MSRV 1.88 → 1.90 | MINOR | `build: raise MSRV to 1.90` |
