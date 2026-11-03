cask "arkos-ide" do
  version "0.8.11"
  sha256 "740a0c9bb8afdadbeeadd2bb27c3a0adf472ab0846a886fe307b528244281a43"

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
