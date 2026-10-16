// §9 Verification at spec version 4 (`rust/ark/src/verify.rs`): what a
// module must satisfy before anything runs it or hashes it. `verify`
// returns the form a function's hash is taken of — orders completed,
// symbols renumbered — so that the hash is of the program as it will run.
// A module may arrive from anywhere, so every rule is checked, by the name
// the reference gives it.

/// Why a module does not verify (`verify::VerifyError`).
public enum VerifyError: Hashable {
    case badSpecVersion(Int64)
    case badSchema(SchemaError)
    case duplicateFunction(String)
    case duplicateRouter(String)
    /// Router, then the complaint: a name in `uses` that is no middleware.
    case inRouter(String, String)
    /// Function, then the complaint.
    case inFunction(String, Complaint)
    /// `docs/plan-live.md` Two declarations of one name in the type section.
    case duplicateType(String)
    /// Two live states of one name.
    case duplicateState(String)
    /// A declaration of the type section, then the complaint.
    case inType(String, Complaint)
    /// A live state, then the complaint.
    case inState(String, Complaint)
    /// A live state, one of its transitions, then the complaint.
    case inTransition(String, String, Complaint)
}

/// What is wrong inside one function (`verify::Complaint`), by the reference's names.
public enum Complaint: Hashable {
    case autosOnNonMutator
    case returnTypeOnMutator
    case noReturnType
    case duplicateName(String)
    case unknownAutoTable(String)
    case nestedOption
    case typeMismatch(String, Ty, Ty)
    case notAStruct(String)
    case noSuchField(String)
    case unknownArg(String)
    case unknownAuto(String)
    case unboundSymbol(Sym)
    case readNotBound
    case readInHelper
    case writeOutsideMutator
    case refuseInHelper
    case unknownTable(String)
    case unknownColumn(String, String)
    case unknownHelper(String)
    case helperNotYetDeclared(String)
    case notAHelper(String)
    case arity(String, Int, Int)
    case badOp(String)
    case mayNotReturn
    case needsAnnotation(String)
    case badRelation(String, String)
    case keyArity(String, Int, Int)
    case stdMisuse(String)
    case unknownRouter(String?)
    case routerOnNonProcedure
    case usesOnNonProcedure
    case usesNotOnRouter([String])
    case notMiddleware(String)
    case middlewareNotYetDeclared(String)
    case middlewareInput(String, String)
    case checksOnMiddleware
    case checkOnWrongType(String, String)
    case onNotUnique(String, [String])
    case notProvided(String)
    case refineOnNonProcedure
    case queryWithoutPlan
    case queryWithBody
    case planOutsideQuery
    case planFeatureOutsideQuery(String)
    case readInPlan
    case autoInPlan
    case noRowBinder
    case membersWithoutGroup
    case emptyRole
    case scopeLeafInPlan
    case notAScope(String)
    case scopeHoldsNothing
    case holdsOutsideScope
    case scopeReadsBeyondCtx(String)
    case existsNotAReference(String, String)
    case nestedExists
    case projectionDropsKey(String, String)
    case notHeld(String, String, [String])
    case privateOutsideMutator
    case nestedPrivate
    case returnInPrivate
    case privateUndeclared
    // `docs/plan-live.md`
    /// A named type the section does not declare, or one that refers to itself.
    case unknownType(String)
    /// A sum with no variant.
    case emptySum
    /// A variant with a field named `t`, where a sum's value carries its variant's name.
    case tagField(String)
    /// An ephemeral marker anywhere but on a `PerDevice` field of a live state's record.
    case ephemeralNotOnState
    /// The state, `ctx.now`, `ctx.device` read outside a transition.
    case liveOutsideState
    /// A variant of a type that is not a sum, or of a sum without it.
    case noSuchVariant(String)
    /// A variant built without exactly its fields: the field named.
    case variantFields(String)
    /// A case whose arms are not the sum's variants, once each, in order.
    case caseArms([String])
    /// A slot of something that is not a `PerDevice`.
    case notPerDevice
    /// A read of a table, an auto or a provided value in a live state.
    case transitionReads(String)
    /// An init that is not a constant.
    case initNotConstant(String)
    /// An init that is not a value of the state's type.
    case initNotAValue(String)
    /// A scope that is not a `Text` or a `List` of them of the context alone.
    case badScope(String)
    /// A transition named as a hook is, or two of one name.
    case transitionName(String)
    /// A hook whose input is not what the engine hands it.
    case hookInput(String)
    /// An ordered transition that reads an ephemeral field (named, or "the whole state").
    case orderedReadsEphemeral(String)
}

/// A module that did not verify, and every reason.
public struct VerifyFailed: Error, CustomStringConvertible {
    public let errors: [VerifyError]
    public var description: String { errors.map { "\($0)" }.joined(separator: "; ") }
}

struct Complain: Error {
    let c: Complaint
}

/// Comparison under the one total order (`view::cmp`).
public func cmp(_ op: CmpOp, _ a: Value, _ b: Value) -> Bool {
    if op == .eq { return a == b }
    if op == .ne { return a != b }
    let o = compareValue(a, b)
    switch op {
    case .lt: return o < 0
    case .le: return o <= 0
    case .gt: return o > 0
    case .ge: return o >= 0
    default: return false
    }
}

// D2 The columns a function names, as the verifier meets them: by table
// where the table is known, and otherwise as a field read of a struct,
// resolved after to the table whose row the struct is.
final class NamedColumns {
    var columns: [TableColumn] = []
    var seen = Set<TableColumn>()
    var fields: [(TyFields, String)] = []

    func add(_ t: String, _ c: String) {
        let tc = TableColumn(table: t, column: c)
        if seen.insert(tc).inserted { columns.append(tc) }
    }

    func resolved(_ sch: Schema) -> Set<TableColumn> {
        var out = Set(columns)
        for (fs, f) in fields {
            for t in sch.tables {
                if t.column(f) == nil { continue }
                if t.columns.allSatisfy({ fs[$0.name] == $0.columnTy }) { out.insert(TableColumn(table: t.name, column: f)) }
            }
        }
        return out
    }
}

/// A table's column, by name.
public struct TableColumn: Hashable, Comparable {
    public let table: String
    public let column: String
    public static func < (a: TableColumn, b: TableColumn) -> Bool {
        let c = compareText(a.table, b.table)
        return c != 0 ? c < 0 : compareText(a.column, b.column) < 0
    }
}

