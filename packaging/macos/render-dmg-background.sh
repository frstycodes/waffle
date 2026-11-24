#!/bin/sh
# Renders dmg-background.html to dmg-background.png and dmg-background@2x.png. Run after changing the
# page or its art; the PNGs are committed, so CI only combines them (tiffutil, in release.yml).
set -e
cd "$(dirname "$0")"
chrome="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
for scale in 1 2; do
  out=dmg-background.png
  [ "$scale" = 2 ] && out=dmg-background@2x.png
  "$chrome" --headless --disable-gpu --hide-scrollbars --virtual-time-budget=4000 \
    --window-size=660,400 --force-device-scale-factor=$scale --screenshot="$PWD/$out" "file://$PWD/dmg-background.html"
done
