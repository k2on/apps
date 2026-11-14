#!/usr/bin/env bash
# The iOS simulator runtime the shots are taken on: the one of the pinned
# Xcode's own SDK (`xcrun --sdk iphonesimulator --show-sdk-version`, 26.2
# for Xcode 26.3), where the runner has it, else the newest. A runner image
# with a newer runtime than the pinned Xcode's (macos-26 has 26.5) takes
# the shots on the same iOS as every other runner, so they compare.
#
#   ios-runtime.sh   prints its key (com.apple.CoreSimulator.SimRuntime.iOS-26-2)
set -euo pipefail
sdk=$(xcrun --sdk iphonesimulator --show-sdk-version 2> /dev/null | tr . -)
xcrun simctl list devices available -j | jq -r --arg want "com.apple.CoreSimulator.SimRuntime.iOS-$sdk" '
  [.devices | to_entries[] | select(.key | test("iOS")) | select(.value | length > 0) | .key] as $keys
  | if ($keys | index($want)) then $want else ($keys | sort_by(split("-") | .[1:] | map(tonumber? // 0)) | last) end'