public enum Verify {
    /// `verify`: the module as it is to be hashed and run, or every reason it is not.
    public static func verify(_ m0: Module) throws -> Module {
        let m = completeOrders(m0)
        if m.spec != SPEC_VERSION { throw VerifyFailed(errors: [.badSpecVersion(m.spec)]) }
        let schemaErrors = checkSchema(m.schema)
        if !schemaErrors.isEmpty { throw VerifyFailed(errors: schemaErrors.map { .badSchema($0) }) }
        let dups = duplicates(m.functions.map { $0.name })
        if !dups.isEmpty { throw VerifyFailed(errors: dups.map { .duplicateFunction($0) }) }
        let rdups = duplicates(m.routers.map { $0.name })
        if !rdups.isEmpty { throw VerifyFailed(errors: rdups.map { .duplicateRouter($0) }) }
        var errs = [VerifyError]()
        // `docs/plan-live.md` The type section and the live states.
        if let d = duplicates(m.types.map { $0.name }).first { throw VerifyFailed(errors: [.duplicateType(d)]) }
        if let d = duplicates(m.states.map { $0.name }).first { throw VerifyFailed(errors: [.duplicateState(d)]) }
        let types = m.typeMap()
        for t in m.types {
            do {
                try typeDeclOk(types, m.states, t.name, t.value)
            } catch let c as Complain {
                errs.append(.inType(t.name, c.c))
            }
        }
        for st in m.states { errs.append(contentsOf: stateOk(m, types, st)) }
        for r in m.routers {
            for u in r.uses where m.lookupFunction(u)?.kind.isMiddleware != true {
                errs.append(.inRouter(r.name, u))
            }
        }
        var named = [String: Set<TableColumn>]()
        for (i, f) in m.functions.enumerated() {
            let seen = NamedColumns()
            do {
                try functionNaming(m, i, f, seen)
            } catch let c as Complain {
                errs.append(.inFunction(f.name, c.c))
            }
            named[f.name] = seen.resolved(m.schema)
        }
        // D2 What a client runs names only what its person holds.
        if errs.isEmpty { errs.append(contentsOf: ScopeCheck.check(m, named)) }
        if !errs.isEmpty { throw VerifyFailed(errors: errs) }
        var out = m
        out.functions = m.functions.map(Normalize.function)
        out.states = m.states.map(Normalize.state)
        return out
    }

    private static func duplicates(_ names: [String]) -> [String] {
        var seen = Set<String>()
        var out = [String]()
        for n in names {
            if !seen.insert(n).inserted && !out.contains(n) { out.append(n) }
        }
        return out
    }

    private static func isSubsequence(_ xs: [String], _ ys: [String]) -> Bool {
        var it = ys.makeIterator()
        for x in xs {
            var found = false
            while let y = it.next() {
                if y == x {
                    found = true
                    break
                }
            }
            if !found { return false }
        }
        return true
    }

    // The context a function is typed in.
    struct G {
        let m: Module
        var named: NamedColumns
        let index: Int
        let f: Function
        var kind: FnKind
        var args: [String: Ty]
        var provided: [String: Ty]
        var locals: [Sym: Ty]
        var isPrivate: Bool
        // `docs/plan-live.md` The type section, which a named type is read through.
        var types: [String: Ty] = [:]
        // Inside a live state's transition: the state's type, resolved and unmarked.
        var live: Ty? = nil

        var schema: Schema { m.schema }

        func names(_ t: String, _ c: String) { named.add(t, c) }

        func bind(_ x: Sym, _ t: Ty) -> G {
            var g = self
            g.locals[x] = t
            return g
        }
    }

    private static func complain(_ c: Complaint) -> Complain { Complain(c: c) }

    private static func functionNaming(_ m: Module, _ i: Int, _ f: Function, _ named: NamedColumns) throws {
        let sch = m.schema
        switch f.kind {
        case .mutator, .query:
            guard let rn = f.router else { throw complain(.unknownRouter(nil)) }
            guard let r = m.lookupRouter(rn) else { throw complain(.unknownRouter(rn)) }
            if !isSubsequence(f.uses, r.uses) { throw complain(.usesNotOnRouter(f.uses)) }
            for u in f.uses {
                let j = m.functions.firstIndex { $0.name == u }
                guard let jj = j, m.functions[jj].kind.isMiddleware else { throw complain(.notMiddleware(u)) }
                let g = m.functions[jj]
                if jj >= i { throw complain(.middlewareNotYetDeclared(u)) }
                for gi in g.input {
                    let here = f.input.first { $0.name == gi.name }?.value.ty
                    if here != gi.value.ty { throw complain(.middlewareInput(u, gi.name)) }
                }
            }
            if f.kind == .mutator && f.ret != nil { throw complain(.returnTypeOnMutator) }
            if f.kind == .query && f.ret == nil { throw complain(.noReturnType) }
            if f.kind == .query && !f.autos.isEmpty { throw complain(.autosOnNonMutator) }
            if f.kind == .query {
                if f.plan == nil { throw complain(.queryWithoutPlan) }
                if !f.body.isEmpty { throw complain(.queryWithBody) }
            }
        case .guardFn, .provide:
            if f.router != nil { throw complain(.routerOnNonProcedure) }
            if !f.uses.isEmpty { throw complain(.usesOnNonProcedure) }
            if !f.autos.isEmpty { throw complain(.autosOnNonMutator) }
            if f.input.contains(where: { !$0.value.checks.isEmpty }) || !f.refine.isEmpty { throw complain(.checksOnMiddleware) }
            if f.kind == .guardFn && f.ret != nil { throw complain(.returnTypeOnMutator) }
            if f.kind == .provide && f.ret == nil { throw complain(.noReturnType) }
        case .scope:
            return try scopeOk(m, i, f)
        case .helper:
            if f.router != nil { throw complain(.routerOnNonProcedure) }
            if !f.uses.isEmpty { throw complain(.usesOnNonProcedure) }
            if !f.autos.isEmpty { throw complain(.autosOnNonMutator) }
            if f.ret == nil { throw complain(.noReturnType) }
            if f.input.contains(where: { !$0.value.checks.isEmpty }) || !f.refine.isEmpty { throw complain(.refineOnNonProcedure) }
        }
        if !f.holds.isEmpty { throw complain(.holdsOutsideScope) }
        // D3 Only a mutator has a server half, and one that has one says so.
        if f.isPrivate && f.kind != .mutator { throw complain(.privateOutsideMutator) }
        if !f.isPrivate && hasPrivate(f.body) { throw complain(.privateUndeclared) }
        if let d = duplicates(f.input.map { $0.name } + f.autos.map { $0.name }).first { throw complain(.duplicateName(d)) }
        let types = m.typeMap()
        for i in f.input { _ = try plainTy(types, i.value.ty) }
        for a in f.autos {
            if case .newId(let t) = a.value, sch.lookupTable(t) == nil { throw complain(.unknownAutoTable(t)) }
        }
        if let r = f.ret { _ = try plainTy(types, r) }
        var args = [String: Ty]()
        for i in f.input { args[i.name] = try plainTy(types, i.value.ty) }
        var provided = [String: Ty]()
        for u in f.uses {
            guard let g = m.lookupFunction(u) else { continue }
            if g.kind == .provide, let r = g.ret { provided[g.name] = try plainTy(types, r) }
        }
        let g = G(m: m, named: named, index: i, f: f, kind: f.kind, args: args, provided: provided, locals: [:], isPrivate: false, types: types)
        // The input's checks, each over its own field as it stands.
        for inp in f.input {
            var inner = inp.value.ty
            if case .option(let o) = inner { inner = o }
            for c in inp.value.checks {
                var fits: Bool
                switch c {
                case .trim, .minLen, .maxLen: fits = inner == .text
                case .range: fits = inner == .int
                case .nonEmpty:
                    if case .list = inner { fits = true } else { fits = false }
                case .exists:
                    if case .id(let t) = inner {
                        if sch.lookupTable(t) == nil { throw complain(.unknownTable(t)) }
                        fits = true
                    } else {
                        fits = false
                    }
                case .refine(let e, _):
                    var gc = g
                    gc.args[inp.name] = inner
                    gc.kind = .helper
                    gc.provided = [:]
                    try expect(gc, "refine", .bool, e)
                    fits = true
                }
                if !fits { throw complain(.checkOnWrongType(inp.name, c.description)) }
            }
        }
        for r in f.refine {
            var gc = g
            gc.kind = .helper
            gc.provided = [:]
            try expect(gc, "refine", .bool, r.e)
        }
        if let p = f.plan {
            if f.kind != .query { throw complain(.planOutsideQuery) }
            let got = Ty.list(try queryPlan(g, p))
            guard let ret = f.ret, same(sch, ret, got) else { throw complain(.typeMismatch("query result", f.ret ?? .bool, got)) }
            return
        }
        try block(g, f.body)
        let mustReturn = f.kind == .query || f.kind == .helper || f.kind == .provide
        if mustReturn && !returns(f.body) { throw complain(.mayNotReturn) }
    }

