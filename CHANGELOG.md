# Changelog

All notable changes to this project are documented in this file. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-11-27

### Added

- First public release for macOS 14 or later (universal: Apple Silicon and Intel) and Linux
  (x86_64).
- Tray daemon with a global hotkey (`Cmd+Shift+S` on macOS, `Ctrl+Shift+S` on Linux).
- Capture of the monitor under the cursor with a frozen-screen selection overlay.
- Editor with arrow, rectangle, ellipse, pen, highlighter, text and pixelate tools; every
  annotation stays editable, with undo and redo.
- Copy to the clipboard or save a timestamped PNG at the full capture resolution.
- `kesit edit <image>` to annotate existing images.
- Settings window: hotkey, save folder, close and reveal behaviour, launch at login.
- Release packages: a signed universal `Kesit.app` (zip and dmg), a Linux tarball, `SHA256SUMS`,
  a Homebrew cask and an install script.

[Unreleased]: https://github.com/emre6943/Kesit/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/emre6943/Kesit/releases/tag/v0.1.0
