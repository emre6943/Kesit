# Contributing

Thanks for helping. Bug reports, Linux test reports and small, focused pull requests are the
most useful. For a larger change, open an issue first so we can agree on the approach.

## Build and test

You need Rust 1.95 or later; on Linux also the packages listed under "Build from source" in the
[README](README.md).

```sh
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo deny check                 # licences and advisories (cargo install cargo-deny)
shellcheck scripts/*.sh
```

CI runs the same checks on macOS and Ubuntu; a pull request needs them green.

Debug builds can render a window into a PNG without showing it to the user or taking focus,
which is handy for checking UI changes:

```sh
KESIT_DEBUG_SHOT=/tmp/editor.png KESIT_DEBUG_THEME=light KESIT_DEBUG_DEMO=select \
  cargo run -- edit some.png      # KESIT_DEBUG_DEMO: select or text; also works for `settings`
```

`./scripts/bundle-macos.sh` builds `dist/Kesit.app` (add `--universal` for both architectures).

## Guidelines

- Keep the background process light: no GPU or egui in the daemon; UI lives in child processes.
- The preview (`src/editor/canvas.rs`) and the exported PNG (`src/annotate.rs`) must stay in step.
- Add a test for pure logic (geometry, hit-testing, config parsing) where it is practical.
- No new network access.

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/): `feat: …`, `fix: …`,
`docs: …`, `refactor: …`, `test: …`, `build: …`, `ci: …`, `chore: …`. Keep the subject under
about 72 characters and explain the why in the body when it is not obvious. Add user-visible
changes to `CHANGELOG.md` under "Unreleased".
