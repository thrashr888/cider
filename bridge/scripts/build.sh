#!/usr/bin/env bash
# Build (and optionally install) the Cider Bridge Catalyst app and the
# cider-bridge native CLI with your team, or package both for distribution.
#
#   bridge/scripts/build.sh --team 5T4QSYSNP2 [--install] [--debug]
#   bridge/scripts/build.sh --team 5T4QSYSNP2 --distribution
#
# The team id comes from --team, else $CIDER_TEAM_ID, else bridge/.env.local
# (a `CIDER_TEAM_ID=...` line; never committed). Requires XcodeGen
# (`brew install xcodegen`) and Xcode with HomeKit (and WeatherKit) enabled on
# the App ID dev.thrasher.cider.bridge in the developer portal (see the RFC).
#
# --install copies the app to ~/Applications/Cider Bridge.app, puts the CLI at
# Contents/MacOS/cider-bridge inside it (re-sealing the bundle signature), and
# symlinks ~/.local/bin/cider-bridge.
#
# --distribution builds the Homebrew artifact instead (see scripts/release.md):
# a universal app signed with Developer ID and WeatherKit only (Apple does not
# grant HomeKit to Developer ID builds), a universal CLI, both notarized with
# the notarytool keychain profile $CIDER_NOTARY_PROFILE (default alchemy-notary),
# the app stapled, then dist/cider-bridge-<version>-macos-universal.tar.gz.
set -euo pipefail

cd "$(dirname "$0")/.."

TEAM="${CIDER_TEAM_ID:-}"
INSTALL=0
DISTRIBUTION=0
CONFIG=Release

if [ -f .env.local ]; then
  # shellcheck disable=SC1091
  source .env.local
  TEAM="${TEAM:-${CIDER_TEAM_ID:-}}"
fi

usage() {
  sed -n '2,21p' "$0" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
  case "$1" in
    --team) TEAM="$2"; shift 2 ;;
    --install) INSTALL=1; shift ;;
    --distribution) DISTRIBUTION=1; shift ;;
    --debug) CONFIG=Debug; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [ "$INSTALL" = 1 ] && [ "$DISTRIBUTION" = 1 ]; then
  echo "error: --install and --distribution are mutually exclusive" >&2
  exit 2
fi
if [ "$DISTRIBUTION" = 1 ] && [ "$CONFIG" = Debug ]; then
  echo "error: --distribution builds Release only" >&2
  exit 2
fi
if ! command -v xcodegen >/dev/null; then
  echo "error: xcodegen not found (brew install xcodegen)" >&2
  exit 2
fi

# The team's certificate of the given kind in the login keychain.
find_identity() {
  security find-identity -v -p codesigning | grep "$1" | grep -o '"[^"]*"' | head -1 | tr -d '"'
}

if [ "$DISTRIBUTION" = 1 ]; then
  IDENTITY=$(find_identity "Developer ID Application")
  if [ -z "$IDENTITY" ]; then
    echo "error: no 'Developer ID Application' signing identity in the keychain" >&2
    exit 2
  fi
  # The team id is in the identity name: "Developer ID Application: Name (TEAM)".
  TEAM="${TEAM:-$(echo "$IDENTITY" | sed -n 's/.*(\([A-Z0-9]*\))$/\1/p')}"
else
  # Xcode signs the app with the same Apple Development certificate.
  IDENTITY=$(find_identity "Apple Development")
  if [ -z "$IDENTITY" ]; then
    echo "error: no 'Apple Development' signing identity in the keychain" >&2
    exit 2
  fi
fi
if [ -z "$TEAM" ]; then
  echo "error: no team id (use --team <id>, CIDER_TEAM_ID, or bridge/.env.local)" >&2
  exit 2
fi

xcodegen generate --quiet

# --- Distribution -----------------------------------------------------------

