import Foundation

/// Why a value is not a module (or closure, schema, type): the path to the
/// node and what was wrong with it. `Ark.Decode.DecodeError`; named apart
/// from `Canon`'s.
public struct ModuleDecodeError: Error, Equatable {
    public var path: [String]
    public var what: String
    public init(_ path: [String], _ what: String) { self.path = path; self.what = what }
}

/// §7.2 A module from a value: the inverse of `Encode`, strict about shape.
public enum Decode {
    typealias Fields = [String: Value]

    /// The whole module. A decoded function's `names` is empty.
    public static func fromValue(_ v: Value) throws -> Module {
        let fs = try tagged(["module"], "module", v)
        let spec = try int(["module", "spec"], try field(fs, "spec"))
        let sch = try schemaFromValue(try field(fs, "schema"))
        let fns = try list(["module", "functions"], try field(fs, "functions"), functionFromValue)
        let live = try list(["module", "live"], try field(fs, "live")) { x -> LiveFrame in
            let ffs = try tagged(["frame"], "frame", x)
            let n = try text(["frame", "name"], try field(ffs, "name"))
            let t = try tyFromValue(try field(ffs, "ty"))
            return LiveFrame(n, t)
        }
        let routers = try list(["module", "routers"], try field(fs, "routers")) { x -> Router in
            let rfs = try tagged(["router"], "router", x)
            let n = try text(["router", "name"], try field(rfs, "name"))
            let sc = try text(["router", n, "scope"], try field(rfs, "scope"))
            let us = try list(["router", n, "uses"], try field(rfs, "uses")) { try text(["router", n, "uses"], $0) }
            return Router(name: n, scope: sc, uses: us)
        }
        return Module(spec: Int(spec), schema: sch, functions: fns, routers: routers, live: live)
    }

    /// A closure: `{ t: "closure", fn, helpers }`.
    public static func closureFromValue(_ v: Value) throws -> Closure {
        let fs = try tagged(["closure"], "closure", v)
        let fn = try functionFromValue(try field(fs, "fn"))
        let hs = try list(["closure", "helpers"], try field(fs, "helpers"), functionFromValue)
        return Closure(fn: fn, helpers: hs)
    }

    public static func schemaFromValue(_ v: Value) throws -> Schema {
        func col(_ x: Value) throws -> Column {
            let fs = try tagged(["column"], "column", x)
            let n = try text(["column", "name"], try field(fs, "name"))
            let t = try tyFromValue(try field(fs, "ty"))
            let nl = try bool(["column", n, "nullable"], try field(fs, "nullable"))
            return Column(n, t, nullable: nl)
        }
        func index(_ x: Value) throws -> Index {
            let fs = try tagged(["index"], "index", x)
            let cs = try list(["index", "columns"], try field(fs, "columns")) { try text(["index", "columns"], $0) }
            let u = try bool(["index", "unique"], try field(fs, "unique"))
            return Index(cs, unique: u)
        }
        func ref(_ x: Value) throws -> Ref {
            let fs = try tagged(["ref"], "ref", x)
            let c = try text(["ref", "column"], try field(fs, "column"))
            let t = try text(["ref", "table"], try field(fs, "table"))
            return Ref(c, t)
        }
        func table(_ x: Value) throws -> Table {
            let fs = try tagged(["table"], "table", x)
            let n = try text(["table", "name"], try field(fs, "name"))
            let cs = try list(["table", n, "columns"], try field(fs, "columns"), col)
            let k = try list(["table", n, "key"], try field(fs, "key")) { try text(["table", n, "key"], $0) }
            let ixs = try list(["table", n, "indexes"], try field(fs, "indexes"), index)
            let rs = try list(["table", n, "refs"], try field(fs, "refs"), ref)
            return Table(n, columns: cs, key: k, indexes: ixs, refs: rs)
        }
        func scope(_ x: Value) throws -> Scope {
            let fs = try tagged(["scope"], "scope", x)
            let n = try text(["scope", "name"], try field(fs, "name"))
            let ts = try list(["scope", n], try field(fs, "tables"), table)
            return Scope(n, tables: ts)
        }
        return Schema(scopes: try list(["schema"], v, scope))
    }

