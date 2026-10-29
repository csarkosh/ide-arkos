#!/bin/sh
# cask.sh VERSION ZIP: records a release in homebrew/arkos-ide.rb, the Homebrew cask's text: its
# version and the notarized zip's sha256. Everything else in the cask is edited in that file, never
# in the tap; tap.sh publishes the file as it stands, so the repository and the tap never drift.
set -eu
VERSION="${1:?usage: packaging/cask.sh VERSION ZIP}"
ZIP="${2:?usage: packaging/cask.sh VERSION ZIP}"
SHA=$(shasum -a 256 "$ZIP" | cut -d' ' -f1)
CASK="$(dirname "$0")/../homebrew/arkos-ide.rb"
sed -i '' -E -e "s/^  version \"[^\"]*\"\$/  version \"$VERSION\"/" -e "s/^  sha256 \"[^\"]*\"\$/  sha256 \"$SHA\"/" "$CASK"
grep -q "^  version \"$VERSION\"\$" "$CASK" && grep -q "^  sha256 \"$SHA\"\$" "$CASK" ||
  { echo "cask.sh: could not set the version and sha256 in $CASK" >&2; exit 1; }
