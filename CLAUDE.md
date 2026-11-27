# Kesit

Screenshot + annotation tool replacing Shottr. Rust, one crate, one binary `kesit`, macOS
(primary, Apple Silicon) and Linux. No Windows.

## Stack

- Daemon: `tao` event loop + `tray-icon` + `global-hotkey`, no GPU context. Single instance via
  `std::fs::File::try_lock` on `<runtime-or-cache dir>/kesit/daemon.lock` (dir 0700, file 0600;
  never `/tmp`).
- Capture + editor + settings: `eframe`/`egui` 0.37 with the `glow` backend, each in a child
  process (`kesit capture`, `kesit edit <png>`, `kesit settings`). Capture runs two eframe windows
  in sequence in one process (overlay, then editor) — eframe keeps the winit event loop between
  `run_native` calls. Each hotkey press spawns a new capture process; `capture` holds
  `overlay.lock` (`src/lock.rs`) only while grabbing + selecting, so editors never block a capture.
- Screen grab `xcap` (macOS: `CGWindowListCreateImage`), export `tiny-skia` + `ab_glyph`
  (Ubuntu-Light from `epaint_default_fonts`, the font egui draws with), clipboard `arboard`,
  folder picker `rfd`, timestamps `jiff`, macOS window tweaks via `objc2-app-kit`. The login item
  (LaunchAgent / XDG autostart) is written by `src/autostart.rs` itself, with escaping.

## Layout

| File | Role |
|---|---|
| `src/main.rs` | Argument dispatch |
| `src/daemon.rs` | Tray, hotkey, child processes, config reload (settings exit + 5 s mtime poll) |
| `src/capture.rs` | Monitor pick, grab, selection overlay |
| `src/editor/` | Annotation editor: `mod.rs` (state, shortcuts, copy/save), `canvas.rs` (painting, pointer, text field), `toolbar.rs`, `tests.rs` (headless egui frames) |
| `src/annotate.rs` | Annotation model, shared geometry, pixelate, text metrics, full-resolution rasteriser |
| `src/shapes.rs` | Hit-testing, handles, move/resize geometry, undo history (pure, tested) |
| `src/icons.rs` | Toolbar icons drawn with the egui painter |
| `src/lock.rs` | Daemon and overlay file locks |
| `src/geometry.rs` | Selection → physical crop, window sizing (pure, tested) |
| `src/platform.rs` | Cursor position, NSWindow level/frame/activation (macOS), X11 pointer (Linux) |
| `src/settings.rs`, `src/config.rs`, `src/hotkey.rs`, `src/autostart.rs`, `src/export.rs` | As named |

## Commands

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
./scripts/bundle-macos.sh     # dist/Kesit.app as org.versesoftware.kesit; ad-hoc, or KESIT_SIGN_IDENTITY
./scripts/install-linux.sh    # Linux only
cargo deny check              # licences, advisories, sources (deny.toml)
```

CI (`.github/workflows/ci.yml`) runs fmt, clippy `-D warnings` and tests on macOS and Ubuntu.

Linux can be type-checked from the Mac in Docker (`rust:1.95-slim-bookworm` plus the apt packages
in README.md; mount the repo read-only and set `CARGO_TARGET_DIR=/tmp/target`).

## Gotchas

- **Screen Recording permission is tied to the code signature.** The bundle is ad-hoc signed, so
  every rebuild changes the cdhash and macOS may re-prompt or silently capture only the wallpaper
  and menu bar. Remove and re-add Kesit in System Settings → Privacy & Security → Screen & System
  Audio Recording, then restart the daemon. When launched from a terminal, the permission belongs
  to the terminal app, not Kesit.
- **`CGWindowListCreateImage` is deprecated** (macOS 14+); xcap 0.9 still uses it. If a future
  macOS drops it, capture needs ScreenCaptureKit.
- **Never initialise egui/GL in the daemon** — it must stay near zero CPU (~12 MB footprint, one
  wakeup per 5 s config poll). All UI lives in child processes.
- **Overlay must cover the menu bar and Dock:** `platform::setup_overlay_window` sets
  `NSScreenSaverWindowLevel` and the screen frame directly; eframe's `AlwaysOnTop` is only
  `NSFloatingWindowLevel`. Focus/activation happens from the second frame, after eframe has shown
  the painted window.
- **Child windows use the Accessory activation policy** (no Dock icon). They are activated
  explicitly with `NSApplication::activate`; if focus ever fails, Esc will not reach the overlay
  until it is clicked.
- **Clipboard shortcuts are egui's** (`clipboard_shortcuts` stays on): Cmd/Ctrl+C arrives as
  `Event::Copy`, which the editor treats as "copy image" only when no text is being edited;
  Cmd/Ctrl+V reaches the text field as `Event::Paste`.
- **Text field focus is owned by the editor**: `SurrenderFocusOn::Never` plus `request_focus` every
  frame while a draft exists. With egui's default, the click that creates the field is attributed
  to the canvas, so its release "clicks elsewhere", the field loses focus and the empty draft is
  committed (dropped) at once.
- **egui's modifier matching ignores an extra Shift** (`consume_key(NONE, Enter)` also eats
  Shift+Enter); check the Shift variant first or use `editor::consume_exact`.
- **Textures are created from a frame**, not in the app creator: only then does egui know the
  renderer's max texture side (2048 before that, which trips a debug assertion on Retina). The
  pixelated copy is only made once a pixelate annotation exists (~12 ms on a Retina frame).
- **No idle repaints:** the editor and overlay tests assert `repaint_delay == MAX` when idle.
  Viewport commands count as repaint requests, so never send one every frame (see the toolbar's
  `requested_width`).
- **UI checks without stealing focus:** debug builds honour `KESIT_DEBUG_SHOT=<png>` (render,
  write the frame, close; never activates), `KESIT_DEBUG_THEME=light|dark`,
  `KESIT_DEBUG_DEMO=select|text`. Do not drive the real mouse/keyboard on a machine in use.
- **Annotations are stored in image pixels**; stroke widths are screen points × capture scale, so
  preview and export share geometry. Keep `editor::canvas::paint` and `annotate::render` in step.
- **Autostart is only written from an `.app` bundle on macOS** (and never from `target/`), so dev
  runs do not point the LaunchAgent at a build directory.
- **Linux clipboard:** the X11/Wayland clipboard dies with its owner, so Copy pipes the raw image
  (stdin, no temp file) to a detached `kesit __clipboard-hold` that serves it until another app
  takes the clipboard.
- **Untrusted input limits:** `kesit edit` decodes by content with 16384 px / 512 MiB limits;
  the config file is read up to 64 KiB and a relative `save_dir` falls back to the default; saves
  use `File::create_new`, so nothing is overwritten or written through a symlink.
- **Wayland:** no global hotkeys and no global cursor position (the primary monitor is captured);
  bind a desktop shortcut to `kesit capture`.
