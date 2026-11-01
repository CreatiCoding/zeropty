#!/bin/sh
# Packs the files a GitHub Release carries (docs/RELEASING.md) into dist/release/:
#   zeropty-<version>-darwin-universal.tar.gz   one folder of that name: zeropty (arm64 + x86_64),
#                                               shim.dylib (arm64, arm64e, x86_64), zeropty-tty,
#                                               LICENSE-APACHE, LICENSE-MIT
#   SHA256SUMS                                  for `shasum -a 256 -c SHA256SUMS`
# Both binaries load on macOS 11 and later, whatever macOS builds them (MACOSX_DEPLOYMENT_TARGET).
# .github/workflows/release.yml runs it on each tag. It runs the same on any Mac with both Rust targets:
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
# Then it unpacks the archive somewhere else and runs it from there: a session, and a program started
# through zeropty-tty that sees a terminal (the shim found next to it, with no absolute path), on x86_64
# too when Rosetta is there.
set -eu
cd "$(dirname "$0")/.."

export MACOSX_DEPLOYMENT_TARGET=11.0
version="$(cargo pkgid -p zeropty | sed 's/.*[#@]//')"
name="zeropty-$version-darwin-universal"
out=dist/release
stage="$out/$name"
rm -rf "$out"
mkdir -p "$stage"

for target in aarch64-apple-darwin x86_64-apple-darwin; do
	cargo build --release --locked -p zeropty --bin zeropty --target "$target"
done
lipo -create -output "$stage/zeropty" target/aarch64-apple-darwin/release/zeropty target/x86_64-apple-darwin/release/zeropty
clang -dynamiclib -O2 -arch arm64 -arch arm64e -arch x86_64 -o "$stage/shim.dylib" shim/shim.c
cp zeropty-tty LICENSE-APACHE LICENSE-MIT "$stage/"
chmod 755 "$stage/zeropty" "$stage/zeropty-tty"
chmod 644 "$stage/shim.dylib" "$stage/LICENSE-APACHE" "$stage/LICENSE-MIT"
# Ad-hoc: arm64 runs nothing unsigned, and lipo leaves the x86_64 slice as the linker made it.
codesign --force -s - "$stage/zeropty" "$stage/shim.dylib"

fail() {
	echo "package: $*" >&2
	exit 1
}
for f in zeropty:"arm64 x86_64" shim.dylib:"arm64 arm64e x86_64"; do
	file="$stage/${f%%:*}"
	lipo "$file" -verify_arch ${f#*:}
	for a in ${f#*:}; do
		minos=$(vtool -arch "$a" -show-build "$file" | awk '/minos/ { print $2 }')
		test "$minos" = "$MACOSX_DEPLOYMENT_TARGET" || fail "${f%%:*} $a needs macOS ${minos:-?}, not $MACOSX_DEPLOYMENT_TARGET"
	done
done

# No ._ files or extended attributes, and no builder's user name in the archive.
COPYFILE_DISABLE=1 tar -C "$out" --no-xattrs --uid 0 --gid 0 --uname root --gname wheel -czf "$out/$name.tar.gz" "$name"
(cd "$out" && shasum -a 256 "$name.tar.gz" >SHA256SUMS)

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
(cd "$tmp" && shasum -a 256 -c SHA256SUMS >/dev/null) || fail "SHA256SUMS does not match"
tar -C "$tmp" -xzf "$tmp/$name.tar.gz"
test "$(cd "$dir" && LC_ALL=C ls -A | tr '\n' ' ')" = "LICENSE-APACHE LICENSE-MIT shim.dylib zeropty zeropty-tty " ||
	fail "unexpected files: $(ls -A "$dir")"
test -x "$dir/zeropty-tty" || fail "zeropty-tty lost its execute bit"
test "$("$z" -V)" = "zeropty $version" || fail "-V: $("$z" -V)"

# A session must print `want`: probe sees a terminal or not. Not sh: macOS drops DYLD_INSERT_LIBRARIES
# for its own binaries, so the shim needs a program of ours.
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

# The x86_64 slices: native on Intel; on Apple silicon only where Rosetta is installed.
if [ "$(uname -m)" = x86_64 ]; then
	echo "x86_64 slices: ran natively"
elif arch -x86_64 /usr/bin/true 2>/dev/null; then
	test "$(arch -x86_64 "$z" -V)" = "zeropty $version" || fail "x86_64 -V: $(arch -x86_64 "$z" -V)"
	# An x86_64-only probe runs under Rosetta and loads the shim's x86_64 slice.
	cc -arch x86_64 -o "$tmp/probe-x86_64" "$tmp/probe.c"
	session tty-x86_64 TTY-YES "$dir/zeropty-tty" "$tmp/probe-x86_64"
	echo "x86_64 slices: ran under Rosetta"
else
	echo "x86_64 slices: not run (no Rosetta here)"
fi
echo "packed: $out/$name.tar.gz"
cat "$out/SHA256SUMS"
