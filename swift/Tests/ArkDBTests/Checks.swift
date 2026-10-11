// The check of each vector kind, as spec/RUNNERS.md says it and
// rust/ark/tests/vectors.rs reads it: each throws `Failed` with its first
// unmet claim, so that one definition runs both ways — a vector must pass
// it and a `falsify/` vector must fail it.
import ArkDB

enum Checks {
    static var root = "../spec/vectors"

    /// A vector by its path under spec/vectors, parsed.
    static func vector(_ path: String) throws -> Value { try readJson(root + "/" + path) }

    static func moduleOf(_ v: Value) throws -> Module { try Decode.module(v) }

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

    // hash ----------------------------------------------------------------

    // The sum of two 256-bit big-endian integers, modulo 2^256: written here
    // rather than taken from `Digest`, so that the runner checks the
    // construction (§8.1) and not the runtime's arithmetic against itself.
    private static func sum256(_ a: [UInt8], _ b: [UInt8]) -> [UInt8] {
        var out = [UInt8](repeating: 0, count: 32)
        var carry = 0
        for i in stride(from: 31, through: 0, by: -1) {
            let s = Int(a[i]) + Int(b[i]) + carry
            out[i] = UInt8(s & 0xff)
            carry = s >> 8
        }
        return out
    }

    /// `check_hash`: the store's own state hash; and, where the vector pins
    /// the construction part by part (`tables`), every leaf, every digest and
    /// the hash of the pairs, each from the canonical encoding and SHA-256 alone.
    static func hash(_ v: Value) throws {
        let m = try moduleOf(try v.field("module"))
        let st = MemoryStore.fromValue(m.schema, try v.field("store"))
        let want = try v.field("hash").text()
        guard let tables = v.opt("tables") else {
            try ensureEq(Hex.encode(Hash.stateHash(st)), want, "state hash")
            return
        }
        let names = m.schema.tables.map { $0.name }
        let ts = try tables.list()
        try ensureEq(ts.count, names.count, "one entry per table of the schema")
        var pairs = [Value]()
        for (t, name) in zip(ts, names) {
            try ensureEq(try t.field("table").text(), name, "the tables in schema order")
            var digest = [UInt8](repeating: 0, count: 32)
            let rows = try t.field("rows").list()
            try ensureEq(rows.count, st.scan(name).count, "\(name): every row of the store")
            for r in rows {
                let l = Sha256.hash(Canon.encode(.text(name)) + Canon.encode(try r.field("row")))
                try ensureEq(Hex.encode(l), try r.field("leaf").text(), "\(name): a row's leaf")
                try ensureBytes(leaf(name, try r.field("row").record()), l, "\(name): the runtime's leaf")
                digest = sum256(digest, l)
            }
            try ensureEq(Hex.encode(digest), try t.field("digest").text(), "\(name): the digest is the sum of the leaves")
            pairs.append(.list([.text(name), .bytes(digest)]))
        }
        try ensureEq(Hex.encode(Sha256.hash(Canon.encode(.list(pairs)))), want, "the hash of the pairs")
        try ensureEq(Hex.encode(Hash.stateHash(st)), want, "state hash")
    }

    // module --------------------------------------------------------------

    /// `check_module`: the bytes decode to the module value; it reads as a
    /// module of spec 4 that writes back as itself and encodes to the bytes;
    /// its module hash; its schema well formed. A `stripped` vector also: the
    /// client's module beside it, the server's with its private blocks
    /// stripped and verified, at the public hash, and both naming every
    /// function by the listed hash.
    static func module(_ v: Value) throws {
        let value = try v.field("module")
        let bytes = try hexBytes(try v.field("bytes"))
        try ensureEq(try Canon.decode(bytes), value, "the bytes decode to the module value")
        let m = try moduleOf(value)
        try ensureEq(m.spec, SPEC_VERSION, "spec version")
        try ensureEq(Encode.module(m), value, "decode then encode is the identity")
        try ensureBytes(Canon.encode(Encode.module(m)), bytes, "the module encodes to the bytes")
        try ensureEq(Hex.encode(Hash.moduleHash(m)), try v.field("hash").text(), "module hash")
        try ensure(checkSchema(m.schema).isEmpty, "the schema is not well formed")
        guard let sval = v.opt("stripped") else { return }
        try ensureEq(try Canon.decode(try hexBytes(try v.field("strippedBytes"))), sval, "the stripped bytes decode to the stripped module")
        let stripped = try moduleOf(sval)
        // The one step a client may skip; this runtime takes it.
        let want = try Verify.verify(stripModule(m))
        try ensureEq(Encode.module(want), sval, "the stripped module is the module with its private blocks taken out")
        try ensureEq(Hex.encode(Hash.moduleHash(stripped)), try v.field("public").text(), "the public module hash")
        func names(_ m: Module) -> [String] {
            Hash.closures(m).entries.map { ($0.value.function.name, $0.key) }
                .sorted { compareText($0.0, $1.0) != 0 ? compareText($0.0, $1.0) < 0 : compareText($0.1, $1.1) < 0 }
                .map { "\($0.0) \($0.1)" }
        }
        let listed = try v.field("functions").list().map { p -> String in
            let xs = try p.list()
            return "\(try xs[0].text()) \(try xs[1].text())"
        }
        try ensureEq(names(m), listed, "the server's module names its functions by the listed hashes")
        try ensureEq(names(stripped), listed, "and so does the client's")
    }

    // verify --------------------------------------------------------------

    /// `check_verify`: it verifies, as itself; its schema, its spec; every function has a closure.
    static func verify(_ v: Value) throws {
        let m = try moduleOf(try v.field("module"))
        try ensure(try v.field("verifies") == .bool(true), "only accepting modules are checked here")
        try ensure(checkSchema(m.schema).isEmpty, "the schema is not well formed")
        try ensureEq(m.spec, SPEC_VERSION, "spec version")
        let verified = try Verify.verify(m)
        try ensureEq(Encode.module(verified), Encode.module(m), "a verified module is its own verified form")
        try ensureEq(Hash.closures(m).count, m.functions.count, "one closure per function")
    }
}
