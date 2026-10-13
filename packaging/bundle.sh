#!/bin/sh
# Builds target/<profile>/bgm-audio.app around the cargo binary.
# Usage: packaging/bundle.sh [debug|release]
set -eu
profile="${1:-release}"
cd "$(dirname "$0")/.."
if [ "$profile" = release ]; then cargo build --release; else cargo build; fi
app="target/$profile/bgm-audio.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp packaging/Info.plist "$app/Contents/Info.plist"
# Icon is rendered from packaging/icon/icon.html by packaging/icon/render.sh.
cp packaging/icon/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
cp "target/$profile/bgm-audio" "$app/Contents/MacOS/bgm-audio"
codesign --force --sign - "$app" >/dev/null 2>&1 || true
echo "$app"
