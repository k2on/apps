#!/usr/bin/env bash
# The iPhone's and the iPad's driver (drive-ios/: the UI test that takes
# the shots), built for any simulator into <derived>, once for every app
# and device: the workflow builds it in the background while the apps
# build and the simulator boots, and apps-ios.sh only runs it. Needs Xcode
# and XcodeGen on the PATH.
#
#   driver-ios.sh <derived>
set -euo pipefail
derived=$1
here=$(cd "$(dirname "$0")" && pwd)
(cd "$here/drive-ios" && xcodegen generate > /dev/null)
xcodebuild build-for-testing -project "$here/drive-ios/Drive.xcodeproj" -scheme Drive \
  -destination 'generic/platform=iOS Simulator' -derivedDataPath "$derived" CODE_SIGNING_ALLOWED=NO -quiet
touch "$derived/built"
