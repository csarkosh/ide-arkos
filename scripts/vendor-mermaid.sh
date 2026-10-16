#!/bin/sh
# Vendors mermaid for the rendered markdown view's diagrams: downloads the pinned release's npm
# tarball, takes dist/mermaid.min.js from it, checks its SHA-256 against the one recorded here,
# and writes crates/arkos/assets/mermaid.min.js with mermaid's MIT licence (the tarball's LICENSE)
# as a comment at the top, since the minified file does not carry it. The binary bundles that file
# (crates/arkos/src/mermaid.rs, include_bytes!). To move to a new release, change VERSION here and
# in mermaid.rs, run this with SHA256 empty to see the new file's hash, record it, and run again.
set -eu
VERSION="11.17.2"
SHA256="581ed7d74bd9048d0e3a91363927d72ef22942d7722546b27f7cc29e35390eb8"
URL="https://registry.npmjs.org/mermaid/-/mermaid-$VERSION.tgz"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/crates/arkos/assets/mermaid.min.js"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

curl --disable --fail --silent --show-error --location --proto =https --max-time 120 --output "$WORK/mermaid.tgz" -- "$URL"
mkdir "$WORK/x"
tar -xzf "$WORK/mermaid.tgz" -C "$WORK/x" package/dist/mermaid.min.js package/LICENSE
GOT="$(shasum -a 256 "$WORK/x/package/dist/mermaid.min.js" | cut -d ' ' -f 1)"
if [ "$GOT" != "$SHA256" ]; then
  echo "vendor-mermaid: dist/mermaid.min.js of $VERSION has SHA-256 $GOT, not the recorded $SHA256" >&2
  exit 1
fi
# The licence goes inside a comment, so it must not hold the comment's end.
if grep -q '\*/' "$WORK/x/package/LICENSE"; then
  echo "vendor-mermaid: the LICENSE holds a comment end and cannot be put in a comment" >&2
  exit 1
fi
mkdir -p "$(dirname "$OUT")"
{
  printf '/*! mermaid %s, vendored from %s (dist/mermaid.min.js, SHA-256 %s).\n\n' "$VERSION" "$URL" "$SHA256"
  cat "$WORK/x/package/LICENSE"
  printf '*/\n'
  cat "$WORK/x/package/dist/mermaid.min.js"
} > "$WORK/mermaid.min.js"
mv "$WORK/mermaid.min.js" "$OUT"
echo "vendor-mermaid: wrote $OUT (mermaid $VERSION)"
