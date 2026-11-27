# Kesit

Kesit is a small screenshot and annotation tool for macOS and Linux, written in Rust.
Press a hotkey, drag over part of the screen, mark it up, then copy or save it.

*Kesit* is Turkish for "cross-section".

## Features

- Global hotkey and a tray menu; the resident process uses no GPU and stays near zero CPU.
- Freezes the monitor under the cursor, so menus and hover states stay put while you select.
- Arrow, rectangle, ellipse, pen, highlighter, text and pixelate tools.
- Every annotation stays editable: select, move, resize, recolour, delete, with undo and redo.
- Exports at the full resolution of the capture; the preview and the PNG use the same geometry
  and font.
- Copies to the clipboard or saves a timestamped PNG, then optionally reveals the file.
- Opens existing images too (`kesit edit <file>`).
- Makes no network connections.

## Usage

| Command | What it does |
|---|---|
| `kesit` | Tray daemon: registers the global hotkey and shows the tray menu (Capture, Settings…, Quit). No window, no Dock icon. |
| `kesit capture` | Freezes the monitor under the cursor. Drag to select, Enter or a double-click takes the whole screen, Esc cancels. Opens the editor on the selection. |
| `kesit edit <image>` | Opens the editor on an existing PNG. Saving writes a new file; the original is not touched. |
| `kesit settings` | Settings window. The daemon picks up the changes when it closes. |

The default hotkey is `Cmd+Shift+S` on macOS and `Ctrl+Shift+S` on Linux. Pressing it while
editors are open starts another capture; any number of editors can be open at once.

### Editor shortcuts

| Key | Action |
|---|---|
| `V` | Select: click an annotation to select it, drag to move, drag a handle to resize |
| `A` `R` `O` `P` `H` `T` `B` | Arrow, Rectangle, Ellipse, Pen, Highlighter, Text, Pixelate |
| `1` `2` `3` | Thin, medium, thick stroke |
| Shift while dragging | Square or circle; a straight line for the pen and highlighter |
| Delete / Backspace | Delete the selected annotation |
| Arrow keys (with Shift: ×10) | Nudge the selected annotation by 1 point (10 points) |
| Cmd/Ctrl+Z, Cmd/Ctrl+Shift+Z | Undo, redo |
| Cmd/Ctrl+C or Enter | Copy the annotated image (and close, by default) |
| Cmd/Ctrl+S | Save `Kesit YYYY-MM-DD at HH.MM.SS.png` to the save folder (and close, by default) |
| Esc | Deselect; with nothing selected, close without saving |

Any tool can grab an existing annotation by its outline. With the Select tool the inside of
rectangles and ellipses counts too. Picking a colour or stroke width while an annotation is
selected changes that annotation.

Text: click and type. Enter or a click elsewhere commits, Shift+Enter starts a new line, Esc
cancels. Double-click a text annotation to edit it again; drag its corner handles to resize it.

Pixelate replaces the area with blocks averaged over the original pixels, in the preview and in
the exported image alike, so the hidden detail is not recoverable from the PNG.

The editor remembers the last tool, colour and stroke width.

## Install

There are no prebuilt releases yet; both scripts build from source (see below for the
toolchain).

### macOS (Apple Silicon, macOS 13 or later)

```sh
./scripts/bundle-macos.sh        # builds dist/Kesit.app, signed ad hoc
cp -R dist/Kesit.app /Applications/
open /Applications/Kesit.app
```

On the first capture macOS asks for the Screen Recording permission (System Settings → Privacy &
Security → Screen & System Audio Recording). Grant it to Kesit, then quit Kesit from the tray
and start it again.

The permission is tied to the app's code signature. An ad-hoc signature changes with every
build, so after rebuilding you may have to remove Kesit from that list and add it again; until
then captures show only the wallpaper and the menu bar. Signing with a stable identity avoids
this:

```sh
KESIT_SIGN_IDENTITY="Apple Development: Your Name (TEAMID)" ./scripts/bundle-macos.sh
```

Launch at login is a LaunchAgent (`~/Library/LaunchAgents/org.versesoftware.kesit.plist`). It is
only written when Kesit runs from an `.app` bundle.

### Linux

```sh
./scripts/install-linux.sh       # installs ~/.local/bin/kesit and a .desktop launcher
kesit &
```

Build dependencies on Debian and Ubuntu:

```sh
sudo apt install pkg-config libgtk-3-dev libxdo-dev libayatana-appindicator3-dev \
  libxcb1-dev libxcb-randr0-dev libpipewire-0.3-dev libclang-dev libwayland-dev \
  libegl-dev libdbus-1-dev libgbm-dev
```

**Wayland:** compositors do not let applications register global hotkeys or read the global
cursor position. Bind a desktop shortcut (for example GNOME Settings → Keyboard → Custom
Shortcuts) to `kesit capture`; it captures the primary monitor. The tray menu works where the
desktop shows tray icons.

**Linux status:** Kesit is developed on macOS. The Linux build compiles and its
tests pass in CI, but the X11 and Wayland paths (capture, overlay, clipboard, tray, autostart)
have had little or no testing on a real desktop. Reports are welcome.

## Configuration

Settings live in `config.toml` in the platform config directory
(`~/Library/Application Support/kesit/` on macOS, `~/.config/kesit/` on Linux). The settings
window edits the same file.

```toml
hotkey = "Cmd+Shift+S"
save_dir = "/Users/you/Downloads"   # must be an absolute path
autostart = true
color = "#FF3B30"                   # last colour used in the editor
stroke_width = 4.0                  # last stroke width used in the editor
tool = "arrow"                      # last tool used in the editor
close_after_copy = true
close_after_save = true
reveal_after_save = true            # show the file in Finder / open its folder
```

A missing or unreadable file falls back to the defaults, and an invalid value falls back to its
own default.

## Building from source

Kesit needs Rust 1.89 or later.

```sh
cargo build --release
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Debug builds can render a window into a PNG without showing it to the user or taking focus,
which is handy for checking UI changes:

```sh
KESIT_DEBUG_SHOT=/tmp/editor.png KESIT_DEBUG_THEME=light KESIT_DEBUG_DEMO=select \
  cargo run -- edit some.png      # KESIT_DEBUG_DEMO: select or text; also works for `settings`
```

## Licence

MIT. See [LICENSE](LICENSE).
