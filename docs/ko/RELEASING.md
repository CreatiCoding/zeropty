# 릴리스

[English](../RELEASING.md) · **한국어**

[README](../../README.ko.md) · [문서 목록](README.md) · [사용법](USAGE.md) · [플러그인](PLUGINS.md) · [FAQ](FAQ.md) · [벤치마크](BENCHMARKS.md) · [설계](DESIGN.md) · [버전 정책](VERSIONING.md) · [릴리스](RELEASING.md)

변경이 커밋에서 CHANGELOG를 거쳐 릴리스가 되기까지의 과정이에요. 무엇이 깨는 변경인지는 [VERSIONING.md](VERSIONING.md), 커밋 형식은 [CONTRIBUTING.md](CONTRIBUTING.md)에 있어요.

## 한눈에 보기

```
커밋 (Conventional Commits) ──▶ PR: scripts/check.sh가 커밋 형식과 CHANGELOG [Unreleased]를 확인
                                     │
                                     ▼  main에 병합
릴리스 PR: node scripts/release.mjs bump <단계>  ──▶ 버전을 모든 곳에서 한 번에 바꾸고
                                                    [Unreleased]를 ## [X.Y.Z] - 날짜로 옮김
                                     │
                                     ▼  병합 후, 관리자가 태그 vX.Y.Z (release.mjs check --tag가 태그와 버전을 대조)
```

