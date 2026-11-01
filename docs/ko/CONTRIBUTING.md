# 기여 안내

[English](../../CONTRIBUTING.md) · **한국어**

zeropty에 관심 가져 주셔서 고마워요. 버그 제보, 문서 고치기, 측정 결과 공유, 코드 기여 모두 환영해요.

- [시작하기 전에](#시작하기-전에)
- [빌드와 테스트](#빌드와-테스트)
- [코드 둘러보기](#코드-둘러보기)
- [변경을 만들 때](#변경을-만들-때)
- [커밋 메시지: Conventional Commits](#커밋-메시지-conventional-commits)
- [변경 기록: CHANGELOG.md](#변경-기록-changelogmd)
- [성능을 바꾸는 변경](#성능을-바꾸는-변경)
- [문서](#문서)
- [Pull Request](#pull-request)

## 시작하기 전에

- **공개 저장소예요.** 커밋에 비밀값, 토큰, 개인 경로(`/Users/이름/...`)나 이름, 내부 호스트 이름, 확인하지 않은 실제 세션 녹화를 넣지 마세요. 녹화한 화면에는 녹화한 사람의 프롬프트와 폴더가 보여요. 커밋 전에 `git diff --cached`를 꼭 보세요.
- **목표는 모든 측정 항목에서 tmux보다 10배.** "tmux보다 낫다"로는 끝이 아니에요. 목표를 쉬운 쪽으로 바꾸지 않아요.
- **특정 사용자나 프로그램을 위한 코드는 넣지 않아요.** 프로그램들은 tmux를 쓰듯 zeropty를 써요. claude는 선택 사항이고, 테스트와 벤치마크는 `cat`, `printf` 같은 평범한 프로그램으로 돌아가야 해요.
- 코드 규칙, 측정 방법, 10배 목표의 원문은 [AGENTS.md](../../AGENTS.md)에 있어요(에이전트와 사람에게 같은 규칙이에요).
- 큰 변경은 먼저 [이슈](https://github.com/CreatiCoding/zeropty/issues)로 방향을 맞추면 좋아요.
- 보안 문제는 이슈로 올리지 말고 [보안 정책](SECURITY.md)을 따라 주세요.
- 모든 참여자는 [행동 강령](CODE_OF_CONDUCT.md)을 지켜요.

## 빌드와 테스트

macOS, Rust(stable, edition 2024. 지원하는 가장 낮은 버전(MSRV)은 1.88), Xcode Command Line Tools가 필요해요. 예제를 돌리려면 Node 18 이상이 있으면 좋아요.

```sh
git clone https://github.com/CreatiCoding/zeropty && cd zeropty
git config core.hooksPath .githooks   # 커밋할 때 메시지를, push 전에 scripts/check.sh를 돌려요 (node 필요)
./build.sh                  # dist/ 에 zeropty, shim, C·Node 바인딩, bash builtin
cargo test --release --workspace   # vt100 대조, 서버, 임베디드, 세션 옮기기, 플러그인, 문서 예제
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace   # API 문서: 경고가 없어야 해요
```

- `./build.sh`는 `dist/`의 바이너리를 지운 뒤 복사해요. macOS는 실행 중인 바이너리를 그 자리에서 덮어쓰면 그 프로세스를 죽여요.
- 화면 해석기를 고쳤다면 무작위 입력을 더 많이 돌려요: `SEEDS=20000 cargo test --release random_streams`
- 다른 세션이 쓰는 체크아웃을 건드리지 않도록 `git worktree`에서 작업하는 걸 권해요.
- 테스트는 병렬로 돌고 세션 프로세스를 동시에 띄워요. 한 번만 통과하는 걸로 끝내지 말고, 타이밍에 기대는 코드(소켓, 프로세스 종료, 대기)를 바꿨다면 전체 테스트를 여러 번 돌려 보세요. 예를 들어 리스너를 닫아도 그 순간 다른 스레드가 띄운 자식 프로세스가 exec 전까지 fd 사본을 들고 있어서, 같은 경로의 소켓이 잠깐 "살아 있는 서버"처럼 연결을 받을 수 있어요. 같은 이유로, fd 한쪽을 닫은 뒤 다른 쪽에서 끝(EOF·EPIPE)을 바로 확인하는 테스트는 닫기와 확인을 `sys::fd::no_spawns()` 가드 안에서 해요. 그동안에는 프로세스가 뜨지 않아서 닫은 fd의 사본이 남지 않아요(`engine::commands`의 `keys_and_buffers_reach_the_program` 참고, 측정은 `no_spawns`의 주석에).

## 코드 둘러보기

| 파일 | 하는 일 |
|---|---|
| `src/term/` | 화면 해석기 (`vt100` crate 0.16과 같은 결과) |
| `src/sys/` | OS 경계 (kqueue, fd, 프로세스, 시간, `tty`) |
| `src/engine/` | 엔진: 세션과 모든 명령. 새 명령은 `commands.rs`의 `COMMANDS`에 한 줄과 핸들러 하나 |
| `src/protocol.rs`, `src/server.rs` | 소켓 프로토콜(바이트)과 소켓 서버 |
| `src/host.rs`, `src/embed.rs` | 호스트 루프 한 바퀴, 호스트 안의 엔진 (자기 스레드 또는 호스트 루프) |
| `src/engine/handover.rs` | 프로세스 사이에 세션 옮기기 |
| `src/plugin.rs`, `src/plugin/` | 플러그인([docs/PLUGINS.md](PLUGINS.md)). 플러그인 작성자용 안정 API는 `api.rs`, 호스트용은 `Plugins`·`Call` |
| `src/main.rs`, `src/cli/` | `zeropty` 프로그램 (독립 서버, CLI, attach, `plugin-run`, `--help`, `--version`) |
| `shim/shim.c` | `isatty` 등을 가로채는 shim |
| `bindings/c`, `bindings/node`, `client/` | C ABI, Node 모듈, Node 클라이언트 |
| `tests/` | `tests/diff.rs`(vt100 대조), `tests/server.rs`, `tests/embed.rs`, `tests/handover.rs`, `tests/version.rs`(버전 번호가 모든 곳에서 같은지) |
| `api/`, `scripts/` | 공개 API 기준선(`api/*.txt`), 릴리스·검사 스크립트([docs/RELEASING.md](RELEASING.md)) |
| `bench/` | 벤치마크 |

구조 그림은 [설계](DESIGN.md#전체-구조)에 있어요.

## 변경을 만들 때

**정확성**

- 화면은 `vt100` crate(0.16)와 같아야 해요. `cargo test`가 녹화한 claude 세션과 무작위 입력을 둘 다에 넣고, 읽는 사이사이에 얼리기·녹이기·압축을 무작위로 섞어서 대조해요.
- 새 코드 경로를 넣었다면 일부러 한 번 망가뜨려(mutation) 테스트가 실패하는지 보세요. 실패하는 테스트가 없으면 테스트를 더해요. 확인한 뒤에는 되돌려요.
- `capture-pane` 출력은 바꾸려는 의도가 없다면 같은 화면에 대해 바이트까지 같아야 해요(저장해 둔 결과와 비교).
- 끝에서 끝까지: 할 수 있다면 `dist/claude-zeropty`로 실제 claude를 세션에 띄워 `send-keys`, `paste-buffer`, `capture-pane -e -S`, ONLCR(`echo` 줄이 0열에서 시작), `kill-session`을 확인해요.

**호환성**

- CLI 동작, 소켓 프로토콜, 세션 레코드, C ABI(`bindings/c/include/zeropty.h`), Node API, 환경변수, `dist/` 이름은 하위 호환을 지켜요(무엇이 호환 범위인지는 [docs/VERSIONING.md](VERSIONING.md)). 더하는 건 괜찮지만 바꾸거나 빼는 건 깨는 변경이라 이슈에서 먼저 이야기하고, 커밋에 깨는 변경 표시를 해요.
- C ABI나 Node 내보내기를 바꿨다면 `./build.sh && node scripts/check-api.mjs --update`로 `api/*.txt`를 같은 커밋에서 갱신해요.
- 버전 번호는 손으로 고치지 않아요. 릴리스할 때 `node scripts/release.mjs bump ...`가 모든 곳을 고쳐요([docs/RELEASING.md](RELEASING.md)).
- 인터페이스는 tmux의 명령 형식이에요. CLI, 소켓, 임베디드 라이브러리에서 똑같이 동작해야 해요.

**코드 스타일**

- 주석은 영어로, **왜** 그렇게 했는지, 함정이 무엇인지, 그 선택을 정당화한 측정값을 남겨요. 예: `// v2: 400 busy claude sessions took 3.5 MB against 4.5 MB with v3`
- 코드는 모두 rustfmt 형식이에요(설정은 `rustfmt.toml`). 커밋 전에 `cargo fmt --all`을 돌리고, `scripts/check.sh`가 `cargo fmt --all --check`로 확인해요([docs/RELEASING.md](RELEASING.md#포맷)).
- `cargo clippy --workspace --all-targets -- -D warnings`가 통과해야 해요. 크레이트 전체로 허용한 린트와 이유는 `Cargo.toml`에 있고, 새 예외는 그 자리에 `#[allow(...)]`와 이유를 달아요.
- 핫 패스(엔진의 tick·출력 처리, 화면 해석기, capture)에서는 동적 디스패치나 할당을 늘리지 마세요. 필요하면 제네릭·트레이트의 정적 디스패치를 써요.
- 공개 항목에는 문서 주석(`///`)을 달아요. crate에 `#![warn(missing_docs)]`가 켜져 있어요.
- 커밋 메시지는 영어로, [Conventional Commits](#커밋-메시지-conventional-commits) 형식으로 써요.

## 커밋 메시지: Conventional Commits

[Conventional Commits 1.0.0](https://www.conventionalcommits.org/ko/v1.0.0/)을 따라요. 메시지는 영어로 써요.

```
<type>(<scope>)!: <무엇을 했는지, 명령형 소문자로>

<왜 바꿨는지. 측정한 숫자가 있으면 그 숫자.>

BREAKING CHANGE: <깨는 변경이면, 사용자가 무엇을 바꿔야 하는지>
Refs: #123
```

- 첫 줄은 100자 이하, 끝에 마침표 없이. 본문과는 빈 줄로 나눠요.
- `!` 또는 `BREAKING CHANGE:` 꼬리말이 **깨는 변경**의 표시예요. 무엇이 깨는 변경인지는 [docs/VERSIONING.md](VERSIONING.md)에 있어요(CLI, 소켓 프로토콜, 세션 레코드, C ABI, Node API, 환경변수, 빌드 결과물).
- 한 커밋에는 한 가지 일만. 리팩터링과 동작 변경은 커밋을 나눠요.

| type | 언제 | CHANGELOG | 릴리스 단계 (1.0 이전 / 이후) |
|---|---|---|---|
| `feat` | 새 명령·플래그·함수·환경변수 | Added | PATCH / MINOR |
| `fix` | 버그 수정 (`fix(security)`는 보안 수정) | Fixed (Security) | PATCH / PATCH |
| `perf` | 더 빠르거나 작게, 동작은 같게 | Changed | PATCH / PATCH |
| `revert` | 이전 커밋 되돌리기 | Changed | PATCH / PATCH |
| `refactor` | 동작이 같은 코드 변경 | – | – |
| `docs` | 문서만 | – | – |
| `test` | 테스트만 | – | – |
| `build` | `build.sh`, Cargo 설정, 의존성 | – | – |
| `ci` | 검사 스크립트·훅 설정 (`scripts/check.sh`, `.githooks/`) | – | – |
| `chore` | 그 밖에 사용자가 보지 않는 것 (`chore(release): X.Y.Z`) | – | – |
| `style` | 포맷만 | – | – |
| 아무 type + `!` | 깨는 변경 | Changed, `**BREAKING**` | MINOR / MAJOR |

scope는 선택이고 소문자예요. 자주 쓰는 것: `cli`, `server`, `engine`, `term`, `embed`, `plugin`, `protocol`, `handover`, `c-abi`, `node`, `shim`, `shell`, `bench`, `release`, `ci`.

예 (가상의 변경이에요):

```
feat(cli): add --version as an alias of -V
fix(term): keep the cursor column after a wide character at the margin
perf(engine): read coalesced output in one kevent batch

400 replaying sessions: <before>% -> <after>% CPU (bench/suite.sh, interleaved with tmux, twice).
refactor(c-abi)!: take options as a struct in zeropty_open

BREAKING CHANGE: zeropty_open(const char *) is now zeropty_open(const zeropty_options *); pass NULL for defaults.
```

## 변경 기록: CHANGELOG.md

- [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/) 형식이에요. 사용자에게 보이는 변경(`feat`, `fix`, `perf`, `revert`, 깨는 변경)은 **같은 PR에서** `## [Unreleased]` 아래 알맞은 제목(Added, Changed, Deprecated, Removed, Fixed, Security)에 한 줄을 더해요. 제목은 커밋 type이 정해요: `feat` → Added, `fix` → Fixed(`fix(security)` → Security), `perf`·`revert` → Changed, 깨는 변경 → Changed나 Removed의 `**BREAKING**` 줄. `scripts/check.sh`가 git-cliff 초안과 같은 규칙으로 새 항목의 제목을 대조해요([docs/RELEASING.md](RELEASING.md#반자동인-이유)).
- 커밋 로그를 붙여 넣지 말고, 사용자가 알아야 할 것을 **영어로** 쓰고, 같은 항목을 한국어로 이 한국어 사본([CHANGELOG.md](CHANGELOG.md))에도 적어요. 초안은 `node scripts/release.mjs notes`가 (영어 커밋 메시지로) 만들어 줘요.
- C ABI나 Node 내보내기를 바꿨다면 `./build.sh && node scripts/check-api.mjs --update`로 `api/*.txt`를 갱신해 같은 커밋에 넣어요. 줄이 사라지면 깨는 변경이에요.
- 깨는 변경은 `- **BREAKING** ...`으로 시작하고, 무엇을 바꿔야 하는지 적어요.
- 성능 수치는 [docs/BENCHMARKS.md](BENCHMARKS.md)에 측정된 값만 써요.
- 사용자에게 보이지 않는 PR(`refactor`, `docs`, `test`, `ci` ...)은 적지 않아도 돼요. 아직 릴리스하지 않은 기능을 고친 커밋처럼 그 커밋만 적을 게 없으면 메시지 끝에 `Changelog: skip` 꼬리말을 달고(깨는 변경은 안 돼요), PR 전체가 그렇다면 PR 설명에 그렇게 적어요. 관리자가 `ZEROPTY_SKIP_CHANGELOG=1 scripts/check.sh`로 확인해요.
- 버전 번호와 `## [X.Y.Z]` 제목은 손대지 않아요. 릴리스할 때 `scripts/release.mjs`가 해요([docs/RELEASING.md](RELEASING.md)).

## 성능을 바꾸는 변경

메모리, CPU, 응답 속도 숫자를 움직이는 변경은 같은 커밋에서 [docs/BENCHMARKS.md](BENCHMARKS.md)를 갱신해요(날짜, 커밋, 기기, 배수).

- tmux와 zeropty는 **같은 부하에서 번갈아** 재요(`bench/latency.mjs`가 그렇게 해요). 하나씩 차례로 재면 이 기기에서 30% 넘게 흔들렸어요.
- 서버가 효율 코어에서 돌 때가 많아서 한 번 잰 값은 ±10\~15% 흔들려요. 결론을 내기 전에 반복하고, 메모리는 200개와 400개 사이의 세션당 증가분을 보는 게 좋아요.
- 먼저 사용자당 프로세스 한도(`sysctl kern.maxprocperuid`)를 확인해요. 남은 벤치마크 서버는 그 소켓에 `kill-server`로 끝내고 `ps -U $(id -u) | wc -l`로 확인해요.
- 프로파일: `sample <pid> 20 -file out.txt`. `zeropty stats`의 `heap_bytes`(할당기에 요청한 바이트)를 footprint와 비교하고, `zeropty mem -t <세션>`으로 화면 하나를 나눠 봐요.
- 가설은 측정으로 확인한 뒤에 고쳐요. "당연한" 원인 여러 개가 틀렸고, 프로파일이나 카운터가 진짜 원인을 보여 줬어요.
- BENCHMARKS.md에는 10배에 못 미친 항목(⛔)과 아직 안 잰 항목을 표시해요. 배수를 반올림해 올리거나 나쁜 줄을 빼지 않아요.
- README에는 tmux 대비 10배 이상으로 잰 항목만, 잰 범위 그대로 올려요. 10배 아래로 떨어진 항목은 README에서 빠져요.

재현 방법: [벤치마크: 측정 다시 하기](BENCHMARKS.md#측정-다시-하기)

## 문서

- README.md와 `docs/`의 본문은 영어로 써요. 한국어 번역은 [README.ko.md](../../README.ko.md)와 [docs/ko/](README.md)에 있고(해요체), 문서가 바뀌면 같은 섹션을 함께 고쳐 영어 파일과 맞춰요. 코드, 명령, 식별자는 두 언어 모두 영어 그대로예요.
- 문서를 고친 뒤 `node scripts/check-links.mjs`로 상대 링크와 `#앵커`가 모두 열리는지 확인해요(영어와 한국어 파일 모두).
- 문서에 넣는 명령과 코드는 실제로 돌려 보고 넣어요.
- 명령 레퍼런스는 [docs/USAGE.md](USAGE.md), 설계와 수치 근거는 [docs/DESIGN.md](DESIGN.md), 모든 측정값은 [docs/BENCHMARKS.md](BENCHMARKS.md)에 있어요.
- README의 그림(로고, 벤치마크, 구조)은 GitHub 모바일 앱에서도 보이도록 `https://cdn.creco.dev/zeropty/`의 절대 주소로 불러와요. 원본은 `docs/assets/`에 있고, `node docs/assets/make.mjs` 뒤 `sh docs/assets/webp.sh`로 다시 만들어요. 같은 이름으로 덮어쓰면 CDN 파일과 README가 따로 바뀌어 그 사이 첫 페이지에 옛 그림(옛 수치)이 보여요. 그래서 다시 그린 그림은 `webp.sh`의 `rev`를 올려 새 이름(예: `bench-v3-light.webp` → `bench-v4-light.webp`)으로 만들고 README의 `<picture>` 주소도 같은 커밋에서 바꿔요. 관리자가 새 파일을 CDN에 올린 뒤 병합해요. 그래야 첫 페이지 그림의 수치가 표와 어긋나는 때가 없어요. `sh docs/assets/cdn-check.sh`가 README의 CDN 그림이 모두 200으로 열리는지 확인하고, 아직 안 올라간 파일 이름을 알려 줘요. `scripts/check.sh`의 마지막 단계도 같은 검사를 해요(`--quick`이면 빠져요). 그림을 바꾼 PR은 관리자가 CDN에 올린 뒤 이 검사가 통과하는 것을 보고 병합해요.
- 터미널 데모는 `./build.sh` 뒤 `node docs/assets/demo.mjs`로 1분 시작을 실제로 돌려 녹화해요(Google Chrome과 `img2webp`가 필요해요). 만든 `demo-{light,dark}.webp`는 저장소에 커밋하지 않고 관리자가 CDN에 올린 뒤, 다른 그림처럼 절대 주소의 `<picture>`로 README에 넣어요.

## Pull Request

1. 브랜치를 만들어 작업해요(`main`에 바로 올리지 않아요, force-push 금지).
2. `scripts/check.sh`를 돌려 모두 통과하는지 확인해요. 저장소에 호스팅 CI는 없고, 이 스크립트가 main에 들어가기 전의 모든 검사예요(릴리스 묶음은 ci.creco.dev 파이프라인 `zeropty-release`가 `v*` 태그 push나 수동 실행 때 만들고, 문서 사이트는 `zeropty-docs`가 push·pull request마다 빌드해요). 관리자도 병합 전에 같은 스크립트를 돌려요. `git config core.hooksPath .githooks`를 해 두면 `.githooks/pre-push`가 push할 때마다 이 스크립트를 돌려요(`ZEROPTY_PRE_PUSH=quick`이면 `--quick`).

   ```sh
   scripts/check.sh            # 모든 검사 (몇 분 걸려요. 첫 MSRV 검사는 처음부터 컴파일해요)
   scripts/check.sh --quick    # 예제, MSRV, rustdoc, CDN 그림, git-cliff 초안을 빼고
   ```

   첫 단계부터 차례로 돌고, 실패하면 그 단계의 이름을 알려 주고 멈춰요.
   - **build:** `./build.sh`(워크스페이스 release 빌드, `dist/`).
   - **tests:** `ZEROPTY_REQUIRE_ALL=1 cargo test --release --workspace --locked`(doctest 포함). 도구(cc, node, python3)가 없어 건너뛰었을 테스트는 실패로 바뀌어서, 통과하면 모든 테스트가 돈 거예요.
   - **versions agree:** `node scripts/release.mjs check`. HEAD에 태그가 있으면 `--tag <태그>`로 태그와 버전도 대조해요.
   - **public API:** `node scripts/check-api.mjs`, 그리고 기준 브랜치가 있으면 `--base <기준>`(사라진 줄이 깨는 변경으로 표시됐는지).
   - **release tooling tests:** `node --test scripts/test/*.test.mjs`.
   - **version smoke tests:** `scripts/smoke-version.sh`(CLI·서버·C·Node 버전).
   - **clippy:** `cargo clippy --workspace --all-targets --locked -- -D warnings`.
   - **rustfmt:** `cargo fmt --all --check`.
   - **commit messages and changelog:** 기준 브랜치 뒤의 커밋이 모두 Conventional Commits인지, 사용자에게 보이는 커밋이 있으면 `[Unreleased]`에 그 type에 맞는 제목 아래 새 항목이 있는지(`node scripts/check-commits.mjs --changelog`).
   - 여기부터는 `--quick`이면 빠져요. **rustdoc:** `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace`(문서 없는 공개 항목, 깨진 문서 링크). **examples:** 셸·Node·C·Rust 예제 6개. **MSRV:** `rustup run 1.88.0 cargo check --workspace --all-targets --locked`(`rustup toolchain install 1.88.0`이 필요해요). **git-cliff draft:** `node scripts/release.mjs notes`가 초안을 만드는지. **README images:** `docs/assets/cdn-check.sh`.

   기준 브랜치는 `$ZEROPTY_CHECK_BASE`, 없으면 지금 브랜치의 upstream, 그것도 없으면 `origin/main`이에요.
3. 예제는 각자 임시 소켓의 서버나 프로세스 안의 엔진을 쓰고 끝날 때 정리해서, 쓰고 있는 세션을 건드리지 않아요.
4. PR은 squash 병합을 기본으로 해요. 그래서 **PR 제목이 main의 커밋 메시지**가 되니 Conventional Commits로 써요. 관리자가 병합 전에 `node scripts/check-commits.mjs --message "<제목>"`으로 제목(squash 메시지)을 확인해요.
5. PR 템플릿의 체크리스트를 채워요. 성능에 영향이 있다면 측정 전후 숫자를 붙여요.
6. 리뷰에서 나온 이야기는 커밋을 더해 반영해요.

기여한 코드는 저장소와 같은 [MIT](../../LICENSE-MIT) 또는 [Apache-2.0](../../LICENSE-APACHE) 라이선스로 배포돼요.
