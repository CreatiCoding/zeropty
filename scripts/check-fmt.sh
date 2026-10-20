#!/bin/sh
# rustfmt for the Rust files a change touches, not the whole tree:
#   - a file the range adds must be formatted;
#   - a file it modifies must stay formatted if it was formatted before (no regressions);
#   - a modified file that was not formatted before gets a warning only, so a fix there does not drag a
#     reformat of the whole file into the diff.
# Superseded: the whole tree is formatted since e12f84f and scripts/check.sh runs `cargo fmt --all --check`
# (docs/RELEASING.md). Kept for checking a range on a branch from before that commit. Messages use the
# `::error file=...::` annotation form.
#   scripts/check-fmt.sh <base> [<head>]     e.g. scripts/check-fmt.sh origin/main
set -eu
cd "$(dirname "$0")/.."
base=${1:?usage: check-fmt.sh <base> [<head>]}
head=${2:-HEAD}
from=$(git merge-base "$base" "$head")
fail=0
checked=0

# Whether the given revision's copy of a file is formatted. rustfmt --check on stdin exits 0 even when it
# prints a diff, so the formatted text is compared with the original instead.
formatted() {
  src=$(git show "$1:$2") || return 1
  out=$(printf '%s\n' "$src" | rustfmt --edition 2024 --config-path rustfmt.toml --emit stdout 2>/dev/null) || return 1
  [ "$src" = "$out" ]
}

for f in $(git diff --name-only --diff-filter=A "$from" "$head" -- '*.rs'); do
  checked=$((checked + 1))
  if ! formatted "$head" "$f"; then
    echo "::error file=$f::new file is not rustfmt-formatted (run: rustfmt $f)"
    fail=1
  fi
done
for f in $(git diff --name-only --diff-filter=M "$from" "$head" -- '*.rs'); do
  checked=$((checked + 1))
  if formatted "$head" "$f"; then
    continue
  elif formatted "$from" "$f"; then
    echo "::error file=$f::was rustfmt-formatted before this change and is not now (run: rustfmt $f)"
    fail=1
  else
    echo "::warning file=$f::not rustfmt-formatted yet; enforced once the tree-wide style commit lands (rustfmt.toml)"
  fi
done
[ "$fail" = 0 ] && echo "rustfmt: $checked Rust file(s) changed since $(git rev-parse --short "$from"); added and already-formatted ones are formatted"
exit "$fail"
