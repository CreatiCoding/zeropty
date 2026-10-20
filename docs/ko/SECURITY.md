# 보안 정책

[English](../../SECURITY.md) · **한국어**

## 지원하는 버전

zeropty는 아직 0.x예요. 보안 수정은 `main` 브랜치의 최신 버전에만 해요.

| 버전 | 지원 |
|---|---|
| `main` 최신 | ✅ |
| 그 이전 | ❌ |

## 취약점 알리기

**보안 취약점은 공개 이슈로 올리지 마세요.** GitHub의 비공개 취약점 제보(Private vulnerability reporting)를 써 주세요.

1. [https://github.com/CreatiCoding/zeropty/security/advisories/new](https://github.com/CreatiCoding/zeropty/security/advisories/new)를 열어요(저장소의 **Security** 탭 → **Report a vulnerability**).
2. 다음을 적어 주세요.
   - 영향을 받는 부분(서버 소켓, attach, shim, C ABI, Node 모듈, 세션 옮기기 등)
   - 재현 방법(가능하면 최소 명령이나 코드)
   - 예상되는 영향(다른 사용자의 세션 접근, 권한 상승, 서비스 거부 등)
   - 확인한 macOS 버전과 zeropty 커밋

위 링크가 열리지 않으면(비공개 제보 기능이 아직 꺼져 있을 때), 취약점 내용은 빼고 "보안 제보 경로가 필요해요"라고만 적은 이슈를 열어 주세요. 관리자가 비공개 경로를 열고 그 이슈에 알려 드려요. 비공개 제보 기능은 관리자가 저장소 **Settings → Code security → Private vulnerability reporting**에서 켜요(GitHub는 공개 저장소에서만 이 기능을 제공해요).

제보는 관리자만 볼 수 있어요. 받은 뒤 확인 결과와 계획을 그 권고(advisory) 스레드로 알려 드리고, 고친 뒤에 함께 공개해요. 제보자를 공개 권고에 올리고 싶지 않다면 알려 주세요.

## 범위

zeropty는 **한 사용자의 기기에서, 그 사용자의 권한으로** 프로그램을 돌리는 도구예요. 그래서 이런 부분이 특히 중요해요.

- **서버 소켓.** 기본 `~/.zeropty/mux.sock`은 0600 권한이고, 새로 만든 폴더는 0700이에요. 서버는 접속한 쪽이 같은 사용자(또는 root)인지 확인해요. 다른 사용자가 명령을 보낼 수 있다면 취약점이에요.
- **세션 옮기기.** `pull-sessions`, `push-sessions`, `restart-server`는 소켓으로 fd를 주고받아요(SCM_RIGHTS).
- **shim.** `zeropty-tty`는 `DYLD_INSERT_LIBRARIES`로 shim을 넣고, SIP·hardened runtime 때문에 서명만 바꾼 사본을 `~/.zeropty/tty/`에 만들어요.
- **입력 처리.** 소켓 요청(JSON), attach 프레임, 세션 레코드를 해석하는 코드.

범위 밖: 세션 안에서 사용자가 직접 실행한 프로그램이 하는 일, 같은 사용자 권한을 이미 가진 공격자가 할 수 있는 일(같은 사용자는 원래 그 세션들을 다룰 수 있어요).
