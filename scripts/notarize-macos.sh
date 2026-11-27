#!/usr/bin/env bash
# Submits a .zip or .dmg to Apple's notary service, waits for the verdict and
# fails unless it is "Accepted". Credentials come from the environment:
# APPLE_ID, APPLE_TEAM_ID and APPLE_APP_PASSWORD (an app-specific password).
#
# Usage: scripts/notarize-macos.sh FILE
set -euo pipefail

FILE="${1:?usage: notarize-macos.sh FILE}"
: "${APPLE_ID:?APPLE_ID is not set}"
: "${APPLE_TEAM_ID:?APPLE_TEAM_ID is not set}"
: "${APPLE_APP_PASSWORD:?APPLE_APP_PASSWORD is not set}"

creds=(--apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD")

echo "==> notarizing $FILE"
result="$(xcrun notarytool submit "$FILE" "${creds[@]}" --wait --timeout 30m --output-format json)"
id="$(plutil -extract id raw -o - - <<<"$result")"
status="$(plutil -extract status raw -o - - <<<"$result")"
echo "submission $id: $status"
if [[ "$status" != "Accepted" ]]; then
  xcrun notarytool log "$id" "${creds[@]}" || true
  exit 1
fi
