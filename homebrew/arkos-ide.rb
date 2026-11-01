cask "arkos-ide" do
  version "0.8.8"
  sha256 "a7ead90c02ecd8b1c1ede998c4c2808b6ea237a80bd13cb788cde5c84a60ce6d"

  url "https://github.com/csarkosh/ide-arkos/releases/download/v#{version}/Arkos-IDE-#{version}-arm64.zip"
  name "Arkos IDE"
  desc "Terminal-pane IDE for running many Claude Code sessions"
  homepage "https://github.com/csarkosh/ide-arkos"

  auto_updates true
  depends_on arch: :arm64
  depends_on macos: :sonoma

  app "Arkos IDE.app"
  # The command line (arkos theme, settings, guide, doctor) on PATH for shells outside Arkos.
  binary "#{appdir}/Arkos IDE.app/Contents/MacOS/arkos"

  zap trash: [
    "~/Library/Application Support/arkos",
    "~/Library/LaunchAgents/sh.csarko.arkosd.plist",
  ]
end
