#!/usr/bin/env bash
# Type-check what of the iOS app Linux can: Rows.swift, Bridge.swift and
# Model.swift, against the ArkDB and ArkDBClient modules SwiftPM built in
# ../../swift (debug, or release with RELEASE=1) and a stand-in for the
# three things of SwiftUI's that Model.swift names — ObservableObject,
# @Published and Color — so that what is checked is the app's use of the
# client, not SwiftUI. The views are not checked: SwiftUI is not on Linux.
#
#   cd swift && nix develop ..#swift -c tools/swift.sh build
#   nix develop .#swift -c harken/ios/tools/typecheck.sh
set -eu
HERE="$(cd "$(dirname "$0")" && pwd)"
APP="$HERE/../Harken"
CONFIG=debug
if [ -n "${RELEASE:-}" ]; then CONFIG=release; fi
BUILD="${SWIFT_BUILD:-$HERE/../../../swift/.build/$CONFIG}"
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
SWIFTC="${SWIFT_EXEC:-swiftc}"
"$SWIFTC" -emit-module -module-name SwiftUI -parse-as-library "$OUT/SwiftUI.swift" -emit-module-path "$OUT/SwiftUI.swiftmodule"
"$SWIFTC" -typecheck -parse-as-library -I "$BUILD" -I "$OUT" "$APP/Rows.swift" "$APP/Bridge.swift" "$APP/Model.swift"
echo "harken/ios: Rows.swift, Bridge.swift and Model.swift type-check against ArkDBClient"
