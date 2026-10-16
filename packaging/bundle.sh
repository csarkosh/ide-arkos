#!/bin/sh
# bundle.sh VERSION BIN_DIR OUT_DIR: assembles OUT_DIR/"Arkos IDE.app" from BIN_DIR/arkos and
# BIN_DIR/arkosd, signs it, and zips it to OUT_DIR/Arkos-IDE-VERSION-arm64.zip.
#
# Signing: ad-hoc, unless CODESIGN_IDENTITY names a Developer ID Application identity in the
# keychain ("Developer ID Application: Name (TEAMID)"); then each binary and the bundle are
# signed with it, with the hardened runtime and a timestamp, which is what notarization
# (notarize.sh) needs. An ad-hoc signature's identity is the binary's hash, so every update
# makes macOS forget the app's permissions (Accessibility, for the browser pane); a Developer
# ID signature's is the certificate, and they stay.
set -eu
VERSION="$1"; BIN="$2"; OUT="$3"
APP="$OUT/Arkos IDE.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
sed "s/@VERSION@/$VERSION/g" "$(dirname "$0")/Info.plist.in" > "$APP/Contents/Info.plist"
cp "$BIN/arkos" "$BIN/arkosd" "$APP/Contents/MacOS/"
chmod 755 "$APP/Contents/MacOS/arkos" "$APP/Contents/MacOS/arkosd"
if [ -f "$(dirname "$0")/AppIcon.icns" ]; then cp "$(dirname "$0")/AppIcon.icns" "$APP/Contents/Resources/"; fi
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
