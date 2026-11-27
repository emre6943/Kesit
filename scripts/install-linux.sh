#!/usr/bin/env bash
# Installs kesit for the current user: ~/.local/bin/kesit and a .desktop
# launcher. Launch at login is managed by kesit itself (Settings).
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$PWD"
BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
APPS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICON_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/256x256/apps"

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "install-linux.sh must run on Linux" >&2
  exit 1
fi

echo "==> cargo build --release"
cargo build --release

echo "==> installing $BIN_DIR/kesit"
install -Dm755 "$ROOT/target/release/kesit" "$BIN_DIR/kesit"

if command -v python3 >/dev/null; then
  mkdir -p "$ICON_DIR"
  python3 "$ROOT/scripts/make-icon.py" "$ICON_DIR/kesit.png"
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
