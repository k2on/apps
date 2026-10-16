// §7.2 A module from a value: the inverse of `Encode` (`rust/ark/src/ir/decode.rs`).
// Strict about shape — an unknown tag, a missing field or a field of the
// wrong type is a `ModuleError` naming where — and lenient about nothing.

/// Where in the module the shape was wrong, and how (`ir::DecodeError`).
public struct ModuleError: Error, CustomStringConvertible {
    public let path: [String]
    public let what: String
    public var description: String { path.joined(separator: "/") + ": " + what }
}

public enum Decode {
    private static func err(_ here: [String], _ what: String) -> ModuleError { ModuleError(path: here, what: what) }

    /// `module_from_value`: the whole module.
    public static func module(_ v: Value) throws -> Module {
        let fs = try tagged(["module"], "module", v)
        let spec = try int(["module", "spec"], try field(fs, "spec"))
        let schema = try self.schema(try field(fs, "schema"))
        let functions = try list(["module", "functions"], try field(fs, "functions"), function)
        let routers = try list(["module", "routers"], try field(fs, "routers"), router)
        let live = try list(["module", "live"], try field(fs, "live")) { x -> LiveFrame in
            let f = try tagged(["frame"], "frame", x)
            return LiveFrame(name: try text(["frame", "name"], try field(f, "name")), ty: try ty(try field(f, "ty")))
        }
        // `docs/plan-live.md` Present only where a module has one: an empty
        // list is not a second spelling of its absence.
        var types = [Named<Ty>]()
        if let v = fs["types"] {
            types = try list(["module", "types"], v, typeDecl)
            if types.isEmpty { throw err(["module", "types"], "an empty type section is written as no field") }
        }
        var states = [LiveStateDecl]()
        if let v = fs["states"] {
            states = try list(["module", "states"], v, state)
            if states.isEmpty { throw err(["module", "states"], "no states is written as no field") }
        }
        return Module(spec: spec, schema: schema, functions: functions, routers: routers, live: live, types: types, states: states)
    }

    private static func typeDecl(_ x: Value) throws -> Named<Ty> {
        let fs = try tagged(["type"], "type", x)
        return Named(try text(["type", "name"], try field(fs, "name")), try ty(try field(fs, "ty")))
    }

    /// `state_from_value`: §3.12 a live state.
    public static func state(_ v: Value) throws -> LiveStateDecl {
        let fs = try tagged(["state"], "state", v)
        let name = try text(["state", "name"], try field(fs, "name"))
        let here = ["state", name]
        func p(_ k: String) -> [String] { here + [k] }
        func hook(_ k: String) throws -> Transition? { try fs[k].map { try transition(p(k), $0) } }
        return LiveStateDecl(
            name: name,
            ty: try ty(try field(fs, "ty")),
            initial: try expr(p("init"), try field(fs, "init")),
            scope: try expr(p("scope"), try field(fs, "scope")),
            transitions: try list(p("transitions"), try field(fs, "transitions")) { try transition(p("transitions"), $0) },
            onJoin: try hook("on_join"),
            onLeave: try hook("on_leave"),
            onRestore: try hook("on_restore"))
    }

    private static func transition(_ at: [String], _ v: Value) throws -> Transition {
        let fs = try tagged(at, "transition", v)
        let name = try text(at, try field(fs, "name"))
        let here = at + [name]
        let input = try list(here, try field(fs, "input")) { try inputField(here, $0) }
        let guards = try list(here, try field(fs, "guards")) { x -> Guard in
            let g = try tagged(here, "guard", x)
            return Guard(e: try expr(here, try field(g, "e")), why: try text(here, try field(g, "why")))
        }
        let body = try list(here, try field(fs, "body")) { try stmt(here, $0) }
        return Transition(name: name, input: input, guards: guards, body: body)
    }

    /// `expr_from_value`: an expression from its value, outside any function.
    public static func expr(_ v: Value) throws -> Expr { try expr([], v) }

