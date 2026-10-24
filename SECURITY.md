# Security policy

**English** · [한국어](docs/ko/SECURITY.md)

## Supported versions

zeropty is still at 0.x. Security fixes go only into the latest version on the `main` branch.

| Version | Supported |
|---|---|
| latest `main` | ✅ |
| anything earlier | ❌ |

## Reporting a vulnerability

**Do not report security vulnerabilities as public issues.** Please use GitHub's private vulnerability reporting.

1. Open [https://github.com/CreatiCoding/zeropty/security/advisories/new](https://github.com/CreatiCoding/zeropty/security/advisories/new) (the repository's **Security** tab → **Report a vulnerability**).
2. Include the following:
   - The affected part (server socket, attach, shim, C ABI, Node module, session handover, ...)
   - How to reproduce it (a minimal command or code, if possible)
   - The expected impact (access to another user's sessions, privilege escalation, denial of service, ...)
   - The macOS version and zeropty commit you tested

If the link above does not open (when private reporting is not enabled yet), open an issue that leaves out the details of the vulnerability and says only "I need a channel for a security report". The maintainer will open a private channel and let you know in that issue. The maintainer enables private reporting under the repository's **Settings → Code security → Private vulnerability reporting** (GitHub offers this feature only for public repositories).

Only the maintainers can see a report. After receiving it, we share what we found and our plan in the advisory thread, and publish the advisory together with the fix. If you would rather not be credited in the public advisory, let us know.

## Scope

zeropty is a tool that runs programs **on one user's machine, with that user's privileges**. That makes these parts especially important:

- **The server socket.** The default `~/.zeropty/mux.sock` has 0600 permissions, and newly created directories have 0700. The server checks that the connecting side is the same user (or root), and refuses a socket directory that is another user's or that others can write; clients check that the server is the same user (or root) before sending anything. If another user can send commands, or receive them, that is a vulnerability.
- **Session handover.** `pull-sessions`, `push-sessions` and `restart-server` pass fds over the socket (SCM_RIGHTS).
- **The shim.** `zeropty-tty` injects the shim with `DYLD_INSERT_LIBRARIES`, and because of SIP and the hardened runtime it creates copies that differ only in their signature under `~/.zeropty/tty/`.
- **Input handling.** The code that parses socket requests (JSON), attach frames and session records.

Out of scope: what a program the user started inside a session does, and what an attacker who already has the same user's privileges can do (the same user can manage those sessions anyway).
