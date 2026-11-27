#!/usr/bin/env bash
# Packs dist/Kesit.app (from bundle-macos.sh) into the release archives:
#   dist/Kesit-<version>-macos-universal.zip   (ditto, keeps the signature)
#   dist/Kesit-<version>-macos-universal.dmg   (drag-to-Applications layout)
# With KESIT_SIGN_IDENTITY set, the disk image is signed too.
#
# Usage: scripts/package-macos.sh VERSION
set -euo pipefail

cd "$(dirname "$0")/.."
VERSION="${1:?usage: package-macos.sh VERSION}"
VERSION="${VERSION#v}"
APP="dist/Kesit.app"
BASE="dist/Kesit-${VERSION}-macos-universal"

if [[ ! -d "$APP" ]]; then
  echo "$APP is missing; run scripts/bundle-macos.sh first" >&2
  exit 1
fi
plist_version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP/Contents/Info.plist")"
if [[ "$plist_version" != "$VERSION" ]]; then
  echo "version mismatch: $APP is $plist_version, expected $VERSION" >&2
  exit 1
fi

echo "==> $BASE.zip"
rm -f "$BASE.zip"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$BASE.zip"

echo "==> $BASE.dmg"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
ditto "$APP" "$STAGE/Kesit.app"
ln -s /Applications "$STAGE/Applications"
rm -f "$BASE.dmg"
# hdiutil occasionally fails with "Resource busy" on CI runners; retry.
for attempt in 1 2 3; do
  if hdiutil create -volname "Kesit $VERSION" -srcfolder "$STAGE" -fs HFS+ \
    -format UDZO -ov "$BASE.dmg" >/dev/null; then
    break
  fi
  if [[ $attempt -eq 3 ]]; then
    echo "hdiutil create failed" >&2
    exit 1
  fi
  sleep 5
done
if [[ -n "${KESIT_SIGN_IDENTITY:-}" ]]; then
  sign_args=(--force --timestamp --sign "$KESIT_SIGN_IDENTITY")
  if [[ -n "${KESIT_SIGN_KEYCHAIN:-}" ]]; then
    sign_args+=(--keychain "$KESIT_SIGN_KEYCHAIN")
  fi
  codesign "${sign_args[@]}" "$BASE.dmg"
fi
hdiutil verify "$BASE.dmg" >/dev/null

ls -l "$BASE.zip" "$BASE.dmg"