if [ "$DISTRIBUTION" = 1 ]; then
  VERSION=$(grep '^version' ../Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
  NOTARY_PROFILE="${CIDER_NOTARY_PROFILE:-alchemy-notary}"
  NAME="cider-bridge-$VERSION-macos-universal"
  WORK=.build/dist
  ARCHIVE="$WORK/CiderBridge.xcarchive"
  EXPORT="$WORK/export"
  STAGE="dist/$NAME"
  TARBALL="dist/$NAME.tar.gz"
  rm -rf "$WORK" "$STAGE" "$TARBALL"
  mkdir -p "$WORK" "$STAGE"

  # Archive: universal, hardened runtime, the WeatherKit-only entitlements.
  # Automatic signing puts a development signature on the archive; the
  # export below re-signs with Developer ID and, with -allowProvisioningUpdates,
  # creates/embeds the Developer ID provisioning profile WeatherKit needs.
  xcodebuild \
    -project CiderBridge.xcodeproj \
    -scheme CiderBridge \
    -configuration Release \
    -destination 'generic/platform=macOS,variant=Mac Catalyst' \
    -archivePath "$ARCHIVE" \
    -derivedDataPath .build/DerivedData \
    -allowProvisioningUpdates \
    CIDER_TEAM_ID="$TEAM" \
    CODE_SIGN_ENTITLEMENTS=Resources/CiderBridge.distribution.entitlements \
    ENABLE_HARDENED_RUNTIME=YES \
    ARCHS="arm64 x86_64" ONLY_ACTIVE_ARCH=NO \
    archive

  cat > "$WORK/ExportOptions.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>method</key>
	<string>developer-id</string>
	<key>signingStyle</key>
	<string>automatic</string>
	<key>teamID</key>
	<string>$TEAM</string>
	<key>destination</key>
	<string>export</string>
</dict>
</plist>
EOF
  xcodebuild \
    -exportArchive \
    -archivePath "$ARCHIVE" \
    -exportOptionsPlist "$WORK/ExportOptions.plist" \
    -exportPath "$EXPORT" \
    -allowProvisioningUpdates
  APP="$EXPORT/Cider Bridge.app"
  test -f "$APP/Contents/embedded.provisionprofile" \
    || { echo "error: no embedded.provisionprofile in the exported app (WeatherKit needs one)" >&2; exit 1; }
  if codesign -d --entitlements - "$APP" 2>/dev/null | grep -q homekit; then
    echo "error: the distribution app carries the HomeKit entitlement" >&2
    exit 1
  fi
  echo "exported: $APP"

  # The CLI: universal, Developer ID, hardened runtime, timestamped.
  swift build -c release --arch arm64 --arch x86_64 --product cider-bridge
  CLI="$(swift build -c release --arch arm64 --arch x86_64 --show-bin-path)/cider-bridge"
  lipo -info "$CLI"
  codesign --force --sign "$IDENTITY" --options runtime --timestamp "$CLI"
  echo "built: $CLI (signed: $IDENTITY)"

  # Notarize both (the app zipped, the CLI zipped), then staple the app; a
  # bare executable cannot hold a staple, Gatekeeper checks it online.
  ditto -c -k --keepParent "$APP" "$WORK/app.zip"
  ditto -c -k "$CLI" "$WORK/cli.zip"
  for zip in app cli; do
    echo "notarizing $zip with keychain profile '$NOTARY_PROFILE'..."
    xcrun notarytool submit "$WORK/$zip.zip" --keychain-profile "$NOTARY_PROFILE" --wait \
      | tee "$WORK/notary-$zip.log"
    grep -q 'status: Accepted' "$WORK/notary-$zip.log" \
      || { echo "error: notarization of $zip was not accepted (see $WORK/notary-$zip.log)" >&2; exit 1; }
  done
  xcrun stapler staple "$APP"
  spctl -a -vv -t exec "$APP"

  # Package.
  ditto "$APP" "$STAGE/Cider Bridge.app"
  ditto "$CLI" "$STAGE/cider-bridge"
  tar -C "$STAGE" -czf "$TARBALL" "Cider Bridge.app" cider-bridge
  echo "packaged: $TARBALL"
  shasum -a 256 "$TARBALL"
  exit 0
fi

# --- Catalyst app -----------------------------------------------------------

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
