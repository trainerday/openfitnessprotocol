#!/bin/sh
# Builds OFP Studio.app and its .dmg (crates/ofp-studio-app), signed and notarized when this computer can.
#
# Signing: a "Developer ID Application" certificate in the keychain. Set APPLE_SIGNING_IDENTITY to choose
# one; otherwise the first one found is used. With none, the app is built unsigned (it only opens on
# this computer without a warning).
# Notarizing: an App Store Connect API key, under the names our other desktop builds use:
#   APPLE_API_KEY      path to the .p8 file
#   APPLE_API_KEY_ID   the key's id
#   APPLE_API_ISSUER   the issuer id
# Tauri wants the id in APPLE_API_KEY and the path in APPLE_API_KEY_PATH, so they are passed on that way.
set -e
cd "$(dirname "$0")/.."

if [ -z "$APPLE_SIGNING_IDENTITY" ]; then
  APPLE_SIGNING_IDENTITY=$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: .*\)"/\1/p' | head -1)
fi
if [ -n "$APPLE_SIGNING_IDENTITY" ]; then
  export APPLE_SIGNING_IDENTITY
  echo "Signing as: $APPLE_SIGNING_IDENTITY"
else
  echo "No Developer ID certificate found: building unsigned."
fi

if [ -n "$APPLE_API_KEY_ID" ] && [ -f "$APPLE_API_KEY" ] && [ -n "$APPLE_API_ISSUER" ]; then
  export APPLE_API_KEY_PATH="$APPLE_API_KEY"
  export APPLE_API_KEY="$APPLE_API_KEY_ID"
  echo "Notarizing with API key $APPLE_API_KEY"
else
  unset APPLE_API_KEY APPLE_API_KEY_PATH APPLE_API_ISSUER
  echo "No App Store Connect API key in the environment: not notarizing."
fi

(cd crates/ofp-studio-app && npx tauri build "$@")

# Tauri notarizes the app inside the disk image, not the image itself. A downloaded image that is not
# notarized can be refused on another Mac, so it is notarized and stapled here as well.
if [ -n "$APPLE_API_KEY_PATH" ]; then
  for dmg in target/release/bundle/dmg/*.dmg target/*-apple-darwin/release/bundle/dmg/*.dmg; do
    [ -f "$dmg" ] || continue
    echo "Notarizing $dmg"
    xcrun notarytool submit "$dmg" --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER" --wait
    xcrun stapler staple "$dmg"
  done
fi