    /// `router_from_value`.
    public static func router(_ v: Value) throws -> Router {
        let fs = try tagged(["router"], "router", v)
        let name = try text(["router", "name"], try field(fs, "name"))
        let here = ["router", name, "uses"]
        return Router(name: name, uses: try list(here, try field(fs, "uses")) { try text(here, $0) })
    }

    /// `closure_from_value`: `{t: "closure", fn, helpers}`.
    public static func closure(_ v: Value) throws -> Closure {
        let fs = try tagged(["closure"], "closure", v)
        return Closure(function: try function(try field(fs, "fn")), helpers: try list(["closure", "helpers"], try field(fs, "helpers"), function))
    }

    /// `schema_from_value`.
    public static func schema(_ v: Value) throws -> Schema { Schema(try list(["schema"], v, table)) }

    private static func table(_ x: Value) throws -> Table {
        let fs = try tagged(["table"], "table", x)
        let name = try text(["table", "name"], try field(fs, "name"))
        func here(_ k: String) -> [String] { ["table", name, k] }
        let columns = try list(here("columns"), try field(fs, "columns"), column)
        let key = try list(here("key"), try field(fs, "key")) { try text(here("key"), $0) }
        let all = try list(here("indexes"), try field(fs, "indexes"), index)
        let refs = try list(here("refs"), try field(fs, "refs"), reference)
        var indexes = [Index]()
        var texts = [String]()
        for (ix, isText) in all {
            if !isText {
                indexes.append(ix)
            } else if ix.columns.count == 1 {
                texts.append(ix.columns[0])
            } else {
                throw err(here("indexes"), "a text index is on one column")
            }
        }
        return Table(name, columns, key, indexes, refs, texts)
    }

    private static func column(_ x: Value) throws -> Column {
        let fs = try tagged(["column"], "column", x)
        let name = try text(["column", "name"], try field(fs, "name"))
        return Column(name, try ty(try field(fs, "ty")), try bool(["column", name, "nullable"], try field(fs, "nullable")))
    }

    // An index, and whether it is of kind `text` (D4): `kind` is written only for one.
    private static func index(_ x: Value) throws -> (Index, Bool) {
        let fs = try tagged(["index"], "index", x)
        let columns = try list(["index", "columns"], try field(fs, "columns")) { try text(["index", "columns"], $0) }
        let unique = try bool(["index", "unique"], try field(fs, "unique"))
        var isText = false
        if let k = fs["kind"] {
            if try text(["index", "kind"], k) == "text" && !unique {
                isText = true
            } else {
                throw err(["index", "kind"], "unknown index kind \(k.show())")
            }
        }
        return (Index(columns, unique), isText)
    }

    private static func reference(_ x: Value) throws -> Ref {
        let fs = try tagged(["ref"], "ref", x)
        return Ref(try text(["ref", "column"], try field(fs, "column")), try text(["ref", "table"], try field(fs, "table")))
    }

    /// `ty_from_value`.
    public static func ty(_ v: Value) throws -> Ty {
        let (t, fs) = try taggedAny(["ty"], v)
        switch t {
        case "bool": return .bool
        case "int": return .int
        case "text": return .text
        case "bytes": return .bytes
        case "id": return .id(try text(["ty", "id"], try field(fs, "table")))
        case "enum": return .enumeration(try list(["ty", "enum"], try field(fs, "variants")) { try text(["ty", "enum"], $0) })
        case "option": return .option(try ty(try field(fs, "of")))
        case "list": return .list(try ty(try field(fs, "of")))
        case "struct":
            let m = try structMap(["ty", "struct"], try field(fs, "fields"))
            return .structure(TyFields(try zip(m.names, m.values).map { ($0.0, try ty($0.1)) }))
        case "named": return .named(try text(["ty", "named"], try field(fs, "name")))
        case "non_empty_list": return .nonEmpty(try ty(try field(fs, "of")))
        case "per_device": return .perDevice(try ty(try field(fs, "of")))
        case "ephemeral": return .ephemeral(try ty(try field(fs, "of")))
        case "sum":
            let m = try structMap(["ty", "sum"], try field(fs, "variants"))
            return .sum(TySum(try m.pairs.map { k, v in
                let f = try structMap(["ty", "sum", k], v)
                return (k, TyFields(try f.pairs.map { ($0.0, try ty($0.1)) }))
            }))
        default: throw err(["ty"], "unknown type tag \(t)")
        }
    }

