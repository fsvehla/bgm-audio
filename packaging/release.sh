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
# release carries an appcast that lists just itself.
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

NOTARIZE=1 packaging/bundle.sh release
rm -rf "$out"
mkdir -p "$out"
ditto -c -k --keepParent target/release/Ambience.app "$out/$zip_name"

# EdDSA signature + length for the appcast, e.g. sparkle:edSignature="…" length="…"
signature=$(packaging/sparkle/dist/bin/sign_update "$out/$zip_name")
sha=$(shasum -a 256 "$out/$zip_name" | cut -d' ' -f1)

python3 - "$notes" "$out/appcast.xml" "$version" "$build" "$min_os" "$signature" \
  "https://github.com/$repo/releases/download/$tag/$zip_name" \
  "https://github.com/$repo/releases/tag/$tag" <<'EOF'
import html, re, sys
from email.utils import formatdate
notes, appcast, version, build, min_os, signature, url, page = sys.argv[1:]

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

open(appcast, "w").write(f"""<?xml version="1.0" encoding="utf-8"?>
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
EOF
xmllint --noout "$out/appcast.xml"

# GitHub release notes: the Markdown notes plus the checksum.
{
  cat "$notes"
  printf '\nSHA-256 of `%s`:\n`%s`\n' "$zip_name" "$sha"
} > "$out/notes.md"

if [ "${DRY_RUN:-0}" = 1 ]; then
  echo "dry run: $out/$zip_name, $out/appcast.xml (not published)" >&2
  exit 0
fi
gh release create "$tag" "$out/$zip_name" "$out/appcast.xml" --repo "$repo" \
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
