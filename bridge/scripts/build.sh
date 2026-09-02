#!/usr/bin/env bash
# Build (and optionally install) the Cider Bridge Catalyst app with your team.
#
#   bridge/scripts/build.sh --team 5T4QSYSNP2 [--install] [--debug]
#
# The team id comes from --team, else $CIDER_TEAM_ID, else bridge/.env.local
# (a `CIDER_TEAM_ID=...` line; never committed). Requires XcodeGen
# (`brew install xcodegen`) and Xcode with HomeKit enabled on the App ID
# dev.thrasher.cider.bridge in the developer portal (see the RFC).
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
  sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'
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

if [ "$INSTALL" = 1 ]; then
  DEST="$HOME/Applications/Cider Bridge.app"
  mkdir -p "$HOME/Applications"
  rm -rf "$DEST"
  ditto "$APP" "$DEST"
  echo "installed: $DEST"
fi