    /// `function_from_value`.
    public static func function(_ v: Value) throws -> Function {
        let fs = try tagged(["fn"], "fn", v)
        let name = try text(["fn", "name"], try field(fs, "name"))
        let here = ["fn", name]
        func p(_ k: String) -> [String] { here + [k] }
        let kindText = try text(p("kind"), try field(fs, "kind"))
        guard let kind = FnKind(rawValue: kindText) else { throw err(here, "unknown kind \(kindText)") }
        let router = try optional(try field(fs, "router")) { try text(p("router"), $0) }
        let uses = try list(p("uses"), try field(fs, "uses")) { try text(p("uses"), $0) }
        let autos = try list(p("autos"), try field(fs, "autos"), auto)
        let input = try list(p("input"), try field(fs, "input")) { try inputField(p("input"), $0) }
        let refine = try list(p("refine"), try field(fs, "refine")) { x -> Refinement in
            let rs = try tagged(p("refine"), "refine", x)
            return Refinement(e: try expr(p("refine"), try field(rs, "e")), why: try why(p("refine"), try field(rs, "why")))
        }
        let ret = try optional(try field(fs, "ret"), ty)
        let body = try list(p("body"), try field(fs, "body")) { try stmt(here, $0) }
        let plan = try fs["plan"].map { try self.plan(p("plan"), $0) }
        // D2 Written for a scope and nothing else.
        var holds = [Hold]()
        if let h = fs["holds"] {
            if kind != .scope { throw err(p("holds"), "only a scope holds") }
            holds = try list(p("holds"), h) { try hold(p("holds"), $0) }
        } else if kind == .scope {
            throw err(p("holds"), "missing field")
        }
        // D3 Present only when true.
        var isPrivate = false
        if let pv = fs["private"] {
            if pv == .bool(true) { isPrivate = true } else { throw err(p("private"), "present only as true") }
        }
        return Function(name: name, kind: kind, router: router, uses: uses, autos: autos, input: input, refine: refine, ret: ret, body: body, plan: plan, holds: holds, isPrivate: isPrivate)
    }

    /// `hold_from_value`.
    public static func hold(_ v: Value) throws -> Hold { try hold(["hold"], v) }

    private static func hold(_ at: [String], _ v: Value) throws -> Hold {
        let fs = try tagged(at, "hold", v)
        let table = try text(at, try field(fs, "table"))
        let here = at + [table]
        let filter = try optional(try field(fs, "filter")) { try pred(here, $0) }
        func names(_ x: Value) throws -> [String] { try list(here, x) { try text(here, $0) } }
        let columns: Projection
        switch (fs["pick"], fs["exclude"]) {
        case (nil, nil): columns = .all
        case (let p?, nil): columns = .pick(try names(p))
        case (nil, let e?): columns = .exclude(try names(e))
        default: throw err(here, "a hold picks or excludes, not both")
        }
        return Hold(table: table, filter: filter, columns: columns)
    }

    private static func why(_ here: [String], _ v: Value) throws -> String? { try optional(v) { try text(here, $0) } }

    private static func inputField(_ here: [String], _ x: Value) throws -> Named<Field> {
        let fs = try tagged(here, "field", x)
        let n = try text(here, try field(fs, "name"))
        let p = here + [n]
        let t = try ty(try field(fs, "ty"))
        return Named(n, Field(ty: t, checks: try list(p, try field(fs, "checks")) { try check(p, $0) }))
    }

