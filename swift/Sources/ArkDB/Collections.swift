// The ordered collections the engine is written over, and a 128-bit integer.
//
// The Rust keeps its rows and indexes in `BTreeMap`s and the Kotlin in
// `TreeMap`s; Swift's standard library has no sorted map, and a `Dictionary`
// iterates in an order seeded per process — so anything whose order can be
// seen is kept here, in a sorted array searched by halves, or in insertion
// order (`OrderedDict`, where the reference keeps a `Vec` of pairs or Kotlin a
// `LinkedHashMap`). A sorted array is a sorted map with an O(n) insert, which
// at the sizes a phone's tables and views have is a memmove; it copies on
// write, so a store's copy costs nothing until it is written.

/// A map whose keys are kept in their `Comparable` order.
public struct SortedMap<K: Comparable, V> {
    public private(set) var keys: [K] = []
    public private(set) var values: [V] = []

    public init() {}

    public var count: Int { keys.count }
    public var isEmpty: Bool { keys.isEmpty }

    /// The first index whose key is not below `k`.
    @inline(__always)
    public func lowerBound(_ k: K) -> Int {
        var lo = 0
        var hi = keys.count
        while lo < hi {
            let mid = (lo + hi) >> 1
            if keys[mid] < k { lo = mid + 1 } else { hi = mid }
        }
        return lo
    }

    /// The first index whose key is above `k`.
    @inline(__always)
    public func upperBound(_ k: K) -> Int {
        var lo = 0
        var hi = keys.count
        while lo < hi {
            let mid = (lo + hi) >> 1
            if k < keys[mid] { hi = mid } else { lo = mid + 1 }
        }
        return lo
    }

    @inline(__always)
    public func index(of k: K) -> Int? {
        let i = lowerBound(k)
        return i < keys.count && keys[i] == k ? i : nil
    }

    public subscript(k: K) -> V? {
        get {
            guard let i = index(of: k) else { return nil }
            return values[i]
        }
        set {
            let i = lowerBound(k)
            if i < keys.count && keys[i] == k {
                if let v = newValue { values[i] = v } else {
                    keys.remove(at: i)
                    values.remove(at: i)
                }
            } else if let v = newValue {
                keys.insert(k, at: i)
                values.insert(v, at: i)
            }
        }
    }

    public func contains(_ k: K) -> Bool { index(of: k) != nil }

    /// Put `v` under `k`, answering what was there.
    @discardableResult
    public mutating func put(_ k: K, _ v: V) -> V? {
        let i = lowerBound(k)
        if i < keys.count && keys[i] == k {
            let old = values[i]
            values[i] = v
            return old
        }
        keys.insert(k, at: i)
        values.insert(v, at: i)
        return nil
    }

    @discardableResult
    public mutating func remove(_ k: K) -> V? {
        guard let i = index(of: k) else { return nil }
        keys.remove(at: i)
        return values.remove(at: i)
    }

    /// The value under `k`, made with `make` where there is none, changed in place.
    public mutating func modify<R>(_ k: K, default make: () -> V, _ f: (inout V) throws -> R) rethrows -> R {
        var i = lowerBound(k)
        if !(i < keys.count && keys[i] == k) {
            keys.insert(k, at: i)
            values.insert(make(), at: i)
            i = lowerBound(k)
        }
        return try f(&values[i])
    }

    /// The value under `k`, changed in place where there is one.
    @discardableResult
    public mutating func modifyIfPresent<R>(_ k: K, _ f: (inout V) throws -> R) rethrows -> R? {
        guard let i = index(of: k) else { return nil }
        return try f(&values[i])
    }

    public mutating func removeAll() {
        keys.removeAll()
        values.removeAll()
    }

    /// The entry at a position, in key order.
    public func at(_ i: Int) -> (key: K, value: V) { (keys[i], values[i]) }

    public var first: (key: K, value: V)? { keys.isEmpty ? nil : (keys[0], values[0]) }
    public var last: (key: K, value: V)? { keys.isEmpty ? nil : (keys[keys.count - 1], values[values.count - 1]) }

    /// The greatest entry, taken out (`TreeMap.pollLastEntry`).
    public mutating func popLast() -> (key: K, value: V)? {
        guard !keys.isEmpty else { return nil }
        return (keys.removeLast(), values.removeLast())
    }

    /// Every entry in key order.
    public var entries: [(key: K, value: V)] { (0..<keys.count).map { (keys[$0], values[$0]) } }
}

extension SortedMap: Equatable where V: Equatable {
    public static func == (a: SortedMap, b: SortedMap) -> Bool { a.keys == b.keys && a.values == b.values }
}

extension SortedMap: Sequence {
    public func makeIterator() -> AnyIterator<(key: K, value: V)> {
        var i = 0
        let ks = keys
        let vs = values
        return AnyIterator {
            guard i < ks.count else { return nil }
            defer { i += 1 }
            return (ks[i], vs[i])
        }
    }
}

/// A set kept in its elements' `Comparable` order.
public struct SortedSet<K: Comparable>: Equatable {
    public private(set) var items: [K] = []

