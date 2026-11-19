#!/bin/sh
# bundle.sh VERSION BIN_DIR OUT_DIR: assembles OUT_DIR/"Arkos IDE.app" from BIN_DIR/arkos and
# BIN_DIR/arkosd, signs it, and zips it to OUT_DIR/Arkos-IDE-VERSION-arm64.zip.
#
# Signing: ad-hoc, unless CODESIGN_IDENTITY names a Developer ID Application identity in the
# keychain ("Developer ID Application: Name (TEAMID)"); then each binary and the bundle are
# signed with it, with the hardened runtime and a timestamp, which is what notarization
# (notarize.sh) needs. An ad-hoc signature's identity is the binary's hash, so every update
# makes macOS forget the app's permissions (the protected folders it was allowed); a Developer
# ID signature's is the certificate, and they stay.
#
# Channel: with ARKOS_CHANNEL=release (the release workflows and release.sh set it), the bundle
# carries Contents/Resources/channel saying `release`, written before signing so the signature
# seals it; that file is what makes a build a release for telemetry (crates/telemetry, channel_of).
# Without it (packaging/test.sh, a hand-made bundle) there is no file and the build is `source`,
# which never uploads.
set -eu
VERSION="$1"; BIN="$2"; OUT="$3"
APP="$OUT/Arkos IDE.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
sed "s/@VERSION@/$VERSION/g" "$(dirname "$0")/Info.plist.in" > "$APP/Contents/Info.plist"
cp "$BIN/arkos" "$BIN/arkosd" "$APP/Contents/MacOS/"
chmod 755 "$APP/Contents/MacOS/arkos" "$APP/Contents/MacOS/arkosd"
if [ -f "$(dirname "$0")/AppIcon.icns" ]; then cp "$(dirname "$0")/AppIcon.icns" "$APP/Contents/Resources/"; fi
# The bundled fonts are embedded in the binary; their licences travel in the bundle, one folder
# per family, as the licences require.
FONTS="$(dirname "$0")/../crates/arkos/fonts/files"
for family in dm-sans roboto-mono cascadia-code monaspace-radon departure-mono; do
  mkdir -p "$APP/Contents/Resources/Fonts/$family"
  cp "$FONTS/$family/OFL.txt" "$FONTS/$family/LICENSE" "$APP/Contents/Resources/Fonts/$family/" 2>/dev/null || true
  ls "$APP/Contents/Resources/Fonts/$family" | grep -q . || { echo "bundle.sh: no licence for the $family font" >&2; exit 1; }
done
case "${ARKOS_CHANNEL:-}" in
  release) echo release > "$APP/Contents/Resources/channel" ;;
  "" | source) ;;
  # A misspelt channel fails rather than quietly shipping a release as `source`.
  *) echo "bundle.sh: ARKOS_CHANNEL is release, source or unset, not $ARKOS_CHANNEL" >&2; exit 1 ;;
esac
if [ -n "${CODESIGN_IDENTITY:-}" ]; then
  # The helpers first, then the bundle: nested code is signed inside out.
  codesign --force --options runtime --timestamp --sign "$CODESIGN_IDENTITY" "$APP/Contents/MacOS/arkosd" "$APP/Contents/MacOS/arkos"
  codesign --force --options runtime --timestamp --sign "$CODESIGN_IDENTITY" "$APP"
  codesign --verify --deep --strict "$APP"
else
  codesign --force --deep --sign - "$APP"
fi
rm -f "$OUT/Arkos-IDE-$VERSION-arm64.zip"
ditto -c -k --keepParent "$APP" "$OUT/Arkos-IDE-$VERSION-arm64.zip"
echo "$OUT/Arkos-IDE-$VERSION-arm64.zip"
