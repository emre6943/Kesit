cask "kesit" do
  version "0.1.0"
  sha256 "0b0e034264d3beb3e550a3c4fcf168d25c3586fc48cd2628479bf1f6fa471175"

  url "https://github.com/emre6943/Kesit/releases/download/v#{version}/Kesit-#{version}-macos-universal.zip"
  name "Kesit"
  desc "Screenshot and annotation tool"
  homepage "https://github.com/emre6943/Kesit"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on macos: :sonoma

  app "Kesit.app"

  uninstall launchctl: "org.versesoftware.kesit",
            quit:      "org.versesoftware.kesit"

  zap trash: [
    "~/Library/Application Support/kesit",
    "~/Library/Caches/kesit",
    "~/Library/LaunchAgents/org.versesoftware.kesit.plist",
    "~/Library/Preferences/org.versesoftware.kesit.plist",
  ]

  caveats <<~EOS
    Kesit needs the Screen Recording permission. On the first capture, allow it in
      System Settings > Privacy & Security > Screen & System Audio Recording
    then quit Kesit from its menu bar icon and open it again.
  EOS
end
