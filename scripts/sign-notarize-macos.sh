#!/usr/bin/env bash
# Sign Pustman.app with a Developer ID, wrap it in a drag-to-Applications .dmg, notarize
# the .dmg with Apple and staple the ticket, so the download opens like any other app.
# Runs on a macOS CI runner; every credential comes from the environment (GitHub secrets)
# and lives only in a temporary keychain and temp files that are removed on exit.
#
# Usage: scripts/sign-notarize-macos.sh path/to/Pustman.app path/to/output.dmg
# Env:   MACOS_CERT_P12_BASE64, MACOS_CERT_PASSWORD, MACOS_SIGN_IDENTITY,
#        APPLE_API_KEY_P8_BASE64, APPLE_API_KEY_ID, APPLE_API_ISSUER_ID
set -euo pipefail

app="$1"
dmg="$2"
: "${MACOS_CERT_P12_BASE64:?}" "${MACOS_CERT_PASSWORD:?}" "${MACOS_SIGN_IDENTITY:?}"
: "${APPLE_API_KEY_P8_BASE64:?}" "${APPLE_API_KEY_ID:?}" "${APPLE_API_ISSUER_ID:?}"

work="$(mktemp -d)"
keychain="$work/signing.keychain-db"
keychain_password="$(uuidgen)"
cleanup() {
    security delete-keychain "$keychain" >/dev/null 2>&1 || true
    rm -rf "$work"
}
trap cleanup EXIT

# A throwaway keychain holding only the Developer ID certificate.
printf '%s' "$MACOS_CERT_P12_BASE64" | base64 --decode > "$work/cert.p12"
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$work/cert.p12" -k "$keychain" -P "$MACOS_CERT_PASSWORD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$keychain_password" "$keychain" >/dev/null
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')

# Hardened runtime + secure timestamp: both are required for notarization.
codesign --force --deep --options runtime --timestamp \
    --keychain "$keychain" --sign "$MACOS_SIGN_IDENTITY" "$app"
codesign --verify --deep --strict --verbose=2 "$app"
# The certificate chain (names only, no secrets): notarization needs "Developer ID
# Application", not "Apple Distribution" or "Apple Development".
codesign --display --verbose=2 "$app" 2>&1 | grep -E '^(Authority|TeamIdentifier|Runtime|Timestamp)='

# The drag-to-Applications disk image.
staging="$work/dmg"
mkdir -p "$staging"
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"
rm -f "$dmg"
hdiutil create -volname "Pustman" -srcfolder "$staging" -ov -format UDZO "$dmg"
codesign --force --timestamp --keychain "$keychain" --sign "$MACOS_SIGN_IDENTITY" "$dmg"

# Notarize with the App Store Connect API key, wait for Apple's verdict, staple it.
printf '%s' "$APPLE_API_KEY_P8_BASE64" | base64 --decode > "$work/AuthKey.p8"
auth=(--key "$work/AuthKey.p8" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER_ID")
xcrun notarytool submit "$dmg" "${auth[@]}" --wait --timeout 30m \
    --output-format json > "$work/submission.json" || true
cat "$work/submission.json"
submission_id="$(plutil -extract id raw -o - "$work/submission.json")"
status="$(plutil -extract status raw -o - "$work/submission.json")"
if [[ "$status" != "Accepted" ]]; then
    # Apple's own report of what it rejected (file paths and issue messages only).
    echo "::error::notarization $status — Apple's log follows"
    xcrun notarytool log "$submission_id" "${auth[@]}" || true
    exit 1
fi
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"
spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg"

echo "signed, notarized and stapled $dmg"
