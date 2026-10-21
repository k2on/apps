#!/usr/bin/env bash
# Type-check what of the iOS app Linux can: Rows.swift, Bridge.swift and
# Model.swift with the generated types (Gen/Harken.swift), the Swift UniFFI
# writes for harken's library and the bridge's typed half — against the
# library's C headers, and a stand-in for the three things of SwiftUI's that
# Model.swift names (ObservableObject, @Published, Color) — so that what is
# checked is the app's use of the bridge, not SwiftUI. The views are not
# checked: SwiftUI is not on Linux.
#
#   typecheck.sh BINDINGS   # a directory of `uniffi-bindgen generate --language swift`
set -eu
HERE="$(cd "$(dirname "$0")" && pwd)"
APP="$HERE/../Harken"
BINDINGS="$1"
TYPED="${TYPED:-$HERE/../../../rust/ark-ffi/swift/Typed.swift}"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT
cat > "$OUT/SwiftUI.swift" <<'SWIFT'
// A stand-in for what Model.swift names of SwiftUI, for type-checking on Linux.
public protocol ObservableObject: AnyObject {}
@propertyWrapper public struct Published<Value> {
    public var wrappedValue: Value
    public init(wrappedValue: Value) { self.wrappedValue = wrappedValue }
}
public struct Color {
    public static let gray = Color(), red = Color(), blue = Color(), green = Color(), orange = Color()
}
SWIFT
mkdir -p "$OUT/ffi"
cp "$BINDINGS"/*.h "$OUT/ffi/"
cat "$BINDINGS"/*.modulemap > "$OUT/ffi/module.modulemap"
SWIFTC="${SWIFT_EXEC:-swiftc}"
"$SWIFTC" -emit-module -module-name SwiftUI -parse-as-library "$OUT/SwiftUI.swift" -emit-module-path "$OUT/SwiftUI.swiftmodule"
"$SWIFTC" -typecheck -parse-as-library -module-name Harken -I "$OUT" -I "$OUT/ffi" \
  "$BINDINGS"/*.swift "$TYPED" "$APP/Gen/Harken.swift" "$APP/Rows.swift" "$APP/Bridge.swift" "$APP/Model.swift"
echo "harken/ios: Rows.swift, Bridge.swift, Model.swift and Gen/Harken.swift type-check against harken's library"
