#!/bin/sh
# Builds a bundle from the dev binaries, a manifest, signs it with a throwaway key and verifies
# the lot: the same steps the release workflow runs, without a runner.
set -eu
cd "$(dirname "$0")/.."
CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"
nice -n 15 "$CARGO" build -q -p arkos -p arkosd -p release-tool
# The target directory may be set elsewhere (a shared one in .cargo/config.toml): ask cargo.
TARGET=$("$CARGO" metadata -q --no-deps --format-version 1 | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
BIN="$TARGET/debug"
# The workspace version, so the bundle's Info.plist and manifest agree with what arkosd prints
# (the daemon's check_bundle compares them).
VERSION=$(sed -n 's/^version = "\([0-9.]*\)"$/\1/p' Cargo.toml)
OUT=$(mktemp -d)
ZIP=$(sh packaging/bundle.sh "$VERSION" "$BIN" "$OUT")
sh packaging/manifest.sh "$VERSION" "$(git rev-parse --short HEAD)" "$ZIP" "file://$ZIP" "$OUT" > /dev/null
KEYS=$("$BIN/release-tool" keygen)
SK=$(echo "$KEYS" | sed -n 's/^secret: //p'); PK=$(echo "$KEYS" | sed -n 's/^public: //p')
ARKOS_TEST_KEY="$SK" "$BIN/release-tool" sign --key-env ARKOS_TEST_KEY "$OUT/manifest.json" > "$OUT/manifest.json.sig"
"$BIN/release-tool" verify --public "$PK" "$OUT/manifest.json" "$OUT/manifest.json.sig"
SAID=$("$OUT/Arkos IDE.app/Contents/MacOS/arkosd" --version)
echo "$SAID"
case "$SAID" in *" $VERSION "* | *" $VERSION") ;; *) echo "arkosd says $SAID, the bundle $VERSION"; exit 1 ;; esac
echo "packaging ok: $OUT"
