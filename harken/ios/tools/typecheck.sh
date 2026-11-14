#!/usr/bin/env bash
# Type-check what of the iOS app Linux can: every file under Harken/ that
# imports nothing of Apple's but Foundation and Combine — the rows, the
# listening session, what plays, the onboarding's machine, the player's core,
# the bridge and the model — with the generated types (Gen/Harken.swift), the
# Swift UniFFI writes for harken's library and the bridge's typed half,
# against the library's C headers and a stand-in for the two things of
# Combine's that Model.swift names (ObservableObject, @Published). The files
# that import SwiftUI, UIKit, AVFoundation, MediaPlayer, AuthenticationServices
# or ImageIO — the screens, the AVPlayer engine, the lock screen, the sign-in
# sheet and the covers — are left out: none of those is on Linux.
#
#   typecheck.sh BINDINGS   # a directory of `uniffi-bindgen generate --language swift`
set -eu
HERE="$(cd "$(dirname "$0")" && pwd)"
APP="$HERE/../Harken"
BINDINGS="$1"
TYPED="${TYPED:-$HERE/../../../rust/ark-ffi/swift/Typed.swift}"
# ArkUI's Foundation half (ui/swift), which the rows import: built first
# as a module of its own, as Xcode builds the package.
CORE="${ARKUI_CORE:-$HERE/../../../ui/swift/Sources/ArkUICore}"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

# The app's files Linux can read: none of Apple's frameworks but Foundation and Combine.
SOURCES=("$APP/Gen/Harken.swift")
for f in "$APP"/*.swift; do
  if ! grep -qE '^import (SwiftUI|UIKit|AVFoundation|AVKit|MediaPlayer|AuthenticationServices|ImageIO|CoreGraphics|UniformTypeIdentifiers)' "$f"; then
    SOURCES+=("$f")
  fi
done

cat > "$OUT/Combine.swift" <<'SWIFT'
// A stand-in for what Model.swift names of Combine, for type-checking on Linux.
public protocol ObservableObject: AnyObject {}
@propertyWrapper public struct Published<Value> {
    public var wrappedValue: Value
    public init(wrappedValue: Value) { self.wrappedValue = wrappedValue }
}
SWIFT
mkdir -p "$OUT/ffi"
cp "$BINDINGS"/*.h "$OUT/ffi/"
cat "$BINDINGS"/*.modulemap > "$OUT/ffi/module.modulemap"
SWIFTC="${SWIFT_EXEC:-swiftc}"
"$SWIFTC" -emit-library -static -emit-module -module-name Combine -parse-as-library "$OUT/Combine.swift" \
  -emit-module-path "$OUT/Combine.swiftmodule" -o "$OUT/libCombine.a"
"$SWIFTC" -emit-library -static -emit-module -module-name ArkUICore -parse-as-library "$CORE"/*.swift \
  -emit-module-path "$OUT/ArkUICore.swiftmodule" -o "$OUT/libArkUICore.a"
"$SWIFTC" -typecheck -parse-as-library -module-name Harken -I "$OUT" -I "$OUT/ffi" \
  "$BINDINGS"/*.swift "$TYPED" "${SOURCES[@]}"
echo "harken/ios: $(for f in "${SOURCES[@]}"; do printf '%s ' "${f#$APP/}"; done)type-check against harken's library"

