import Foundation

/// The operators generated code calls (`Ark.Eval` §6.4, §6.5): checked
/// arithmetic with the spec's fault texts, comparison under the total order,
/// and the list forms with faulting closures.
public enum Ops {
    // MARK: arithmetic

    static func ints(_ a: Value, _ b: Value) -> (Int64, Int64) {
        return (a.asInt(), b.asInt())
    }

    public static func add(_ a: Value, _ b: Value) throws -> Value {
        let (x, y) = ints(a, b)
        let (r, o) = x.addingReportingOverflow(y)
        if o { throw Fault.refuse("integer overflow") }
        return .int(r)
    }

    public static func sub(_ a: Value, _ b: Value) throws -> Value {
        let (x, y) = ints(a, b)
        let (r, o) = x.subtractingReportingOverflow(y)
        if o { throw Fault.refuse("integer overflow") }
        return .int(r)
    }

    public static func mul(_ a: Value, _ b: Value) throws -> Value {
        let (x, y) = ints(a, b)
        let (r, o) = x.multipliedReportingOverflow(by: y)
        if o { throw Fault.refuse("integer overflow") }
        return .int(r)
    }

    /// Truncating toward zero; `Int64.min / -1` is an overflow.
    public static func div(_ a: Value, _ b: Value) throws -> Value {
        let (x, y) = ints(a, b)
        if y == 0 { throw Fault.refuse("division by zero") }
        if x == Int64.min && y == -1 { throw Fault.refuse("integer overflow") }
        return .int(x / y)
    }

    /// The remainder with the dividend's sign; `Int64.min % -1` is an overflow.
    public static func mod(_ a: Value, _ b: Value) throws -> Value {
        let (x, y) = ints(a, b)
        if y == 0 { throw Fault.refuse("division by zero") }
        if x == Int64.min && y == -1 { throw Fault.refuse("integer overflow") }
        return .int(x % y)
    }

    public static func neg(_ a: Value) throws -> Value {
        let n = a.asInt()
        if n == Int64.min { throw Fault.refuse("integer overflow") }
        return .int(-n)
    }

    /// The same arithmetic for the interpreter, by operator.
    static func arith(_ op: Op, _ a: Value, _ b: Value) throws -> Value {
        switch op {
        case .add: return try add(a, b)
        case .sub: return try sub(a, b)
        case .mul: return try mul(a, b)
        case .div: return try div(a, b)
        case .mod: return try mod(a, b)
        default: throw Fault.bug("not an arithmetic operator: \(op.rawValue)")
        }
    }

    // MARK: comparison and logic

    /// Comparison is the total order, so `NULL = NULL` is true.
    public static func compare(_ op: CmpOp, _ a: Value, _ b: Value) -> Bool {
        let o = compareValue(a, b)
        switch op {
        case .eq: return o == 0
        case .ne: return o != 0
        case .lt: return o < 0
        case .le: return o <= 0
        case .gt: return o > 0
        case .ge: return o >= 0
        }
    }

    public static func cmp(_ op: CmpOp, _ a: Value, _ b: Value) -> Value {
        return .bool(compare(op, a, b))
    }

    public static func not(_ a: Value) -> Value {
        return .bool(!a.asBool())
    }

    /// An argument (or auto) by name; a bug if missing.
    public static func arg(_ args: Args, _ name: String) -> Value {
        guard let v = args[name] else { fatalError("Ops.arg: missing argument \(name)") }
        return v
    }

    // MARK: options and lists

    public static func match(_ v: Value, _ some: (Value) throws -> Value, _ none: () throws -> Value) throws -> Value {
        if v.isNull() { return try none() }
        return try some(v)
    }

    public static func map(_ xs: Value, _ f: (Value) throws -> Value) throws -> Value {
        var out: [Value] = []
        for x in xs.asList() { out.append(try f(x)) }
        return .list(out)
    }

    public static func filter(_ xs: Value, _ f: (Value) throws -> Value) throws -> Value {
        var out: [Value] = []
        for x in xs.asList() { if try f(x).asBool() { out.append(x) } }
        return .list(out)
    }

    /// Every element is evaluated, left to right, as `Ark.Eval` does.
    public static func any(_ xs: Value, _ f: (Value) throws -> Value) throws -> Value {
        var r = false
        for x in xs.asList() { if try f(x).asBool() { r = true } }
        return .bool(r)
    }

    public static func all(_ xs: Value, _ f: (Value) throws -> Value) throws -> Value {
        var r = true
        for x in xs.asList() { if !(try f(x).asBool()) { r = false } }
        return .bool(r)
    }

    /// Stable, under the total order of the key.
    public static func sortBy(_ xs: Value, _ key: (Value) throws -> Value) throws -> Value {
        var keyed: [(Value, Value)] = []
        for x in xs.asList() { keyed.append((x, try key(x))) }
        return .list(stableSort(keyed) { compareValue($0.1, $1.1) }.map { $0.0 })
    }

    public static func fold(_ xs: Value, _ initial: Value, _ f: (Value, Value) throws -> Value) throws -> Value {
        var acc = initial
        for x in xs.asList() { acc = try f(acc, x) }
        return acc
    }
}

/// A stable sort by a three-way comparison: a merge sort, since the
/// standard library does not promise stability.
func stableSort<T>(_ xs: [T], _ cmp: (T, T) -> Int) -> [T] {
    if xs.count <= 1 { return xs }
    let mid = xs.count / 2
    let a = stableSort(Array(xs[0..<mid]), cmp)
    let b = stableSort(Array(xs[mid...]), cmp)
    var out: [T] = []
    out.reserveCapacity(xs.count)
    var i = 0, j = 0
    while i < a.count && j < b.count {
        if cmp(b[j], a[i]) < 0 { out.append(b[j]); j += 1 } else { out.append(a[i]); i += 1 }
    }
    while i < a.count { out.append(a[i]); i += 1 }
    while j < b.count { out.append(b[j]); j += 1 }
    return out
}
