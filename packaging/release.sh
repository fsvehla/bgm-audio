#!/bin/sh
# Builds, notarizes and publishes a release with its Sparkle appcast.
#
# Usage: packaging/release.sh            (version = CFBundleShortVersionString)
#        DRY_RUN=1 packaging/release.sh  (everything except tagging and uploading)
#
# Needs: packaging/notes/<version>.md (release notes, Markdown), a clean tree
# pushed to origin, the Sparkle signing key in the keychain (generate_keys) and
# the notarytool profile (see bundle.sh).
#
# The app's SUFeedURL points at releases/latest/download/appcast.xml, so each
# release carries an appcast that lists just itself. Windows: the Windows workflow
# builds Ambience-<v>-windows-{x64,arm64}.zip for the same commit; they are signed with
# the same key and listed in appcast-windows.xml (WinSparkle, one enclosure per arch).
set -eu
cd "$(dirname "$0")/.."
repo=fsvehla/bgm-audio
plist=packaging/Info.plist
version=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$plist")
build=$(/usr/libexec/PlistBuddy -c "Print :CFBundleVersion" "$plist")
min_os=$(/usr/libexec/PlistBuddy -c "Print :LSMinimumSystemVersion" "$plist")
notes="packaging/notes/$version.md"
tag="v$version"
zip_name="Ambience-$version-macos.zip"
out=target/release/dist
[ -f "$notes" ] || { echo "missing $notes" >&2; exit 1; }

if [ "${DRY_RUN:-0}" != 1 ]; then
  [ -z "$(git status --porcelain)" ] || { echo "working tree not clean" >&2; exit 1; }
  git fetch -q origin
  [ "$(git rev-parse HEAD)" = "$(git rev-parse '@{u}')" ] || { echo "HEAD not pushed" >&2; exit 1; }
  if gh release view "$tag" --repo "$repo" >/dev/null 2>&1; then
    echo "$tag already exists" >&2; exit 1
  fi
fi

# Windows zips from the CI run of this commit (release builds need a Windows host).
commit=$(git rev-parse HEAD)
run_id=$(gh run list --repo "$repo" --workflow windows.yml --commit "$commit" --limit 1 \
  --json databaseId -q '.[0].databaseId')
[ -n "$run_id" ] || { echo "no Windows CI run for $commit" >&2; exit 1; }
gh run watch "$run_id" --repo "$repo" --exit-status --interval 30 >/dev/null ||
  { echo "Windows CI run $run_id failed" >&2; exit 1; }

NOTARIZE=1 packaging/bundle.sh release
rm -rf "$out"
mkdir -p "$out"
ditto -c -k --keepParent target/release/Ambience.app "$out/$zip_name"
win_x64="Ambience-$version-windows-x64.zip"
win_arm64="Ambience-$version-windows-arm64.zip"
gh run download "$run_id" --repo "$repo" --dir "$out/windows"
mv "$out/windows/Ambience-windows-x64/$win_x64" "$out/windows/Ambience-windows-arm64/$win_arm64" "$out/"
rm -rf "$out/windows"

# EdDSA signature + length for the appcast, e.g. sparkle:edSignature="…" length="…"
signature=$(packaging/sparkle/dist/bin/sign_update "$out/$zip_name")
sha=$(shasum -a 256 "$out/$zip_name" | cut -d' ' -f1)
sig_x64=$(packaging/sparkle/dist/bin/sign_update "$out/$win_x64")
sig_arm64=$(packaging/sparkle/dist/bin/sign_update "$out/$win_arm64")

python3 - "$notes" "$out" "$version" "$build" "$min_os" "$signature" \
  "https://github.com/$repo/releases/download/$tag" "$zip_name" \
  "https://github.com/$repo/releases/tag/$tag" \
  "$win_x64" "$sig_x64" "$win_arm64" "$sig_arm64" <<'EOF'
import html, re, sys
from email.utils import formatdate
(notes, out_dir, version, build, min_os, signature, base, zip_name, page,
 win_x64, sig_x64, win_arm64, sig_arm64) = sys.argv[1:]
url = f"{base}/{zip_name}"