    public static func tyFromValue(_ v: Value) throws -> Ty {
        let (t, fs) = try taggedAny(["ty"], v)
        switch t {
        case "bool": return .bool
        case "int": return .int
        case "text": return .text
        case "bytes": return .bytes
        case "id": return .id(try text(["ty", "id"], try field(fs, "table")))
        case "enum": return .enumOf(try list(["ty", "enum"], try field(fs, "variants")) { try text(["ty", "enum"], $0) })
        case "option": return .option(try tyFromValue(try field(fs, "of")))
        case "list": return .list(try tyFromValue(try field(fs, "of")))
        case "struct":
            let m = try structMap(["ty", "struct"], try field(fs, "fields"))
            var out: [String: Ty] = [:]
            for k in m.keys.sorted(by: { compareText($0, $1) < 0 }) { out[k] = try tyFromValue(m[k]!) }
            return .structOf(out)
        default: throw ModuleDecodeError(["ty"], "unknown type tag " + t)
        }
    }

    public static func functionFromValue(_ v: Value) throws -> Function {
        let fs = try tagged(["fn"], "fn", v)
        let n = try text(["fn", "name"], try field(fs, "name"))
        let here = ["fn", n]
        let kindText = try text(here + ["kind"], try field(fs, "kind"))
        guard let k = FnKind(wireName: kindText) else { throw ModuleDecodeError(here, "unknown kind " + kindText) }
        let sc = try optional(try field(fs, "scope")) { try text(here + ["scope"], $0) }
        let rt = try optional(try field(fs, "router")) { try text(here + ["router"], $0) }
        let uses = try list(here + ["uses"], try field(fs, "uses")) { try text(here + ["uses"], $0) }
        let autos = try list(here + ["autos"], try field(fs, "autos")) { x -> NamedAuto in
            let (t, afs) = try taggedAny(["auto"], x)
            let an = try text(["auto", "name"], try field(afs, "name"))
            switch t {
            case "new_id": return NamedAuto(an, .newId(try text(["auto", an], try field(afs, "table"))))
            case "now": return NamedAuto(an, .now)
            default: throw ModuleDecodeError(["auto", an], "unknown auto " + t)
            }
        }
        let input = try list(here + ["input"], try field(fs, "input")) { x -> NamedField in
            let ffs = try tagged(here + ["field"], "field", x)
            let fname = try text(here + ["field", "name"], try field(ffs, "name"))
            let p = here + ["input", fname]
            let t = try tyFromValue(try field(ffs, "ty"))
            let cs = try list(p, try field(ffs, "checks")) { try check(p, $0) }
            return NamedField(fname, Field(t, checks: cs))
        }
        let refine = try list(here + ["refine"], try field(fs, "refine")) { x -> Refine in
            let rfs = try tagged(here + ["refine"], "refine", x)
            return Refine(try expr(here + ["refine"], try field(rfs, "e")), try optional(try field(rfs, "why")) { try text(here + ["refine"], $0) })
        }
        let ret = try optional(try field(fs, "ret"), tyFromValue)
        let body = try list(here + ["body"], try field(fs, "body")) { try stmt(here, $0) }
        return Function(name: n, kind: k, scope: sc, router: rt, uses: uses, autos: autos, input: input, refine: refine, ret: ret, body: body, names: [:])
    }

    static func check(_ here: [String], _ v: Value) throws -> Check {
        let (t, fs) = try taggedAny(here, v)
        let p = here + [t]
        func why() throws -> String? { return try optional(try field(fs, "why")) { try text(p, $0) } }
        func n(_ k: String) throws -> Int { return Int(try int(p, try field(fs, k))) }
        func optInt(_ k: String) throws -> Int? { return try optional(try field(fs, k)) { Int(try int(p, $0)) } }
        switch t {
        case "trim": return .trim
        case "min_len": return .minLen(try n("n"), try why())
        case "max_len": return .maxLen(try n("n"), try why())
        case "range": return .range(try optInt("lo"), try optInt("hi"), try why())
        case "non_empty": return .nonEmpty(try why())
        case "exists": return .exists(try why())
        case "refine": return .refine(try expr(p, try field(fs, "e")), try why())
        default: throw ModuleDecodeError(here, "unknown check " + t)
        }
    }

