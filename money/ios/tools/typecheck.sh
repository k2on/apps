#!/usr/bin/env bash
# Type-check what of money's iOS app Linux can: every file under Money/ that
# imports nothing of Apple's but Foundation and Combine — the budget's
# arithmetic, the first run's machine, the session and the model — with the
# generated types (Gen/Money.swift), the Swift UniFFI writes for money's
# library and the bridge's typed half, against the library's C headers, a
# stand-in for the two things of Combine's that Model.swift names
# (ObservableObject, @Published) and ArkUI's Foundation half (ArkUICore).
# The screens (SwiftUI, Charts, AuthenticationServices) are left out: none of
# those is on Linux. harken's tools/typecheck.sh, for money.
#
# Then the tests (MoneyTests/) are compiled against the same sources with a
# stand-in for XCTest's assertions, and — when `MONEY_FFI_LIB` names money's
# library built for this machine (`libmoney_ffi.so`) — linked against it and
# run, over the demo's household.
#
#   typecheck.sh BINDINGS   # a directory of `uniffi-bindgen generate --language swift`
set -eu
HERE="$(cd "$(dirname "$0")" && pwd)"
APP="$HERE/../Money"
TESTS="$HERE/../MoneyTests"
BINDINGS="$1"
TYPED="${TYPED:-$HERE/../../../rust/ark-ffi/swift/Typed.swift}"
# ArkUI's Foundation half (swift/ArkUI), which the rows import: built first
# as a module of its own, as Xcode builds the package.
CORE="${ARKUI_CORE:-$HERE/../../../swift/ArkUI/Sources/ArkUICore}"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

# The app's files Linux can read: none of Apple's frameworks but Foundation and Combine.
SOURCES=("$APP/Gen/Money.swift")
for f in "$APP"/*.swift; do
  if ! grep -qE '^import (SwiftUI|UIKit|AVFoundation|AVKit|MediaPlayer|AuthenticationServices|ImageIO|CoreGraphics|UniformTypeIdentifiers|Charts)' "$f"; then
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
cat > "$OUT/XCTest.swift" <<'SWIFT'
// A stand-in for XCTest's assertions, for running MoneyTests on Linux: a
// failure is counted and said, and the runner exits non-zero after.
import Foundation
open class XCTestCase { public init() {} }
public var xcFailures = 0
private func fail(_ what: String, _ message: String, _ file: StaticString, _ line: UInt) {
    xcFailures += 1
    print("\(file):\(line): \(what) failed\(message.isEmpty ? "" : ": " + message)")
}
public func XCTAssertEqual<T: Equatable>(_ a: @autoclosure () throws -> T, _ b: @autoclosure () throws -> T, _ message: @autoclosure () -> String = "", file: StaticString = #filePath, line: UInt = #line) {
    do {
        let x = try a(), y = try b()
        if x != y { fail("XCTAssertEqual", "(\(x)) is not (\(y)) \(message())", file, line) }
    } catch { fail("XCTAssertEqual", "threw \(error)", file, line) }
}
public func XCTAssertTrue(_ a: @autoclosure () throws -> Bool, _ message: @autoclosure () -> String = "", file: StaticString = #filePath, line: UInt = #line) {
    if (try? a()) != true { fail("XCTAssertTrue", message(), file, line) }
}
public func XCTAssertFalse(_ a: @autoclosure () throws -> Bool, _ message: @autoclosure () -> String = "", file: StaticString = #filePath, line: UInt = #line) {
    if (try? a()) != false { fail("XCTAssertFalse", message(), file, line) }
}
public func XCTAssertNil<T>(_ a: @autoclosure () throws -> T?, _ message: @autoclosure () -> String = "", file: StaticString = #filePath, line: UInt = #line) {
    if let v = try? a() { fail("XCTAssertNil", "(\(v)) \(message())", file, line) }
}
public func XCTAssertNotNil<T>(_ a: @autoclosure () throws -> T?, _ message: @autoclosure () -> String = "", file: StaticString = #filePath, line: UInt = #line) {
    if (try? a()) == nil { fail("XCTAssertNotNil", message(), file, line) }
}
public func XCTFail(_ message: String = "", file: StaticString = #filePath, line: UInt = #line) { fail("XCTFail", message, file, line) }
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
echo "money/ios: $(for f in "${SOURCES[@]}"; do printf '%s ' "${f#$APP/}"; done)type-check against money's library"

# The tests: one module with the sources, as `@testable import Money` reads
# them, and a runner calling every `func test…` of every `final class …Tests`.
TESTFILES=("$TESTS"/*.swift)
mkdir -p "$OUT/tests"
for f in "${TESTFILES[@]}"; do
  sed -e '/^@testable import Money/d' -e '/^import XCTest/d' "$f" > "$OUT/tests/$(basename "$f")"
done
{
  echo "import Foundation"
  echo "@main enum Run { static func main() {"
  for f in "${TESTFILES[@]}"; do
    class=$(sed -n 's/^final class \([A-Za-z]*Tests\).*/\1/p' "$f")
    for t in $(sed -n 's/^    func \(test[A-Za-z0-9_]*\)().*/\1/p' "$f"); do
      echo "  print(\"$class.$t\"); $class().$t()"
    done
  done
  echo "  if xcFailures > 0 { print(\"\\(xcFailures) failed\"); exit(1) }"
  echo "  print(\"every test passed\")"
  echo "} }"
} > "$OUT/tests/Run.swift"
if [ -z "${MONEY_FFI_LIB:-}" ]; then
  "$SWIFTC" -typecheck -parse-as-library -module-name Money -I "$OUT" -I "$OUT/ffi" \
    "$BINDINGS"/*.swift "$TYPED" "${SOURCES[@]}" "$OUT"/tests/*.swift "$OUT/XCTest.swift"
  echo "money/ios: MoneyTests type-check (not run: no MONEY_FFI_LIB)"
else
  "$SWIFTC" -parse-as-library -module-name Money -I "$OUT" -I "$OUT/ffi" \
    "$BINDINGS"/*.swift "$TYPED" "${SOURCES[@]}" "$OUT"/tests/*.swift "$OUT/XCTest.swift" \
    -L "$OUT" -lCombine -lArkUICore -L "$(dirname "$MONEY_FFI_LIB")" -lmoney_ffi -Xlinker -rpath -Xlinker "$(dirname "$MONEY_FFI_LIB")" -o "$OUT/tests/run"
  "$OUT/tests/run"
fi
