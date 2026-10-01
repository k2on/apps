//! `docs/plan-db.md` D4: text search. `Pred::Has(column, needle)` holds when
//! the text column contains the needle, both folded by the pinned `lower`;
//! a text index on the column (`.index_text(col)`) keeps, per trigram of
//! the folded value, the rows holding it, and a `Has` reads the
//! intersection of its needle's trigrams' postings — the union over the
//! branches of an `or` — before `keep` confirms each row.
//!
//! Here: what the index reads (the guard, through the counting store), the
//! Unicode folding, the fallback of a needle too short to have a trigram,
//! the overlay, the contract of a view over a `Has` filter under churn, the
//! wire form and the verifier.

#[path = "support/churn.rs"]
mod churn;
#[path = "support/counting.rs"]
mod counting;

use std::collections::BTreeSet;

use ark::authoring::*;
use ark::eval::{self, Args};
use ark::ir;
use ark::schema::Schema;
use ark::store::{self, Change, MemoryStore, Overlay, Row as StoreRow, Store};
use ark::value::Value;
use ark::view::{self, Env, Patch};
use churn::drive;
use counting::{Counting, Reads};

pub struct Lib {
    pub media: Table<Media>,
    pub bare: Table<Bare>,
}
impl Tables for Lib {
    fn open() -> Self {
        Lib {
            media: table(),
            bare: table(),
        }
    }
}

/// Media with a text index on the title and on the creator.
pub struct Media {
    pub id: Text,
    pub title: Text,
    pub creator: Opt<Text>,
    pub pos: Int,
}
impl Row for Media {
    const NAME: &str = "media";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns()
            .text(Self::id)
            .text(Self::title)
            .text(Self::creator)
            .nullable()
            .int(Self::pos)
            .key((Self::id,))
            .index((Self::pos,))
            .index_text(Self::title)
            .index_text(Self::creator)
    }
}
#[allow(non_upper_case_globals)]
impl Media {
    pub const id: Col<Self, Text> = col("id");
    pub const title: Col<Self, Text> = col("title");
    pub const creator: Col<Self, Opt<Text>> = col("creator");
    pub const pos: Col<Self, Int> = col("pos");
}

