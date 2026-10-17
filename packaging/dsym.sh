#!/bin/sh
# dsym.sh VERSION BIN_DIR OUT_DIR: zips BIN_DIR/arkos.dSYM and BIN_DIR/arkosd.dSYM (the release
# profile's split-debuginfo = "packed" makes them) to OUT_DIR/Arkos-IDE-VERSION-dSYM.zip, which
# the release publishes beside the app zip so a crash report's frames symbolicate with `atos`
# (docs/development.md). Each dSYM must carry its binary's LC_UUID: symbols from another build
# would name the wrong lines.
set -eu
VERSION="$1"; BIN="$2"; OUT="$3"
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
for NAME in arkos arkosd; do
  [ -d "$BIN/$NAME.dSYM" ] || { echo "dsym.sh: no $BIN/$NAME.dSYM" >&2; exit 1; }
  WANT=$(dwarfdump --uuid "$BIN/$NAME" | awk '{print $2}')
  GOT=$(dwarfdump --uuid "$BIN/$NAME.dSYM" | awk '{print $2}')
  [ -n "$WANT" ] && [ "$WANT" = "$GOT" ] || { echo "dsym.sh: $NAME is $WANT, its dSYM $GOT" >&2; exit 1; }
  cp -R "$BIN/$NAME.dSYM" "$STAGE/"
done
ZIP="$OUT/Arkos-IDE-$VERSION-dSYM.zip"
rm -f "$ZIP"
# The two dSYMs at the zip's top level, as `unzip` should give them back, without the AppleDouble
# files a copy's extended attributes would add.
ditto -c -k --norsrc --noextattr --noacl "$STAGE" "$ZIP"
echo "$ZIP"
