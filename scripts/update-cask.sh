#!/usr/bin/env bash
# Points packaging/homebrew/Casks/kesit.rb at a published release: sets the
# version and the zip's sha256, read from the release's SHA256SUMS.
#
# Usage: scripts/update-cask.sh VERSION
set -euo pipefail

cd "$(dirname "$0")/.."
REPO="emre6943/Kesit"
CASK="packaging/homebrew/Casks/kesit.rb"
VERSION="${1:?usage: update-cask.sh VERSION}"
VERSION="${VERSION#v}"
ASSET="Kesit-${VERSION}-macos-universal.zip"

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]]; then
  echo "unexpected version: $VERSION" >&2
  exit 1
fi

url="https://github.com/$REPO/releases/download/v${VERSION}/SHA256SUMS"
if ! sums="$(curl -fsSL "$url" 2>/dev/null)"; then
  # Falls back to the GitHub CLI, which also works while the repo is private.
  sums="$(gh release download "v${VERSION}" --repo "$REPO" --pattern SHA256SUMS --output -)"
fi
sha="$(awk -v f="$ASSET" '$2 == f || $2 == "*" f { print $1 }' <<<"$sums")"
if [[ ! "$sha" =~ ^[0-9a-f]{64}$ ]]; then
  echo "no sha256 for $ASSET in $url" >&2
  exit 1
fi

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
sed -e "s/^  version \".*\"$/  version \"${VERSION}\"/" \
  -e "s/^  sha256 \".*\"$/  sha256 \"${sha}\"/" "$CASK" > "$tmp"
cat "$tmp" > "$CASK"
echo "$CASK: version $VERSION, sha256 $sha"
echo "Copy it to Casks/kesit.rb in the emre6943/homebrew-tap repository."
