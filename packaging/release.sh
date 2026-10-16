#!/bin/sh
# release.sh VERSION: a release from this Mac, the same steps as the release workflows, with the
# Developer ID signature and notarization. Needs, in the keychain, the identity
# "Developer ID Application: ... (TEAMID)" and the notary profile "arkos-notary" (made once
# with `xcrun notarytool store-credentials arkos-notary --key <.p8> --key-id <id> --issuer <id>`),
# and the manifest key in ARKOS_RELEASE_KEY (from your password manager, never a file in the
# repository). It commits the version bump, tags, pushes both atomically and publishes the
# GitHub Release, as the workflow does; run it on main with a clean tree.
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
nice -n 10 "$CARGO" build --release --locked -p arkos -p arkosd -p release-tool
TARGET=$("$CARGO" metadata -q --no-deps --format-version 1 | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
DIST=$(mktemp -d)
ZIP=$(CODESIGN_IDENTITY="$IDENTITY" sh packaging/bundle.sh "$VERSION" "$TARGET/release" "$DIST")
ZIP=$(NOTARY_PROFILE=arkos-notary sh packaging/notarize.sh "$ZIP")
URL="https://github.com/$REPO/releases/download/v$VERSION/$(basename "$ZIP")"
sh packaging/manifest.sh "$VERSION" "$(git rev-parse --short HEAD)" "$ZIP" "$URL" "$DIST" > /dev/null
"$TARGET/release/release-tool" sign --key-env ARKOS_RELEASE_KEY "$DIST/manifest.json" > "$DIST/manifest.json.sig"
KEY=$(sed -n 's/^ *"\(.*\)",$/\1/p' crates/protocol/src/release_key.rs | head -1)
"$TARGET/release/release-tool" verify --public "$KEY" "$DIST/manifest.json" "$DIST/manifest.json.sig"
spctl -a -t exec -vv "$DIST/Arkos IDE.app" 2>&1 | grep -q 'source=Notarized Developer ID' || { echo "the app is not notarized"; exit 1; }
git push --atomic origin HEAD:main "v$VERSION"
PREV=$(git describe --tags --abbrev=0 HEAD~1 2>/dev/null || true)
if [ -n "$PREV" ]; then NOTES=$(git log --format='- %s' "$PREV..HEAD~1"); else NOTES="First release."; fi
gh release create "v$VERSION" "$ZIP" "$DIST/manifest.json" "$DIST/manifest.json.sig" --title "Arkos IDE $VERSION" --notes "$NOTES"
echo "released $VERSION from $DIST"