    private static func noNestedOption(_ t: Ty) throws {
        switch t {
        case .option(let o), .perDevice(let o):
            // A slot reads as an option of its type, which may not be one.
            if case .option = o { throw complain(.nestedOption) }
            try noNestedOption(o)
        case .list(let o), .nonEmpty(let o), .ephemeral(let o): try noNestedOption(o)
        case .structure(let fs): for x in fs.types { try noNestedOption(x) }
        case .sum(let vs): for fs in vs.variants { for x in fs.types { try noNestedOption(x) } }
        default: break
        }
    }

    // A declared type as the checker reads it: named types resolved, no option
    // of an option, and no ephemeral marker — which belongs on a live state's
    // own fields alone.
    private static func plainTy(_ types: [String: Ty], _ t: Ty) throws -> Ty {
        let r: Ty
        do {
            r = try Types.resolve(types, t)
        } catch let e as Types.TypeError {
            throw complain(.unknownType(e.why))
        }
        try noNestedOption(r)
        if marked(r) { throw complain(.ephemeralNotOnState) }
        return r
    }

    // Whether a type carries an ephemeral marker anywhere.
    private static func marked(_ t: Ty) -> Bool {
        switch t {
        case .ephemeral: return true
        case .option(let x), .list(let x), .nonEmpty(let x), .perDevice(let x): return marked(x)
        case .structure(let fs): return fs.types.contains(where: marked)
        case .sum(let vs): return vs.variants.contains { $0.types.contains(where: marked) }
        default: return false
        }
    }

    // A value read where it is declared a non-empty list is a list to the
    // checker: what it was promised is held at the boundary (`Types`).
    private static func readAs(_ t: Ty) -> Ty {
        if case .nonEmpty(let x) = t { return .list(x) }
        return t
    }

    // A block definitely returns when its last statement does, or is an `if`
    // both of whose branches do.
    private static func returns(_ b: Block) -> Bool {
        guard let s = b.last else { return false }
        switch s {
        case .ret, .refuse: return true
        case .ifS(_, let a, let e): return returns(a) && returns(e)
        default: return false
        }
    }

    private static func block(_ g: G, _ b: Block) throws {
        var g2 = g
        for s in b { g2 = try stmt(g2, s) }
    }

    private static func stmt(_ g: G, _ s: Stmt) throws -> G {
        func readOk() throws { if g.kind == .helper { throw complain(.readInHelper) } }
        func mutating() throws { if g.kind != .mutator { throw complain(.writeOutsideMutator) } }
        switch s {
        case .letS(let x, let e):
            var t: Ty
            switch e {
            case .select(let p):
                try readOk()
                t = try planTy(g, p)
            case .get(let tb, let k):
                try readOk()
                try keyed(g, tb, k)
                t = .option(try table(g, tb).rowTy)
            case .exists(let tb, let k):
                try readOk()
                try keyed(g, tb, k)
                t = .bool
            default:
                t = try infer(g, nil, e)
            }
            return g.bind(x, t)
        case .ifS(let c, let a, let b):
            try expect(g, "if condition", .bool, c)
            try block(g, a)
            try block(g, b)
        case .forS(let x, let o, let body):
            let t = try elemOf("for", try infer(g, nil, o))
            try block(g.bind(x, t), body)
        case .insert(let t, let row, let on), .upsert(let t, let row, let on):
            try insertLike(g, t, row, on, mutating)
        case .update(let t, let key, let x, let row):
            try mutating()
            try keyed(g, t, key)
            written(g, t, row)
            let tbl = try table(g, t)
            try rowFits(g.bind(x, tbl.rowTy), tbl, row)
        case .delete(let t, let key):
            try mutating()
            try keyed(g, t, key)
        case .refuse(let e):
            if g.kind == .helper { throw complain(.refuseInHelper) }
            try expect(g, "refuse", .text, e)
        case .ret(let e):
            if g.isPrivate { throw complain(.returnInPrivate) }
            switch (g.f.ret, e) {
            case (nil, nil): break
            case (nil, _): throw complain(.returnTypeOnMutator)
            case (_, nil): throw complain(.noReturnType)
            case (let want?, let e?): try expect(g, "return", try plainTy(g.types, want), e)
            }
        // D3 Typed as the body around it is; what it names goes nowhere.
        case .privateS(let body):
            if g.kind != .mutator { throw complain(.privateOutsideMutator) }
            if g.isPrivate { throw complain(.nestedPrivate) }
            var gp = g
            gp.named = NamedColumns()
            gp.isPrivate = true
            try block(gp, body)
        }
        return g
    }

    private static func insertLike(_ g: G, _ t: String, _ e: Expr, _ on: [String], _ mutating: () throws -> Void) throws {
        try mutating()
        let tbl = try table(g, t)
        written(g, t, e)
        for c in on { g.names(t, c) }
        try rowFits(g, tbl, e)
        if !on.isEmpty {
            let want = Set(on)
            let unique = tbl.indexes.contains { $0.unique && Set($0.columns) == want && $0.columns.count == on.count }
            if !unique { throw complain(.onNotUnique(t, on)) }
        }
    }

    // The columns a write names: every field of the struct it writes, written out (D2).
    private static func written(_ g: G, _ t: String, _ e: Expr) {
        if case .structE(let fs) = e { for k in fs.names { g.names(t, k) } }
    }

    // A written row may leave nullable columns out, so it is checked field by field.
    private static func rowFits(_ g: G, _ tbl: Table, _ e: Expr) throws {
        let row = tbl.rowTy
        let rf = row.structFields!
        let got = try infer(g, row, e)
        let site = "write \(tbl.name)"
        guard case .structure(let gf) = got else { throw complain(.typeMismatch(site, row, got)) }
        for (k, ty) in gf.pairs {
            guard let w = rf[k] else { throw complain(.unknownColumn(tbl.name, k)) }
            if w != ty { throw complain(.typeMismatch("\(site).\(k)", w, ty)) }
        }
        for c in tbl.columns where !c.nullable && !gf.has(c.name) { throw complain(.typeMismatch(site, row, got)) }
    }

    private static func table(_ g: G, _ t: String) throws -> Table {
        guard let tb = g.schema.lookupTable(t) else { throw complain(.unknownTable(t)) }
        return tb
    }

    private static func keyed(_ g: G, _ t: String, _ ks: [Expr]) throws {
        let want = try table(g, t).keyTy
        if want.count != ks.count { throw complain(.keyArity(t, want.count, ks.count)) }
        for (w, k) in zip(want, ks) { try expect(g, "key of \(t)", w, k) }
    }

