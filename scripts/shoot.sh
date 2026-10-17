#!/bin/sh
# Takes Arkos IDE's pictures from a dev build: one scene per storyboard in scripts/shoot/stories
# (multitasker, web-developer, blogger), each a real session in a demo project, then the hero
# composed from the scenes' stills. Any scene can be redone alone; the hero is recomposed from
# whatever stills exist.
#
#   scripts/shoot.sh                       # every scene, then the hero
#   scripts/shoot.sh multitasker           # one scene, then the hero
#   scripts/shoot.sh compose               # only the hero, from the stills in docs/images
#   scripts/shoot.sh --release blogger     # from the release build
#
# What a scene needs: your Claude Code account for a few minutes (the prompts run for real), the
# Screen Recording permission for "Arkos Shoot" (the tool runs as an app of its own, so macOS
# attributes the capture to it rather than to whatever shell started it; allow it when asked the
# first time), and the Accessibility permission for the shoot's copy of Arkos (the browser pane;
# System Settings > Privacy & Security > Accessibility, add ~/Library/Caches/arkos-shoot/app). The
# window need not stay in front: captures go by window id. Progress captures and the panes'
# states land in docs/images/progress/<scene>/ every 20 s.
set -eu
cd "$(dirname "$0")/.."
ROOT=$(pwd)
CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"
PROFILE=debug
FLAGS=""
if [ "${1:-}" = "--release" ]; then PROFILE=release; FLAGS="--release"; shift; fi
OUT="${OUT:-$ROOT/docs/images}"
STORIES="$ROOT/scripts/shoot/stories"
SCENES="$*"

# A locked screen (or a sleeping display) stops macOS from drawing the window, so every capture
# would show its first frame. Refuse up front rather than take pictures of a frozen window.
if [ "$SCENES" != "compose" ] && python3 -I -c '
import ctypes, ctypes.util, sys
cg = ctypes.cdll.LoadLibrary(ctypes.util.find_library("CoreGraphics"))
cf = ctypes.cdll.LoadLibrary(ctypes.util.find_library("CoreFoundation"))
cg.CGSessionCopyCurrentDictionary.restype = ctypes.c_void_p
cf.CFDictionaryGetValue.restype = ctypes.c_void_p
cf.CFDictionaryGetValue.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
cf.CFStringCreateWithCString.restype = ctypes.c_void_p
cf.CFStringCreateWithCString.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_uint32]
cf.CFBooleanGetValue.argtypes = [ctypes.c_void_p]
d = cg.CGSessionCopyCurrentDictionary()
v = d and cf.CFDictionaryGetValue(d, cf.CFStringCreateWithCString(None, b"CGSSessionScreenIsLocked", 0x08000100))
sys.exit(0 if v and cf.CFBooleanGetValue(v) else 1)
' 2>/dev/null; then
  echo "shoot.sh: the screen is locked, so the window would never draw; unlock the Mac and run again" >&2
  exit 1
fi
[ -n "$SCENES" ] || SCENES=$(ls "$STORIES" | sed 's/\.toml$//')

if [ "$SCENES" != compose ]; then
  nice -n 10 "$CARGO" build -q $FLAGS -p arkos -p arkosd -p arkos-shoot
  TARGET=$("$CARGO" metadata -q --no-deps --format-version 1 | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
  BIN="$TARGET/$PROFILE"

  # The tool as an app, started through Launch Services: macOS attributes its screen captures to
  # "Arkos Shoot", whatever shell this runs from, and the Developer ID signature keeps that
  # grant across rebuilds. SHOOT_DIRECT=1 (a CI runner, whose shell already holds the grants and
  # has no Developer ID) runs the tool straight from this shell instead.
  TOOL="$HOME/Library/Caches/arkos-shoot/tool/Arkos Shoot.app"
  rm -rf "$TOOL"
  mkdir -p "$TOOL/Contents/MacOS"
  cat > "$TOOL/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>sh.csarko.arkos.shoot-tool</string>
  <key>CFBundleName</key><string>Arkos Shoot</string>
  <key>CFBundleExecutable</key><string>arkos-shoot</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>LSUIElement</key><true/>
</dict></plist>
PLIST
  cp "$BIN/arkos-shoot" "$TOOL/Contents/MacOS/arkos-shoot"
  IDENTITY=$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: .*\)"/\1/p' | head -1)
  if [ -n "$IDENTITY" ]; then
    codesign --force --options runtime --timestamp --sign "$IDENTITY" "$TOOL" 2>/dev/null
  else
    echo "no Developer ID in the keychain: the tool is unsigned, and macOS forgets its Screen Recording grant at every rebuild" >&2
  fi

  for SCENE in $SCENES; do
    STORY="$STORIES/$SCENE.toml"
    [ -f "$STORY" ] || { echo "no story $STORY" >&2; exit 2; }
    LOG="$OUT/progress/$SCENE.log"
    mkdir -p "$OUT/progress"
    rm -f "$OUT/progress/$SCENE/result.txt"
    echo "== $SCENE"
    # -W waits; the environment this shell has (PATH for python3 with Pillow, git, swift; SHELL for
    # the demo's shells) goes along, since a Launch Services start has only launchd's.
    if [ "${SHOOT_DIRECT:-}" = 1 ]; then
      "$BIN/arkos-shoot" run --story "$STORY" --out "$OUT" --bin "$BIN" > "$LOG" 2>&1 || true
    else
      open -W -n -a "$TOOL" --env "PATH=$PATH" --env "SHELL=${SHELL:-/bin/zsh}" --stdout "$LOG" --stderr "$LOG" \
        --args run --story "$STORY" --out "$OUT" --bin "$BIN"
    fi
    cat "$LOG" | grep -v '^/Users' || true
    RESULT=$(cat "$OUT/progress/$SCENE/result.txt" 2>/dev/null || echo "failed: no result written")
    case "$RESULT" in ok*) ;; *) echo "$SCENE: $RESULT" >&2; exit 1 ;; esac
  done
fi

# The hero, from the stills that exist: the multi tasker in front, the others behind.
for THEME in dark light; do
  set --
  for SCENE in multitasker web-developer blogger; do
    STILL="$OUT/$SCENE-$THEME.png"
    [ -f "$STILL" ] || continue
    CAPTION=$(sed -n 's/^caption = "\(.*\)"$/\1/p' "$STORIES/$SCENE.toml")
    set -- "$@" "$STILL:$CAPTION"
  done
  if [ $# -ge 2 ]; then
    python3 -I scripts/shoot/frame.py compose "$OUT/hero-$THEME.png" "$@"
    # The tall one, for a place beside text (csarko.sh's card on a desktop).
    python3 -I scripts/shoot/frame.py compose-portrait "$OUT/hero-portrait-$THEME.png" "$@"
  else
    echo "hero-$THEME: fewer than two stills in $OUT; not composed"
  fi
done
