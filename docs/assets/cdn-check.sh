#!/bin/sh
# Checks that every image README.md and README.ko.md load from https://cdn.creco.dev/zeropty/ is served
# (HTTP 200).
# README points at a new name when a picture is redrawn (docs/assets/webp.sh), and the maintainer
# uploads the file from docs/assets/ by hand; until then the front page shows a broken image. scripts/check.sh
# runs this as its last step (skipped with --quick); run it by hand before merging a change that renames a
# picture, once the maintainer has uploaded the new file.
# usage: sh docs/assets/cdn-check.sh [file.md ...]   (default: README.md README.ko.md)
set -e
root=$(dirname "$0")/../..
[ $# -gt 0 ] || set -- "$root/README.md" "$root/README.ko.md"
urls=$(grep -oh 'https://cdn\.creco\.dev/zeropty/[A-Za-z0-9._-]*' "$@" | sort -u)
[ -n "$urls" ] || { echo "no CDN images in $*"; exit 1; }
bad=0
for u in $urls; do
  code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 20 "$u" || echo 000)
  echo "$code $u"
  if [ "$code" != 200 ]; then
    bad=$((bad + 1))
    f=$(basename "$u")
    [ -f "$(dirname "$0")/$f" ] && echo "    upload docs/assets/$f to the CDN"
  fi
done
[ $bad -eq 0 ] || { echo "$bad image(s) not on the CDN yet"; exit 1; }
