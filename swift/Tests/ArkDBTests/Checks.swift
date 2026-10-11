// The check of each vector kind, as spec/RUNNERS.md says it and
// rust/ark/tests/vectors.rs reads it: each throws `Failed` with its first
// unmet claim, so that one definition runs both ways — a vector must pass
// it and a `falsify/` vector must fail it.
import ArkDB

enum Checks {
    static var root = "../spec/vectors"

    /// A vector by its path under spec/vectors, parsed.
    static func vector(_ path: String) throws -> Value { try readJson(root + "/" + path) }

    // codec ---------------------------------------------------------------

    /// `check_codec`: the value encodes to the bytes, and the bytes decode to the value.
    static func codec(_ v: Value) throws {
        let value = try v.field("value")
        let bytes = try hexBytes(try v.field("bytes"))
        try ensureBytes(Canon.encode(value), bytes, "encode")
        try ensureEq(try Canon.decode(bytes), value, "decode")
    }

    // order ---------------------------------------------------------------

    /// `check_order`: the input sorted by the total order is the sorted list.
    static func order(_ v: Value) throws {
        let input = try v.field("input").list().sorted { compareValue($0, $1) < 0 }
        try ensureEq(Value.list(input), try v.field("sorted"), "sorted")
    }
}