    /// §9 `same`: two types the same, where a table's row is that table's row
    /// whichever schema it was written against — structs differing only in
    /// nullable columns of one table, carried whole by the wider, are the
    /// same type (`docs/plan-db.md` D1).
    static func same(_ sch: Schema, _ a: Ty, _ b: Ty) -> Bool {
        if a == b { return true }
        switch (a, b) {
        case (.option(let x), .option(let y)): return same(sch, x, y)
        case (.list(let x), .list(let y)), (.perDevice(let x), .perDevice(let y)): return same(sch, x, y)
        // `docs/plan-live.md` A non-empty list is a list to the checker, and
        // held to its one element where it crosses a boundary.
        case (.nonEmpty(let x), .list(let y)), (.list(let x), .nonEmpty(let y)), (.nonEmpty(let x), .nonEmpty(let y)): return same(sch, x, y)
        case (.structure(let x), .structure(let y)):
            let (wide, narrow) = x.count >= y.count ? (x, y) : (y, x)
            for (k, t) in narrow.pairs {
                guard let w = wide[k], same(sch, w, t) else { return false }
            }
            let extra = wide.names.filter { !narrow.has($0) }
            if extra.isEmpty { return true }
            return sch.tables.contains { tbl in
                tbl.columns.allSatisfy { wide[$0.name] == $0.columnTy } && extra.allSatisfy { tbl.column($0)?.nullable == true }
            }
        default: return false
        }
    }

    private static func expect(_ g: G, _ site: String, _ want: Ty, _ e: Expr) throws {
        let got = try infer(g, want, e)
        if !same(g.schema, got, want) { throw complain(.typeMismatch(site, want, got)) }
    }

    private static func elemOf(_ site: String, _ t: Ty) throws -> Ty {
        guard case .list(let o) = t else { throw complain(.typeMismatch(site, .list(t), t)) }
        return o
    }

    // §9.1 Typing of expressions; the expected type is used only where an
    // expression cannot be typed on its own.
    private static func infer(_ g: G, _ want: Ty?, _ e: Expr) throws -> Ty {
        switch e {
        case .lit(let v): return try lit(want, v)
        case .arg(let n):
            guard let t = g.args[n] else { throw complain(.unknownArg(n)) }
            return readAs(t)
        case .auto(let n):
            let a = g.f.autos.first { $0.name == n }?.value
            switch a {
            case .newId(let t)? where g.kind == .mutator: return .id(t)
            case .now? where g.kind == .mutator: return .int
            default: throw complain(.unknownAuto(n))
            }
        case .variable(let x):
            guard let t = g.locals[x] else { throw complain(.unboundSymbol(x)) }
            return t
        case .ctxUser, .ctxSession: return .text
        case .hasRole(let r):
            if r.isEmpty { throw complain(.emptyRole) }
            return .bool
        case .provided(let f):
            guard let t = g.provided[f] else { throw complain(.notProvided(f)) }
            return t
        case .field(let x, let name):
            let t = try infer(g, nil, x)
            guard case .structure(let fs) = t else { throw complain(.notAStruct(name)) }
            guard let ft = fs[name] else { throw complain(.noSuchField(name)) }
            g.named.fields.append((fs, name))
            return readAs(ft)
        case .structE(let fs):
            // `docs/plan-live.md` A struct written where a `PerDevice` is
            // wanted is its slots, by device id: `{}` is every slot empty.
            if case .perDevice(let t)? = want {
                for (k, x) in zip(fs.names, fs.exprs) { try expect(g, "slot \(k)", t, x) }
                return .perDevice(t)
            }
            let wf = want?.structFields
            var out = [(String, Ty)]()
            for (k, x) in zip(fs.names, fs.exprs) { out.append((k, try infer(g, wf?[k], x))) }
            return .structOf(out)
        case .listE(let items):
            var we: Ty? = nil
            if case .list(let o)? = want { we = o }
            if case .nonEmpty(let o)? = want { we = o }
            let ts = try items.map { try infer(g, we, $0) }
            if ts.isEmpty {
                if let w = we { return .list(w) }
                throw complain(.needsAnnotation("empty list"))
            }
            let t = ts[0]
            for t2 in ts.dropFirst() where !same(g.schema, t2, t) { throw complain(.typeMismatch("list element", t, t2)) }
            return .list(t)
        case .some(let x):
            var wo: Ty? = nil
            if case .option(let o)? = want { wo = o }
            let t = try infer(g, wo, x)
            if case .option = t { throw complain(.nestedOption) }
            return .option(t)
        case .none(let t):
            let o = Ty.option(try plainTy(g.types, t))
            try noNestedOption(o)
            return o
        case .match(let x, let s, let a, let n):
            let o = try infer(g, nil, x)
            guard case .option(let inner) = o else { throw complain(.typeMismatch("match", .option(o), o)) }
            let ta = try infer(g.bind(s, inner), want, a)
            let tb = try infer(g, ta, n)
            if !same(g.schema, ta, tb) { throw complain(.typeMismatch("match arms", ta, tb)) }
            return ta
        case .ife(let c, let a, let n):
            try expect(g, "if", .bool, c)
            let ta = try infer(g, want, a)
            let tb = try infer(g, ta, n)
            if !same(g.schema, ta, tb) { throw complain(.typeMismatch("if arms", ta, tb)) }
            return ta
        case .op(let o, let args):
            func all(_ t: Ty) throws -> Ty {
                for x in args { try expect(g, t == .bool ? "boolean operator" : "arithmetic", t, x) }
                return t
            }
            let n = args.count
            switch o {
            case .and, .or:
                if n == 2 { return try all(.bool) }
            case .not:
                if n == 1 { return try all(.bool) }
            case .neg:
                if n == 1 { return try all(.int) }
            default:
                if n == 2 { return try all(.int) }
            }
            throw complain(.badOp(o.debugName))
        case .cmp(_, let l, let r):
            let ta = try infer(g, nil, l)
            try expect(g, "comparison", ta, r)
            return .bool
        case .call(let fn, let args):
            guard let j = g.m.functions.firstIndex(where: { $0.name == fn }) else { throw complain(.unknownHelper(fn)) }
            let f = g.m.functions[j]
            if f.kind != .helper { throw complain(.notAHelper(fn)) }
            if j >= g.index { throw complain(.helperNotYetDeclared(fn)) }
            if args.count != f.input.count { throw complain(.arity(fn, f.input.count, args.count)) }
            for (p, x) in zip(f.input, args) { try expect(g, "argument of \(fn)", try plainTy(g.types, p.value.ty), x) }
            guard let r = f.ret else { throw complain(.notAHelper(fn)) }
            return try plainTy(g.types, r)
        case .std(let f, let args):
            return try stdTy(f, try args.map { try infer(g, nil, $0) }, want)
        case .map(let o, let s, let b):
            let t = try elemOf("map", try infer(g, nil, o))
            var wl: Ty? = nil
            if case .list(let w)? = want { wl = w }
            return .list(try infer(g.bind(s, t), wl, b))
        case .filter(let o, let s, let b):
            let t = try elemOf("filter", try infer(g, nil, o))
            try expect(g.bind(s, t), "filter body", .bool, b)
            return .list(t)
        case .any(let o, let s, let b), .all(let o, let s, let b):
            let t = try elemOf("any/all", try infer(g, nil, o))
            try expect(g.bind(s, t), "any/all body", .bool, b)
            return .bool
        case .sortBy(let o, let s, let k):
            let t = try elemOf("sort_by", try infer(g, nil, o))
            _ = try infer(g.bind(s, t), nil, k)
            return .list(t)
        case .fold(let o, let z, let acc, let s, let b):
            let t = try elemOf("fold", try infer(g, nil, o))
            let a = try infer(g, want, z)
            try expect(g.bind(s, t).bind(acc, a), "fold body", a, b)
            return a
        case .select, .get, .exists:
            throw complain(.readNotBound)
        // `docs/plan-live.md` A sum built: one of its variants, with exactly
        // that variant's fields, each of its type.
        case .variant(let t, let n, let fs):
            let sum = try plainTy(g.types, t)
            guard case .sum(let vs) = sum, let wantFs = vs[n] else { throw complain(.noSuchVariant(n)) }
            if let k = fs.names.first(where: { !wantFs.has($0) }) ?? wantFs.names.first(where: { k in !fs.names.contains(where: { textEquals($0, k) }) }) {
                throw complain(.variantFields(k))
            }
            for (k, x) in zip(fs.names, fs.exprs) { try expect(g, "variant \(n).\(k)", wantFs[k]!, x) }
            return sum
        // A sum taken apart: an arm per variant, in order, each over the variant's record.
        case .caseOf(let x, let arms):
            guard case .sum(let vs) = try infer(g, nil, x) else { throw complain(.caseArms(arms.map { $0.name })) }
            let names = arms.map { $0.name }
            if names.count != vs.names.count || !zip(names, vs.names).allSatisfy({ textEquals($0.0, $0.1) }) { throw complain(.caseArms(names)) }
            var out: Ty? = nil
            for a in arms {
                let t = try infer(g.bind(a.sym, .structure(vs[a.name]!)), out ?? want, a.e)
                if let o = out {
                    if !same(g.schema, o, t) { throw complain(.typeMismatch("case arms", o, t)) }
                } else {
                    out = t
                }
            }
            guard let o = out else { throw complain(.emptySum) }
            return o
        case .liveState:
            guard let l = g.live else { throw complain(.liveOutsideState) }
            return l
        case .ctxNow:
            if g.live == nil { throw complain(.liveOutsideState) }
            return .int
        case .ctxDevice:
            if g.live == nil { throw complain(.liveOutsideState) }
            return .text
        case .slot(let m, let k):
            guard case .perDevice(let t) = try infer(g, nil, m) else { throw complain(.notPerDevice) }
            try expect(g, "slot key", .text, k)
            return .option(t)
        case .setSlot(let m, let k, let v):
            let pd = try infer(g, nil, m)
            guard case .perDevice(let t) = pd else { throw complain(.notPerDevice) }
            try expect(g, "slot key", .text, k)
            try expect(g, "slot value", .option(t), v)
            return pd
        case .slots(let m):
            guard case .perDevice(let t) = try infer(g, nil, m) else { throw complain(.notPerDevice) }
            return .list(.structOf([("device", .text), ("value", t)]))
        }
    }

