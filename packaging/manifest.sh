#!/bin/sh
# manifest.sh VERSION GIT ZIP URL OUT_DIR: writes OUT_DIR/manifest.json for the zip.
set -eu
VERSION="$1"; GIT="$2"; ZIP="$3"; URL="$4"; OUT="$5"
SIZE=$(stat -f %z "$ZIP")
SHA=$(shasum -a 256 "$ZIP" | cut -d' ' -f1)
NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)
cat > "$OUT/manifest.json" <<EOF
{"format":1,"version":"$VERSION","git":"$GIT","published":"$NOW","arm64":{"url":"$URL","size":$SIZE,"sha256":"$SHA"},"min_protocol":1}
EOF
echo "$OUT/manifest.json"
