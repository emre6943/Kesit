#!/usr/bin/env bash
# Builds dist/Kesit.app: release binary, Info.plist, generated icon and a
# code signature. Does not install anything.
#
# Signing: set KESIT_SIGN_IDENTITY to a keychain identity (e.g. "Apple
# Development: Name (TEAMID)" or a self-signed code-signing certificate) to
# sign with it; a stable identity keeps the Screen Recording permission across
# rebuilds. Without it the bundle is signed ad hoc.
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$PWD"
BUNDLE_ID="org.versesoftware.kesit"
APP="$ROOT/dist/Kesit.app"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "bundle-macos.sh must run on macOS" >&2
  exit 1
fi

echo "==> cargo build --release"
cargo build --release

echo "==> assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$ROOT/target/release/kesit" "$APP/Contents/MacOS/kesit"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleDisplayName</key><string>Kesit</string>
  <key>CFBundleExecutable</key><string>kesit</string>
  <key>CFBundleIconFile</key><string>Kesit</string>
  <key>CFBundleIdentifier</key><string>${BUNDLE_ID}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>Kesit</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSScreenCaptureUsageDescription</key><string>Kesit captures your screen when you press its hotkey so you can annotate and share a screenshot.</string>
</dict>
</plist>
PLIST
plutil -lint "$APP/Contents/Info.plist" >/dev/null

echo "==> generating icon"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
python3 "$ROOT/scripts/make-icon.py" "$TMP/icon_1024.png"
ICONSET="$TMP/Kesit.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$TMP/icon_1024.png" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$TMP/icon_1024.png" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Kesit.icns"

if [[ -n "${KESIT_SIGN_IDENTITY:-}" ]]; then
  echo "==> signing as $BUNDLE_ID with \"$KESIT_SIGN_IDENTITY\""
  codesign --force --sign "$KESIT_SIGN_IDENTITY" --identifier "$BUNDLE_ID" --timestamp=none "$APP"
else
  echo "==> ad-hoc signing as $BUNDLE_ID (set KESIT_SIGN_IDENTITY to use a keychain identity)"
  codesign --force --sign - --identifier "$BUNDLE_ID" --timestamp=none "$APP"
fi
codesign --verify --strict "$APP"

echo "==> done"
du -sh "$APP" | awk '{print "bundle: " $1}'
echo "binary: $(stat -f %z "$APP/Contents/MacOS/kesit") bytes"
echo "Install by copying $APP to /Applications, then run it once."
