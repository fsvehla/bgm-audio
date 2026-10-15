#!/bin/sh
# Builds target/<profile>/Ambience.app around the cargo binary (bgm-audio) and signs it.
#
# Usage: packaging/bundle.sh [debug|release]
#
# Signing:
#   SIGN_IDENTITY=<sha1|name>  identity to sign with; default: the newest valid
#                              "Developer ID Application" certificate in the keychain,
#                              or ad-hoc ("-") if there is none.
#   SIGN_IDENTITY=-            force ad-hoc signing (local use only).
# Notarization (release only, needs a Developer ID signature):
#   NOTARIZE=1                 submit to Apple, wait, staple the ticket.
#   NOTARY_PROFILE=<name>      notarytool keychain profile (default: notary). Create once:
#                              xcrun notarytool store-credentials notary --apple-id <id> --team-id <team>
set -eu
profile="${1:-release}"
cd "$(dirname "$0")/.."

# Newest valid Developer ID Application identity, by certificate start date.
newest_developer_id() {
  tmp=$(mktemp -d)
  valid=$(security find-identity -v -p codesigning | awk '/"Developer ID Application/ {print $2}')
  security find-certificate -a -p -c "Developer ID Application" 2>/dev/null |
    awk -v dir="$tmp" '/BEGIN CERTIFICATE/ {n++} {print > (dir "/c" n ".pem")}'
  best="" best_start=0
  for pem in "$tmp"/c*.pem; do
    [ -f "$pem" ] || continue
    sha=$(openssl x509 -in "$pem" -noout -fingerprint -sha1 | sed 's/.*=//; s/://g')
    echo "$valid" | grep -qx "$sha" || continue
    start=$(openssl x509 -in "$pem" -noout -startdate | sed 's/notBefore=//')
    start=$(date -j -f "%b %e %T %Y %Z" "$start" +%s)
    if [ "$start" -gt "$best_start" ]; then best="$sha" best_start="$start"; fi
  done
  rm -rf "$tmp"
  echo "$best"
}

if [ "$profile" = release ]; then cargo build --release; else cargo build; fi
app="target/$profile/Ambience.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp packaging/Info.plist "$app/Contents/Info.plist"
# Icon is rendered from packaging/icon/icon.html by packaging/icon/render.sh.
cp packaging/icon/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
cp "target/$profile/bgm-audio" "$app/Contents/MacOS/Ambience"

identity="${SIGN_IDENTITY:-$(newest_developer_id)}"
identity="${identity:--}"
if [ "$identity" = - ]; then
  codesign --force --sign - "$app"
  echo "signed ad-hoc (local use only)" >&2
else
  # Hardened runtime + secure timestamp: required for notarization.
  codesign --force --options runtime --timestamp --sign "$identity" "$app"
  codesign --verify --strict --verbose=1 "$app" >&2
  echo "signed with $(codesign -dvv "$app" 2>&1 | sed -n 's/^Authority=//p' | head -1)" >&2
fi

if [ "${NOTARIZE:-0}" = 1 ]; then
  [ "$identity" != - ] || { echo "notarization needs a Developer ID signature" >&2; exit 1; }
  zip="target/$profile/Ambience.zip"
  ditto -c -k --keepParent "$app" "$zip"
  xcrun notarytool submit "$zip" --keychain-profile "${NOTARY_PROFILE:-notary}" --wait
  xcrun stapler staple "$app"
  rm -f "$zip"
  spctl --assess --type execute --verbose=2 "$app" >&2
fi
echo "$app"
