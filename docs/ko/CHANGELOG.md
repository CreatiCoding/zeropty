# Changelog

[English](../../CHANGELOG.md) · **한국어**

zeropty의 사용자에게 보이는 변경을 모두 여기에 적어요.

형식은 [Keep a Changelog 1.1.0](https://keepachangelog.com/ko/1.1.0/)을 따르고(섹션 이름 `Added`·`Changed`·`Deprecated`·
`Removed`·`Fixed`·`Security`는 도구가 읽으므로 영어 그대로 써요), 버전은 [Semantic Versioning](https://semver.org/lang/ko/)을
따라요. 무엇이 깨는 변경인지(CLI, 소켓 프로토콜, C ABI, Node API, 환경변수 ...)는 [docs/VERSIONING.md](VERSIONING.md)에,
항목이 여기에 들어오는 과정은 [docs/RELEASING.md](RELEASING.md)에 있어요.

항목은 커밋 기록이 아니라 zeropty를 쓰는 사람에게 쓰는 글이에요. 사용자에게 보이는 변경은 같은 PR에서 `[Unreleased]` 아래에
한 줄을 더해요. `node scripts/release.mjs notes`가 마지막 릴리스 뒤의 Conventional Commits로 (영어) 초안을 만들어 주고,
그걸 다듬어 [영어 CHANGELOG.md](../../CHANGELOG.md)에 적고, 같은 항목을 한국어로 이 사본에 적어요. 깨는 변경은 `**BREAKING**`으로 시작해요.

## [Unreleased]

## [0.1.9] - 2026-10-25

### Security

- Node의 `attachPlugin`(`client/plugin.mjs`)도 이제 다른 사용자(root 제외)가 띄운 서버를 거절해요. CLI와 `zeropty.mjs`는
  0.1.8부터 그랬어요. 소켓이 다른 사용자도 닿는 디렉터리에 있으면(`ZEROPTY_SOCK`이나 `sock`이 `/tmp` 아래), 거기서 먼저
  기다린 다른 사용자가 플러그인의 명령을 알게 되고, 그 명령을 불러 결과를 읽을 수 있었어요.

## [0.1.8] - 2026-10-24

### Security

- 다른 로컬 사용자가 내 서버인 척할 수 있었어요. `HOME`이 없으면 소켓이 `/tmp/.zeropty/mux.sock`이었고, 누구든
  `/tmp/.zeropty`를 먼저 만들어 거기서 기다릴 수 있었어요. 그러면 CLI가 `send-keys` 글자까지 모든 명령을 그쪽에 보냈어요.
  이제 `HOME`이 없으면 홈 폴더를 사용자 데이터베이스(passwd)에서 가져오고, CLI, bash 내장 명령, `zeropty plugin-run`,
  `pull-sessions`/`push-sessions`, Node 클라이언트는 다른 사용자로 도는 서버(root 제외)를 거부해요. 서버는 다른 사용자의
  소켓 폴더나, 스티키 비트 없이 다른 사람이 쓸 수 있는 소켓 폴더를 tmux처럼 거부해요(누군가 소켓을 치울 수 있어요).
  새로 생긴 것: Rust 클라이언트용으로 확인을 거치는 연결 `zeropty::server::connect`.
- `ZEROPTY_PLUGIN_DIR`의 플러그인은 처음 찾을 때 한 번만 확인하고 그 뒤로는 경로로 다시 띄웠어요. 그래서 다른 사용자가
  (다른 사람이 쓸 수 있는 상위 폴더를 통해) 디렉터리를 바꿔치기하면 그 프로그램이 내 권한으로 돌았어요. 이제 띄울 때마다
  확인하고, `<이름>.log`는 0600으로 만들며 링크를 따라 열지 않아요.
- Linux의 `zeropty-tty`는 shim 경로에 공백이 있으면(`/opt/my tools/shim.so`) 로더가 경로의 일부를 각 프로그램의 폴더 기준
  상대 경로로 열어서, 거기 있는 아무 라이브러리나 불러왔어요. 이제 로더가 나누는 경로(Linux에서는 공백이나 콜론, macOS에서는
  콜론)는 거부하고, 상대 경로인 `ZEROPTY_SHIM`은 절대 경로로 바꿔요.
- 프로그램 출력이 서버를, 모든 세션과 함께 멈출 수 있었어요. 한 줄짜리 화면에서 줄이 넘어가거나 한 칸짜리 화면에 넓은 문자가
  오면 panic이 났고, `ESC[65535@` 하나에 0.25초 동안 바빴어요(이제 삽입 횟수는 화면 크기까지예요). 한 칸짜리 화면의 넓은
  문자는 tmux처럼 버려요.
- `send-keys -N`에 아주 큰 횟수를 주면 서버 메모리가 바닥나거나 panic이 났어요. 이제 `send-keys` 한 번에 16 MiB를 넘는 키는
  오류예요.
- 프로그램이 터미널을 non-blocking으로 만들고 가득 채운 뒤에는 shim이 모드 변경(비밀번호용 echo 끄기, raw 모드)을 잃어버리면서도
  `tcsetattr`은 성공이라고 답했어요. 이제 PTY처럼 자리가 날 때까지 기다려요.

## [0.1.7] - 2026-10-24

### Fixed

- Linux: 시그널만 보내는 키(`send-keys C-c` 등)가, 키 처리가 끝나기 전에 시그널로 프로그램이 끝나면 가끔
  "write: Broken pipe"로 실패하던 문제를 고쳤어요.

### Security

- 크기를 바꾼 세션의 크기 파일이 공유 디렉터리에 있으면 사용자를 공격하는 데 쓰일 수 있었어요. `ZEROPTY_SOCK`이 `/tmp`의
  소켓을 가리킬 때나, Linux에서 임베드 엔진의 기본 `size_dir`이 그래요. 크기는 `<파일>.new`를 거쳐 썼는데, 이 파일을
  심볼릭 링크를 따라가고 FIFO에서 기다리며 열었고, 이름은 디렉터리에서 보였어요. 그래서 다른 로컬 사용자가 그 자리에
  심볼릭 링크를 놓으면 사용자가 쓸 수 있는 아무 파일이나 덮어쓰게 할 수 있었고, FIFO를 놓으면 서버와 그 안의 모든 세션을
  멈출 수 있었어요. 이제 임시 파일은 새로 만들고(`O_EXCL`), 링크를 따라가거나 기다리지 않으며, 권한은 0600이에요.
  shim도 크기 파일을 읽을 때 심볼릭 링크를 따라가거나 FIFO에서 기다리지 않아요.

## [0.1.6] - 2026-10-24

### Added

- Linux 지원(x86_64, glibc 2.29 이상, 커널 5.3 이상). 엔진은 kqueue 대신 epoll에서 기다리고 `zeropty::engine` 타입은
  그대로예요. shim은 `LD_PRELOAD`(`shim.so`)로 걸려서, `zeropty-tty`가 동적 링크된 프로그램을 복사·서명 없이 그대로
  돌려요. 릴리스마다 `zeropty-<version>-linux-x86_64.tar.gz`(`zeropty`, `shim.so`, `zeropty-tty`, `claude-zeropty`)도
  붙고, 해시는 같은 `SHA256SUMS`에 있어요. 정적 링크된 프로그램(대부분의 Go 바이너리)은 shim을 불러오지 않아서 터미널을
  못 봐요.

## [0.1.5] - 2026-10-21

### Fixed

- `zeropty-tty`로 띄운 job control 셸(zsh, `bash -i`)에서 작업에 친 `C-c`, `C-\`, `C-z`가 작업이 아니라 셸로
  갔어요. 그래서 `sleep 30`을 멈출 수 없었어요. 작업은 자기 프로세스 그룹을 이끄는데, 셸의 `tcsetpgrp`를 shim이
  삼켰기 때문이에요. 이제 shim이 이를 알리고, zeropty는 tty가 포그라운드 그룹에 하듯 그 그룹에 시그널을 보내요.
  그래서 파이프라인을 포함해 `C-z`와 `fg`도 돼요. 세션 프로그램 아래에 멤버가 있는 프로세스 그룹만 받아들여요.
  업그레이드 전에 띄운 셸은 예전 shim을 그대로 쓰니 다시 띄워야 해요.

## [0.1.4] - 2026-10-21

### Fixed

- `zeropty-tty`: `ssh`, `ssh-keygen`, `ssh-add` 등 `readpassphrase`나 `getpass`를 쓰는 프로그램에 친 암호가 세션 화면에
  보였고, 그래서 `capture-pane` 결과와 스크롤백에도 남았어요. libc가 거기서 에코를 자기 내부 호출로 끄는데 shim이 그
  호출에 닿지 못했기 때문이에요. 이제 세션 터미널에서는 shim이 이 두 함수를 직접 처리해요. `zeropty-tty ssh-keygen`처럼 shim이 들어간
  프로그램에 해당하고, 세션 안 셸에서 친 `/usr/bin` 프로그램은 여전히 shim 없이 돌아요. 세션 밖에서는 바뀐 게 없어요.

## [0.1.3] - 2026-10-21

### Added

- 이제 릴리스마다 미리 빌드한 macOS 묶음 `zeropty-<버전>-darwin-universal.tar.gz`(Apple silicon·Intel, macOS 11 이상)와
  `SHA256SUMS`가 붙어서, Rust 없이 설치할 수 있어요. 받고, `shasum -a 256 -c SHA256SUMS`로 확인하고, 풀어서 그 폴더에서
  실행하면 돼요. `zeropty`, `shim.dylib`, `zeropty-tty`, `claude-zeropty`와 라이선스가 들어 있고 ad-hoc 서명돼
  있어요. 임베딩 라이브러리와 bash builtin은 여전히 `./build.sh`로 만들어요([README](../../README.ko.md#설치)).

## [0.1.2] - 2026-10-20

### Fixed

- `zeropty-tty`: `/dev/tty`를 읽기+쓰기로 연 프로그램(stdin을 파이프로 받은 `fzf`)은 키는 읽어도 쓰기가 모두 `EBADF`로
  실패해서, fzf가 목록을 그리지 못하고 선택도 받지 못했어요. 이제 세션의 터미널은 PTY가 파일 하나인 것처럼 소켓 하나예요.
  stdin, stdout, stderr가 모두 그 소켓이고(stdin은 파이프였어요), 어느 모드로 연 `/dev/tty`도 그 소켓이에요. `C-d`는
  여전히 입력을 아주 끝내고, 그 뒤에도 출력은 계속 나와요. PTY처럼 stdin, stdout, stderr 중 하나에 건 `O_NONBLOCK`은 이제
  셋 모두에 걸려요. 예전 서버가 시작해서 `restart-server`로 이어진 세션은 입력 파이프를 그대로 써요. 이 버전이 시작한
  세션을 0.1.1 이하로 되돌려 넘기지는 마세요(`C-d`가 닿지 않아요).

## [0.1.1] - 2026-10-20

### Fixed

- `zeropty-tty`: shim이 어느 프로그램이든 fd 0-2를 터미널로 쳐서, stdout을 파이프나 파일로 돌린 프로그램의 출력에
  `\e]zeropty-tty;N\a` 모드 신호가 섞이고, stdin이 `/dev/null`인 프로그램이 실패하고(kqueue `EINVAL`), 자식 프로세스도
  같은 영향을 받았어요. 이제 서버가 새 `ZEROPTY_TTY_ID` 변수로 알려 주는 세션 자신의 파이프와 소켓만 터미널로 치고,
  모드 신호는 세션 출력으로만 보내요.
- `zeropty-tty`: 서명한 사본이 링크 대상의 이름을 써서(`claude`가 `claude.exe`로 실행됨) `ps`, `pgrep claude`로
  찾을 수 없었어요. 이제 사본이든 아니든 프로그램은 실행할 때 쓴 이름으로 돌아요.
- `display-message`의 `-t`가 없는 세션을 가리키면 이제 tmux처럼 세션 변수를 비운 포맷을 출력하고 종료 코드 0이에요(전에는
  종료 코드 1로 실패). 세션이 있는지 확인하려면 `has-session`을 쓰세요. 다른 명령은 tmux처럼 여전히 종료 코드 1로
  실패해요.
- 없는 `-t` 대상에 대한 오류 문구를 tmux와 같게 맞췄어요. `:`가 없을 때 이름만 쓰면 `can't find pane: x`(`capture-pane`,
  `send-keys`, `pipe-pane`, `paste-buffer`, `resize-pane`)나 `can't find window: x`(`resize-window`), id는 `$`·`@`·`%`에
  따라 세션·창·pane 오류, `x.0`은 `can't find window: x`예요. `set-environment`는 `no such session: `과 받은 대상
  그대로예요. 종료 코드는 그대로(1)예요.