/// The same rows and no text index: every search a scan.
pub struct Bare {
    pub id: Text,
    pub title: Text,
    pub creator: Opt<Text>,
    pub pos: Int,
}
impl Row for Bare {
    const NAME: &str = "bare";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns()
            .text(Self::id)
            .text(Self::title)
            .text(Self::creator)
            .nullable()
            .int(Self::pos)
            .key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl Bare {
    pub const id: Col<Self, Text> = col("id");
    pub const title: Col<Self, Text> = col("title");
    pub const creator: Col<Self, Opt<Text>> = col("creator");
    pub const pos: Col<Self, Int> = col("pos");
}

pub struct Needle {
    pub needle: Text,
}
impl Input for Needle {
    fn schema() -> Object<Self> {
        object().field("needle", text())
    }
}

pub struct Hit {
    pub title: Text,
    pub pos: Int,
}
impl Record for Hit {
    fn fields() -> Fields<Self> {
        fields().field("title", text()).field("pos", int())
    }
}

fn lib() -> Router<Lib> {
    let r = router::<Lib>("lib");
    r.routes((
        // harken's search: the title or the creator, in position order. A
        // bare plan, so a read of it is `view::read`'s.
        r.input::<Needle>().query("search", |_ctx, db, input| {
            db.media
                .filter(Media::title.has(input.needle).or(Media::creator.has(input.needle)))
                .order_by(Media::pos.asc())
        }),
        // The title alone, projected: a view's plan.
        r.input::<Needle>().query("titled", |_ctx, db, input| {
            db.media
                .filter(Media::title.has(input.needle))
                .map(|m, ()| Hit { title: m.title, pos: m.pos })
        }),
        // The first five, beside an order: a window over a search.
        r.input::<Needle>().query("first_five", |_ctx, db, input| {
            db.media
                .filter(Media::title.has(input.needle).and(Media::pos.ge(0)))
                .order_by(Media::pos.desc())
                .limit(5)
                .map(|m, ()| Hit { title: m.title, pos: m.pos })
        }),
        r.input::<Needle>().query("bare_search", |_ctx, db, input| {
            db.bare
                .filter(Bare::title.has(input.needle).or(Bare::creator.has(input.needle)))
                .order_by(Bare::pos.asc())
        }),
    ))
}

fn module() -> Module {
    Module::new((lib(),))
}

fn t(s: &str) -> Value {
    Value::text(s)
}

fn media(id: &str, title: &str, creator: Option<&str>, pos: i64) -> StoreRow {
    [
        ("id", t(id)),
        ("title", t(title)),
        ("creator", creator.map(t).unwrap_or(Value::Null)),
        ("pos", Value::int(pos)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

fn put(st: &mut MemoryStore, row: StoreRow) {
    st.apply_change(&Change::Add("media".into(), row.clone()));
    st.apply_change(&Change::Add("bare".into(), row));
}

fn env(needle: &str) -> Env {
    Env {
        helpers: vec![],
        ctx: eval::Ctx::default(),
        args: Args::from([("needle".to_string(), t(needle))]),
        provided: Args::new(),
    }
}

fn plan(built: &ir::Module, name: &str) -> ir::Plan {
    built
        .lookup_function(name)
        .unwrap_or_else(|| panic!("no query {name}"))
        .plan
        .clone()
        .expect("a plan")
}

fn read(sch: &Schema, built: &ir::Module, name: &str, needle: &str, st: &dyn Store) -> Vec<Value> {
    let e = env(needle);
    view::read(sch, &plan(built, name), &e.scope(sch), st).expect("read")
}

const WORDS: [&str; 10] = [
    "Sonata",
    "Aria",
    "Prelude",
    "Fugue",
    "Atasonat",
    "Variation",
    "Nocturne",
    "Étude",
    "Waltz",
    "Sonatina",
];
const PEOPLE: [Option<&str>; 7] = [
    Some("Bach"),
    Some("Chopin"),
    Some("Liszt"),
    Some("Satie"),
    Some("Sonata Arctica"),
    Some("Ravel"),
    None,
];

/// `n` media: a title from ten words — one of them `Atasonat`, which holds
/// every trigram of "sonata" and not "sonata" — numbered, and a creator
/// from seven, one of whom is a band called Sonata Arctica and one nobody.
fn library(sch: &Schema, n: usize) -> MemoryStore {
    let mut st = MemoryStore::empty(sch.clone());
    for i in 0..n {
        put(
            &mut st,
            media(&format!("m{i:05}"), &format!("{} {i}", WORDS[i % 10]), PEOPLE[i % 7], i as i64),
        );
    }
    st
}

// Whether a text holds every trigram of a needle: what the index answers
// for a row, computed from the row alone.
fn holds_grams(v: &Value, needle: &str) -> bool {
    match v {
        Value::Text(s) => store::trigrams(&store::fold(needle)).is_subset(&store::trigrams(&store::fold(s))),
        _ => false,
    }
}

fn contains(v: &Value, needle: &str) -> bool {
    matches!(v, Value::Text(s) if store::fold(s).contains(&store::fold(needle)))
}

/// The guard: a search of 8,000 media for "sonata" in the title or the
/// creator examines exactly the rows the postings' intersection names —
/// per branch, the rows whose folded title (or creator) holds all four of
/// son, ona, nat, ata, and the union of the two — and reads no row by key.
/// At 8,000 that is 2,514 rows examined (the Sonata and Atasonat titles,
/// and Sonata Arctica's), of which 1,829 answer: `Atasonat` holds every
/// trigram and not the needle, and `keep` refuses it (`Sonatina` lacks
/// "ata" and is never read). The same search of the same rows with no
/// text index (`bare`) examines all 8,000. A view over the title's search
/// hydrates through the title's intersection, and a row arriving that
/// matches is one `get` and an `Insert`.
///
/// Falsified by skipping the index (`MemoryStore::scan_where_text`
/// answering `scan_where_eq`): the search examines 8,000 rows and the
/// first assertion fails.
#[test]
fn a_search_examines_the_postings_intersection() {
    let m = module();
    let built = m.build();
    let sch = &built.schema;
    let n = 8000;
    let mut st = library(sch, n);
    let needle = "sonata";
    let rows = st.rows("media");
    let intersection = rows
        .values()
        .filter(|r| holds_grams(&r["title"], needle) || holds_grams(&r["creator"], needle))
        .count();
    let matches = rows
        .values()
        .filter(|r| contains(&r["title"], needle) || contains(&r["creator"], needle))
        .count();
    assert!(intersection > matches, "the decoy holds the trigrams and not the needle");

    let counted = Counting::new(&st);
    let got = read(sch, built, "search", needle, &counted);
    let indexed = counted.reads();
    assert_eq!(
        indexed,
        Reads { gets: 0, rows: intersection },
        "the rows examined are the postings' intersection"
    );
    assert_eq!(got.len(), matches);
    let poss: Vec<i64> = got.iter().map(|r| r.field("pos").as_int()).collect();
    assert!(poss.windows(2).all(|w| w[0] < w[1]), "in position order");

    let counted = Counting::new(&st);
    let bare = read(sch, built, "bare_search", needle, &counted);
    assert_eq!(counted.reads(), Reads { gets: 0, rows: n }, "without the index, every row");
    assert_eq!(bare.len(), matches);
    eprintln!(
        "a search of {n} media for {needle:?}: {indexed:?} through the index ({matches} answer), {:?} without",
        counted.reads()
    );

    // A view over the title alone: hydrated through the title's
    // intersection; a matching row arriving is admitted for one `get`.
    let titled = plan(built, "titled");
    let counted = Counting::new(&st);
    let mut v = view::hydrate(sch, &titled, env(needle), &counted).expect("hydrate");
    let title_grams = rows.values().filter(|r| holds_grams(&r["title"], needle)).count();
    assert_eq!(counted.reads(), Reads { gets: 0, rows: title_grams });
    let before = v.rows().len();
    let arrive = Change::Add("media".into(), media("new", "A Sonata, new", None, -1));
    st.apply_change(&arrive);
    let counted = Counting::new(&st);
    let ps = view::push_all(sch, &counted, &[arrive], &mut v).expect("push");
    assert_eq!(counted.reads(), Reads { gets: 1, rows: 0 });
    assert!(matches!(ps.as_slice(), [Patch::Insert { .. }]), "{ps:?}");
    assert_eq!(v.rows().len(), before + 1);
    assert!(view::contract(sch, &st, &v));
}

/// A needle of fewer than three characters has no trigram: the read falls
/// back to the scan, and is still right. Falsified by answering an empty
/// set for a needle with no trigram (`text_keys` treating no list as no
/// rows): the search finds nothing.
#[test]
fn a_short_needle_is_a_scan() {
    let m = module();
    let built = m.build();
    let sch = &built.schema;
    let st = library(sch, 700);
    let counted = Counting::new(&st);
    let got = read(sch, built, "search", "ar", &counted);
    assert_eq!(counted.reads(), Reads { gets: 0, rows: 700 });
    let rows = st.rows("media");
    let want = rows
        .values()
        .filter(|r| contains(&r["title"], "ar") || contains(&r["creator"], "ar"))
        .count();
    assert_eq!(got.len(), want);
    assert!(want > 0);
    // The empty needle: every text holds it, a `Null` creator not.
    assert_eq!(read(sch, built, "titled", "", &st).len(), 700);
}

/// Folding is the pinned `lower`, on both sides: a title with non-ASCII
/// capitals is found by its lowercase needle and by its uppercase one, a
/// Greek one by its lowercase letters, through the index and without it
/// alike — and no accent is folded away. Falsified by folding with
/// `to_ascii_lowercase` (in `store::fold`): "élé" no longer finds "Élégie".
#[test]
fn a_capital_is_found_by_its_lowercase() {
    let m = module();
    let built = m.build();
    let sch = &built.schema;
    let mut st = MemoryStore::empty(sch.clone());
    for (id, title, pos) in [("a", "Élégie", 1), ("b", "ΣΟΦΙΑ", 2), ("c", "Elegy", 3), ("d", "Straße", 4)] {
        put(&mut st, media(id, title, None, pos));
    }
    let titles = |name: &str, needle: &str| -> Vec<Value> { read(sch, built, name, needle, &st).iter().map(|r| r.field("title")).collect() };
    for name in ["search", "bare_search"] {
        assert_eq!(titles(name, "élé"), vec![t("Élégie")], "{name}");
        assert_eq!(titles(name, "ÉLÉGIE"), vec![t("Élégie")], "{name}");
        assert_eq!(titles(name, "σοφ"), vec![t("ΣΟΦΙΑ")], "{name}");
        assert_eq!(titles(name, "ele"), vec![t("Elegy")], "{name}: an accent is not folded away");
        assert_eq!(
            titles(name, "STRASSE"),
            Vec::<Value>::new(),
            "{name}: a simple mapping, one character for one"
        );
        assert_eq!(titles(name, "straß"), vec![t("Straße")], "{name}");
    }
}

/// Through an overlay — a mutator's reads, a replica's pending — a search
/// is the base's indexed answer for the rows not written, and the writes
/// judged by `keep`: an added match found, an edited row that no longer
/// matches gone, a removed one gone. Falsified by asking the base for the
/// written keys too (dropping the `!ws.contains_key` from the overlay's
/// `scan_where_text`): the edited row is found under its old title.
#[test]
fn an_overlay_searches_its_writes_too() {
    let m = module();
    let built = m.build();
    let sch = &built.schema;
    let base = library(sch, 100);
    let changes = [
        Change::Add("media".into(), media("new", "Sonata nova", None, 1000)),
        Change::Edit(
            "media".into(),
            base.get("media", &[t("m00000")]).unwrap(),
            media("m00000", "Nothing", Some("Nobody"), 0),
        ),
        Change::Remove("media".into(), base.get("media", &[t("m00010")]).unwrap()),
    ];
    let mut ov = Overlay::new(&base);
    let mut merged = base.clone();
    for c in &changes {
        ov.apply_change(c);
        merged.apply_change(c);
    }
    let a = read(sch, built, "search", "sonata", &ov);
    let b = read(sch, built, "search", "sonata", &merged);
    assert_eq!(a, b);
    let ids: BTreeSet<Value> = a.iter().map(|r| r.field("id")).collect();
    assert!(ids.contains(&t("new")) && !ids.contains(&t("m00000")) && !ids.contains(&t("m00010")));
}

// The contract, under churn ---------------------------------------------------

fn small(sch: &Schema) -> MemoryStore {
    let mut st = MemoryStore::empty(sch.clone());
    for (id, title, creator, pos) in [
        ("a", "Aria", Some("Bach"), 1),
        ("b", "Variation 1", Some("Bach"), 2),
        ("c", "Ariadne", Some("Strauss"), 3),
        ("d", "Prelude", None, 4),
        ("e", "x12", Some("Ariosto"), 5),
        ("f", "Sonata", Some("Clementi"), 0),
        ("g", "Aria da capo", None, 7),
    ] {
        put(&mut st, media(id, title, creator, pos));
    }
    st
}

/// A view over a `Has` filter is maintained as any filter is: a changed row
/// re-admitted by the predicate. Under churn — titles and creators drawn
/// from each other's values, so rows move in and out of the search — after
/// every batch the view is a fresh hydrate, for a needle the index serves
/// ("ari", and "ARI", folded), one too short to ("x1"), over an `or` of two
/// columns and under a window. A hydrate reads through the text index and
/// a push re-admits by the predicate, so the two are held to each other.
/// Falsified by reading an `or` through its first branch alone (`needles`
/// keeping `fs[..1]` of an `Any`): `search` hydrates without the rows that
/// match only by creator, which a push then admits — "the view is not a
/// fresh hydrate".
#[test]
fn a_view_over_a_search_is_maintained() {
    let m = module();
    let built = m.build();
    let sch = &built.schema;
    for name in ["titled", "search", "first_five"] {
        for needle in ["ari", "x1", "ARI"] {
            let p = plan(built, name);
            let mut inserts = 0;
            for seed in 0..8 {
                let tally = drive(&format!("{name}({needle})"), sch, &p, &env(needle), small(sch), seed, 60);
                inserts += tally.inserts;
            }
            assert!(inserts > 0, "{name}({needle}): the search moved");
        }
    }
}

// The wire and the verifier ------------------------------------------------

/// The module writes a `Has` as `phas` and a text index as an `index` of
/// kind `text`, and reads both back; a module without either writes no
/// such key (`ir::module_value` of a schema with no text index is what it
/// was — `tests/vectors.rs` holds every existing file to its bytes).
/// Falsified by decoding `phas` as `pnot` of its column: the round trip
/// differs.
#[test]
fn the_wire_form_round_trips() {
    let m = module();
    let built = m.build();
    let v = ir::module_value(built);
    let bytes = ark::canon::encode(&v);
    let back = ir::module_from_value(&ark::canon::decode(&bytes).expect("canon")).expect("decode");
    assert_eq!(ir::module_value(&back), v);
    assert_eq!(back.schema, built.schema);
    let json = format!("{v:?}");
    assert!(json.contains("\"phas\"") && json.contains("\"kind\""), "the new keys are there");
    let media = back.schema.lookup_table("media").unwrap();
    assert_eq!(media.text, vec!["title".to_string(), "creator".to_string()]);
    assert!(back.schema.lookup_table("bare").unwrap().text.is_empty());
}

/// A `Has` is on a text column and its needle is text; anything else is no
/// module. Falsified by dropping the column's type check from `pred_ok`.
#[test]
fn the_verifier_holds_has_to_text() {
    let m = module();
    let mut bad = m.build().clone();
    let f = bad.functions.iter_mut().find(|f| f.name == "titled").unwrap();
    f.plan.as_mut().unwrap().filter = Some(ir::Pred::Has("pos".into(), ir::Expr::Lit(t("1"))));
    assert!(ark::verify::verify(&bad).is_err(), "a Has on an Int column");
    let f = bad.functions.iter_mut().find(|f| f.name == "titled").unwrap();
    f.plan.as_mut().unwrap().filter = Some(ir::Pred::Has("title".into(), ir::Expr::Lit(Value::int(1))));
    assert!(ark::verify::verify(&bad).is_err(), "an Int needle");
    let f = bad.functions.iter_mut().find(|f| f.name == "titled").unwrap();
    f.plan.as_mut().unwrap().filter = Some(ir::Pred::Has("creator".into(), ir::Expr::Lit(t("x"))));
    assert!(ark::verify::verify(&bad).is_ok(), "a nullable text column");
}
