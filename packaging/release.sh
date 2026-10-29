#!/bin/sh
# release.sh VERSION: a release from this Mac, the same steps as the release workflows, with the
# Developer ID signature and notarization. Needs, in the keychain, the identity
# "Developer ID Application: ... (TEAMID)" and the notary profile "arkos-notary" (made once
# with `xcrun notarytool store-credentials arkos-notary --key <.p8> --key-id <id> --issuer <id>`),
# and the manifest key in ARKOS_RELEASE_KEY (from your password manager, never a file in the
# repository). It commits the version bump, tags, records the release in the Homebrew cask,
# pushes atomically, publishes the GitHub Release and then the cask to csarkosh/homebrew-tap (with
# this Mac's git access), as the workflow does; run it on main with a clean tree.
set -eu
cd "$(dirname "$0")/.."
VERSION="${1:?usage: packaging/release.sh VERSION}"
CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"
echo "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || { echo "not major.minor.patch"; exit 1; }
[ "$(git branch --show-current)" = main ] || { echo "release from main"; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "the tree is not clean"; exit 1; }
git rev-parse -q --verify "refs/tags/v$VERSION" >/dev/null && { echo "tag v$VERSION exists"; exit 1; }
: "${ARKOS_RELEASE_KEY:?ARKOS_RELEASE_KEY (the manifest key) is not set}"
IDENTITY=$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: .*\)"/\1/p' | head -1)
[ -n "$IDENTITY" ] || { echo "no valid Developer ID Application identity in the keychain"; exit 1; }
REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner)
sed -i '' -E "s/^version = \"[0-9.]+\"\$/version = \"$VERSION\"/" Cargo.toml
grep -q "version = \"$VERSION\"" Cargo.toml
"$CARGO" update -w -q
git commit -qam "Release $VERSION"
git tag "v$VERSION"
# The source root and the Cargo home are remapped out of the binaries, so a crash report's file
# reads /src/crates/... or /cargo/... and never names this Mac's folders (the toolchain's own
# paths already read /rustc/<hash>). Cargo hands rustc the workspace's own sources by relative
# paths, crates/..., which the first prefix does not match, hence the third.
RUSTFLAGS="--remap-path-prefix=$PWD=/src --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo --remap-path-prefix=crates=/src/crates" \
  nice -n 10 "$CARGO" build --release --locked -p arkos -p arkosd -p release-tool
TARGET=$("$CARGO" metadata -q --no-deps --format-version 1 | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
# A crash report names the build by its commit; a build that could not read it would say 0000000.
# The version is read first, so a binary that crashes or prints nothing fails here too.
for BIN in arkos arkosd; do
  V=$("$TARGET/release/$BIN" --version)
  case "$V" in "$BIN "*" ("*")") ;; *) echo "$BIN --version said '$V'"; exit 1 ;; esac
  case "$V" in *"(unknown)") echo "$BIN was built without its commit hash"; exit 1 ;; esac
done
DIST=$(mktemp -d)
ZIP=$(ARKOS_CHANNEL=release CODESIGN_IDENTITY="$IDENTITY" sh packaging/bundle.sh "$VERSION" "$TARGET/release" "$DIST")
[ "$(cat "$DIST/Arkos IDE.app/Contents/Resources/channel")" = release ] || { echo "the bundle is not on the release channel"; exit 1; }
ZIP=$(NOTARY_PROFILE=arkos-notary sh packaging/notarize.sh "$ZIP")
DSYM=$(sh packaging/dsym.sh "$VERSION" "$TARGET/release" "$DIST")
URL="https://github.com/$REPO/releases/download/v$VERSION/$(basename "$ZIP")"
sh packaging/manifest.sh "$VERSION" "$(git rev-parse --short HEAD)" "$ZIP" "$URL" "$DIST" > /dev/null
"$TARGET/release/release-tool" sign --key-env ARKOS_RELEASE_KEY "$DIST/manifest.json" > "$DIST/manifest.json.sig"
KEY=$(sed -n 's/^ *"\(.*\)",$/\1/p' crates/protocol/src/release_key.rs | head -1)
"$TARGET/release/release-tool" verify --public "$KEY" "$DIST/manifest.json" "$DIST/manifest.json.sig"
spctl -a -t exec -vv "$DIST/Arkos IDE.app" 2>&1 | grep -q 'source=Notarized Developer ID' || { echo "the app is not notarized"; exit 1; }
sh packaging/cask.sh "$VERSION" "$ZIP"
git commit -qm "Cask: arkos-ide $VERSION" homebrew/arkos-ide.rb
git push --atomic origin HEAD:main "v$VERSION"
PREV=$(git describe --tags --abbrev=0 "v$VERSION~1" 2>/dev/null || true)
if [ -n "$PREV" ]; then NOTES=$(git log --format='- %s' --invert-grep --grep='^Cask: arkos-ide ' "$PREV..v$VERSION~1"); else NOTES="First release."; fi
gh release create "v$VERSION" "$ZIP" "$DSYM" "$DIST/manifest.json" "$DIST/manifest.json.sig" --title "Arkos IDE $VERSION" --notes "$NOTES"
sh packaging/tap.sh "$VERSION"
echo "released $VERSION from $DIST"