    private static func check(_ here: [String], _ v: Value) throws -> Check {
        let (t, fs) = try taggedAny(here, v)
        let p = here + [t]
        func w() throws -> String? { try why(p, try field(fs, "why")) }
        func optInt(_ k: String) throws -> Int64? { try optional(try field(fs, k)) { try int(p, $0) } }
        switch t {
        case "trim": return .trim
        case "min_len": return .minLen(try int(p, try field(fs, "n")), try w())
        case "max_len": return .maxLen(try int(p, try field(fs, "n")), try w())
        case "range": return .range(try optInt("lo"), try optInt("hi"), try w())
        case "non_empty": return .nonEmpty(try w())
        case "exists": return .exists(try w())
        case "refine": return .refine(try expr(p, try field(fs, "e")), try w())
        default: throw err(here, "unknown check \(t)")
        }
    }

    private static func auto(_ x: Value) throws -> Named<Auto> {
        let (t, fs) = try taggedAny(["auto"], x)
        let n = try text(["auto", "name"], try field(fs, "name"))
        switch t {
        case "new_id": return Named(n, .newId(try text(["auto", n], try field(fs, "table"))))
        case "now": return Named(n, .now)
        default: throw err(["auto", n], "unknown auto \(t)")
        }
    }

    private static func stmt(_ here: [String], _ v: Value) throws -> Stmt {
        let (t, fs) = try taggedAny(here, v)
        let p = here + [t]
        func e(_ k: String) throws -> Expr { try expr(p, try field(fs, k)) }
        func b(_ k: String) throws -> Block { try list(p, try field(fs, k)) { try stmt(p, $0) } }
        func names(_ k: String) throws -> [String] { try list(p, try field(fs, k)) { try text(p, $0) } }
        func es(_ k: String) throws -> [Expr] { try list(p, try field(fs, k)) { try expr(p, $0) } }
        func tx(_ k: String) throws -> String { try text(p, try field(fs, k)) }
        func s(_ k: String) throws -> Sym { try sym(p, try field(fs, k)) }
        switch t {
        case "let": return .letS(try s("sym"), try e("e"))
        case "if": return .ifS(try e("c"), try b("then"), try b("else"))
        case "for": return .forS(try s("sym"), try e("in"), try b("body"))
        case "insert": return .insert(try tx("table"), try e("row"), try names("on"))
        case "upsert": return .upsert(try tx("table"), try e("row"), try names("on"))
        case "update": return .update(try tx("table"), try es("key"), try s("sym"), try e("row"))
        case "delete": return .delete(try tx("table"), try es("key"))
        case "refuse": return .refuse(try e("e"))
        case "return": return .ret(try optional(try field(fs, "e")) { try expr(p, $0) })
        case "private": return .privateS(try b("body"))
        default: throw err(here, "unknown statement \(t)")
        }
    }

