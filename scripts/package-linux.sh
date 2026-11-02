#!/bin/sh
# Packs the Linux files a GitHub Release carries (docs/RELEASING.md) into dist/release/:
#   zeropty-<version>-linux-<arch>.tar.gz   one folder of that name: zeropty, shim.so (for LD_PRELOAD),
#                                           zeropty-tty, LICENSE-APACHE, LICENSE-MIT
#   SHA256SUMS                              for `sha256sum -c SHA256SUMS`
# Built on the oldest glibc it should run on (.github/workflows/release.yml uses Debian 11, glibc 2.31):
# a binary needs the glibc it was linked against or newer. Then it unpacks the archive somewhere else and
# runs it from there: a session, and a program started through zeropty-tty that sees a terminal.
set -eu
cd "$(dirname "$0")/.."

version="$(cargo pkgid -p zeropty | sed 's/.*[#@]//')"
name="zeropty-$version-linux-$(uname -m)"
out=dist/release
stage="$out/$name"
rm -rf "$out"
mkdir -p "$stage"

cargo build --release --locked -p zeropty --bin zeropty
cp target/release/zeropty "$stage/"
cc -shared -fPIC -O2 -Wall -o "$stage/shim.so" shim/shim.c -ldl
cp zeropty-tty LICENSE-APACHE LICENSE-MIT "$stage/"
chmod 755 "$stage/zeropty" "$stage/zeropty-tty"
chmod 644 "$stage/shim.so" "$stage/LICENSE-APACHE" "$stage/LICENSE-MIT"

fail() {
	echo "package: $*" >&2
	exit 1
}
# The newest glibc symbol each file needs: what the oldest Linux it runs on must have.
for f in zeropty shim.so; do
	echo "$f needs glibc $(objdump -T "$stage/$f" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -Vu | tail -1)"
done

tar -C "$out" --owner 0 --group 0 --numeric-owner -czf "$out/$name.tar.gz" "$name"
(cd "$out" && sha256sum "$name.tar.gz" >SHA256SUMS)

# ---------------------------------------------------------------- smoke test of the archive
tmp="$(mktemp -d)"
export ZEROPTY_SOCK="$tmp/s" ZEROPTY_CACHE="$tmp/cache"
# The builder's own shim must not stand in for the one next to zeropty-tty.
unset ZEROPTY_SHIM
dir="$tmp/$name"
z="$dir/zeropty"
cleanup() {
	"$z" kill-server >/dev/null 2>&1 || true
	rm -rf "$tmp"
}
trap cleanup EXIT
cp "$out/SHA256SUMS" "$out/$name.tar.gz" "$tmp/"
(cd "$tmp" && sha256sum -c SHA256SUMS >/dev/null) || fail "SHA256SUMS does not match"
tar -C "$tmp" -xzf "$tmp/$name.tar.gz"
test "$(cd "$dir" && LC_ALL=C ls -A | tr '\n' ' ')" = "LICENSE-APACHE LICENSE-MIT shim.so zeropty zeropty-tty " ||
	fail "unexpected files: $(ls -A "$dir")"
test -x "$dir/zeropty-tty" || fail "zeropty-tty lost its execute bit"
test "$("$z" -V)" = "zeropty $version" || fail "-V: $("$z" -V)"

# A session must print `want`: probe sees a terminal or not.
session() {
	s=$1 want=$2
	shift 2
	"$z" new-session -d -s "$s" "$*"
	"$z" wait-pane -t "=$s:" -m TTY- -T 10000 >/dev/null || fail "$s: no output: [$("$z" capture-pane -p -t "=$s:" 2>&1)] $("$z" ls 2>&1)"
	got=$("$z" capture-pane -p -t "=$s:" | grep TTY- || true)
	test "$got" = "$want" || fail "$s: $got, not $want"
}
printf '#include <stdio.h>\n#include <unistd.h>\nint main(void) { puts(isatty(0) ? "TTY-YES" : "TTY-NO"); fflush(stdout); pause(); }\n' >"$tmp/probe.c"
cc -o "$tmp/probe" "$tmp/probe.c"
session raw TTY-NO "$tmp/probe"
session tty TTY-YES "$dir/zeropty-tty" "$tmp/probe"
echo "packed: $out/$name.tar.gz"
cat "$out/SHA256SUMS"
