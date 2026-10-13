#!/bin/sh
# Renders packaging/icon/icon.html to AppIcon.icns with headless Chrome.
# Usage: packaging/icon/render.sh [variant]   (default: d, the chosen design)
set -eu
variant="${1:-d}"
dir="$(cd "$(dirname "$0")" && pwd)"
chrome="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
out="$dir/build"
mkdir -p "$out"

png="$out/icon-$variant.png"
"$chrome" --headless --disable-gpu --hide-scrollbars --force-device-scale-factor=1 \
  --default-background-color=00000000 --window-size=1024,1024 \
  --screenshot="$png" "file://$dir/icon.html?v=$variant" >/dev/null 2>&1

iconset="$out/AppIcon.iconset"
rm -rf "$iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$dir/AppIcon.icns"
echo "$dir/AppIcon.icns (from $png)"