## [0.1.0] - 2026-10-20

첫 공개 버전이에요. 터미널 프로그램을 많이 다루는 프로그램(코딩 에이전트, 스크립트, 테스트 하네스)을 위한 세션 엔진으로,
tmux의 명령 형식을 PTY 없이 받아요. macOS 전용이에요. 첫 공개 커밋 `c612089`에
`v0.1.0` 태그를 달았어요.

알려진 한계: macOS만 돼요(kqueue, DYLD shim). 세션 하나에 프로그램 하나라서 창·패널·상태 줄·복사 모드·키 바인딩·설정
파일이 없어요. `wait-pane`은 임베디드 함수 호출로는 못 쓰고 소켓으로만 돼요(임베디드 `listen` 포함). SIP로 보호되거나
hardened runtime으로 서명된 프로그램은 shim을 못 읽어서 `zeropty-tty`가 다시 서명한 복사본을 실행하고, setuid·entitlement
프로그램(`top`, `ps`)은 안 돼요. `C-d`의 파일 끝은 세션 입력을 닫아요(파이프에는 한 번짜리 EOF가 없어요). 1.0 전에는 MINOR
릴리스가 docs/VERSIONING.md의 인터페이스를 깰 수 있어요.

### Added

- **서버와 CLI**(`zeropty`), tmux와 같은 인자: `new-session`, `has-session`, `kill-session`, `list-sessions`, `kill-server`,
  `send-keys`(키 이름을 tmux 3과 바이트 단위로 대조), `capture-pane`(`-p -e -J -N -S -E -b -q`), `load-buffer`,
  `paste-buffer`, `display-message`, `pipe-pane`, `rename-session`, `resize-window`/`resize-pane`,
  `set-environment -g`/`show-environment -g`, `attach-session`(`-r` 읽기 전용), `-V`, tmux 포맷 변수 25개. 모르는
  플래그는 오류예요. 첫 `new-session`이 서버를 띄워요.
