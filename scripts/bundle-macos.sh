#!/usr/bin/env bash
# Builds dist/Kesit.app: release binary, Info.plist, generated icon and a
# code signature with the hardened runtime. Does not install anything.
# Used by local builds and by the release workflow alike.
#
# Usage: scripts/bundle-macos.sh [--universal | --target TRIPLE] [--version X.Y.Z]
#
#   --universal       Build aarch64 and x86_64 and merge them with lipo. Both
#                     targets must be installed (rustup target add ...).
#   --target TRIPLE   Build for one target instead of the host.
#   --version X.Y.Z   Expected version; fails unless it equals Cargo.toml's.
#
# Signing: set KESIT_SIGN_IDENTITY to a keychain identity ("Developer ID
# Application: ..." for distribution, "Apple Development: ..." or a
# self-signed code-signing certificate for local use) to sign with it; a stable
# identity keeps the Screen Recording permission across rebuilds. Without it
# the bundle is signed ad hoc. KESIT_SIGN_KEYCHAIN selects a keychain file,
# and KESIT_SIGN_TIMESTAMP=none skips the secure timestamp (offline builds;
# notarization needs the timestamp).
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$PWD"
BUNDLE_ID="org.versesoftware.kesit"
APP="$ROOT/dist/Kesit.app"
MIN_MACOS="14.0"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

usage() {
  sed -n '6,11p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

TARGETS=()
EXPECTED_VERSION=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --universal) TARGETS=(aarch64-apple-darwin x86_64-apple-darwin) ;;
    --target)
      [[ $# -ge 2 ]] || usage
      TARGETS=("$2")
      shift
      ;;
    --version)
      [[ $# -ge 2 ]] || usage
      EXPECTED_VERSION="${2#v}"
      shift
      ;;
    -h | --help) usage ;;
    *)
      echo "unknown argument: $1" >&2
      usage
      ;;
  esac
  shift
done

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "bundle-macos.sh must run on macOS" >&2
  exit 1
fi
if [[ -z "$VERSION" ]]; then
  echo "cannot read the version from Cargo.toml" >&2
  exit 1
fi
if [[ -n "$EXPECTED_VERSION" && "$EXPECTED_VERSION" != "$VERSION" ]]; then
  echo "version mismatch: expected $EXPECTED_VERSION, Cargo.toml has $VERSION" >&2
  exit 1
fi

# The binary's minimum macOS matches LSMinimumSystemVersion.
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-$MIN_MACOS}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

if [[ ${#TARGETS[@]} -eq 0 ]]; then
  echo "==> cargo build --release (host)"
  cargo build --release --locked
  cp "$ROOT/target/release/kesit" "$TMP/kesit"
else
  slices=()
  for target in "${TARGETS[@]}"; do
    echo "==> cargo build --release --target $target"
    cargo build --release --locked --target "$target"
    slices+=("$ROOT/target/$target/release/kesit")
  done
  if [[ ${#slices[@]} -gt 1 ]]; then
    lipo -create -output "$TMP/kesit" "${slices[@]}"
  else
    cp "${slices[0]}" "$TMP/kesit"
  fi
fi

echo "==> assembling $APP ($VERSION)"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$TMP/kesit" "$APP/Contents/MacOS/kesit"

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
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
  <key>LSMinimumSystemVersion</key><string>${MIN_MACOS}</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSScreenCaptureUsageDescription</key><string>Kesit captures your screen when you press its hotkey so you can annotate and share a screenshot.</string>
</dict>
</plist>
PLIST
plutil -lint "$APP/Contents/Info.plist" >/dev/null

echo "==> generating icon"
python3 "$ROOT/scripts/make-icon.py" "$TMP/icon_1024.png"
ICONSET="$TMP/Kesit.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$TMP/icon_1024.png" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$TMP/icon_1024.png" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Kesit.icns"

# Hardened runtime in every case, so local builds behave like releases. Kesit
# needs no entitlements: screen capture is a TCC permission, not one.
sign_args=(--force --options runtime --identifier "$BUNDLE_ID")
if [[ -n "${KESIT_SIGN_KEYCHAIN:-}" ]]; then
  sign_args+=(--keychain "$KESIT_SIGN_KEYCHAIN")
fi
if [[ -n "${KESIT_SIGN_IDENTITY:-}" ]]; then
  echo "==> signing as $BUNDLE_ID with the configured identity"
  if [[ "${KESIT_SIGN_TIMESTAMP:-}" == "none" ]]; then
    sign_args+=(--timestamp=none)
  else
    sign_args+=(--timestamp)
  fi
  codesign "${sign_args[@]}" --sign "$KESIT_SIGN_IDENTITY" "$APP"
else
  echo "==> ad-hoc signing as $BUNDLE_ID (set KESIT_SIGN_IDENTITY to use a keychain identity)"
  codesign "${sign_args[@]}" --timestamp=none --sign - "$APP"
fi
codesign --verify --deep --strict "$APP"

echo "==> done"
echo "archs:  $(lipo -archs "$APP/Contents/MacOS/kesit")"
du -sh "$APP" | awk '{print "bundle: " $1}'
echo "binary: $(stat -f %z "$APP/Contents/MacOS/kesit") bytes"
echo "Install by copying $APP to /Applications, then run it once."
