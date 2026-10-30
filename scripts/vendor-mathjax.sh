#!/bin/sh
# Vendors MathJax for the rendered markdown view's display math: downloads the pinned release's npm
# tarball, takes es5/tex-svg-full.js from it (TeX in, SVG out, its glyphs as paths, so no font
# files), checks its SHA-256 against the one recorded here, and writes
# crates/arkos/assets/mathjax-tex-svg.js with MathJax's Apache 2.0 licence (the tarball's LICENSE)
# as a comment at the top. The binary bundles that file (crates/arkos/src/md_math.rs,
# include_bytes!). To move to a new release, change VERSION here and in md_math.rs, run this with
# SHA256 empty to see the new file's hash, record it, and run again.
set -eu
VERSION="3.2.2"
SHA256="a4354ff94fd868aea0cc6eaaa79a57fda0588646fc46ee3700a349ee0a11cbe6"
URL="https://registry.npmjs.org/mathjax/-/mathjax-$VERSION.tgz"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/crates/arkos/assets/mathjax-tex-svg.js"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

curl --disable --fail --silent --show-error --location --proto =https --max-time 120 --output "$WORK/mathjax.tgz" -- "$URL"
mkdir "$WORK/x"
tar -xzf "$WORK/mathjax.tgz" -C "$WORK/x" package/es5/tex-svg-full.js package/LICENSE
GOT="$(shasum -a 256 "$WORK/x/package/es5/tex-svg-full.js" | cut -d ' ' -f 1)"
if [ "$GOT" != "$SHA256" ]; then
  echo "vendor-mathjax: es5/tex-svg-full.js of $VERSION has SHA-256 $GOT, not the recorded $SHA256" >&2
  exit 1
fi
# The licence goes inside a comment, so it must not hold the comment's end.
if grep -q '\*/' "$WORK/x/package/LICENSE"; then
  echo "vendor-mathjax: the LICENSE holds a comment end and cannot be put in a comment" >&2
  exit 1
fi
mkdir -p "$(dirname "$OUT")"
{
  printf '/*! MathJax %s, vendored from %s (es5/tex-svg-full.js, SHA-256 %s).\n\n' "$VERSION" "$URL" "$SHA256"
  cat "$WORK/x/package/LICENSE"
  printf '*/\n'
  cat "$WORK/x/package/es5/tex-svg-full.js"
} > "$WORK/mathjax-tex-svg.js"
mv "$WORK/mathjax-tex-svg.js" "$OUT"
echo "vendor-mathjax: wrote $OUT (MathJax $VERSION)"
