cask "arkos-ide" do
  version "0.8.49"
  sha256 "d099587826b6b3c44220748b5899cf4720a5fe2a1e94df5204b8a5359ddfa9d1"

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