    private static func lit(_ want: Ty?, _ v: Value) throws -> Ty {
        switch v {
        case .null: throw complain(.needsAnnotation("null literal; use None"))
        case .bool: return .bool
        case .int: return .int
        case .text:
            if case .enumeration? = want { return want! }
            return .text
        case .bytes: return .bytes
        case .id:
            if case .id? = want { return want! }
            throw complain(.needsAnnotation("id literal"))
        case .list: throw complain(.needsAnnotation("list literal; use a list expression"))
        case .record: throw complain(.needsAnnotation("struct literal; use a struct expression"))
        }
    }

    // §9.2 The type of a read in a body: the plan must be v3-shaped (§1.4).
    private static func planTy(_ g: G, _ p: Plan) throws -> Ty {
        try v3Shaped(g, p)
        let t = try table(g, p.table)
        if let f = p.filter { try predOk(g, t, f) }
        for o in p.order {
            if case .column(let c) = o.key {
                g.names(t.name, c)
                if t.column(c) == nil { throw complain(.unknownColumn(t.name, c)) }
            }
        }
        var fields = t.rowTy.structFields!
        for r in p.related { fields = fields.with(r.name, try planTy(g, r.plan)) }
        return .list(.structure(fields))
    }

    // §1.4 What a read outside a query may be, or the first thing it has that it may not.
    private static func v3Shaped(_ g: G, _ p: Plan) throws {
        func feature(_ what: String) -> Complain { complain(.planFeatureOutsideQuery(what)) }
        if case .group = p.source { throw feature("a group source") }
        if p.members != nil { throw complain(.membersWithoutGroup) }
        if !p.lookups.isEmpty { throw feature("a lookup") }
        if p.having != nil { throw feature("a having") }
        if p.project != nil { throw feature("a projection") }
        if p.order.contains(where: { if case .expr = $0.key { return true } else { return false } }) { throw feature("an expression order key") }
        if !p.related.isEmpty && p.row == nil { throw complain(.noRowBinder) }
        for r in p.related {
            // The one related form a body may read: `child.col == row.key`.
            var ok = false
            if r.on.count == 1, let row = p.row, case .field(let fe, let fname) = r.on[0].e, fe == .variable(row) {
                ok = g.schema.childrenOf(p.table).contains { $0.child == r.plan.table && $0.column == r.on[0].column }
                    && g.schema.lookupTable(p.table)?.key == [fname]
            }
            if !ok { throw feature("a related plan that is not a reference") }
        }
    }

    // §1.10 A query's plan: every node typed under its own binders.
    private static func queryPlan(_ g: G, _ p: Plan) throws -> Ty {
        try planReadsNothing(p)
        return try nodeTy(g, p)
    }

    private static func nodeTy(_ g: G, _ p: Plan) throws -> Ty {
        let t = try table(g, p.table)
        if let f = p.filter { try predOk(g, t, f) }
        var rowTy: Ty
        switch p.source {
        case .table: rowTy = t.rowTy
        case .group(_, let by):
            var fs = [(String, Ty)]()
            for c in by {
                g.names(t.name, c)
                guard let col = t.column(c) else { throw complain(.unknownColumn(t.name, c)) }
                fs.append((c, col.columnTy))
            }
            rowTy = .structOf(fs)
        }
        if !p.isBare && p.row == nil { throw complain(.noRowBinder) }
        if p.members != nil {
            if case .group = p.source {} else { throw complain(.membersWithoutGroup) }
        }
        var node = g
        node.locals = [:]
        if let r = p.row { node = node.bind(r, rowTy) }
        if let m = p.members { node = node.bind(m, .list(t.rowTy)) }
        for l in p.lookups {
            let lt = try table(g, l.table)
            let want = lt.keyTy
            if want.count != l.key.count { throw complain(.keyArity(l.table, want.count, l.key.count)) }
            for (w, k) in zip(want, l.key) {
                let got = try infer(node, w, k)
                if got != w && got != .option(w) { throw complain(.typeMismatch("key of \(l.table)", w, got)) }
            }
            node = node.bind(l.sym, .option(lt.rowTy))
        }
        var fields = rowTy.structFields!
        for r in p.related {
            let ct = try table(g, r.plan.table)
            for o in r.on {
                g.names(ct.name, o.column)
                guard let col = ct.column(o.column) else { throw complain(.unknownColumn(ct.name, o.column)) }
                let want = col.columnTy
                let got = try infer(node, want, o.e)
                if got != want && stripOption(got) != stripOption(want) { throw complain(.typeMismatch("on \(ct.name).\(o.column)", want, got)) }
            }
            let child = Ty.list(try nodeTy(g, r.plan))
            fields = fields.with(r.name, child)
            node = node.bind(r.sym, child)
        }
        if let h = p.having { try expect(node, "having", .bool, h) }
        for o in p.order {
            switch o.key {
            case .column(let c):
                g.names(t.name, c)
                if !(rowTy.structFields!.has(c)) { throw complain(.unknownColumn(t.name, c)) }
            case .expr(let e):
                _ = try infer(node, nil, e)
            }
        }
        if let pr = p.project { return try infer(node, nil, pr) }
        return .structure(fields)
    }

