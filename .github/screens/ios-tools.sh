#!/usr/bin/env bash
# What ios.sh records with, built into <dir>: the recorder (record.swift),
# every frame the simulator composes, from its framebuffer, in place of
# simctl's recordVideo, which keeps a sixth of them; and the time warp
# loaded into the showcase (timewarp.c), signed as the simulator asks.
# Needs only Xcode, so it is built while the simulator boots.
#
#   ios-tools.sh <dir>
set -euo pipefail
dir=$1
mkdir -p "$dir"
xcrun swiftc -O .github/screens/record.swift -o "$dir/record"
xcrun -sdk iphonesimulator clang -arch arm64 -mios-simulator-version-min=17.0 -O2 -Wall \
  -dynamiclib -o "$dir/timewarp.dylib" .github/screens/timewarp.c
codesign --force --sign - "$dir/timewarp.dylib"