- tmux에 없는 명령: `wait-pane`(폴링 없이 글자나 조용해짐을 기다림), `grep-panes`(모든 세션 화면 검색),
  `pull-sessions`/`push-sessions`(프로그램을 건드리지 않고 실행 중인 세션을 다른 프로세스로 옮김),
  `restart-server`(세션을 끝내지 않고 서버 업그레이드).
- **PTY 없음**: 세션의 프로그램은 파이프와 이야기하고, 줄 규칙(Enter, `C-c` 시그널, 줄 편집, 에코, `C-d`)은 엔진이
  흉내 내요. 터미널이 필요한 프로그램(claude, vim, less, python)에는 `shim.dylib`과 `claude-zeropty`, `zeropty-tty`가
  `isatty`, `tcgetattr`, 창 크기, `/dev/tty`에 답해요.
- **화면 에뮬레이터**: 칸당 6바이트이고, 녹화한 claude 세션과 무작위 입력으로 `vt100` 크레이트(0.16)와 대조해요.
  조용한 화면은 접어 두었다가 나중에 zstd로 압축해요.
- **소켓 프로토콜**: `~/.zeropty/mux.sock`(`ZEROPTY_SOCK`)에서 요청과 응답 한 줄씩 JSON, 선택할 수 있는 raw 응답 형식,
  attach 프레임.
