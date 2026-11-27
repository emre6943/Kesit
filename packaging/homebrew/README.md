# emre6943/homebrew-tap

Homebrew casks for [Kesit](https://github.com/emre6943/Kesit), a screenshot and annotation tool
for macOS.

```sh
brew tap emre6943/tap
brew trust emre6943/tap   # recent Homebrew only; skip if the command does not exist
brew install --cask kesit
```

Kesit needs macOS 14 (Sonoma) or later. On the first capture, allow it under System Settings →
Privacy & Security → Screen & System Audio Recording, then quit Kesit from its menu bar icon and
open it again.

Upgrade with `brew upgrade --cask kesit`. Uninstall with `brew uninstall --cask kesit`; add
`--zap` to also remove its settings, caches and login item.

## Maintaining

This directory is kept in the Kesit repository under `packaging/homebrew/` and copied to the
`emre6943/homebrew-tap` repository on each release:

1. Publish the Kesit release (the release workflow attaches the zip and `SHA256SUMS`).
2. In the Kesit repository, run `scripts/update-cask.sh <version>`.
3. Copy `packaging/homebrew/Casks/kesit.rb` to `Casks/kesit.rb` here, then commit and push.
4. Check it: `brew style --cask Casks/kesit.rb` and
   `brew audit --cask --online emre6943/tap/kesit`.