- **변경 기록의 원천은 커밋이에요.** 모든 커밋은 [Conventional Commits](https://www.conventionalcommits.org/ko/v1.0.0/) 형식이고(`scripts/check.sh`와 `.githooks/commit-msg`가 확인), 그 타입으로 릴리스 단계가 정해져요.
- **CHANGELOG.md는 사람이 읽는 글이에요.** [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/) 형식으로, 사용자에게 보이는 변경(`feat`, `fix`, `perf`, `revert`, 깨는 변경)은 같은 PR에서 `## [Unreleased]` 아래에 한 줄씩 적어요. `node scripts/release.mjs notes`가 커밋에서 초안을 만들어 줘요([git-cliff](https://git-cliff.org), 설정은 `cliff.toml`).
- CHANGELOG 항목은 문서처럼 영어로 쓰고, 같은 항목을 한국어 사본 [CHANGELOG.md](CHANGELOG.md)에도 적어요. git-cliff 초안은 커밋 메시지에서 나오니, 그걸 읽고 사용자에게 맞는 문장으로 다시 써요. 섹션 이름(`Added`, `Changed` ...)과 코드·식별자는 두 파일 모두 그대로예요.
- **문장은 사람이, 구조는 기계가 맞춰요.** PR마다 `scripts/check.sh`(`check-commits.mjs --changelog`)가 `[Unreleased]`에 새로 생긴 항목의 섹션을 git-cliff 초안의 섹션과 대조해요(아래 [반자동인 이유](#반자동인-이유)).
- **공개 인터페이스는 기계로도 확인해요.** `api/c-abi.txt`(`libzeropty_c.dylib`의 `zeropty_*` 심볼과 `zeropty.h` 선언)와 `api/node.txt`(Node 모듈 내보내기)가 기준선이에요. `scripts/check.sh`가 빌드 결과와 비교하고(`node scripts/check-api.mjs`), PR에서 기준선의 줄이 사라지면 깨는 변경 표시(`!`·`BREAKING CHANGE:`)와 `**BREAKING**` 항목을 요구해요([VERSIONING.md](VERSIONING.md#기계-검사)).
- 이 저장소의 도구는 **커밋, 태그, push, 배포를 하지 않아요.** 파일만 바꾸고, 나머지는 사람이 확인하고 해요.

## 반자동인 이유

CHANGELOG를 git-cliff 출력 그대로 쓰지 않고, 초안을 사람이 사용자에게 맞게 다시 써요. 일부러 고른 방식이에요.

- **독자가 달라요.** 커밋 메시지는 코드를 고치는 사람에게 쓰는 한 줄이고, CHANGELOG는 zeropty를 쓰는 사람에게 쓰는 문서예요. 커밋 제목 나열로는 "무엇을 바꿔야 하는지"가 빠져요.
- **여러 커밋이 한 항목이 돼요.** 기능 하나가 `feat` 커밋 여러 개로 나뉘거나, 아직 릴리스하지 않은 기능을 같은 주기에 `fix`로 고치면, 초안에는 여러 줄이지만 사용자에게는 한 줄이에요.
- **대신 빠뜨리거나 엉뚱한 곳에 적는 실수는 `scripts/check.sh`가 막아요.** `check-commits.mjs --changelog`는 PR의 커밋을 `cliff.toml`과 같은 규칙(`scripts/lib/conventional.mjs`의 `sectionsFor`)으로 분류하고, `[Unreleased]`에서 **새로 생기거나 바뀐 항목**이 그 섹션에 있는지 봐요.

| 커밋 | git-cliff 초안의 섹션 | 검사가 요구하는 새 항목 |
|---|---|---|
| `feat` | Added | `### Added` 아래 |
| `fix` | Fixed | `### Fixed` 아래 |
| `fix(security)` | Security | `### Security` 아래 |
| `perf`, `revert` | Changed | `### Changed` 아래 |
| 깨는 변경(`!`, `BREAKING CHANGE:`) | Changed | `### Changed`나 `### Removed` 아래 `- **BREAKING** ...` |
| `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `style` | (빠짐) | 없음 |
| 아무 type + `Changelog: skip` 꼬리말 (깨는 변경 제외) | (빠짐) | 없음 |

- `[Unreleased]`의 제목은 `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`만, 이 순서로, 한 번씩 쓸 수 있어요. 항목은 모두 `###` 제목 아래에 있어야 해요. `ZEROPTY_SKIP_CHANGELOG=1`이어도 항상 확인해요.
- 문장은 비교하지 않아요. 항목은 초안을 옮기지 않고 다시 쓰니, 섹션만 맞으면 통과예요.
- 두 분류 규칙(`cliff.toml`과 `sectionsFor`)이 어긋나면 `scripts/test/cliff.test.mjs`가 실패해요. 실제 git-cliff로 커밋을 렌더링해서 각 줄의 섹션을 `sectionsFor`와 비교해요.
- 아직 릴리스하지 않은 기능(`[Unreleased]`에 있는 기능)을 고친 `fix`처럼 사용자에게 새 소식이 아닌 커밋은 커밋 메시지 끝에 `Changelog: skip` 꼬리말을 달아요. 그 커밋만 검사와 git-cliff 초안에서 빠지고, 같은 PR의 다른 커밋은 그대로 검사돼요. 필요하면 기존 항목의 문장을 고쳐요. 깨는 변경에는 쓸 수 없어요(커밋 검사가 거부해요).
- PR 전체가 사용자에게 보이지 않는데 type이 `feat`/`fix`라면 PR 설명에 그렇게 적고, 관리자가 `ZEROPTY_SKIP_CHANGELOG=1 scripts/check.sh`로 항목 검사를 건너뛰어요. 이때도 커밋 형식 검사는 해요.

## 버전이 적힌 곳

`release.mjs bump`가 아래를 한 번에 바꾸고, `tests/version.rs`·`bindings/c` 단위 테스트·`release.mjs check`가 서로 같은지 확인해요.

| 파일 | 내용 |
|---|---|
| `Cargo.toml` | `[workspace.package] version`. 세 크레이트가 `version.workspace = true`로 물려받아요 |
| `Cargo.lock` | `zeropty`, `zeropty-c`, `zeropty-node` 항목 |
| `bindings/c/include/zeropty.h` | `ZEROPTY_VERSION_MAJOR`·`MINOR`·`PATCH`·`PRERELEASE`·`IS_PRERELEASE`·`STRING` (`NUMBER`는 계산돼요) |
| `client/zeropty.mjs`, `client/embedded.mjs` | `export const VERSION` |
| `CHANGELOG.md` | `## [X.Y.Z] - YYYY-MM-DD`와 맨 아래 비교 링크 |

`zeropty --version`, 서버의 `#{version}`, `zeropty_version()`, Node의 `version()`은 빌드할 때 Cargo 버전에서 나와서 따로 고칠 곳이 없어요.

`bump`는 한국어 사본 [CHANGELOG.md](CHANGELOG.md)를 건드리지 않아요. 릴리스 섹션을 다듬을 때(아래 4단계) 같은 제목과 항목을 그곳에도 손으로 옮겨요.

CHANGELOG 맨 아래 링크는 **있는 태그만** 가리켜요. 이전 버전의 태그가 있으면 `compare/vA...vB`, 아직 없으면 `[Unreleased]` 링크가 시작하는 커밋에서 시작해요(지금은 0.1.0 커밋 `ddd8811`). 없는 태그를 가리키는 링크가 생기면 `node --test scripts/test`가 실패해요.

## 명령

```sh
node scripts/release.mjs check              # 모든 곳의 버전이 같은지 (scripts/check.sh도 실행)
node scripts/release.mjs check --tag v0.2.0 # 그리고 태그가 그 버전인지 (태그가 달린 HEAD에서 scripts/check.sh도 실행)
node scripts/release.mjs next               # 마지막 태그 이후 커밋으로 계산한 다음 버전
node scripts/release.mjs notes              # [Unreleased] 초안 (git-cliff, 없으면 npx로 받아요)
node scripts/release.mjs bump minor --dry-run
node scripts/release.mjs bump <major|minor|patch|auto|X.Y.Z[-pre]> [--date YYYY-MM-DD] [--generate]
node scripts/check-commits.mjs --range origin/main..HEAD --changelog   # scripts/check.sh의 커밋·변경 기록 검사만 (섹션 대조 포함)
scripts/smoke-version.sh                    # dist/의 CLI·서버·C·Node 버전이 같은지 (scripts/check.sh의 version smoke tests 단계)
rustup run 1.88.0 cargo check --workspace --all-targets --locked   # MSRV (scripts/check.sh의 MSRV 단계)
node scripts/check-api.mjs                  # dist/(./build.sh)의 C ABI·Node 내보내기가 api/*.txt와 같은지
node scripts/check-api.mjs --base origin/main   # 기준선에서 사라진 줄이 깨는 변경으로 표시됐는지 (PR 검사)
node scripts/check-api.mjs --update         # 인터페이스를 일부러 바꿨을 때 기준선 갱신 (같은 커밋에)
node --test scripts/test/*.test.mjs         # 도구 자체의 테스트 (git-cliff 렌더링 포함)
```

- `bump auto`는 `next`가 계산한 단계를 써요. 1.0 이전에는 깨는 변경이 있으면 MINOR, 아니면 PATCH예요.
- `bump`는 먼저 `check`가 통과하는지, 작업 트리가 깨끗한지 보고, 하나라도 실패하면 아무 파일도 바꾸지 않아요.
- `[Unreleased]`가 비어 있으면 멈춰요. `--generate`를 주면 git-cliff 초안으로 채워요(그래도 읽고 고쳐 써요).
- 버전을 바꾼 뒤 `cargo metadata --locked`로 `Cargo.lock`이 맞는지 확인해요.

## 릴리스 절차

1. **main에서 `scripts/check.sh`가 통과하는지 확인해요.** `--quick` 없이 모든 단계(MSRV, 예제, rustdoc 포함)가 통과해야 해요.
2. **단계를 정해요.**
   ```sh
   git switch main && git pull
   node scripts/release.mjs next
   ```
   계산 결과를 [VERSIONING.md](VERSIONING.md)의 규칙과 맞춰 봐요. MSRV를 올렸거나 `build`·`refactor` 커밋이 사용자에게 보이는 변화를 냈다면 계산에 안 잡히니 직접 단계를 골라요.
3. **릴리스 브랜치에서 버전을 올려요.**
   ```sh
   git switch -c release/0.2.0
   node scripts/release.mjs bump minor      # 또는 auto, 0.2.0, 0.2.0-rc.1
   ```
4. **CHANGELOG를 다듬어요.** `## [0.2.0]` 섹션을 사용자 입장에서 읽어요.
   - 같은 일을 한 줄로 묶고, 내부 사정은 빼요.
   - 깨는 변경은 `**BREAKING**`과 함께 무엇을 바꿔야 하는지 적어요.
   - 성능 수치를 적는다면 [BENCHMARKS.md](BENCHMARKS.md)에 측정된 값만 써요.
   - 같은 변경을 한국어 사본 [CHANGELOG.md](CHANGELOG.md)에도 해요.
5. **확인해요.**
   ```sh
   cargo build --release --workspace && cargo test --release --workspace && ./build.sh
   dist/zeropty --version                   # zeropty 0.2.0
   node scripts/release.mjs check
   ```
6. **커밋하고 PR을 올려요.** 커밋 메시지는 `chore(release): 0.2.0`이에요(git-cliff가 이 커밋은 CHANGELOG에서 빼요). PR 제목도 같게 해요.
7. **병합한 뒤 관리자가 태그를 만들어요.**
   ```sh
   git switch main && git pull
   git tag -a v0.2.0 -m "zeropty 0.2.0"
   git push origin v0.2.0
   ```
   push하기 전에 `node scripts/release.mjs check --tag v0.2.0`으로 태그와 버전이 같은지 확인해요(태그가 달린 HEAD에서는 `scripts/check.sh`도 이 검사를 해요). GitHub Release를 만든다면 본문에 CHANGELOG의 `## [0.2.0]` 섹션을 그대로 붙여요.

> 0.1.0은 첫 공개 커밋(`ddd8811`)의 상태이고 아직 태그가 없어요. 관리자가 공개할 때 **그 커밋에** 첫 태그를 달아요(`git tag -a v0.1.0 ddd8811 -m "zeropty 0.1.0"`). 그 전까지 `next`·`notes`는 저장소의 처음부터 계산하고(첫 커밋은 Conventional Commits 형식이 아니어서 빠져요), CHANGELOG 링크는 그 커밋을 가리켜요. 태그를 단 뒤에는 `[Unreleased]` 링크를 `compare/v0.1.0...HEAD`로 바꿔도 되고, 다음 `bump`가 알아서 바꿔요.

## 포맷

워크스페이스 전체가 rustfmt 형식이에요(설정은 `rustfmt.toml`). 진행 중인 브랜치가 main에 다 들어간 뒤 `style: cargo fmt --all` 커밋(`e12f84f`) 하나로 한 번에 포맷했어요.

- `scripts/check.sh`의 rustfmt 단계가 `cargo fmt --all --check`로 트리 전체를 확인해요. 커밋 전에 `cargo fmt --all`을 돌려요.
- 포맷 커밋 앞에서 갈라진 오래된 브랜치는 충돌한 파일에서 자기 쪽을 고르고 `cargo fmt --all`을 다시 돌리면 돼요. 포맷은 기계적이라 결과가 같아요.
- `git blame`에서 포맷 커밋을 건너뛰려면 `git blame --ignore-rev e12f84f <파일>`로 봐요.

## 프리릴리스와 패치 릴리스

- **프리릴리스:** `node scripts/release.mjs bump 0.2.0-rc.1`. 태그는 `v0.2.0-rc.1`. 정식 릴리스는 `bump patch`(`0.2.0-rc.1` → `0.2.0`)나 `bump 0.2.0`이에요. 헤더의 `ZEROPTY_VERSION_PATCH`와 `ZEROPTY_VERSION_NUMBER`는 숫자만 담아서 `0.2.0-rc.1`과 `0.2.0`이 같아요. 둘은 `ZEROPTY_VERSION_PRERELEASE`(`"rc.1"`, 정식은 `""`)와 `ZEROPTY_VERSION_IS_PRERELEASE`(`#if`로 쓸 수 있는 1/0), `ZEROPTY_VERSION_STRING`(`"0.2.0-rc.1"`)으로 구별해요.
- **이전 버전 수정(backport):** 태그에서 브랜치를 만들어요(`git switch -c release/0.1.x v0.1.0`). 수정 커밋을 cherry-pick하고, `[Unreleased]`에 적고, `bump patch`로 같은 절차를 따라요. main의 CHANGELOG에도 그 버전 섹션을 옮겨 적어요.

## 문제가 생기면

| 증상 | 해결 |
|---|---|
| `tests/version.rs` 실패 | 버전을 손으로 고쳤어요. `node scripts/release.mjs check`로 어긋난 곳을 보고, `bump`로 다시 맞춰요 |
| `scripts/check.sh`의 commit messages 단계: 커밋 형식 | `git rebase -i`로 메시지를 고쳐요(`reword`). fixup 커밋은 `--autosquash`로 합쳐요 |
| `scripts/check.sh`의 commit messages 단계: changelog | `## [Unreleased]`에 줄을 더해요. 사용자에게 안 보이는 변경이면 `ZEROPTY_SKIP_CHANGELOG=1 scripts/check.sh`로 항목 검사를 건너뛰어요 |
| `needs an entry under ### Fixed for: ...` | 항목을 다른 섹션에 적었어요. 커밋 type에 맞는 섹션으로 옮겨요([반자동인 이유](#반자동인-이유)의 표). `node scripts/release.mjs notes`가 git-cliff의 분류를 보여줘요 |
| `[Unreleased]: "### Fixes" is not one of ...` | 제목은 Keep a Changelog의 여섯 개만, `Added` → `Security` 순서로 써요 |
| 깨는 변경인데 `**BREAKING**` 줄이 없다고 실패 | `[Unreleased]`에 `- **BREAKING** ...` 줄을 더해요 |
| `check-api.mjs`: `api/*.txt does not match the build` | 인터페이스를 일부러 바꿨다면 `node scripts/check-api.mjs --update` 후 `api/`를 같이 커밋해요. 아니라면 바뀐 선언·내보내기를 되돌려요 |
| `check-api.mjs --base`: `public API removed or changed` | 기준선에서 줄이 사라졌어요(함수 제거, 시그니처 변경). 의도했다면 커밋에 `!`나 `BREAKING CHANGE:`, `[Unreleased]`에 `- **BREAKING** ...`을 더하고, 아니라면 새 이름으로 더하는 방식으로 바꿔요 |
| `scripts/check.sh`의 rustfmt 단계 실패 | `cargo fmt --all`로 포맷하고 커밋해요([포맷](#포맷)) |
| `scripts/check.sh`의 MSRV 단계: `Rust 1.88.0 is not installed` | `rustup toolchain install 1.88.0` |
| `bump`가 `nothing under ## [Unreleased]`로 멈춤 | 항목을 쓰거나 `--generate`를 줘요 |
| 태그를 잘못 만들었어요 | push 전이면 `git tag -d vX.Y.Z`. push한 뒤라면 지우지 말고 다음 PATCH를 내요 |
