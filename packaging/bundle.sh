#!/bin/sh
# bundle.sh VERSION BIN_DIR OUT_DIR: assembles OUT_DIR/"Arkos IDE.app" from BIN_DIR/arkos and
# BIN_DIR/arkosd, ad-hoc signs it, and zips it to OUT_DIR/Arkos-IDE-VERSION-arm64.zip.
set -eu
VERSION="$1"; BIN="$2"; OUT="$3"
APP="$OUT/Arkos IDE.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
sed "s/@VERSION@/$VERSION/g" "$(dirname "$0")/Info.plist.in" > "$APP/Contents/Info.plist"
cp "$BIN/arkos" "$BIN/arkosd" "$APP/Contents/MacOS/"
chmod 755 "$APP/Contents/MacOS/arkos" "$APP/Contents/MacOS/arkosd"
if [ -f "$(dirname "$0")/AppIcon.icns" ]; then cp "$(dirname "$0")/AppIcon.icns" "$APP/Contents/Resources/"; fi
codesign --force --deep --sign - "$APP"
rm -f "$OUT/Arkos-IDE-$VERSION-arm64.zip"
ditto -c -k --keepParent "$APP" "$OUT/Arkos-IDE-$VERSION-arm64.zip"
echo "$OUT/Arkos-IDE-$VERSION-arm64.zip"