    private static func stripOption(_ t: Ty) -> Ty {
        if case .option(let o) = t { return o }
        return t
    }

    // §1.3 No expression in a plan reads or draws an auto, at any depth.
    private static func planReadsNothing(_ p: Plan) throws {
        var es = [Expr]()
        if let f = p.filter { predExprs(f, &es) }
        for l in p.lookups { es += l.key }
        for r in p.related { es += r.on.map { $0.e } }
        if let h = p.having { es.append(h) }
        if let pr = p.project { es.append(pr) }
        for o in p.order { if case .expr(let e) = o.key { es.append(e) } }
        for e in es { if let c = forbidden(e) { throw complain(c) } }
        for r in p.related { try planReadsNothing(r.plan) }
    }

    private static func predExprs(_ p: Pred, _ out: inout [Expr]) {
        switch p {
        case .cmp(_, _, let e), .has(_, let e), .when(let e): out.append(e)
        case .inP(_, let xs): out += xs
        case .all(let ps), .any(let ps): for q in ps { predExprs(q, &out) }
        case .not(let q), .exists(_, _, let q): predExprs(q, &out)
        }
    }

    // The first read or auto inside an expression.
    private static func forbidden(_ e: Expr) -> Complaint? {
        switch e {
        case .select, .get, .exists: return .readInPlan
        case .auto: return .autoInPlan
        default:
            for c in children(e) { if let f = forbidden(c) { return f } }
            return nil
        }
    }

    private static func predOk(_ g: G, _ t: Table, _ p: Pred) throws {
        func col(_ c: String) throws -> Column {
            g.names(t.name, c)
            guard let column = t.column(c) else { throw complain(.unknownColumn(t.name, c)) }
            return column
        }
        switch p {
        case .cmp(let c, _, let e): try expect(g, "filter on \(c)", try col(c).columnTy, e)
        case .inP(let c, let items):
            let ty = try col(c).columnTy
            for x in items { try expect(g, "filter on \(c)", ty, x) }
        case .all(let ps), .any(let ps): for q in ps { try predOk(g, t, q) }
        case .not(let q): try predOk(g, t, q)
        // D4 A substring of a text column, the needle text.
        case .has(let c, let e):
            let ty = try col(c).columnTy
            if ty != .text && ty != .option(.text) { throw complain(.typeMismatch("has on \(c)", .text, ty)) }
            try expect(g, "has on \(c)", .text, e)
        case .when, .exists:
            throw complain(.scopeLeafInPlan)
        }
    }

    // D2 A scope: middleware with nothing but holds, at least one.
    private static func scopeOk(_ m: Module, _ i: Int, _ f: Function) throws {
        func not(_ what: String) -> Complain { complain(.notAScope(what)) }
        if f.router != nil { throw complain(.routerOnNonProcedure) }
        if !f.uses.isEmpty { throw complain(.usesOnNonProcedure) }
        if !f.autos.isEmpty { throw complain(.autosOnNonMutator) }
        if !f.input.isEmpty || !f.refine.isEmpty { throw not("input") }
        if f.ret != nil { throw not("a return type") }
        if !f.body.isEmpty { throw not("a body") }
        if f.plan != nil { throw not("a plan") }
        if f.holds.isEmpty { throw complain(.scopeHoldsNothing) }
        let g = G(m: m, named: NamedColumns(), index: i, f: f, kind: .helper, args: [:], provided: [:], locals: [:], isPrivate: false)
        for h in f.holds {
            let t = try table(g, h.table)
            if let fl = h.filter { try scopePredOk(g, t, fl, false) }
            var named = [String]()
            switch h.columns {
            case .all: break
            case .pick(let cs), .exclude(let cs): named = cs
            }
            var seen = Set<String>()
            for c in named {
                if t.column(c) == nil { throw complain(.unknownColumn(t.name, c)) }
                if !seen.insert(c).inserted { throw complain(.projectionDropsKey(t.name, c)) }
            }
            if let k = t.key.first(where: { !h.columns.keeps($0) }) { throw complain(.projectionDropsKey(t.name, k)) }
        }
    }

    // A scope's predicate: right-hand sides of the context and literals alone,
    // plus `When` and one `Exists` through a reference.
    private static func scopePredOk(_ g: G, _ t: Table, _ p: Pred, _ nested: Bool) throws {
        func col(_ c: String) throws -> Column {
            guard let column = t.column(c) else { throw complain(.unknownColumn(t.name, c)) }
            return column
        }
        switch p {
        case .cmp(let c, _, let e):
            try ctxOnly(e)
            try expect(g, "scope on \(t.name).\(c)", try col(c).columnTy, e)
        case .inP(let c, let items):
            let ty = try col(c).columnTy
            for x in items {
                try ctxOnly(x)
                try expect(g, "scope on \(t.name).\(c)", ty, x)
            }
        case .has(_, let e):
            try ctxOnly(e)
            try predOk(g, t, p)
        case .when(let e):
            try ctxOnly(e)
            try expect(g, "scope condition", .bool, e)
        case .all(let ps), .any(let ps): for q in ps { try scopePredOk(g, t, q, nested) }
        case .not(let q): try scopePredOk(g, t, q, nested)
        case .exists(let tb, let c, let q):
            if nested { throw complain(.nestedExists) }
            let child = try table(g, tb)
            let refers = child.refs.contains { $0.column == c && $0.table == t.name } && t.key.count == 1
            if !refers { throw complain(.existsNotAReference(tb, c)) }
            try scopePredOk(g, child, q, true)
        }
    }

    // An expression a scope may hold: the context, literals, and the
    // operators and comparisons over them.
    private static func ctxOnly(_ e: Expr) throws {
        func beyond(_ what: String) -> Complain { complain(.scopeReadsBeyondCtx(what)) }
        switch e {
        case .lit, .ctxUser, .ctxSession, .hasRole: break
        case .op(_, let args): for x in args { try ctxOnly(x) }
        case .cmp(_, let l, let r):
            try ctxOnly(l)
            try ctxOnly(r)
        case .arg: throw beyond("an argument")
        case .auto: throw beyond("an auto")
        case .variable: throw beyond("a local")
        case .provided: throw beyond("a provided value")
        case .select, .get, .exists: throw beyond("a read")
        case .call: throw beyond("a call")
        default: throw beyond("an expression of more than the context")
        }
    }

    // §9.3 Signatures of the standard library.
    // `docs/plan-live.md` One declaration of the type section: it resolves —
    // every name it reaches is declared, and none refers to itself — a sum has
    // a variant and no variant a field named `t`, and an ephemeral marker is on
    // a `PerDevice` field of a record some live state is.
    private static func typeDeclOk(_ types: [String: Ty], _ states: [LiveStateDecl], _ name: String, _ t: Ty) throws {
        let r: Ty
        do {
            r = try Types.resolve(types, t)
        } catch let e as Types.TypeError {
            throw complain(.unknownType(e.why))
        }
        try noNestedOption(r)
        try sumsOk(r)
        if marked(r) {
            let isState = states.contains { $0.ty == .named(name) }
            if !isState || !stateMarksOk(r) { throw complain(.ephemeralNotOnState) }
        }
    }

