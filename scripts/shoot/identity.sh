#!/bin/sh
# Makes the "Arkos Shoot" code-signing identity, once: a self-signed certificate in the login
# keychain, trusted for code signing. The shoot signs its app bundle with it, which gives the
# bundle an identity that survives rebuilds; an ad-hoc signature's identity is the binary's
# hash, so macOS forgot the Accessibility grant (the browser pane needs it) at every rebuild.
# Two prompts: the login keychain password (to add the key) and an administrator's (to trust
# the certificate). After this, add the bundle under System Settings > Privacy & Security >
# Accessibility once more, and never again.
set -eu
NAME="Arkos Shoot"
if security find-identity -v -p codesigning 2>/dev/null | grep -q "\"$NAME\""; then
  echo "the \"$NAME\" identity exists already"
  exit 0
fi
# macOS's own openssl (LibreSSL): a PKCS12 from Homebrew's OpenSSL 3 uses a MAC `security import` cannot read.
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
/usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -keyout "$TMP/key.pem" -out "$TMP/cert.pem" \
  -subj "/CN=$NAME" -addext "keyUsage=critical,digitalSignature" -addext "extendedKeyUsage=critical,codeSigning" 2>/dev/null
/usr/bin/openssl pkcs12 -export -inkey "$TMP/key.pem" -in "$TMP/cert.pem" -out "$TMP/identity.p12" -passout pass:shoot -name "$NAME"
# -T lets codesign use the key without a prompt at every signing.
security import "$TMP/identity.p12" -k "$HOME/Library/Keychains/login.keychain-db" -P shoot -T /usr/bin/codesign
# Trusted for code signing by this user (an administrator's password is asked for).
security add-trusted-cert -d -r trustRoot -p codeSign -k "$HOME/Library/Keychains/login.keychain-db" "$TMP/cert.pem"
security find-identity -v -p codesigning | grep "\"$NAME\"" || { echo "the identity was not created"; exit 1; }
echo "done: the shoot signs its bundle as \"$NAME\" from now on"
