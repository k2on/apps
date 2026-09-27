import Foundation

/// The version of this specification a module was written against.
public typealias SpecVersion = Int
public let specVersion: SpecVersion = 1

/// A local variable, alpha-normalised: the `n`th binding in a function.
public typealias Sym = Int
public typealias Block = [Stmt]

public struct Module: Equatable {
    public var spec: SpecVersion
    public var schema: Schema
    /// In declaration order; a helper may be called only by functions after it.
    public var functions: [Function]
    /// §3.9 The live section: frame types by name.
    public var live: [LiveFrame]
    public init(spec: SpecVersion, schema: Schema, functions: [Function], live: [LiveFrame]) {
        self.spec = spec; self.schema = schema; self.functions = functions; self.live = live
    }

    public func lookupFunction(_ n: String) -> Function? {
        return functions.first { $0.name == n }
    }
}

public struct LiveFrame: Equatable {
    public var name: String
    public var ty: Ty
    public init(_ name: String, _ ty: Ty) { self.name = name; self.ty = ty }
}

public enum FnKind: Equatable {
    case mutator
    case query
    case helper
}

/// The non-determinism a mutator is allowed, by type.
public enum Auto: Equatable {
    case newId(TableName)
    case now
}

public struct NamedAuto: Equatable {
    public var name: String
    public var auto: Auto
    public init(_ name: String, _ auto: Auto) { self.name = name; self.auto = auto }
}

public struct NamedArg: Equatable {
    public var name: String
    public var ty: Ty
    public init(_ name: String, _ ty: Ty) { self.name = name; self.ty = ty }
}

public struct Function: Equatable {
    public var name: String
    public var kind: FnKind
    /// The scope a mutator belongs to; nil for queries and helpers.
    public var scope: ScopeName?
    public var autos: [NamedAuto]
    public var args: [NamedArg]
    /// The result type of a query or helper; nil for a mutator.
    public var ret: Ty?
    public var body: Block
    /// The author's names for symbols; not hashed, not required.
    public var names: [Sym: String]
    public init(name: String, kind: FnKind, scope: ScopeName?, autos: [NamedAuto], args: [NamedArg], ret: Ty?, body: Block, names: [Sym: String] = [:]) {
        self.name = name; self.kind = kind; self.scope = scope; self.autos = autos
        self.args = args; self.ret = ret; self.body = body; self.names = names
    }
}

/// §3.1 Statements. Named after `Ark.IR`'s constructors, since `let`,
/// `if`, `for` and `return` are keywords.
public indirect enum Stmt: Equatable {
    case sLet(Sym, Expr)
    case sIf(Expr, Block, Block)
    case sFor(Sym, Expr, Block)
    case sPut(TableName, Expr)
    case sDelete(TableName, [Expr])
    case sRefuse(Expr)
    case sReturn(Expr?)
}

/// §3.2 Expressions.
public indirect enum Expr: Equatable {
    case lit(Value)
    case arg(String)
    case auto(String)
    case variable(Sym)
    case ctxUser
    case ctxSession
    case field(Expr, FieldName)
    case structOf([FieldName: Expr])
    case list([Expr])
    case some(Expr)
    case none(Ty)
    /// `match e { Some x -> a; None -> b }`
    case match(Expr, Sym, Expr, Expr)
    case ife(Expr, Expr, Expr)
    case op(Op, [Expr])
    case cmp(CmpOp, Expr, Expr)
    case call(String, [Expr])
    case std(StdFn, [Expr])
    case map(Expr, Sym, Expr)
    case filter(Expr, Sym, Expr)
    case any(Expr, Sym, Expr)
    case all(Expr, Sym, Expr)
    case sortBy(Expr, Sym, Expr)
    /// `fold xs init (acc, x -> body)`
    case fold(Expr, Expr, Sym, Sym, Expr)
    case select(Plan)
    case get(TableName, [Expr])
    case exists(TableName, [Expr])
}

public enum Op: String, Equatable {
    case add, sub, mul, div, mod, neg, and, or, not
}

