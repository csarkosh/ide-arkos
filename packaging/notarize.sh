#!/bin/sh
# notarize.sh ZIP: sends the zip (a bundle signed with a Developer ID, bundle.sh with
# CODESIGN_IDENTITY) to Apple's notary service, waits for the verdict, staples the ticket to the
# app beside the zip, and rewrites the zip from the stapled app. Run it before manifest.sh: the
# manifest records the final zip's size and hash.
#
# Credentials, one of:
#   NOTARY_KEY, NOTARY_KEY_ID, NOTARY_ISSUER   an App Store Connect API key (.p8 path, its id,
#                                              the issuer id), what the release workflow uses
#   NOTARY_PROFILE                             a keychain profile made once on this Mac with
#                                              xcrun notarytool store-credentials <profile>
set -eu
ZIP="$1"
OUT=$(dirname "$ZIP")
APP="$OUT/Arkos IDE.app"
[ -d "$APP" ] || { echo "notarize.sh: $APP is not beside $ZIP" >&2; exit 1; }
if [ -n "${NOTARY_KEY:-}" ]; then
  set -- --key "$NOTARY_KEY" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER"
elif [ -n "${NOTARY_PROFILE:-}" ]; then
  set -- --keychain-profile "$NOTARY_PROFILE"
else
  echo "notarize.sh: set NOTARY_KEY, NOTARY_KEY_ID and NOTARY_ISSUER, or NOTARY_PROFILE" >&2
  exit 1
fi
LOG=$(mktemp)
if ! xcrun notarytool submit "$ZIP" "$@" --wait --output-format json > "$LOG"; then
  cat "$LOG" >&2
  exit 1
fi
STATUS=$(sed -n 's/.*"status" *: *"\([^"]*\)".*/\1/p' "$LOG" | tail -1)
ID=$(sed -n 's/.*"id" *: *"\([^"]*\)".*/\1/p' "$LOG" | head -1)
if [ "$STATUS" != "Accepted" ]; then
  echo "notarize.sh: Apple said $STATUS for submission $ID; the log:" >&2
  xcrun notarytool log "$ID" "$@" >&2 || true
  exit 1
fi
xcrun stapler staple "$APP"
xcrun stapler validate "$APP"
rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"
echo "$ZIP"
