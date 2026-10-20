#!/bin/sh
# Every check a change must pass before it reaches main, run on this Mac. There is no hosted CI: this
# script is it, and .githooks/pre-push runs it before each push (CONTRIBUTING.md).
#
#   scripts/check.sh            all checks (a few minutes; the first MSRV check compiles from scratch)
#   scripts/check.sh --quick    skips the slow or networked ones: examples, MSRV, rustdoc, CDN images,
#                               the git-cliff draft
#
# Commit messages, CHANGELOG entries and breaking API lines are checked for the commits not yet on the
# base: $ZEROPTY_CHECK_BASE if set, else the branch's upstream, else origin/main.
# Stops at the first failing step and names it. Tests run with ZEROPTY_REQUIRE_ALL=1: a test that would
# skip for a missing tool (cc, node, python3) fails instead, so a green run means every test ran.
set -eu
cd "$(dirname "$0")/.."
# From a git hook in a linked worktree (CONTRIBUTING.md suggests one), git exports GIT_DIR for this
# repository. It would override `git -C <dir>` in the tests that build a scratch repository, which then
# commit here instead.
unset $(git rev-parse --local-env-vars)

quick=no
for a in "$@"; do
  case $a in
    --quick) quick=yes ;;
    -h | --help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "check.sh: unknown argument $a (try --help)" >&2; exit 2 ;;
  esac
done

current=""
trap '[ -n "$current" ] && printf "\n✗ check failed: %s\n" "$current" >&2' EXIT
step() {
  current=$1
  printf '\n== %s\n' "$1"
}
skip() { printf '  (skipped: %s)\n' "$1"; }

base=${ZEROPTY_CHECK_BASE:-$(git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null || echo origin/main)}
git rev-parse --verify --quiet "$base^{commit}" >/dev/null || base=""
# A rewritten history (e.g. main squashed into one commit) shares no commit with the old base: there is
# nothing to compare commit by commit, so those checks are skipped, loudly.
if [ -n "$base" ] && ! git merge-base "$base" HEAD >/dev/null 2>&1; then
  echo "check.sh: $base and HEAD share no history (rewritten?); commit, changelog and API-removal checks against it are skipped" >&2
  base=""
fi

step "build (./build.sh: workspace, release, dist/)"
./build.sh

step "tests (workspace, release, doctests; ZEROPTY_REQUIRE_ALL=1)"
ZEROPTY_REQUIRE_ALL=1 cargo test --release --workspace --locked

step "versions agree"
node scripts/release.mjs check
if tag=$(git describe --exact-match --tags HEAD 2>/dev/null); then
  node scripts/release.mjs check --tag "$tag"
fi

step "public API matches api/*.txt"
node scripts/check-api.mjs
[ -n "$base" ] && node scripts/check-api.mjs --base "$base"

step "release tooling tests"
node --test scripts/test/*.test.mjs

step "version smoke tests (CLI, server, C ABI, Node)"
scripts/smoke-version.sh

step "clippy (deny warnings)"
cargo clippy --workspace --all-targets --locked -- -D warnings

step "rustfmt"
cargo fmt --all --check

step "links between the docs (files and #anchors)"
node scripts/check-links.mjs

step "commit messages and changelog"
if [ -z "$base" ]; then
  skip "no base to compare with (set ZEROPTY_CHECK_BASE)"
elif [ -z "$(git rev-list "$base..HEAD")" ]; then
  skip "no commits after $base"
else
  node scripts/check-commits.mjs --changelog --range "$base..HEAD"
fi

if [ $quick = yes ]; then
  current=""
  printf '\n✓ quick checks passed (run without --quick for examples, MSRV, rustdoc, CDN images)\n'
  exit 0
fi

step "rustdoc (warnings are errors)"
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --locked

step "examples"
tmp=$(mktemp -d)
sh examples/shell/quickstart.sh
node examples/node/quickstart.mjs
node examples/node/embedded.mjs
cc examples/c/hello.c -Idist -Ldist -lzeropty_c -o "$tmp/zeropty-hello"
DYLD_LIBRARY_PATH=dist "$tmp/zeropty-hello"
cargo run --release --locked --example embed
cargo run --release --locked --example polled
rm -rf "$tmp"

step "MSRV (Rust 1.88.0)"
# Keep in step with rust-version in Cargo.toml.
if rustup run 1.88.0 rustc --version >/dev/null 2>&1; then
  rustup run 1.88.0 cargo check --workspace --all-targets --locked
else
  echo "Rust 1.88.0 is not installed: rustup toolchain install 1.88.0" >&2
  exit 1
fi

step "git-cliff draft for [Unreleased]"
node scripts/release.mjs notes >/dev/null

step "README images on the CDN"
sh docs/assets/cdn-check.sh

current=""
printf '\n✓ all checks passed\n'