- **임베드**: C ABI(`libzeropty_c.dylib`, `zeropty.h`: `zeropty_open`, `zeropty_command`, `zeropty_listen`, polled용
  `zeropty_open_polled`/`zeropty_fd`/`zeropty_timeout_ms`/`zeropty_process`)와 Node 모듈(`zeropty.node`,
  `client/embedded.mjs`)로 엔진을 프로세스 안에서 돌려요. `listen`은 같은 소켓 프로토콜을 호스트 프로세스에서 제공해요.
- **클라이언트**: `client/zeropty.mjs`(Node, 연결 하나를 유지), 프로세스를 띄우지 않고 소켓과 이야기하는 zsh 함수
  (`shell/zeropty.zsh`)와 bash builtin(`shell/zeropty_bash.c`).
- 환경변수 `ZEROPTY_SOCK`, `ZEROPTY_HISTORY`, `ZEROPTY_COALESCE_MS`, `ZEROPTY_FREEZE_MS`, `ZEROPTY_COLD`,
  `ZEROPTY_COLD_DIR`, `ZEROPTY_RECORD`(docs/USAGE.md).
- tmux 3.7c와 Mac mini M1 한 대에서 번갈아 잰 벤치마크(docs/BENCHMARKS.md): 잰 14개 항목 중 11개가 10배 이상이고, 3개는
  아직이에요(독립 서버의 대기 400개 메모리 6.2배, tmux 제어 모드 대비 `send-keys` 임베디드 8~10배·소켓 2.2~2.8배).
  이유는 그 문서에 적었어요.
