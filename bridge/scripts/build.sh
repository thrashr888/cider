#!/usr/bin/env bash
# Build (and optionally install) the Cider Bridge Catalyst app and the
# cider-bridge native CLI with your team.
#
#   bridge/scripts/build.sh --team 5T4QSYSNP2 [--install] [--debug]
#
# The team id comes from --team, else $CIDER_TEAM_ID, else bridge/.env.local
# (a `CIDER_TEAM_ID=...` line; never committed). Requires XcodeGen
# (`brew install xcodegen`) and Xcode with HomeKit (and WeatherKit) enabled on
# the App ID dev.thrasher.cider.bridge in the developer portal (see the RFC).
#
# --install copies the app to ~/Applications/Cider Bridge.app, puts the CLI at
# Contents/MacOS/cider-bridge inside it (re-sealing the bundle signature), and
# symlinks ~/.local/bin/cider-bridge.
set -euo pipefail

cd "$(dirname "$0")/.."

TEAM="${CIDER_TEAM_ID:-}"
INSTALL=0
CONFIG=Release

if [ -f .env.local ]; then
  # shellcheck disable=SC1091
  source .env.local
  TEAM="${TEAM:-${CIDER_TEAM_ID:-}}"
fi

usage() {
  sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
  case "$1" in
    --team) TEAM="$2"; shift 2 ;;
    --install) INSTALL=1; shift ;;
    --debug) CONFIG=Debug; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [ -z "$TEAM" ]; then
  echo "error: no team id (use --team <id>, CIDER_TEAM_ID, or bridge/.env.local)" >&2
  exit 2
fi
if ! command -v xcodegen >/dev/null; then
  echo "error: xcodegen not found (brew install xcodegen)" >&2
  exit 2
fi

# The signing identity for the CLI: the team's Apple Development certificate
# in the login keychain (Xcode signs the app with the same one).
IDENTITY=$(security find-identity -v -p codesigning | grep "Apple Development" | grep -o '"[^"]*"' | head -1 | tr -d '"')
if [ -z "$IDENTITY" ]; then
  echo "error: no 'Apple Development' signing identity in the keychain" >&2
  exit 2
fi

# --- Catalyst app -----------------------------------------------------------

xcodegen generate --quiet
xcodebuild \
  -project CiderBridge.xcodeproj \
  -scheme CiderBridge \
  -configuration "$CONFIG" \
  -destination 'platform=macOS,variant=Mac Catalyst' \
  -derivedDataPath .build/DerivedData \
  -allowProvisioningUpdates \
  -allowProvisioningDeviceRegistration \
  CIDER_TEAM_ID="$TEAM" \
  build

APP=".build/DerivedData/Build/Products/$CONFIG-maccatalyst/Cider Bridge.app"
echo "built: $APP"

# --- Native CLI -------------------------------------------------------------

SWIFT_CONFIG=release
[ "$CONFIG" = Debug ] && SWIFT_CONFIG=debug
swift build -c "$SWIFT_CONFIG" --product cider-bridge
CLI=".build/$SWIFT_CONFIG/cider-bridge"
codesign --force --sign "$IDENTITY" --options runtime "$CLI"
echo "built: $CLI (signed: $IDENTITY)"

# --- Install ----------------------------------------------------------------

if [ "$INSTALL" = 1 ]; then
  DEST="$HOME/Applications/Cider Bridge.app"
  mkdir -p "$HOME/Applications"
  rm -rf "$DEST"
  ditto "$APP" "$DEST"

  # The CLI lives inside the bundle; adding it breaks the bundle's seal, so
  # re-sign the app in place, keeping its identifier, entitlements, and flags.
  CLI_DEST="$DEST/Contents/MacOS/cider-bridge"
  ditto "$CLI" "$CLI_DEST"
  codesign --force --sign "$IDENTITY" --preserve-metadata=identifier,entitlements,flags,runtime "$DEST"
  codesign --verify --deep --strict "$DEST"
  echo "installed: $DEST"
  echo "installed: $CLI_DEST"

  mkdir -p "$HOME/.local/bin"
  ln -sfn "$CLI_DEST" "$HOME/.local/bin/cider-bridge"
  echo "linked: $HOME/.local/bin/cider-bridge -> $CLI_DEST"
fi
