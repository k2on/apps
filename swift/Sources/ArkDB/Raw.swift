// `docs/plan-guards.md` D4 The authority's raw writes (`rust/ark/src/raw.rs`):
// two functions in every module by construction, as the standard library
// is, that write a row or take one away with no domain logic between —
//
//     ark.put_row(table: Text, row: Struct)      the row, judged as `put` is
//     ark.delete_row(table: Text, key: List)     the row under the key, as `delete` is
//
// named by fixed hashes, `sha256(enc(Text name))`, which no closure's hash
// can be. Only a server's authority authors one; every peer replays one,
// natively, here — which is all a client does with them.

/// Which of the two an entry names (`raw::Raw`).
public enum Raw: String, CaseIterable {
    case putRow = "ark.put_row"
    case deleteRow = "ark.delete_row"

    /// The actor of every raw write a server makes (`raw::AUTHOR`).
    public static let author = "authority"

    /// Its fixed hash.
    public var hash: FnHash { Raw.hashOf(rawValue) }

    /// `hash_of`: `sha256(enc(Text name))`, how a function no module carries is named.
    public static func hashOf(_ name: String) -> FnHash { Sha256.hash(Canon.encode(.text(name))) }

    private static let hashes: [(Raw, FnHash)] = Raw.allCases.map { ($0, $0.hash) }

    /// `of`: which raw write a hash names, if it names one.
    public static func of(_ fh: [UInt8]) -> Raw? { hashes.first { $0.1 == fh }?.0 }

    /// `is_raw`.
    public static func isRaw(_ fh: [UInt8]) -> Bool { of(fh) != nil }

    /// `table_of`: the table its arguments name, or the function's name where they do not.
    public static func tableOf(_ raw: Raw, _ args: [String: Value]) -> String { args["table"]?.textValue ?? raw.rawValue }

    /// `call_of`: the raw write that makes `change`, and its arguments — an
    /// `Add` or an `Edit` puts the new row, a `Remove` deletes the row's key.
    public static func callOf(_ schema: Schema, _ change: Change) -> (Raw, [String: Value]) {
        var args: [String: Value] = ["table": .text(change.table)]
        switch change {
        case .add(_, let r):
            args["row"] = .record(r)
            return (.putRow, args)
        case .edit(_, _, let n):
            args["row"] = .record(n)
            return (.putRow, args)
        case .remove(let t, let r):
            args["key"] = .list(schema.lookupTable(t)?.keyOf(r) ?? [])
            return (.deleteRow, args)
        }
    }

    /// `apply`: a raw write over a store, judged by the table's constraints
    /// exactly as a mutator's `put` and `delete` are — its changes, applied,
    /// or the verdict with the store untouched; arguments that are not the
    /// function's are a bug.
    public static func apply(_ raw: Raw, _ args: [String: Value], _ store: Store) throws -> Ran {
        let table: String
        switch args["table"] {
        case .text(let t)?: table = t
        case nil: throw EvalFault.bug(.missingArg("table"))
        case let t?: throw EvalFault.bug(.typeError("\(raw.rawValue): table is a text, not \(t.show())"))
        }
        let written: Written
        switch raw {
        case .putRow:
            switch args["row"] {
            case .record(let r)?: written = store.put(table, Rules.rowFor(store, table, r))
            case nil: throw EvalFault.bug(.missingArg("row"))
            case let r?: throw EvalFault.bug(.typeError("\(raw.rawValue): row is a struct, not \(r.show())"))
            }
        case .deleteRow:
            switch args["key"] {
            case .list(let k)?: written = store.delete(table, k)
            case nil: throw EvalFault.bug(.missingArg("key"))
            case let k?: throw EvalFault.bug(.typeError("\(raw.rawValue): key is a list, not \(k.show())"))
            }
        }
        switch written {
        case .ok(let c): return .ok(c.map { [$0] } ?? [])
        case .refused(let r): return .refused(r)
        }
    }
}
