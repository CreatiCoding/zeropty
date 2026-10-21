#!/bin/sh
# Renders the README's SVGs to small 1x WebP. README loads them from
# https://cdn.creco.dev/zeropty/ rather than the repository, since
# the GitHub mobile app shows repository images as broken; upload after rerunning.
# Needs Google Chrome and cwebp (brew install webp). Run after `node make.mjs`.
#
# The CDN serves a file under the same name for good, so a redrawn picture gets a new name: bump
# its revision below, point README.md at the new name, and upload the new files before the change
# lands on main. An old name keeps showing the old picture (and its old numbers) on the front page.
# Names already on the CDN: logo-*, bench-*, how-* (first drawing), bench-v2-*, how-v2-*, bench-v3-*, bench-v4-*, how-v3-*. sh cdn-check.sh lists what is missing.
#
# Two languages: Korean (README.ko.md: bench-v4-light.webp, ...) and English (README.md: bench-v4-en-light.webp,
# from bench-en-light.svg). WEBP_LANGS="en" renders one of them only (default: "ko en").
set -e
rev() { case $1 in bench) echo -v4 ;; how) echo -v3 ;; esac; }   # logo: unchanged since the first upload
cd "$(dirname "$0")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
chrome="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
for lang in ${WEBP_LANGS:-ko en}; do
  sfx=""; [ "$lang" = en ] && sfx=-en
  for n in logo bench how; do
  for t in light dark; do
    f="$PWD/$n$sfx-$t.svg"
    w=$(sed -n 's/.*<svg[^>]* width="\([0-9.]*\)".*/\1/p' "$f" | head -1)
    h=$(sed -n 's/.*<svg[^>]* height="\([0-9.]*\)".*/\1/p' "$f" | head -1)
    bg=white; [ $t = dark ] && bg='#0d1117'
    # The mobile app drops <img> tags that carry width/height, so README sets no
    # width and GitHub scales each image to the column (~830px). The logo gets a
    # wider canvas so it still shows at about 440px.
    cw=$w; [ $n = logo ] && cw=980
    printf '<html><body style="margin:0;background:%s;text-align:center"><img src="file://%s" width="%s" height="%s" style="display:block;margin:0 auto"></body></html>' "$bg" "$f" "$w" "$h" > "$tmp/$n-$t.html"
    "$chrome" --headless=new --disable-gpu --user-data-dir="$tmp/prof" --hide-scrollbars --force-device-scale-factor=1 \
      --window-size=$cw,$h --screenshot="$tmp/$n-$t.png" "file://$tmp/$n-$t.html" >/dev/null 2>&1
    cwebp -quiet -q 85 -m 6 "$tmp/$n-$t.png" -o "$n$(rev $n)$sfx-$t.webp"
  done
  done
done
echo "wrote $(ls *.webp | tr '\n' ' ')"
