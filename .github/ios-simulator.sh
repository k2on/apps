#!/usr/bin/env bash
# `ios-simulator.sh <App>`: the app's Xcode project (generated already, in
# <app>/ios) built in Release for the simulator, as this commit, and tarred
# to $RUNNER_TEMP/simulator.tgz so its executable keeps its mode through an
# artifact: what app-screens.yaml takes the app's iOS screenshots from
# (.github/screens/apps.md), since a TestFlight build runs on a phone alone.
# BUNDLE_ID is the app's bundle id, as the archive was given it.
set -euo pipefail
App=$1
app=$(tr '[:upper:]' '[:lower:]' <<< "$App")
cd "$app/ios"
xcodebuild build \
  -project "$App.xcodeproj" \
  -scheme "$App" \
  -configuration Release \
  -sdk iphonesimulator \
  -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath "$RUNNER_TEMP/simulator" \
  ARCHS=arm64 \
  CODE_SIGNING_ALLOWED=NO \
  PRODUCT_BUNDLE_IDENTIFIER="$BUNDLE_ID" \
  CURRENT_PROJECT_VERSION="$(git log -1 --format=%ct)"
tar -czf "$RUNNER_TEMP/simulator.tgz" -C "$RUNNER_TEMP/simulator/Build/Products/Release-iphonesimulator" "$App.app"
