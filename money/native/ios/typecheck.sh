#!/usr/bin/env bash
# Type-check what of money's iOS app Linux can: every file under Money/ that
# imports nothing of Apple's but Foundation and Combine (its rows' and its
# money's), and the app library's screen holder (ui/swift's ArkApp,
# ArkScreen.swift) — with the Swift UniFFI writes for money's library and the
# bridge's typed half, against the library's C headers, a stand-in for the
# two things of Combine's that ArkScreen names (ObservableObject,
# @Published) and ArkUI's Foundation half (ArkUICore).
# The screens (SwiftUI, Charts, AuthenticationServices) are left out: none of
# those is on Linux. harken's tools/typecheck.sh, for money.
#
#   typecheck.sh BINDINGS   # a directory of `uniffi-bindgen generate --language swift`
set -eu
HERE="$(cd "$(dirname "$0")" && pwd)"
APP="$HERE/Money"
BINDINGS="$1"
TYPED="${TYPED:-$HERE/../../../rust/ark-ffi/swift/Typed.swift}"
# ArkUI's Foundation half (ui/swift), which the rows import: built first
# as a module of its own, as Xcode builds the package.
CORE="${ARKUI_CORE:-$HERE/../../../ui/swift/Sources/ArkUICore}"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

# The app's files Linux can read: none of Apple's frameworks but Foundation and Combine.
SOURCES=()
# And the device modules money takes on (money-app's dependencies: nix
# hands them as ARK_MODULES, ui/prebuild.py's `modules` says them here):
# their Swift is behind `canImport(UIKit)` / `canImport(AppKit)`, so on
# Linux it is the classes over the protocols UniFFI wrote, and each name
# is checked.
MODULES="${MODULES:-$HERE/../../../modules}"
for m in ${ARK_MODULES:-$(python3 "$HERE/../../../ui/prebuild.py" modules money)}; do
  for f in "$MODULES/$m"/swift/*.swift; do
    [ -e "$f" ] && SOURCES+=("$f")
  done
done
SOURCES+=("$HERE/../../../ui/swift/Sources/ArkApp/ArkScreen.swift")
for f in "$APP"/*.swift; do
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
"$SWIFTC" -typecheck -parse-as-library -module-name Money -I "$OUT" -I "$OUT/ffi" \
  "$BINDINGS"/*.swift "$TYPED" "${SOURCES[@]}"
echo "money/native/ios: $(for f in "${SOURCES[@]}"; do printf '%s ' "${f#$APP/}"; done)type-check against money's library"