    public init() {}
    public init<S: Sequence>(_ xs: S) where S.Element == K {
        for x in xs { insert(x) }
    }

    public var count: Int { items.count }
    public var isEmpty: Bool { items.isEmpty }

    @inline(__always)
    func lowerBound(_ k: K) -> Int {
        var lo = 0
        var hi = items.count
        while lo < hi {
            let mid = (lo + hi) >> 1
            if items[mid] < k { lo = mid + 1 } else { hi = mid }
        }
        return lo
    }

    public func contains(_ k: K) -> Bool {
        let i = lowerBound(k)
        return i < items.count && items[i] == k
    }

    /// Whether it was not there.
    @discardableResult
    public mutating func insert(_ k: K) -> Bool {
        let i = lowerBound(k)
        if i < items.count && items[i] == k { return false }
        items.insert(k, at: i)
        return true
    }

    /// Whether it was there.
    @discardableResult
    public mutating func remove(_ k: K) -> Bool {
        let i = lowerBound(k)
        guard i < items.count && items[i] == k else { return false }
        items.remove(at: i)
        return true
    }

    public mutating func formUnion<S: Sequence>(_ xs: S) where S.Element == K { for x in xs { insert(x) } }
    public mutating func subtract<S: Sequence>(_ xs: S) where S.Element == K { for x in xs { remove(x) } }

    public var first: K? { items.first }
    public var last: K? { items.last }
}

extension SortedSet: Sequence {
    public func makeIterator() -> IndexingIterator<[K]> { items.makeIterator() }
}

/// A map that iterates in the order its keys were first put (the reference's
/// `Vec` of pairs, Kotlin's `LinkedHashMap`).
public struct OrderedDict<K: Hashable, V> {
    public private(set) var keys: [K] = []
    var index: [K: Int] = [:]
    public private(set) var values: [V] = []

    public init() {}

    public var count: Int { keys.count }
    public var isEmpty: Bool { keys.isEmpty }

    public subscript(k: K) -> V? {
        get { index[k].map { values[$0] } }
        set {
            if let i = index[k] {
                if let v = newValue { values[i] = v } else { remove(k) }
            } else if let v = newValue {
                index[k] = keys.count
                keys.append(k)
                values.append(v)
            }
        }
    }

    public func contains(_ k: K) -> Bool { index[k] != nil }

    @discardableResult
    public mutating func remove(_ k: K) -> V? {
        guard let i = index[k] else { return nil }
        keys.remove(at: i)
        let v = values.remove(at: i)
        index = [:]
        for (j, key) in keys.enumerated() { index[key] = j }
        return v
    }

    public mutating func modify<R>(_ k: K, default make: () -> V, _ f: (inout V) throws -> R) rethrows -> R {
        if index[k] == nil { self[k] = make() }
        return try f(&values[index[k]!])
    }

    public var entries: [(key: K, value: V)] { (0..<keys.count).map { (keys[$0], values[$0]) } }
}

extension OrderedDict: Equatable where V: Equatable {
    public static func == (a: OrderedDict, b: OrderedDict) -> Bool { a.keys == b.keys && a.values == b.values }
}

/// A signed 128-bit integer, two's complement (Rust's `i128`), with what the
/// engine asks of one: a sum of 64-bit terms, kept exactly (§13 and R9's
/// kept counts and sums), and whether it fits back in 64 bits. Swift 5.8 has
/// no `Int128`.
public struct Wide: Hashable, Comparable, CustomStringConvertible {
    public var hi: Int64
    public var lo: UInt64

    public init(hi: Int64, lo: UInt64) {
        self.hi = hi
        self.lo = lo
    }

    public init(_ x: Int64) {
        hi = x < 0 ? -1 : 0
        lo = UInt64(bitPattern: x)
    }

    public static let zero = Wide(0)
    public static let one = Wide(1)

    public static func + (a: Wide, b: Wide) -> Wide {
        let (lo, carry) = a.lo.addingReportingOverflow(b.lo)
        return Wide(hi: a.hi &+ b.hi &+ (carry ? 1 : 0), lo: lo)
    }

    public static func - (a: Wide, b: Wide) -> Wide {
        let (lo, borrow) = a.lo.subtractingReportingOverflow(b.lo)
        return Wide(hi: a.hi &- b.hi &- (borrow ? 1 : 0), lo: lo)
    }

    public static func < (a: Wide, b: Wide) -> Bool { a.hi != b.hi ? a.hi < b.hi : a.lo < b.lo }

    /// Whether it is an `Int64`.
    public var fits: Bool { hi == (Int64(bitPattern: lo) < 0 ? -1 : 0) }

    /// The `Int64` nearest it: itself when it fits, a bound otherwise.
    public var clamped: Int64 {
        if fits { return Int64(bitPattern: lo) }
        return hi < 0 ? Int64.min : Int64.max
    }

    public var description: String {
        if fits { return String(Int64(bitPattern: lo)) }
        return "Wide(\(hi), \(lo))"
    }
}
