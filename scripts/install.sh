#!/usr/bin/env bash
# Downloads the latest Kesit release, checks it against SHA256SUMS and
# installs it for the current user: Kesit.app into /Applications on macOS,
# ~/.local/bin/kesit on Linux (x86_64). Never uses sudo.
#
#   curl -fsSL https://raw.githubusercontent.com/emre6943/Kesit/main/scripts/install.sh | bash
#
# KESIT_VERSION=0.1.0 installs a specific release instead of the latest.
set -euo pipefail

REPO="emre6943/Kesit"

fail() {
  echo "kesit install: $*" >&2
  exit 1
}

for tool in curl tar; do
  command -v "$tool" >/dev/null || fail "$tool is required"
done

version="${KESIT_VERSION:-}"
if [[ -z "$version" ]]; then
  latest="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest")"
  version="${latest##*/}"
  [[ "$version" == v* ]] || fail "could not find the latest release"
fi
version="${version#v}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || fail "unexpected version: $version"

case "$(uname -s)-$(uname -m)" in
  Darwin-*) asset="Kesit-${version}-macos-universal.zip" ;;
  Linux-x86_64) asset="kesit-${version}-linux-x86_64.tar.gz" ;;
  *) fail "no prebuilt release for $(uname -s) $(uname -m); build from source instead" ;;
esac

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
base="https://github.com/$REPO/releases/download/v${version}"
echo "==> downloading $asset"
curl -fsSL -o "$tmp/$asset" "$base/$asset"
curl -fsSL -o "$tmp/SHA256SUMS" "$base/SHA256SUMS"

expected="$(awk -v f="$asset" '$2 == f || $2 == "*" f { print $1 }' "$tmp/SHA256SUMS")"
[[ -n "$expected" ]] || fail "$asset is not listed in SHA256SUMS"
if command -v sha256sum >/dev/null; then
  actual="$(sha256sum "$tmp/$asset" | awk '{ print $1 }')"
else
  actual="$(shasum -a 256 "$tmp/$asset" | awk '{ print $1 }')"
fi
[[ "$actual" == "$expected" ]] || fail "checksum mismatch for $asset"
echo "==> checksum ok"

if [[ "$(uname -s)" == "Darwin" ]]; then
  dest="/Applications"
  [[ -w "$dest" ]] || fail "$dest is not writable by this user"
  ditto -x -k "$tmp/$asset" "$tmp/app"
  [[ -d "$tmp/app/Kesit.app" ]] || fail "the archive does not contain Kesit.app"
  rm -rf "$dest/Kesit.app"
  ditto "$tmp/app/Kesit.app" "$dest/Kesit.app"
  echo "==> installed $dest/Kesit.app"
  echo "Start it with: open -a Kesit"
  echo "On the first capture, allow Kesit under System Settings > Privacy & Security >"
  echo "Screen & System Audio Recording, then quit Kesit from its menu bar icon and start it again."
else
  tar -xzf "$tmp/$asset" -C "$tmp"
  "$tmp/kesit-${version}-linux-x86_64/install-linux.sh"
fi
