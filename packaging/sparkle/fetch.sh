#!/bin/sh
# Downloads the pinned Sparkle release into packaging/sparkle/dist (gitignored):
# Sparkle.framework for the app bundle, bin/ tools (generate_keys, sign_update).
set -eu
version=2.10.0
sha256=c2bf58aa8387266ac179357b1415d6f2635f044da8be41042af32425dae6da0c
dir="$(cd "$(dirname "$0")" && pwd)"
dist="$dir/dist"
if [ -f "$dist/VERSION" ] && [ "$(cat "$dist/VERSION")" = "$version" ]; then
  exit 0
fi
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL -o "$tmp/sparkle.tar.xz" \
  "https://github.com/sparkle-project/Sparkle/releases/download/$version/Sparkle-$version.tar.xz"
echo "$sha256  $tmp/sparkle.tar.xz" | shasum -a 256 -c - >/dev/null
rm -rf "$dist"
mkdir -p "$dist"
tar -xJf "$tmp/sparkle.tar.xz" -C "$dist"
echo "$version" > "$dist/VERSION"
echo "Sparkle $version in $dist" >&2