- 플러그인 시스템([docs/PLUGINS.md](PLUGINS.md)): 세션 생성·종료·이름 변경·출력 이벤트를 받고 새 명령을 더해요.
  Rust `Plugin` 트레이트(`API_VERSION` 1), 소켓의 `plugin-attach`(언어 무관, 프로세스 밖), C 콜백
  `zeropty_add_plugin`, dylib 플러그인 `zeropty plugin-run`/`zeropty_load_plugin`, 옵트인 자동 시작 디렉터리
  `ZEROPTY_PLUGIN_DIR`, 상태 확인 `list-plugins`. 플러그인이 없으면 비용이 없고, 패닉·멈춤·느린 플러그인은 엔진을
  붙잡지 못해요.
- 기다리지 않는 명령 호출: C `zeropty_command_start`·`zeropty_call_poll`·`zeropty_call_due_ms`·`zeropty_call_free`,
  Node `Engine.commandStart`.
- `zeropty --version`: `zeropty -V`와 같은 출력(`zeropty 0.1.0`)이에요. zsh 함수, bash builtin, `client/zeropty.mjs`도
  `-V`처럼 프로그램을 실행해 답해요.
- 포맷 변수 `#{version}`: 실행 중인 서버(또는 임베디드 엔진)의 버전이에요. 이 변수만 쓰는 `display-message -p`는 세션이
  없어도 돼요(`zeropty display -p '#{version}'`).
- C ABI: `zeropty.h`에 `ZEROPTY_VERSION_MAJOR`·`_MINOR`·`_PATCH`·`_PRERELEASE`·`_IS_PRERELEASE`·`_STRING`·`_NUMBER`,
  링크된 라이브러리의 버전을 돌려주는 `zeropty_version()`. `ZEROPTY_VERSION_NUMBER`는 프리릴리스를 담지 않아서
  `0.2.0-rc.1`과 `0.2.0`이 같은 값이에요. 둘은 `ZEROPTY_VERSION_IS_PRERELEASE`로 구별해요.
- Node: `zeropty.node`와 `client/embedded.mjs`의 `version()`, `client/zeropty.mjs`와 `client/embedded.mjs`의 `VERSION`.
- 버전 정책 `docs/VERSIONING.md`: 깨는 변경으로 보는 호환 범위(CLI의 인자·출력·종료 코드, 소켓 프로토콜, 세션 레코드,
  C ABI, Node API, 환경변수, `dist/` 이름)와 MSRV(Rust 1.88). 1.0 전에는 MINOR 릴리스가 이 범위를 깰 수 있고, 그때는
  `**BREAKING**` 항목으로 알려요.
- `zeropty --help`(`-h`, `help`): 명령과 옵션 목록을, `zeropty help <명령>`은 그 명령의 사용법과 문서 링크를 출력해요. 서버 없이 답해요.
- 문서: 한국어 README 개편, 문서 목록(`docs/README.md`), FAQ, 실행되는 예제(`examples/`), 모든 공개 Rust 항목의 rustdoc과 예제.

### Fixed

- 출력을 읽는 쪽이 파이프를 일찍 닫아도(`zeropty capture-pane -p ... | head -1`) CLI가 패닉하지 않아요.

[Unreleased]: https://github.com/CreatiCoding/zeropty/compare/v0.1.9...HEAD
[0.1.9]: https://github.com/CreatiCoding/zeropty/compare/v0.1.8...v0.1.9
[0.1.8]: https://github.com/CreatiCoding/zeropty/compare/v0.1.7...v0.1.8
[0.1.7]: https://github.com/CreatiCoding/zeropty/compare/v0.1.6...v0.1.7
[0.1.6]: https://github.com/CreatiCoding/zeropty/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/CreatiCoding/zeropty/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/CreatiCoding/zeropty/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/CreatiCoding/zeropty/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/CreatiCoding/zeropty/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/CreatiCoding/zeropty/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/CreatiCoding/zeropty/releases/tag/v0.1.0