/// Comparison under the total order.
public enum CmpOp: String, Equatable {
    case eq, ne, lt, le, gt, ge
}

public struct OrderBy: Equatable {
    public var column: FieldName
    public var dir: Dir
    public init(_ column: FieldName, _ dir: Dir) { self.column = column; self.dir = dir }
}

/// §3.3 A plan: what `select` pulls and what a view maintains.
public struct Plan: Equatable {
    public var table: TableName
    public var filter: Pred?
    public var order: [OrderBy]
    public var limit: Int?
    public var related: [Related]
    public init(table: TableName, filter: Pred? = nil, order: [OrderBy] = [], limit: Int? = nil, related: [Related] = []) {
        self.table = table; self.filter = filter; self.order = order; self.limit = limit; self.related = related
    }
}

public struct Related: Equatable {
    public var name: FieldName
    public var relation: Relation
    public var plan: Plan
    public init(name: FieldName, relation: Relation, plan: Plan) {
        self.name = name; self.relation = relation; self.plan = plan
    }
}

/// A filter over one row; the right-hand sides may not mention the row.
/// Cases are named as the wire spells them.
public indirect enum Pred: Equatable {
    case pcmp(FieldName, CmpOp, Expr)
    case pin(FieldName, [Expr])
    case pall([Pred])
    case pany([Pred])
    case pnot(Pred)
}

/// §3.4 The standard library, by name. The raw value is the Haskell
/// constructor's spelling, which is how the wire names one.
public enum StdFn: String, CaseIterable, Equatable {
    case trim = "Trim"
    case isEmpty = "IsEmpty"
    case concat = "Concat"
    case lower = "Lower"
    case isAlnum = "IsAlnum"
    case chars = "Chars"
    case textLen = "TextLen"
    case startsWith = "StartsWith"
    case splitOnce = "SplitOnce"
    case textOfInt = "TextOfInt"
    case hex = "Hex"
    case min = "Min"
    case max = "Max"
    case clamp = "Clamp"
    case abs = "Abs"
    case fnv1a64 = "Fnv1a64"
    case sha256 = "Sha256"
    case idOfText = "IdOfText"
    case textOfId = "TextOfId"
    case nilId = "NilId"
    case utf8 = "Utf8"
    case first = "First"
    case last = "Last"
    case len = "Len"
    case contains = "Contains"
    case reverse = "Reverse"
    case isSome = "IsSome"
    case unwrapOr = "UnwrapOr"

    /// How many arguments each function takes.
    public var arity: Int {
        switch self {
        case .startsWith, .splitOnce, .min, .max, .contains, .unwrapOr: return 2
        case .clamp: return 3
        case .nilId: return 0
        default: return 1
        }
    }
}

// MARK: - Builders, as GENERATED.md spells them

extension Plan {
    public static func from(_ table: TableName) -> Plan { return Plan(table: table) }

    public func filter(_ p: Pred) -> Plan {
        var q = self; q.filter = p; return q
    }

    public func orderBy(_ column: FieldName, _ dir: Dir) -> Plan {
        var q = self; q.order.append(OrderBy(column, dir)); return q
    }

    public func limit(_ n: Int) -> Plan {
        var q = self; q.limit = n; return q
    }

    public func related(_ name: FieldName, _ parent: TableName, _ child: TableName, _ column: FieldName, _ childPlan: Plan) -> Plan {
        var q = self
        q.related.append(Related(name: name, relation: Relation(parent: parent, child: child, column: column), plan: childPlan))
        return q
    }
}

extension Pred {
    /// Right-hand sides are values, already evaluated.
    public static func cmp(_ column: FieldName, _ op: CmpOp, _ v: Value) -> Pred { return .pcmp(column, op, .lit(v)) }
    public static func inList(_ column: FieldName, _ vs: [Value]) -> Pred { return .pin(column, vs.map { .lit($0) }) }
    public static func all(_ ps: [Pred]) -> Pred { return .pall(ps) }
    public static func any(_ ps: [Pred]) -> Pred { return .pany(ps) }
    public static func not(_ p: Pred) -> Pred { return .pnot(p) }
}