    private static func expr(_ here: [String], _ v: Value) throws -> Expr {
        let (t, fs) = try taggedAny(here, v)
        let p = here + [t]
        func e(_ k: String) throws -> Expr { try expr(p, try field(fs, k)) }
        func s(_ k: String) throws -> Sym { try sym(p, try field(fs, k)) }
        func es(_ k: String) throws -> [Expr] { try list(p, try field(fs, k)) { try expr(p, $0) } }
        func tx(_ k: String) throws -> String { try text(p, try field(fs, k)) }
        switch t {
        case "lit": return .lit(try field(fs, "v"))
        case "arg": return .arg(try tx("name"))
        case "auto": return .auto(try tx("name"))
        case "var": return .variable(try s("sym"))
        case "ctx_user": return .ctxUser
        case "ctx_session": return .ctxSession
        case "has_role": return .hasRole(try tx("role"))
        case "provided": return .provided(try tx("fn"))
        case "field": return .field(try e("e"), try tx("name"))
        case "struct":
            let m = try structMap(p, try field(fs, "fields"))
            return .structE(ExprFields(sorted: m.names, try m.values.map { try expr(p, $0) }))
        case "list": return .listE(try es("items"))
        case "some": return .some(try e("e"))
        case "none": return .none(try ty(try field(fs, "ty")))
        case "match": return .match(try e("e"), try s("sym"), try e("some"), try e("none"))
        case "ife": return .ife(try e("c"), try e("then"), try e("else"))
        case "op":
            let name = try tx("op")
            guard let o = Op(rawValue: name) else { throw err(p, "unknown operator \(name)") }
            return .op(o, try es("args"))
        case "cmp": return .cmp(try cmpOp(p, try field(fs, "op")), try e("l"), try e("r"))
        case "call": return .call(try tx("fn"), try es("args"))
        case "std":
            let name = try tx("fn")
            guard let f = StdFn(rawValue: name) else { throw err(p, "unknown standard function \(name)") }
            return .std(f, try es("args"))
        case "map": return .map(try e("in"), try s("sym"), try e("body"))
        case "filter": return .filter(try e("in"), try s("sym"), try e("body"))
        case "any": return .any(try e("in"), try s("sym"), try e("body"))
        case "all": return .all(try e("in"), try s("sym"), try e("body"))
        case "sort_by": return .sortBy(try e("in"), try s("sym"), try e("key"))
        case "fold": return .fold(try e("in"), try e("init"), try s("acc"), try s("sym"), try e("body"))
        case "select": return .select(try plan(p, try field(fs, "plan")))
        case "get": return .get(try tx("table"), try es("key"))
        case "exists": return .exists(try tx("table"), try es("key"))
        case "variant":
            let m = try structMap(p, try field(fs, "fields"))
            return .variant(try ty(try field(fs, "ty")), try tx("name"), ExprFields(sorted: m.names, try m.values.map { try expr(p, $0) }))
        case "case":
            return .caseOf(try e("e"), try list(p, try field(fs, "arms")) { x -> Arm in
                let a = try tagged(p, "arm", x)
                return Arm(name: try text(p, try field(a, "name")), sym: try sym(p, try field(a, "sym")), e: try expr(p, try field(a, "e")))
            })
        case "live_state": return .liveState
        case "ctx_now": return .ctxNow
        case "ctx_device": return .ctxDevice
        case "slot": return .slot(try e("e"), try e("key"))
        case "set_slot": return .setSlot(try e("e"), try e("key"), try e("v"))
        case "slots": return .slots(try e("e"))
        default: throw err(here, "unknown expression \(t)")
        }
    }

    /// `plan_from_value`: a plan read from its value alone, outside any module.
    public static func plan(_ v: Value) throws -> Plan { try plan([], v) }

    // §1.8 The v3 keys always, the v4 ones read when present.
    private static func plan(_ at: [String], _ v: Value) throws -> Plan {
        let fs = try tagged(at, "plan", v)
        let table = try text(at, try field(fs, "table"))
        let here = at + [table]
        let filter = try optional(try field(fs, "filter")) { try pred(here, $0) }
        let order = try list(here, try field(fs, "order")) { x -> OrderKey in
            let bs = try tagged(here, "by", x)
            let key: Key
            switch (bs["column"], bs["expr"]) {
            case (let c?, nil): key = .column(try text(here, c))
            case (nil, let ex?): key = .expr(try expr(here, ex))
            default: throw err(here, "an order key is a column or an expression")
            }
            let dt = try text(here, try field(bs, "dir"))
            switch dt {
            case "asc": return OrderKey(key, .asc)
            case "desc": return OrderKey(key, .desc)
            default: throw err(here, "unknown direction \(dt)")
            }
        }
        let limit = try optional(try field(fs, "limit")) { try int(here, $0) }
        let related = try list(here, try field(fs, "related")) { x -> Related in
            let rs = try tagged(here, "related", x)
            let name = try text(here, try field(rs, "name"))
            let sub = here + [name]
            let sym = try self.sym(sub, try field(rs, "sym"))
            let on = try list(sub, try field(rs, "on")) { pair -> On in
                guard case .list(let xs) = pair, xs.count == 2 else { throw err(sub, "an on pair is [column, expr]") }
                return On(column: try text(sub, xs[0]), e: try expr(sub, xs[1]))
            }
            return Related(name: name, sym: sym, on: on, plan: try plan(sub, try field(rs, "plan")))
        }
        let source: Source
        if let g = fs["group"] {
            source = .group(table, try list(here, g) { try text(here, $0) })
        } else {
            source = .table(table)
        }
        let row = try fs["row"].map { try sym(here, $0) }
        let members = try fs["members"].map { try sym(here, $0) }
        var lookups = [Lookup]()
        if let ls = fs["lookups"] {
            lookups = try list(here, ls) { x -> Lookup in
                let l = try tagged(here, "lookup", x)
                return Lookup(name: try text(here, try field(l, "name")), sym: try sym(here, try field(l, "sym")), table: try text(here, try field(l, "table")), key: try list(here, try field(l, "key")) { try expr(here, $0) })
            }
        }
        let having = try fs["having"].map { try expr(here, $0) }
        let project = try fs["project"].map { try expr(here, $0) }
        return Plan(source: source, filter: filter, row: row, members: members, lookups: lookups, related: related, having: having, project: project, order: order, limit: limit)
    }

