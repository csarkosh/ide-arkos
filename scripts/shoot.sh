#!/bin/sh
# Takes Arkos IDE's screenshots from a dev build: builds arkos, arkosd and arkos-shoot, then runs
# the storyboard in scripts/shoot/story.toml. Real Claude Code sessions run in the demo project
# (they use your Claude Code account for a few minutes), a second arkosd of its own keeps the
# Arkos you are using untouched, and the window is captured with screencapture, which needs
# the Screen Recording permission for the app this runs in. Keep your hands off the keyboard
# and mouse while it runs: the window must stay where it is and in front.
#
#   scripts/shoot.sh              # debug build, images into docs/images/
#   scripts/shoot.sh --release    # the release build
#   OUT=/tmp/shots scripts/shoot.sh
set -eu
cd "$(dirname "$0")/.."
CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"
PROFILE=debug
FLAGS=""
if [ "${1:-}" = "--release" ]; then PROFILE=release; FLAGS="--release"; fi
nice -n 10 "$CARGO" build -q $FLAGS -p arkos -p arkosd -p arkos-shoot
TARGET=$("$CARGO" metadata -q --no-deps --format-version 1 | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
exec "$TARGET/$PROFILE/arkos-shoot" run --story scripts/shoot/story.toml --out "${OUT:-docs/images}" --bin "$TARGET/$PROFILE"
