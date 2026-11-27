#!/usr/bin/env bash
# Builds the Linux release tarball dist/kesit-<version>-linux-<arch>.tar.gz:
# the binary, a .desktop file, the icon, install-linux.sh, LICENSE and README.
#
# Usage: scripts/package-linux.sh VERSION
set -euo pipefail

cd "$(dirname "$0")/.."
VERSION="${1:?usage: package-linux.sh VERSION}"
VERSION="${VERSION#v}"
CARGO_VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
ARCH="$(uname -m)"
NAME="kesit-${VERSION}-linux-${ARCH}"

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "package-linux.sh must run on Linux" >&2
  exit 1
fi
if [[ "$VERSION" != "$CARGO_VERSION" ]]; then
  echo "version mismatch: expected $VERSION, Cargo.toml has $CARGO_VERSION" >&2
  exit 1
fi

echo "==> cargo build --release"
cargo build --release --locked

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
DIR="$STAGE/$NAME"
mkdir -p "$DIR"
install -m755 target/release/kesit "$DIR/kesit"
install -m755 scripts/install-linux.sh "$DIR/install-linux.sh"
install -m644 packaging/linux/kesit.desktop LICENSE README.md "$DIR/"
python3 scripts/make-icon.py "$DIR/kesit.png"

mkdir -p dist
tar -C "$STAGE" --owner=0 --group=0 -czf "dist/$NAME.tar.gz" "$NAME"
ls -l "dist/$NAME.tar.gz"
