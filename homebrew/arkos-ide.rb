cask "arkos-ide" do
  version "0.8.9"
  sha256 "f09d0dd24e0484058e4ccf6bcc43b9b79a5c44aa1caba344d9abb7f2a55173c6"

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
