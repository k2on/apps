#!/usr/bin/env bash
# Type-check what of an Ark app's Swift Linux can, against that app's
# library: every file of the app library (ArkApp) that imports nothing of
# Apple's but Foundation and Combine (the screen holder, how money is
# written) and the device modules the app takes on — with the Swift UniFFI
# writes for the app's library and the bridge's typed half, against the
# library's C headers, a stand-in for the two things of Combine's that
# ArkScreen names (ObservableObject, @Published) and ArkUI's Foundation half
# (ArkUICore). The shell and the screens (SwiftUI, UIKit) are left out: none
# of those is on Linux. Nothing of an app's own is Swift.
#
#   typecheck.sh APP BINDINGS   # BINDINGS: a directory of `uniffi-bindgen generate --language swift`
set -eu
HERE="$(cd "$(dirname "$0")" && pwd)"
APP="$1"
BINDINGS="$2"
LIB="$HERE/Sources/ArkApp"
TYPED="${TYPED:-$HERE/../../rust/ark-ffi/swift/Typed.swift}"
# ArkUI's Foundation half (ui/swift), which the library imports: built first
# as a module of its own, as Xcode builds the package.
CORE="${ARKUI_CORE:-$HERE/Sources/ArkUICore}"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

# The files Linux can read: none of Apple's frameworks but Foundation and Combine.
SOURCES=()
# The device modules the app takes on (its crate's dependencies: nix
# hands them as ARK_MODULES, ui/prebuild.py's `modules` says them here):
# their Swift is behind `canImport(UIKit)` / `canImport(AppKit)`, so on
# Linux it is the classes over the protocols UniFFI wrote, and each name
# is checked.
MODULES="${MODULES:-$HERE/../../modules}"
for m in ${ARK_MODULES:-$(python3 "$HERE/../prebuild.py" modules "$APP")}; do
  for f in "$MODULES/$m"/swift/*.swift; do
    [ -e "$f" ] && SOURCES+=("$f")
  done
done
for f in "$LIB"/*.swift; do
  if ! grep -qE '^import (SwiftUI|UIKit|AVFoundation|AVKit|MediaPlayer|AuthenticationServices|ImageIO|CoreGraphics|UniformTypeIdentifiers|Charts)' "$f"; then
    SOURCES+=("$f")
  fi
done

cat > "$OUT/Combine.swift" <<'SWIFT'
// A stand-in for what ArkScreen.swift names of Combine, for type-checking on Linux.
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
"$SWIFTC" -typecheck -parse-as-library -module-name App -I "$OUT" -I "$OUT/ffi" \
  "$BINDINGS"/*.swift "$TYPED" "${SOURCES[@]}"
echo "ui/swift: $(for f in "${SOURCES[@]}"; do printf '%s ' "${f##*/}"; done)type-check against $APP's library"

