// R9's kept numbers held to what a fresh read answers, beyond the views
// vectors: those exercise a group's members kept as a count and a sum and
// a related list kept as a count or a maximum, one level down, and nothing
// deeper. This runs a plan whose kept lists have kept lists of their own —
// a composer's works counted and their songs summed, each work's songs
// summed over its movements, each movement's songs counted — over the
// schema and store of views/related-three-deep.json, through seeded random
// batches of changes to every table it reads, and after each batch holds
// the view to its contract (a fresh hydrate), to the answer `View.read`
// gives by folding the lists, and to the splice of its patches.
import ArkDB

enum KeptNumbers {
    static func v(_ s: Sym) -> Expr { .variable(s) }
    static func lit(_ n: Int64) -> Expr { .lit(.int(n)) }
    static func field(_ e: Expr, _ n: String) -> Expr { .field(e, n) }
    static func len(_ s: Sym) -> Expr { .std(.Len, [v(s)]) }

    // `fold(list, 0, |acc, x| acc + x.f)`: a sum the view keeps.
    static func sum(_ list: Sym, _ acc: Sym, _ x: Sym, _ f: String) -> Expr {
        .fold(v(list), lit(0), acc, x, .op(.add, [v(acc), field(v(x), f)]))
    }

    static func by(_ c: String) -> [OrderKey] { [OrderKey(.column(c), .asc)] }

    /// person → work → movement → song, every list beneath the person kept as numbers.
    static func plan() -> Plan {
        var song = Plan.from("song")
        song.order = by("id")
        var movement = Plan.from("movement")
        movement.row = 2
        movement.order = by("id")
        movement.related = [Related(name: "song", sym: 3, on: [On(column: "movement_id", e: .some(field(v(2), "id")))], plan: song)]
        movement.project = .structE(ExprFields([("no", field(v(2), "no")), ("n", len(3))]))
        var work = Plan.from("work")
        work.row = 1
        work.order = by("id")
        work.related = [Related(name: "movement", sym: 4, on: [On(column: "work_id", e: field(v(1), "id"))], plan: movement)]
        work.project = .structE(ExprFields([("songs", sum(4, 20, 21, "n")), ("movements", len(4))]))
        work.having = .cmp(.ge, len(4), lit(0))
        var person = Plan.from("person")
        person.row = 0
        person.order = by("name")
        person.related = [Related(name: "work", sym: 5, on: [On(column: "composer", e: field(v(0), "name"))], plan: work)]
        person.project = .structE(ExprFields([("name", field(v(0), "name")), ("works", len(5)), ("songs", sum(5, 22, 23, "songs")), ("movements", sum(5, 24, 25, "movements"))]))
        return person
    }

    static func run(_ root: String) throws -> (Int, Int) {
        let vec = try readJson(root + "/views/related-three-deep.json")
        let sch = try Checks.moduleOf(try vec.field("module")).schema
        let plan = plan()
        let shape = Views.shape(sch, plan, [])
        if shape.kept.count != 3 { throw Failed("the plan keeps \(shape.kept.count) lists as numbers, not 3") }
        var rng: UInt64 = 0x2545_F491_4F6C_DD1D
        func next(_ n: Int) -> Int {
            rng = rng &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
            return Int((rng >> 33) % UInt64(n))
        }
        let persons = ["bach", "handel", "vivaldi", "anon"]
        let works = (1...6).map { "w\($0)" }
        let movements = (1...10).map { "m\($0)" }
        let songs = (1...14).map { "s\($0)" }
        func t(_ s: String) -> Value { .text(s) }
        let st = MemoryStore.fromValue(sch, try vec.field("store_before"))
        let env = ViewEnv([], Ctx("alice", "session-1"), [:], [:])
        let view = try Views.hydrate(sch, plan, env, st)
        var patches = 0
        let steps = 400
        for step in 0..<steps {
            var batch = [Change]()
            for _ in 0..<(1 + next(3)) {
                var change: Change
                switch next(4) {
                case 0:
                    let k = persons[next(persons.count)]
                    let old = st.get("person", [t(k)])
                    if let o = old, next(3) == 0 {
                        change = .remove("person", o)
                    } else {
                        change = .add("person", Record([("name", t(k)), ("born", .int(Int64(next(100))))]))
                    }
                case 1:
                    let k = works[next(works.count)]
                    let old = st.get("work", [t(k)])
                    let composer = t(persons[next(persons.count)])
                    let new = Record([("id", t(k)), ("composer", composer), ("title", t("t\(next(9))"))])
                    if let o = old { change = next(3) == 0 ? .remove("work", o) : .edit("work", o, new) } else { change = .add("work", new) }
                case 2:
                    let k = movements[next(movements.count)]
                    let old = st.get("movement", [t(k)])
                    let w = t(works[next(works.count)])
                    let new = Record([("id", t(k)), ("work_id", w), ("no", .int(Int64(next(5)))), ("part", t("p"))])
                    if let o = old { change = next(3) == 0 ? .remove("movement", o) : .edit("movement", o, new) } else { change = .add("movement", new) }
                default:
                    let k = songs[next(songs.count)]
                    let old = st.get("song", [t(k)])
                    let mv: Value = next(5) == 0 ? .null : t(movements[next(movements.count)])
                    let new = Record([("id", t(k)), ("title", t("x")), ("creator", t("c")), ("movement_id", mv), ("pos", .int(Int64(next(9))))])
                    if let o = old { change = next(3) == 0 ? .remove("song", o) : .edit("song", o, new) } else { change = .add("song", new) }
                }
                st.applyChange(change)
                batch.append(change)
            }
            let before = view.rows()
            let ps = try Views.pushAll(sch, st, batch, view)
            patches += ps.count
            if !Views.contract(sch, st, view) { throw Failed("step \(step): the view is not a fresh hydrate") }
            let read = try View.read(sch, plan, env.scope(sch), st)
            if view.rows() != read { throw Failed("step \(step): the view answers \(Value.list(view.rows()).show()), a fold \(Value.list(read).show())") }
            if Views.splice(ps, before) != view.rows() { throw Failed("step \(step): the patches do not splice") }
        }
        return (steps, patches)
    }
}
