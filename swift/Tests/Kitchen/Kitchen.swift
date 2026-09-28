import ArkAuthoring

// Everything in the vocabulary that neither the demo nor harken reaches —
// update, upsert, delete by a composite key, forEach, ifElse, unless,
// pick, a helper, fold, Opt.map, a list input, whole-input refine, opt
// fields, isIn, with, limit, helpers called with named arguments (one
// calling another), a record, equality on options, and one auto named
// twice — in one small domain. The tests hold its
// Native to Eval over its own Emit, case by case.

public struct Shelf {
    public var counter: Table<Counter>
    public var tag: Table<Tag>
}
extension Shelf: Tables {
    public static func open() -> Self {
        Shelf(counter: table(), tag: table())
    }
}

public struct Counter {
    public var id: Id<Counter>
    public var name: Text
    public var n: Int
    public var note: Opt<Text>
}
extension Counter: Row {
    public static let NAME = "counter"
    public typealias Key = Id<Counter>
    public static func columns() -> Columns<Self> {
        Columns<Self>()
            .id(Self.id)
            .text(Self.name)
            .int(Self.n)
            .text(Self.note)
            .nullable()
            .key(Self.id)
            .unique(Self.name)
    }
}
extension Counter {
    public static let id = col<Counter, Id<Counter>>("id")
    public static let name = col<Counter, Text>("name")
    public static let n = col<Counter, Int>("n")
    public static let note = col<Counter, Opt<Text>>("note")
    public static let tag = rel<Counter, Tag>("tag")
}

public struct Tag {
    public var counterId: Id<Counter>
    public var label: Text
    public var weight: Int
}
extension Tag: Row {
    public static let NAME = "tag"
    public typealias Key = (Id<Counter>, Text)
    public static func columns() -> Columns<Self> {
        Columns<Self>()
            .id(Self.counterId)
            .refs(Counter.self)
            .text(Self.label)
            .int(Self.weight)
            .key(Self.counterId, Self.label)
    }
}
extension Tag {
    public static let counterId = col<Tag, Id<Counter>>("counter_id")
    public static let label = col<Tag, Text>("label")
    public static let weight = col<Tag, Int>("weight")
}

public struct Make {
    public var name: Text
    public var note: Opt<Text>
    public var labels: List<Text>
}
extension Make: Input {
    public static var schema: Object<Self> {
        object()
            .field("name", text().trim().min(1))
            .field("note", opt(text().trim().max(10).why("a short note")))
            .field("labels", list(text().min(1)).nonEmpty())
            .refine { input in input.name.ne("forbidden") }
            .why("not that name")
    }
}

public struct Bump {
    public var counter: Id<Counter>
    public var by: Int
}
extension Bump: Input {
    public static var schema: Object<Self> {
        object().field("counter", id(Counter.self).exists()).field("by", int().range(1, 100).refine { by in by.ne(13) }.why("unlucky"))
    }
}

public struct Named {
    public var name: Text
}
extension Named: Input {
    public static var schema: Object<Self> {
        object().field("name", text())
    }
}

let double = helper("double", "x") { (x: Int) in x.mul(2) }

/// What `summaries` lists: not a row of any table.
public struct Summary {
    public var name: Text
    public var n: Int
    public var noted: Bool
    public var hi: Bool
    public var first: Opt<Text>
}
extension Summary: Record {
    public static func fields() -> Fields<Self> {
        Fields<Self>()
            .field("name", text())
            .field("n", int())
            .field("noted", bool())
            .field("hi", bool())
            .field("first", opt(text()))
    }
}

/// A name as a key part: lowercased, spaces as dashes.
public func slugOf(_ text: Text) -> Text {
    helper("slug_of", ("text", text)) { text in
        concat(text.trim().lower().chars().map { c in pick(c.eq(" "), "-", c) })
    }
}

/// A tag's key under its counter: two named arguments, and a helper inside.
public func labelKey(_ name: Text, _ label: Text) -> Text {
    helper("label_key", ("name", name), ("label", label)) { name, label in
        concat(list([slugOf(name), "/", slugOf(label)]))
    }
}

/// A counter as `summaries` lists it, read against every tag: a helper
/// returning a record, and equality on an option both ways.
public func summarize(_ counter: Counter, _ tags: List<Tag>) -> Summary {
    helper("summarize", ("counter", counter), ("tags", tags)) { counter, tags in
        Summary(
            name: counter.name,
            n: counter.n,
            noted: counter.note.ne(none(Text.self)).and(counter.note.ne(some("x"))),
            hi: counter.note.eq(some("hi")),
            first: tags.filter { row in row.counterId.eq(counter.id) }.first().map { row in labelKey(counter.name, row.label) }
        )
    }
}

public func kitchen() -> Router<Shelf> {
    let kitchen = router(Shelf.self, "kitchen")
    return kitchen.routes(
        kitchen.input(Make.self).mutation("make") { ctx, db, input in
            let id: Id<Counter> = ctx.newId("id")
            db.counter.insert(Counter(id: id, name: input.name, n: 0, note: input.note)).on(Counter.name)
            return forEach(input.labels) { label in db.tag.upsert(Tag(counterId: id, label: label, weight: label.len())) }
        },
        kitchen.input(Bump.self).mutation("bump") { _, db, input in
            unless(input.by.lt(60)) { refuse("too much") }
            db.counter.update(input.counter) { row in
                Counter(id: row.id, name: row.name, n: pick(row.n.add(input.by).gt(10), 10, row.n.add(input.by)), note: row.note.map { t in t.lower() })
            }
            return ifElse(input.by.eq(7), then: { db.tag.delete(input.counter, "lucky") }, else: {
                db.tag.upsert(Tag(counterId: input.counter, label: "bumped", weight: double(input.by)))
            })
        },
        kitchen.query("top") { _, db, _ in
            db.counter.filter(Counter.n.ge(0).and(Counter.name.ne("x"))).orderBy(Counter.n.desc()).limit(5).with(Counter.tag).all()
        },
        kitchen.query("total") { _, db, _ in
            let counters = db.counter.all()
            return counters.fold(Int(0)) { acc, row in acc.add(row.n) }.add(counters.filter { row in row.note.isSome() }.len())
        },
        kitchen.input(Named.self).mutation("stamp") { ctx, db, input in
            let id: Id<Counter> = ctx.newId("id")
            db.counter.insert(Counter(id: id, name: input.name, n: ctx.now("at"), note: none(Text.self))).on(Counter.name)
            return db.tag.upsert(Tag(counterId: id, label: labelKey(input.name, " Stamped Here "), weight: ctx.now("at")))
        },
        kitchen.query("summaries") { _, db, _ in
            let tags = db.tag.all()
            return db.counter.orderBy(Counter.name.asc()).all().map { row in summarize(row, tags) }
        },
        kitchen.input(Named.self).query("named") { _, db, input in
            db.counter.filter(Counter.name.isIn(list([input.name, "b"]))).first().map { row in row.n }.unwrapOr(-1)
        }
    )
}

public func kitchenModule() -> Module {
    Module(kitchen())
}