def inline(text):
    text = html.escape(text, quote=False)
    text = re.sub(r"`([^`]+)`", r"<code>\1</code>", text)
    text = re.sub(r"\*\*([^*]+)\*\*", r"<b>\1</b>", text)
    return re.sub(r"\[([^\]]+)\]\(([^)]+)\)", r'<a href="\2">\1</a>', text)

# Small Markdown subset for Sparkle's release-notes view: headings, bullets, paragraphs.
out, in_list = [], False
for line in open(notes).read().splitlines():
    if line.startswith("- "):
        if not in_list:
            out.append("<ul>"); in_list = True
        out.append(f"<li>{inline(line[2:])}</li>")
        continue
    if in_list:
        out.append("</ul>"); in_list = False
    if line.startswith("#"):
        level = min(len(line) - len(line.lstrip("#")) + 1, 4)
        out.append(f"<h{level}>{inline(line.lstrip('#').strip())}</h{level}>")
    elif line.strip():
        out.append(f"<p>{inline(line)}</p>")
if in_list:
    out.append("</ul>")
description = "\n".join(out)

open(f"{out_dir}/appcast.xml", "w").write(f"""<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>Ambience</title>
    <item>
      <title>Version {version}</title>
      <pubDate>{formatdate(usegmt=True)}</pubDate>
      <sparkle:version>{build}</sparkle:version>
      <sparkle:shortVersionString>{version}</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>{min_os}</sparkle:minimumSystemVersion>
      <sparkle:fullReleaseNotesLink>{page}</sparkle:fullReleaseNotesLink>
      <description><![CDATA[{description}]]></description>
      <enclosure url="{url}" type="application/octet-stream" {signature} />
    </item>
  </channel>
</rss>
""")

# WinSparkle picks the enclosure whose sparkle:os matches its architecture.
open(f"{out_dir}/appcast-windows.xml", "w").write(f"""<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>Ambience</title>
    <item>
      <title>Version {version}</title>
      <pubDate>{formatdate(usegmt=True)}</pubDate>
      <sparkle:version>{build}</sparkle:version>
      <sparkle:shortVersionString>{version}</sparkle:shortVersionString>
      <sparkle:fullReleaseNotesLink>{page}</sparkle:fullReleaseNotesLink>
      <description><![CDATA[{description}]]></description>
      <enclosure url="{base}/{win_x64}" sparkle:os="windows-x64" type="application/octet-stream" {sig_x64} />
      <enclosure url="{base}/{win_arm64}" sparkle:os="windows-arm64" type="application/octet-stream" {sig_arm64} />
    </item>
  </channel>
</rss>
""")
EOF
xmllint --noout "$out/appcast.xml" "$out/appcast-windows.xml"

# GitHub release notes: the Markdown notes plus the checksums.
{
  cat "$notes"
  printf '\nSHA-256:\n'
  for f in "$zip_name" "$win_x64" "$win_arm64"; do
    printf -- '- `%s`: `%s`\n' "$f" "$(shasum -a 256 "$out/$f" | cut -d' ' -f1)"
  done
} > "$out/notes.md"

if [ "${DRY_RUN:-0}" = 1 ]; then
  echo "dry run: $out (not published)" >&2
  exit 0
fi
gh release create "$tag" "$out/$zip_name" "$out/appcast.xml" \
  "$out/$win_x64" "$out/$win_arm64" "$out/appcast-windows.xml" --repo "$repo" \
  --target "$(git rev-parse HEAD)" --title "Ambience $version" --notes-file "$out/notes.md"
echo "https://github.com/$repo/releases/tag/$tag"

# Homebrew: point the cask in fsvehla/homebrew-tap at this release. The cask has
# auto_updates (Sparkle), so this only matters for fresh installs.
tap=$(mktemp -d)
trap 'rm -rf "$tap"' EXIT
git clone -q git@github.com:fsvehla/homebrew-tap.git "$tap"
perl -pi -e "s/^  version \".*\"/  version \"$version\"/; s/^  sha256 \".*\"/  sha256 \"$sha\"/" \
  "$tap/Casks/ambience.rb"
git -C "$tap" commit -q -am "ambience $version"
git -C "$tap" push -q
echo "cask: fsvehla/tap/ambience $version" >&2