    // Every sum in a type has a variant, and no variant a field named `t`.
    private static func sumsOk(_ t: Ty) throws {
        switch t {
        case .sum(let vs):
            if vs.isEmpty { throw complain(.emptySum) }
            for (v, fs) in vs.pairs {
                if fs.has(Types.tag) { throw complain(.tagField(v)) }
                for x in fs.types { try sumsOk(x) }
            }
        case .option(let x), .list(let x), .nonEmpty(let x), .perDevice(let x), .ephemeral(let x): try sumsOk(x)
        case .structure(let fs): for x in fs.types { try sumsOk(x) }
        default: break
        }
    }

    // A state's resolved type marks ephemeral only its own record's fields,
    // each a `PerDevice`, with nothing marked beneath.
    private static func stateMarksOk(_ t: Ty) -> Bool {
        guard case .structure(let fs) = t else { return !marked(t) }
        return fs.types.allSatisfy { f in
            if case .ephemeral(let x) = f {
                if case .perDevice = x { return !marked(x) }
                return false
            }
            return !marked(f)
        }
    }

    /// `docs/plan-live.md` `state_ok`: its type resolves and marks only what it
    /// may; its init is a constant value of it; its scope is a `Text` or a
    /// list of them of the context alone; and every transition and hook is
    /// typed over the state, its context and its input, reads nothing else,
    /// and — ordered — reads no ephemeral field.
    private static func stateOk(_ m: Module, _ types: [String: Ty], _ st: LiveStateDecl) -> [VerifyError] {
        func here(_ c: Complaint) -> VerifyError { .inState(st.name, c) }
        let markedTy: Ty
        do {
            markedTy = try Types.resolve(types, st.ty)
        } catch {
            return [here(.unknownType("\(error)"))]
        }
        do {
            try noNestedOption(markedTy)
            try sumsOk(markedTy)
        } catch let c as Complain {
            return [here(c.c)]
        } catch {
            return [here(.unknownType("\(error)"))]
        }
        if !stateMarksOk(markedTy) { return [here(.ephemeralNotOnState)] }
        let ty = Types.unmarked(markedTy)
        let g = G(m: m, named: NamedColumns(), index: m.functions.count, f: transitionShell(st.name, ty), kind: .helper, args: [:], provided: [:], locals: [:], isPrivate: false, types: types)
        var errs = [VerifyError]()
        // The init: a constant of the state's type, and a value of it.
        if let c = constantOrNot(st.initial) {
            errs.append(here(.initNotConstant(c)))
        } else {
            do {
                try expect(g, "init", ty, st.initial)
                do {
                    if let why = Types.whyNot(types, st.ty, try Eval.constant(st.initial)) { errs.append(here(.initNotAValue(why))) }
                } catch {
                    errs.append(here(.initNotAValue("\(error)")))
                }
            } catch let c as Complain {
                errs.append(here(c.c))
            } catch {
                errs.append(here(.initNotAValue("\(error)")))
            }
        }
        // The scope: of the context alone.
        do {
            try ctxOnlyScope(st.scope)
            let t = try infer(g, nil, st.scope)
            if t != .text && t != .list(.text) { errs.append(here(.badScope("a scope is text or a list of text, not \(t)"))) }
        } catch let c as Complain {
            errs.append(here(c.c))
        } catch {
            errs.append(here(.badScope("\(error)")))
        }
        if let d = duplicates(st.transitions.map { $0.name }).first { errs.append(here(.transitionName(d))) }
        for t in st.transitions where isHook(t.name) {
            errs.append(.inTransition(st.name, t.name, .transitionName(t.name)))
        }
        var all: [(Transition, String?)] = st.transitions.map { ($0, nil) }
        if let t = st.onJoin { all.append((t, onJoinName)) }
        if let t = st.onLeave { all.append((t, onLeaveName)) }
        if let t = st.onRestore { all.append((t, onRestoreName)) }
        for (t, hook) in all {
            do {
                try transitionOk(m, types, markedTy, ty, t, hook)
            } catch let c as Complain {
                errs.append(.inTransition(st.name, t.name, c.c))
            } catch {
                errs.append(.inTransition(st.name, t.name, .transitionReads("\(error)")))
            }
        }
        return errs
    }

    // The function a transition is typed as: a body returning the state.
    private static func transitionShell(_ name: String, _ ret: Ty) -> Function {
        var f = Normalize.transitionFunction(Transition(name: name, input: [], guards: [], body: []))
        f.ret = ret
        return f
    }

    private static func transitionOk(_ m: Module, _ types: [String: Ty], _ markedTy: Ty, _ ty: Ty, _ t: Transition, _ hook: String?) throws {
        // A hook is handed what the engine has: a device's record, or nothing.
        if let h = hook {
            if h == onRestoreName {
                if !t.input.isEmpty { throw complain(.hookInput(h)) }
            } else {
                let ok = t.input.count == 1 && t.input[0].name == "device" && t.input[0].value.checks.isEmpty && t.input[0].value.ty == deviceTy()
                if !ok { throw complain(.hookInput(h)) }
            }
            if t.name != h { throw complain(.transitionName(t.name)) }
        }
        if let d = duplicates(t.input.map { $0.name }).first { throw complain(.duplicateName(d)) }
        var args = [String: Ty]()
        for i in t.input { args[i.name] = try plainTy(types, i.value.ty) }
        // A function of the state, its context and its input alone.
        var exprs = t.guards.map { $0.e }
        for i in t.input {
            for c in i.value.checks {
                switch c {
                case .exists: throw complain(.transitionReads("an exists check"))
                case .refine(let e, _): exprs.append(e)
                default: break
                }
            }
        }
        blockExprs(t.body, &exprs)
        for e in exprs {
            if let what = transitionForbidden(e) { throw complain(.transitionReads(what)) }
        }
        let g = G(m: m, named: NamedColumns(), index: m.functions.count, f: transitionShell(t.name, ty), kind: .provide, args: args, provided: [:], locals: [:], isPrivate: false, types: types, live: ty)
        for i in t.input {
            var inner = args[i.name]!
            if case .option(let o) = inner { inner = o }
            for c in i.value.checks {
                var fits: Bool
                switch c {
                case .trim, .minLen, .maxLen: fits = inner == .text
                case .range: fits = inner == .int
                case .nonEmpty:
                    switch inner {
                    case .list, .nonEmpty: fits = true
                    default: fits = false
                    }
                case .exists: fits = false
                case .refine(let e, _):
                    var gc = g
                    gc.args[i.name] = readAs(inner)
                    try expect(gc, "refine", .bool, e)
                    fits = true
                }
                if !fits { throw complain(.checkOnWrongType(i.name, c.description)) }
            }
        }
        for gd in t.guards { try expect(g, "guard", .bool, gd.e) }
        try block(g, t.body)
        if !returns(t.body) { throw complain(.mayNotReturn) }
        if Ordering.classify(markedTy, t) == .ordered, let f = Ordering.orderedReadsEphemeral(markedTy, t) {
            throw complain(.orderedReadsEphemeral(f))
        }
    }

    // Every expression a block holds, at any depth.
    private static func blockExprs(_ b: Block, _ out: inout [Expr]) {
        for s in b {
            switch s {
            case .letS(_, let e), .refuse(let e), .insert(_, let e, _), .upsert(_, let e, _): out.append(e)
            case .ret(let e): if let e = e { out.append(e) }
            case .ifS(let c, let a, let e):
                out.append(c)
                blockExprs(a, &out)
                blockExprs(e, &out)
            case .forS(_, let xs, let body):
                out.append(xs)
                blockExprs(body, &out)
            case .update(_, let ks, _, let e):
                out.append(contentsOf: ks)
                out.append(e)
            case .delete(_, let ks): out.append(contentsOf: ks)
            case .privateS(let body): blockExprs(body, &out)
            }
        }
    }