    static func stmt(_ here: [String], _ v: Value) throws -> Stmt {
        let (t, fs) = try taggedAny(here, v)
        let p = here + [t]
        switch t {
        case "let": return .sLet(try sym(p, try field(fs, "sym")), try expr(p, try field(fs, "e")))
        case "if":
            return .sIf(try expr(p, try field(fs, "c")),
                        try list(p, try field(fs, "then")) { try stmt(p, $0) },
                        try list(p, try field(fs, "else")) { try stmt(p, $0) })
        case "for":
            return .sFor(try sym(p, try field(fs, "sym")), try expr(p, try field(fs, "in")),
                         try list(p, try field(fs, "body")) { try stmt(p, $0) })
        case "insert", "upsert":
            let tb = try text(p, try field(fs, "table"))
            let row = try expr(p, try field(fs, "row"))
            let on = try list(p, try field(fs, "on")) { try text(p, $0) }
            return t == "insert" ? .sInsert(tb, row, on) : .sUpsert(tb, row, on)
        case "update":
            return .sUpdate(try text(p, try field(fs, "table")), try list(p, try field(fs, "key")) { try expr(p, $0) },
                            try sym(p, try field(fs, "sym")), try expr(p, try field(fs, "row")))
        case "delete": return .sDelete(try text(p, try field(fs, "table")), try list(p, try field(fs, "key")) { try expr(p, $0) })
        case "refuse": return .sRefuse(try expr(p, try field(fs, "e")))
        case "return": return .sReturn(try optional(try field(fs, "e")) { try expr(p, $0) })
        default: throw ModuleDecodeError(here, "unknown statement " + t)
        }
    }

    static func expr(_ here: [String], _ v: Value) throws -> Expr {
        let (t, fs) = try taggedAny(here, v)
        let p = here + [t]
        func e(_ k: String) throws -> Expr { return try expr(p, try field(fs, k)) }
        func s(_ k: String) throws -> Sym { return try sym(p, try field(fs, k)) }
        func es(_ k: String) throws -> [Expr] { return try list(p, try field(fs, k)) { try expr(p, $0) } }
        switch t {
        case "lit": return .lit(try field(fs, "v"))
        case "arg": return .arg(try text(p, try field(fs, "name")))
        case "auto": return .auto(try text(p, try field(fs, "name")))
        case "var": return .variable(try s("sym"))
        case "ctx_user": return .ctxUser
        case "ctx_session": return .ctxSession
        case "provided": return .provided(try text(p, try field(fs, "fn")))
        case "field": return .field(try e("e"), try text(p, try field(fs, "name")))
        case "struct":
            let m = try structMap(p, try field(fs, "fields"))
            var out: [String: Expr] = [:]
            for k in m.keys.sorted(by: { compareText($0, $1) < 0 }) { out[k] = try expr(p, m[k]!) }
            return .structOf(out)
        case "list": return .list(try es("items"))
        case "some": return .some(try e("e"))
        case "none": return .none(try tyFromValue(try field(fs, "ty")))
        case "match": return .match(try e("e"), try s("sym"), try e("some"), try e("none"))
        case "ife": return .ife(try e("c"), try e("then"), try e("else"))
        case "op": return .op(try op(p, try text(p, try field(fs, "op"))), try es("args"))
        case "cmp": return .cmp(try cmpOp(p, try text(p, try field(fs, "op"))), try e("l"), try e("r"))
        case "call": return .call(try text(p, try field(fs, "fn")), try es("args"))
        case "std": return .std(try stdFn(p, try text(p, try field(fs, "fn"))), try es("args"))
        case "map": return .map(try e("in"), try s("sym"), try e("body"))
        case "filter": return .filter(try e("in"), try s("sym"), try e("body"))
        case "any": return .any(try e("in"), try s("sym"), try e("body"))
        case "all": return .all(try e("in"), try s("sym"), try e("body"))
        case "sort_by": return .sortBy(try e("in"), try s("sym"), try e("key"))
        case "fold": return .fold(try e("in"), try e("init"), try s("acc"), try s("sym"), try e("body"))
        case "select": return .select(try plan(p, try field(fs, "plan")))
        case "get": return .get(try text(p, try field(fs, "table")), try es("key"))
        case "exists": return .exists(try text(p, try field(fs, "table")), try es("key"))
        default: throw ModuleDecodeError(here, "unknown expression " + t)
        }
    }

