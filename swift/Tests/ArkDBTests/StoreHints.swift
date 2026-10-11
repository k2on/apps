// A store's hints held to the definition (`rust/ark/src/store.rs`, "an
// indexed read is the scan filtered"): `scanWhereEq`, `scanWhereText` and
// `scanOrdered` are told which columns a filter holds equal, between
// bounds, or to contain a needle, and read through the store's indexes —
// and must answer exactly what scanning every row, keeping what the filter
// admits and sorting, answers. Over a `MemoryStore` and over an `Overlay`
// on one, through seeded random rows and random reads.
import ArkDB

enum StoreHints {
    static let parent = Table("p", [Column("id", .int, false)], ["id"], [], [])
    static let item = Table(
        "t",
        [Column("id", .int, false), Column("a", .int, false), Column("c", .int, true), Column("b", .text, false), Column("p", .int, true)],
        ["id"],
        [Index(["a", "c"], false), Index(["c"], false)],
        [Ref("p", "p")],
        ["b"])
    static let schema = Schema([parent, item])

    static var rng: UInt64 = 0x9E37_79B9_7F4A_7C15

    static func next(_ n: Int) -> Int {
        rng = rng &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
        return Int((rng >> 33) % UInt64(n))
    }

    static let words = ["Água", "agua", "Bach", "BACH partita", "water music", "Wassermusik", "ab", "", "aguas"]

    static func row(_ id: Int) -> Row {
        let a = Value.int(Int64(next(4)))
        let c: Value = next(4) == 0 ? .null : .int(Int64(next(5)))
        let b = Value.text(words[next(words.count)])
        let p: Value = next(3) == 0 ? .null : .int(Int64(next(3)))
        return Record([("id", .int(Int64(id))), ("a", a), ("c", c), ("b", b), ("p", p)])
    }

    static func write(_ st: Store) {
        let id = next(40)
        let old = st.get("t", [.int(Int64(id))])
        if let o = old, next(4) == 0 { st.applyChange(.remove("t", o)) } else { st.applyChange(.add("t", row(id))) }
    }

    static func intOrNull() -> Value { next(6) == 0 ? .null : .int(Int64(next(5) - 1)) }

    static func bound() -> Bound {
        switch next(3) {
        case 0: return .unbounded
        case 1: return .included(intOrNull())
        default: return .excluded(intOrNull())
        }
    }

    static func inSpan(_ s: Span, _ v: Value) -> Bool {
        var okLo = true
        switch s.lo {
        case .unbounded: break
        case .included(let x): okLo = cmp(.ge, v, x)
        case .excluded(let x): okLo = cmp(.gt, v, x)
        }
        var okHi = true
        switch s.hi {
        case .unbounded: break
        case .included(let x): okHi = cmp(.le, v, x)
        case .excluded(let x): okHi = cmp(.lt, v, x)
        }
        return okLo && okHi
    }

    static func check(_ st: Store, _ what: String) throws {
        let all = st.scan("t")
        let cols = ["id", "a", "c", "p"]
        let eq: [Eq] = cols.filter { _ in next(3) == 0 }.map { ($0, intOrNull()) }
        var spans = [Span]()
        if next(2) == 0 {
            let c = cols[next(cols.count)]
            let lo = bound()
            spans = [Span(column: c, lo: lo, hi: bound())]
        }
        var needle: String? = nil
        if next(3) == 0 {
            let w = words[next(words.count)]
            needle = fold(String(w.unicodeScalars.prefix(1 + next(5)).map(Character.init)))
        }
        let keep: Keep = { r in
            eq.allSatisfy { cmp(.eq, r[$0.column] ?? .null, $0.value) } && spans.allSatisfy { inSpan($0, r[$0.column] ?? .null) }
                && (needle == nil || (r["b"]?.textValue.map { textContains(fold($0), needle!) } ?? false))
        }
        let want = all.filter(keep)
        let got = needle != nil ? st.scanWhereText("t", eq, spans, [[("b", needle!)]], keep) : st.scanWhereEq("t", eq, spans, keep)
        if got != want { throw Failed("\(what): a read through the hints with eq \(eq), spans \(spans), needle \(needle ?? "none") answered \(got.count) rows, the scan \(want.count)") }
        let orderCols = ["a", "c", "id"]
        var order = [(String, Dir)]()
        for _ in 0...next(2) {
            let c = orderCols[next(orderCols.count)]
            let d: Dir = next(2) == 0 ? .asc : .desc
            if !order.contains(where: { $0.0 == c }) { order.append((c, d)) }
        }
        let limit = next(6)
        if let ordered = st.scanOrdered("t", eq, spans, order, keep, limit) {
            let sorted = Array(want.sorted { Rules.compareRows(item, order, $0, $1) < 0 }.prefix(limit))
            if ordered != sorted { throw Failed("\(what): an ordered read by \(order), eq \(eq), spans \(spans), limit \(limit) answered \(ordered.count) rows, the sort \(sorted.count)") }
        }
    }

    /// 600 reads, each the scan filtered and sorted, or the first that is not.
    static func run() throws -> Int {
        let st = MemoryStore.empty(schema)
        for k in 0...2 { st.applyChange(.add("p", Record([("id", .int(Int64(k)))]))) }
        var reads = 0
        for round in 0..<300 {
            write(st)
            try check(st, "store, round \(round)")
            let overlay = Overlay(st)
            for _ in 0..<next(4) { write(overlay) }
            try check(overlay, "overlay, round \(round)")
            reads += 2
        }
        return reads
    }
}