    // What a transition may not reach for, named: a read, an auto, a provided value.
    private static func transitionForbidden(_ e: Expr) -> String? {
        switch e {
        case .select, .get, .exists: return "a read"
        case .auto: return "an auto"
        case .provided: return "a provided value"
        default:
            for c in children(e) {
                if let w = transitionForbidden(c) { return w }
            }
            return nil
        }
    }

    // The first thing an init reaches that is not a constant, named.
    private static func constantOrNot(_ e: Expr) -> String? {
        switch e {
        case .arg: return "an argument"
        case .auto: return "an auto"
        case .variable: return "a local"
        case .provided: return "a provided value"
        case .ctxUser, .ctxSession, .hasRole, .ctxNow, .ctxDevice, .liveState: return "the context"
        case .select, .get, .exists: return "a read"
        case .call: return "a call"
        case .match, .map, .filter, .any, .all, .sortBy, .fold, .caseOf: return "a binder"
        default:
            for c in children(e) {
                if let w = constantOrNot(c) { return w }
            }
            return nil
        }
    }

    // A scope's expression: the context, literals, lists of them, and the
    // operators, comparisons and standard functions over them.
    private static func ctxOnlyScope(_ e: Expr) throws {
        switch e {
        case .listE(let es), .std(_, let es): for x in es { try ctxOnlyScope(x) }
        case .ife(let c, let a, let b):
            try ctxOnlyScope(c)
            try ctxOnlyScope(a)
            try ctxOnlyScope(b)
        default:
            do {
                try ctxOnly(e)
            } catch let c as Complain {
                throw complain(.badScope("\(c.c)"))
            }
        }
    }

    private static func stdTy(_ f: StdFn, _ ts: [Ty], _ want: Ty?) throws -> Ty {
        func option(_ t: Ty) throws -> Ty {
            if case .option = t { throw complain(.nestedOption) }
            return .option(t)
        }
        func misuse() -> Complain { complain(.stdMisuse("\(f.rawValue) applied to [\(ts.map { $0.description }.joined(separator: ", "))]")) }
        let a = ts.count > 0 ? ts[0] : nil
        let b = ts.count > 1 ? ts[1] : nil
        func arity(_ n: Int) throws { if ts.count != n { throw misuse() } }
        func isList(_ t: Ty?) -> Ty? {
            if case .list(let o)? = t { return o }
            return nil
        }
        func isOption(_ t: Ty?) -> Ty? {
            if case .option(let o)? = t { return o }
            return nil
        }
        switch f {
        case .Trim, .Lower:
            try arity(1)
            if a == .text { return .text }
        case .IsEmpty, .IsAlnum:
            try arity(1)
            if a == .text { return .bool }
        case .Concat:
            try arity(1)
            if isList(a) == .text { return .text }
        case .Chars:
            try arity(1)
            if a == .text { return .list(.text) }
        case .TextLen:
            try arity(1)
            if a == .text { return .int }
        case .StartsWith:
            try arity(2)
            if a == .text && b == .text { return .bool }
        case .SplitOnce:
            try arity(2)
            if a == .text && b == .text { return .option(.structOf([("before", .text), ("after", .text)])) }
        case .TextOfInt:
            try arity(1)
            if a == .int { return .text }
        case .Hex:
            try arity(1)
            if a == .bytes { return .text }
        case .Min, .Max:
            try arity(2)
            if a == .int && b == .int { return .int }
        case .Clamp:
            try arity(3)
            if ts.allSatisfy({ $0 == .int }) { return .int }
        case .Abs:
            try arity(1)
            if a == .int { return .int }
        case .Fnv1a64:
            try arity(1)
            if a == .text { return .int }
        case .Sha256:
            try arity(1)
            if a == .bytes { return .bytes }
        case .IdOfText:
            try arity(1)
            if a != .text { throw misuse() }
            if let o = isOption(want), case .id = o { return want! }
            throw complain(.needsAnnotation("id_of_text needs its table from context"))
        case .TextOfId:
            try arity(1)
            if case .id? = a { return .text }
        case .NilId:
            try arity(0)
            if case .id? = want { return want! }
            throw complain(.needsAnnotation("nil id needs its table from context"))
        case .Utf8:
            try arity(1)
            if a == .text { return .bytes }
        case .First, .Last:
            try arity(1)
            if let o = isList(a) { return try option(o) }
        case .Len:
            try arity(1)
            if isList(a) != nil { return .int }
        case .Contains:
            try arity(2)
            if let o = isList(a), o == b { return .bool }
        case .Reverse:
            try arity(1)
            if isList(a) != nil { return a! }
        case .IsSome:
            try arity(1)
            if isOption(a) != nil { return .bool }
        case .UnwrapOr:
            try arity(2)
            if let o = isOption(a), let bb = b, o == bb { return bb }
        case .Unwrap:
            try arity(1)
            if let o = isOption(a) { return o }
        }
        throw misuse()
    }

    /// §9.4 `complete_orders`: every order made total — the table's key
    /// columns ascending (a group's `by`), after whatever the author ordered
    /// by, omitting any already there — in every plan of every function,
    /// related plans included.
    public static func completeOrders(_ m: Module) -> Module {
        let sch = m.schema
        var out = m
        out.functions = m.functions.map { f in
            var g = f
            g.body = f.body.map { completeStmt(sch, $0) }
            g.plan = f.plan.map { completeOrder(sch, $0) }
            return g
        }
        return out
    }

    private static func completeStmt(_ sch: Schema, _ s: Stmt) -> Stmt {
        func ce(_ e: Expr) -> Expr { completeExpr(sch, e) }
        func cb(_ b: Block) -> Block { b.map { completeStmt(sch, $0) } }
        switch s {
        case .letS(let x, let e): return .letS(x, ce(e))
        case .insert(let t, let r, let on): return .insert(t, ce(r), on)
        case .upsert(let t, let r, let on): return .upsert(t, ce(r), on)
        case .refuse(let e): return .refuse(ce(e))
        case .ret(let e): return .ret(e.map(ce))
        case .update(let t, let k, let x, let r): return .update(t, k.map(ce), x, ce(r))
        case .ifS(let c, let a, let b): return .ifS(ce(c), cb(a), cb(b))
        case .forS(let x, let o, let b): return .forS(x, ce(o), cb(b))
        case .delete(let t, let k): return .delete(t, k.map(ce))
        case .privateS(let b): return .privateS(cb(b))
        }
    }

    private static func completeExpr(_ sch: Schema, _ e: Expr) -> Expr {
        if case .select(let p) = e { return .select(completeOrder(sch, p)) }
        return mapChildren(e) { completeExpr(sch, $0) }
    }

    /// `eval::complete_order`: a plan's order made total, in every related plan too.
    public static func completeOrder(_ sch: Schema, _ p: Plan) -> Plan {
        var keys: [String]
        switch p.source {
        case .table(let t): keys = sch.lookupTable(t)?.key ?? []
        case .group(_, let by): keys = by
        }
        var order = p.order
        for k in keys where !order.contains(where: { $0.key == .column(k) }) { order.append(OrderKey(.column(k), .asc)) }
        var q = p
        q.order = order
        q.related = p.related.map { r in
            var r2 = r
            r2.plan = completeOrder(sch, r.plan)
            return r2
        }
        return q
    }
}