    static func plan(_ here: [String], _ v: Value) throws -> Plan {
        let fs = try tagged(here, "plan", v)
        let tbl = try text(here, try field(fs, "table"))
        let f = try optional(try field(fs, "filter")) { try pred(here + [tbl], $0) }
        let o = try list(here, try field(fs, "order")) { x -> OrderBy in
            let bfs = try tagged(here, "by", x)
            let c = try text(here, try field(bfs, "column"))
            let d: Dir
            switch try text(here, try field(bfs, "dir")) {
            case "asc": d = .asc
            case "desc": d = .desc
            case let other: throw ModuleDecodeError(here, "unknown direction " + other)
            }
            return OrderBy(c, d)
        }
        let l = try optional(try field(fs, "limit")) { try int(here, $0) }
        let rs = try list(here, try field(fs, "related")) { x -> Related in
            let rfs = try tagged(here, "related", x)
            let n = try text(here, try field(rfs, "name"))
            let parent = try text(here, try field(rfs, "parent"))
            let child = try text(here, try field(rfs, "child"))
            let col = try text(here, try field(rfs, "column"))
            let pl = try plan(here + [n], try field(rfs, "plan"))
            return Related(name: n, relation: Relation(parent: parent, child: child, column: col), plan: pl)
        }
        return Plan(table: tbl, filter: f, order: o, limit: l.map { Int($0) }, related: rs)
    }

    static func pred(_ here: [String], _ v: Value) throws -> Pred {
        let (t, fs) = try taggedAny(here, v)
        switch t {
        case "pcmp":
            return .pcmp(try text(here, try field(fs, "column")),
                         try cmpOp(here, try text(here, try field(fs, "op"))),
                         try expr(here, try field(fs, "e")))
        case "pin": return .pin(try text(here, try field(fs, "column")), try list(here, try field(fs, "items")) { try expr(here, $0) })
        case "pall": return .pall(try list(here, try field(fs, "items")) { try pred(here, $0) })
        case "pany": return .pany(try list(here, try field(fs, "items")) { try pred(here, $0) })
        case "pnot": return .pnot(try pred(here, try field(fs, "e")))
        default: throw ModuleDecodeError(here, "unknown predicate " + t)
        }
    }

    static func op(_ here: [String], _ s: String) throws -> Op {
        guard let o = Op(rawValue: s) else { throw ModuleDecodeError(here, "unknown operator " + s) }
        return o
    }

    static func cmpOp(_ here: [String], _ s: String) throws -> CmpOp {
        guard let o = CmpOp(rawValue: s) else { throw ModuleDecodeError(here, "unknown comparison " + s) }
        return o
    }

    static func stdFn(_ here: [String], _ s: String) throws -> StdFn {
        guard let f = StdFn(rawValue: s) else { throw ModuleDecodeError(here, "unknown standard function " + s) }
        return f
    }

    // MARK: primitives

    static func tagged(_ here: [String], _ want: String, _ v: Value) throws -> Fields {
        let (t, fs) = try taggedAny(here, v)
        if t == want { return fs }
        throw ModuleDecodeError(here, "expected " + want + ", found " + t)
    }

    static func taggedAny(_ here: [String], _ v: Value) throws -> (String, Fields) {
        guard case .record(let fs) = v else { throw ModuleDecodeError(here, "expected a struct") }
        guard case .text(let t)? = fs["t"] else { throw ModuleDecodeError(here, "a node needs a text tag \"t\"") }
        return (t, fs)
    }

    static func field(_ fs: Fields, _ k: String) throws -> Value {
        guard let v = fs[k] else { throw ModuleDecodeError([k], "missing field") }
        return v
    }

    static func optional<T>(_ v: Value, _ f: (Value) throws -> T) throws -> T? {
        if v.isNull() { return nil }
        return try f(v)
    }

    static func list<T>(_ here: [String], _ v: Value, _ f: (Value) throws -> T) throws -> [T] {
        guard case .list(let xs) = v else { throw ModuleDecodeError(here, "expected a list") }
        return try xs.map(f)
    }

    static func structMap(_ here: [String], _ v: Value) throws -> Fields {
        guard case .record(let m) = v else { throw ModuleDecodeError(here, "expected a struct") }
        return m
    }

    static func text(_ here: [String], _ v: Value) throws -> String {
        guard case .text(let t) = v else { throw ModuleDecodeError(here, "expected text") }
        return t
    }

    static func int(_ here: [String], _ v: Value) throws -> Int64 {
        guard case .int(let n) = v else { throw ModuleDecodeError(here, "expected an int") }
        return n
    }

    static func bool(_ here: [String], _ v: Value) throws -> Bool {
        guard case .bool(let b) = v else { throw ModuleDecodeError(here, "expected a bool") }
        return b
    }

    static func sym(_ here: [String], _ v: Value) throws -> Sym {
        return Int(try int(here, v))
    }
}