    private static func pred(_ here: [String], _ v: Value) throws -> Pred {
        let (t, fs) = try taggedAny(here, v)
        switch t {
        case "pcmp": return .cmp(try text(here, try field(fs, "column")), try cmpOp(here, try field(fs, "op")), try expr(here, try field(fs, "e")))
        case "pin": return .inP(try text(here, try field(fs, "column")), try list(here, try field(fs, "items")) { try expr(here, $0) })
        case "pall": return .all(try list(here, try field(fs, "items")) { try pred(here, $0) })
        case "pany": return .any(try list(here, try field(fs, "items")) { try pred(here, $0) })
        case "pnot": return .not(try pred(here, try field(fs, "e")))
        case "phas": return .has(try text(here, try field(fs, "column")), try expr(here, try field(fs, "e")))
        case "pwhen": return .when(try expr(here, try field(fs, "e")))
        case "pexists": return .exists(try text(here, try field(fs, "table")), try text(here, try field(fs, "column")), try pred(here, try field(fs, "pred")))
        default: throw err(here, "unknown predicate \(t)")
        }
    }

    private static func cmpOp(_ here: [String], _ v: Value) throws -> CmpOp {
        let name = try text(here, v)
        guard let o = CmpOp(rawValue: name) else { throw err(here, "unknown comparison \(name)") }
        return o
    }

    // Primitives --------------------------------------------------------

    private static func tagged(_ here: [String], _ want: String, _ v: Value) throws -> Record {
        let (t, fs) = try taggedAny(here, v)
        if t != want { throw err(here, "expected \(want), found \(t)") }
        return fs
    }

    private static func taggedAny(_ here: [String], _ v: Value) throws -> (String, Record) {
        guard case .record(let fs) = v else { throw err(here, "expected a struct") }
        guard case .text(let t)? = fs["t"] else { throw err(here, "a node needs a text tag \"t\"") }
        return (t, fs)
    }

    private static func field(_ fs: Record, _ k: String) throws -> Value {
        guard let v = fs[k] else { throw err([k], "missing field") }
        return v
    }

    private static func optional<T>(_ v: Value, _ f: (Value) throws -> T) throws -> T? {
        if case .null = v { return nil }
        return try f(v)
    }

    private static func list<T>(_ here: [String], _ v: Value, _ f: (Value) throws -> T) throws -> [T] {
        guard case .list(let xs) = v else { throw err(here, "expected a list") }
        return try xs.map(f)
    }

    private static func structMap(_ here: [String], _ v: Value) throws -> Record {
        guard case .record(let r) = v else { throw err(here, "expected a struct") }
        return r
    }

    private static func text(_ here: [String], _ v: Value) throws -> String {
        guard case .text(let s) = v else { throw err(here, "expected text") }
        return s
    }

    private static func int(_ here: [String], _ v: Value) throws -> Int64 {
        guard case .int(let n) = v else { throw err(here, "expected an int") }
        return n
    }

    private static func bool(_ here: [String], _ v: Value) throws -> Bool {
        guard case .bool(let b) = v else { throw err(here, "expected a bool") }
        return b
    }

    private static func sym(_ here: [String], _ v: Value) throws -> Sym { try int(here, v) }
}
