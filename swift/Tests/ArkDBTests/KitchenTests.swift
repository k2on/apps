import Foundation
import ArkDB
import ArkAuthoring
import Kitchen

// The rest of the vocabulary (Tests/Kitchen), Native against Eval over its
// own Emit, case by case.

func kitchenTests() throws {
    run("authoring/kitchen-agrees") {
        let km = kitchenModule()
        let m = try km.ir()
        var natives: [FnHash: Procedure] = [:]
        for (h, p) in km.procedures() { natives[h] = p }
        check("every helper precedes its first caller, and a helper's helper precedes it",
              m.functions.map { $0.name } == ["make", "double", "bump", "top", "total", "slug_of", "label_key", "stamp", "summarize", "summaries", "named"],
              "\(m.functions.map { $0.name })")
        check("the helper is in bump's closure", Hash.closure(m, m.lookupFunction("bump")!).helpers.map { $0.name } == ["double"])
        check("top's plan hangs the tags beneath", {
            guard case .sLet(_, .select(let p))? = m.lookupFunction("top")?.body.first else { return false }
            return p.related.map { $0.name } == ["tag"] && p.limit == 5 && p.related.first?.relation.column == "counter_id"
        }())
        let ctx = ArkDB.Ctx(user: "u", session: "s")
        let c1 = Value.id(rawId(1)), c2 = Value.id(rawId(2))
        let empty = MemoryStore(schema: m.schema)
        let one = try seeded(m, [("make", ctx, ["id": c1], ["name": .text("a"), "note": .text("Hi"), "labels": .list([.text("x"), .text("yy")])])])
        let two = try seeded(m, [
            ("make", ctx, ["id": c1], ["name": .text("a"), "note": .text("Hi"), "labels": .list([.text("x"), .text("yy")])]),
            ("make", ctx, ["id": c2], ["name": .text("b"), "note": .null, "labels": .list([.text("lucky")])]),
            ("bump", ctx, [:], ["counter": c1, "by": .int(4)]),
        ])
        func mk(_ label: String, _ args: Args, _ st: MemoryStore, id: Value = c2) -> Case {
            return Case(label: label, function: "make", ctx: ctx, autos: ["id": id], args: args, store: st)
        }
        func bump(_ label: String, _ c: Value, _ by: Int64, _ st: MemoryStore) -> Case {
            return Case(label: label, function: "bump", ctx: ctx, autos: [:], args: ["counter": c, "by": .int(by)], store: st)
        }
        let cases: [Case] = [
            mk("make: lands with its tags", ["name": .text(" c "), "note": .null, "labels": .list([.text("p"), .text("qq")])], empty),
            mk("make: no labels", ["name": .text("c"), "note": .null, "labels": .list([])], empty),
            mk("make: an empty label", ["name": .text("c"), "note": .null, "labels": .list([.text("")])], empty),
            mk("make: a long note", ["name": .text("c"), "note": .text("  much too long a note  "), "labels": .list([.text("p")])], empty),
            mk("make: the forbidden name", ["name": .text(" forbidden "), "note": .null, "labels": .list([.text("p")])], empty),
            mk("make: the same name again is a no-op, and its tags go on the first", ["name": .text("a"), "note": .null, "labels": .list([.text("z")])], one),
            bump("bump: no such counter", c2, 1, one),
            bump("bump: out of range", c1, 0, one),
            bump("bump: unlucky", c1, 13, one),
            bump("bump: too much", c1, 70, one),
            bump("bump: under the cap", c1, 3, one),
            bump("bump: over the cap", c1, 30, one),
            bump("bump: lucky seven", c2, 7, two),
            Case(label: "top", function: "top", ctx: ctx, autos: [:], args: [:], store: two),
            Case(label: "total", function: "total", ctx: ctx, autos: [:], args: [:], store: two),
            Case(label: "stamp: a new name", function: "stamp", ctx: ctx, autos: ["id": c2, "at": .int(1234)], args: ["name": .text("Big Mix")], store: one),
            Case(label: "stamp: a name that is there leaves its tag no counter", function: "stamp", ctx: ctx, autos: ["id": c2, "at": .int(1234)], args: ["name": .text("a")], store: one),
            Case(label: "summaries", function: "summaries", ctx: ctx, autos: [:], args: [:], store: two),
            Case(label: "named: there", function: "named", ctx: ctx, autos: [:], args: ["name": .text("a")], store: two),
            Case(label: "named: not there", function: "named", ctx: ctx, autos: [:], args: ["name": .text("zz")], store: one),
        ]
        check("every kitchen procedure has a case", Set(cases.map { $0.function }) == Set(m.functions.filter { $0.kind.isProcedure }.map { $0.name }))
        let bad = try agreement(m, natives, cases)
        check("Native agrees with Ark.Eval on \(cases.count) kitchen cases", bad.isEmpty, bad.joined(separator: "; "))
        // The cases mean what they say.
        let lucky = try Eval.apply(m, "bump", ctx, [:], ["counter": c2, "by": .int(7)], two)
        if case .applied(let st, _) = lucky {
            check("lucky seven deletes the tag", st.getRow("tag", [c2, .text("lucky")]) == nil && st.getRow("counter", [c2])?["n"] == .int(7))
        } else {
            check("lucky seven applies", false)
        }
        if case .applied(let st, _) = try Eval.apply(m, "bump", ctx, [:], ["counter": c1, "by": .int(30)], one) {
            check("the cap holds and the note is lowered", st.getRow("counter", [c1])?["n"] == .int(10) && st.getRow("counter", [c1])?["note"] == .text("hi")
                  && st.getRow("tag", [c1, .text("bumped")])?["weight"] == .int(60))
        } else {
            check("over the cap applies", false)
        }
        check("total folds", try Eval.query(m, "total", [:], two) == .int(4 + 1))
    }

    // What harken's domain needs of the vocabulary: helpers called with
    // named arguments, a record, equality on options, one auto named twice.
    run("authoring/kitchen-helpers-records-options-autos") {
        let m = try kitchenModule().ir()
        let fn = { (n: String) in m.lookupFunction(n)! }
        let summary: Ty = .structOf(["name": .text, "n": .int, "noted": .bool, "hi": .bool, "first": .option(.text)])
        check("a named helper is a Helper with its parameters, in order, and its return type",
              fn("label_key").kind == .helper && fn("label_key").input == [NamedField("name", .text), NamedField("label", .text)] && fn("label_key").ret == .text)
        check("its body is its value over its EArgs", {
            guard case .sReturn(.std(.concat, let args))? = fn("label_key").body.last, args.count == 1, case .list(let parts) = args[0] else { return false }
            return parts.count == 3 && parts[0] == .call("slug_of", [.arg("name")]) && parts[2] == .call("slug_of", [.arg("label")])
        }())
        check("a call is ECall with the arguments in order", {
            guard case .sUpsert(_, .structOf(let row), _)? = fn("stamp").body.last else { return false }
            return row["label"] == .call("label_key", [.arg("name"), .lit(.text(" Stamped Here "))])
        }(), "\(fn("stamp").body.last.map { "\($0)" } ?? "none")")
        check("a record is its TStruct", Summary.ty == summary && fn("summarize").ret == summary && fn("summaries").ret == .list(summary))
        check("building a record is EStruct", {
            guard case .sReturn(.structOf(let fs))? = fn("summarize").body.last else { return false }
            return Set(fs.keys) == ["name", "n", "noted", "hi", "first"] && fs["name"] == .field(.arg("counter"), "name")
        }())
        check("equality on options is ECmp, both ways", {
            guard case .sReturn(.structOf(let fs))? = fn("summarize").body.last else { return false }
            guard case .op(.and, let conj)? = fs["noted"], conj.count == 2 else { return false }
            return conj[0] == .cmp(.ne, .field(.arg("counter"), "note"), .none(.text))
                && conj[1] == .cmp(.ne, .field(.arg("counter"), "note"), .some(.lit(.text("x"))))
                && fs["hi"] == .cmp(.eq, .field(.arg("counter"), "note"), .some(.lit(.text("hi"))))
        }(), "\(fn("summarize").body)")
        check("one name drawn twice is one auto", fn("stamp").autos == [NamedAuto("id", .newId("counter")), NamedAuto("at", .now)], "\(fn("stamp").autos)")
        check("summaries' closure carries every helper it reaches",
              Set(Hash.closure(m, fn("summaries")).helpers.map { $0.name }) == ["summarize", "label_key", "slug_of"])
        let ctx = ArkDB.Ctx(user: "u", session: "s")
        let c1 = Value.id(rawId(1)), c2 = Value.id(rawId(2))
        let st = try seeded(m, [
            ("make", ctx, ["id": c1], ["name": .text("a"), "note": .text("hi"), "labels": .list([.text("x")])]),
            ("stamp", ctx, ["id": c2, "at": .int(99)], ["name": .text("Big Mix")]),
        ])
        check("the frozen time is read twice", st.getRow("counter", [c2])?["n"] == .int(99)
              && st.getRow("tag", [c2, .text("big-mix/stamped-here")])?["weight"] == .int(99))
        let rows = try Eval.query(m, "summaries", [:], st)
        check("a record comes back as a struct of its fields", rows == .list([
            .record(["name": .text("Big Mix"), "n": .int(99), "noted": .bool(false), "hi": .bool(false), "first": .text("big-mix/big-mix/stamped-here")]),
            .record(["name": .text("a"), "n": .int(0), "noted": .bool(true), "hi": .bool(true), "first": .text("a/x")]),
        ]), "\(rows)")
        check("evaluate runs a helper natively, outside any procedure", try evaluate { labelKey(" My Mix ", "B C") } == .success(.text("my-mix/b-c")))
        check("and evaluate reports a refusal", try evaluate { Int(Int64.max).add(1) } == .failure(.refused("integer overflow")))
    }
}
