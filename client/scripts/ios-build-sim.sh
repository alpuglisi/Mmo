#!/usr/bin/env bash
# Build the iOS app for the simulator, unsigned (no Apple account needed),
# from the generated project (`npx cap add ios`, then `npx cap sync ios`
# after client changes), and copy App.app to an output directory.
#
#   client/scripts/ios-build-sim.sh [out-dir]        (default ios-out/sim, from the repository root)
#
# Debug configuration (CAPACITOR_DEBUG=true: the WKWebView is inspectable,
# which the simulator smoke run needs). Works with Capacitor 8's default
# Swift Package Manager project (ios/App/App.xcodeproj) and with a CocoaPods
# one (ios/App/App.xcworkspace, after `pod install`).
#
# Env:
#   CONFIGURATION       Debug (default) or Release
#   DERIVED_DATA        xcodebuild -derivedDataPath (default ios-out/build)
#   SPM_PACKAGES        -clonedSourcePackagesDirPath (default ios-out/spm; cached in CI)
#   XCODEBUILD          xcodebuild binary (default xcodebuild)
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
client="$(cd "$here/.." && pwd)"
root="$(cd "$client/.." && pwd)"

OUT="${1:-$root/ios-out/sim}"
CONFIGURATION="${CONFIGURATION:-Debug}"
DERIVED_DATA="${DERIVED_DATA:-$root/ios-out/build}"
SPM_PACKAGES="${SPM_PACKAGES:-$root/ios-out/spm}"
XCODEBUILD="${XCODEBUILD:-xcodebuild}"
ios="$client/ios/App"

die() { echo "ios-build-sim: $*" >&2; exit 1; }

[ -d "$ios" ] || die "no iOS project at $ios (cd client && npm run build && npx cap add ios)"
if [ -d "$ios/App.xcworkspace" ]; then
  container=(-workspace "$ios/App.xcworkspace")
elif [ -d "$ios/App.xcodeproj" ]; then
  container=(-project "$ios/App.xcodeproj")
else
  die "neither App.xcworkspace nor App.xcodeproj in $ios"
fi

echo "== xcodebuild ${container[*]} -scheme App -configuration $CONFIGURATION (iphonesimulator, unsigned)"
"$XCODEBUILD" -version || true
"$XCODEBUILD" "${container[@]}" \
  -scheme App \
  -configuration "$CONFIGURATION" \
  -sdk iphonesimulator \
  -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath "$DERIVED_DATA" \
  -clonedSourcePackagesDirPath "$SPM_PACKAGES" \
  CODE_SIGNING_ALLOWED=NO \
  COMPILER_INDEX_STORE_ENABLE=NO \
  build

app="$DERIVED_DATA/Build/Products/$CONFIGURATION-iphonesimulator/App.app"
[ -d "$app" ] || die "xcodebuild succeeded but there is no $app"
mkdir -p "$OUT"
rm -rf "$OUT/App.app"
cp -R "$app" "$OUT/App.app"
echo "== built $OUT/App.app"
