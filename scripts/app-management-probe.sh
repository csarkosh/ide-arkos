#!/bin/sh
# Does macOS let a background process replace an ad-hoc signed app bundle in /Applications by
# two renames while a copy of it runs? Makes "Sarkos Probe.app", runs it, swaps it the way the
# updater will, and reports. Removes everything it made. Run it once on the Mac that matters.
set -eu
APP="/Applications/Sarkos Probe.app"
STAGE="$(mktemp -d)"
cleanup() { rm -rf "$APP" "$APP.old" "$APP.new" "$STAGE"; }
trap cleanup EXIT
make_bundle() { # $1 dir, $2 version
  mkdir -p "$1/Contents/MacOS"
  cat > "$1/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>sh.csarko.sarkos-probe</string>
<key>CFBundleName</key><string>Sarkos Probe</string>
<key>CFBundleExecutable</key><string>probe</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$2</string>
<key>LSUIElement</key><true/>
</dict></plist>
EOF
  printf '#!/bin/sh\necho "probe %s running $$"; sleep 30\n' "$2" > "$1/Contents/MacOS/probe"
  chmod 755 "$1/Contents/MacOS/probe"
  codesign --force --deep --sign - "$1"
}
make_bundle "$APP" 1
make_bundle "$STAGE/Sarkos Probe.app" 2
"$APP/Contents/MacOS/probe" & RUNNING=$!
sleep 1
echo "swapping while pid $RUNNING runs the old bundle"
mv "$STAGE/Sarkos Probe.app" "$APP.new"
if ! mv "$APP" "$APP.old"; then echo "RESULT: first rename refused (App Management)"; exit 1; fi
if ! mv "$APP.new" "$APP"; then echo "RESULT: second rename refused (App Management)"; mv "$APP.old" "$APP"; exit 1; fi
sleep 1
if kill -0 "$RUNNING" 2>/dev/null; then echo "old copy still running: good"; else echo "RESULT: the running copy was killed by the swap"; exit 1; fi
NEW="$("$APP/Contents/MacOS/probe" & sleep 0.5; kill $! 2>/dev/null; wait $! 2>/dev/null || true)"
"$APP/Contents/MacOS/probe" | head -1 | grep -q "probe 2" && echo "RESULT: swap OK, no prompt seen (check System Settings > Privacy > App Management for a new entry)"
kill "$RUNNING" 2>/dev/null || true
