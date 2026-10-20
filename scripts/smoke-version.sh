#!/bin/sh
# Version smoke test on the built dist/ (run ./build.sh first): every way to ask for the version gives the
# workspace's. scripts/check.sh runs it after the build; it runs the same way on its own.
#   zeropty --version / -V, a running server's #{version}, display -p without #{version} (still an error
#   with no session), zeropty_version() and ZEROPTY_VERSION_* in C, version()/VERSION in Node.
set -eu
cd "$(dirname "$0")/.."

want="$(cargo pkgid -p zeropty | sed 's/.*[#@]//')"
tmp="$(mktemp -d)"
export ZEROPTY_SOCK="$tmp/s"
cleanup() {
	dist/zeropty kill-server >/dev/null 2>&1 || true
	node -e 'require("fs").rmSync(process.argv[1], { recursive: true, force: true })' "$tmp"
}
trap cleanup EXIT

fail() {
	echo "smoke-version: $*" >&2
	exit 1
}

test "$(dist/zeropty --version)" = "zeropty $want" || fail "--version: $(dist/zeropty --version)"
test "$(dist/zeropty -V)" = "zeropty $want" || fail "-V: $(dist/zeropty -V)"

printf '%s\n' \
	'#include <stdio.h>' '#include <string.h>' '#include "zeropty.h"' \
	'int main(void) {' \
	'  const char *v = zeropty_version();' \
	'  printf("c %s %d %d \"%s\"\n", v, ZEROPTY_VERSION_NUMBER, ZEROPTY_VERSION_IS_PRERELEASE, ZEROPTY_VERSION_PRERELEASE);' \
	'  return strcmp(v, ZEROPTY_VERSION_STRING) != 0;' \
	'}' >"$tmp/version.c"
clang -Wall -Werror -Idist -Ldist -lzeropty_c -Wl,-rpath,"$PWD/dist" -o "$tmp/version" "$tmp/version.c"
"$tmp/version" || fail "zeropty_version() != ZEROPTY_VERSION_STRING"
"$tmp/version" | grep -qF "c $want " || fail "C: $("$tmp/version") (want $want)"

node --input-type=module -e "
  import { version, VERSION } from './client/embedded.mjs'
  import { VERSION as CLIENT } from './client/zeropty.mjs'
  if (version() !== '$want' || VERSION !== '$want' || CLIENT !== '$want') { console.error(version(), VERSION, CLIENT); process.exit(1) }
  console.log('node', version())"

# A running server's #{version}, in a session and, once that is gone, with none: the server's own variable
# needs no session, but any other format still does (exit codes are public, docs/VERSIONING.md).
dist/zeropty new-session -d -s v 'exec cat'
test "$(dist/zeropty display-message -p -t v '#{session_name} #{version}')" = "v $want" || fail "#{version} in a session"
dist/zeropty kill-session -t v
test "$(dist/zeropty display-message -p '#{version}')" = "$want" || fail "#{version} with no session"
if dist/zeropty display-message -p 'hello' >/dev/null 2>&1; then fail "display -p 'hello' with no session exited 0"; fi
echo "versions agree on $want: CLI, server, C, Node"
