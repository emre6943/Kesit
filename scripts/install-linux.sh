#!/usr/bin/env bash
# Installs kesit for the current user: ~/.local/bin/kesit, an icon and a
# .desktop launcher. Launch at login is managed by kesit itself (Settings).
#
# Run from a release tarball, it installs the prebuilt binary next to it;
# run from a source checkout, it builds with cargo first.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
APPS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICON_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/256x256/apps"

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "install-linux.sh must run on Linux" >&2
  exit 1
fi

if [[ -f "$HERE/kesit" ]]; then
  # Release tarball: kesit, kesit.png and this script side by side.
  BINARY="$HERE/kesit"
  ICON="$HERE/kesit.png"
  MAKE_ICON=""
else
  ROOT="$(cd "$HERE/.." && pwd)"
  echo "==> cargo build --release"
  (cd "$ROOT" && cargo build --release --locked)
  BINARY="$ROOT/target/release/kesit"
  ICON=""
  MAKE_ICON="$ROOT/scripts/make-icon.py"
fi

echo "==> installing $BIN_DIR/kesit"
install -Dm755 "$BINARY" "$BIN_DIR/kesit"

mkdir -p "$ICON_DIR"
if [[ -n "$ICON" && -f "$ICON" ]]; then
  install -m644 "$ICON" "$ICON_DIR/kesit.png"
elif [[ -n "$MAKE_ICON" ]] && command -v python3 >/dev/null; then
  python3 "$MAKE_ICON" "$ICON_DIR/kesit.png"
fi

# Quotes a path for a desktop entry's Exec key (Desktop Entry Specification).
desktop_exec() {
  printf '"%s"' "$(printf '%s' "$1" | sed -e 's/\\/\\\\\\\\/g' -e 's/["`$]/\\\\&/g' -e 's/%/%%/g')"
}
EXEC="$(desktop_exec "$BIN_DIR/kesit")"

echo "==> installing $APPS_DIR/kesit.desktop"
mkdir -p "$APPS_DIR"
cat > "$APPS_DIR/kesit.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Kesit
Comment=Screenshot and annotate
Exec=$EXEC
Icon=kesit
Terminal=false
Categories=Utility;Graphics;
Actions=Capture;Settings;

[Desktop Action Capture]
Name=Capture
Exec=$EXEC capture

[Desktop Action Settings]
Name=Settings
Exec=$EXEC settings
DESKTOP
if command -v update-desktop-database >/dev/null; then
  update-desktop-database "$APPS_DIR" || true
fi

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "note: $BIN_DIR is not on your PATH" ;;
esac
if [[ "${XDG_SESSION_TYPE:-}" == "wayland" ]]; then
  echo "note: Wayland does not let apps grab global hotkeys."
  echo "      Bind a desktop shortcut to: \"$BIN_DIR/kesit\" capture"
fi
echo "==> done. Start the daemon with: kesit"
