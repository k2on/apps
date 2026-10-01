//! `views/` (§13): a plan kept up to date. Nine queries over one small
//! library — the demo's playlist and items, and people, works, movements
//! and songs — each hydrated over the same store and pushed the same
//! batches of changes; the vector is the plan, the store, the batches, and
//! after each batch the patches a screen splices and the answer.
//!
//! Every expectation is what the engine answered (`view::hydrate`,
//! `view::push_all`), asserted before it is written: after every batch
//! the view equals a fresh hydrate (`view::contract`), its rows are the
//! fresh answer, the patches splice the old answer into the new, and each
//! case shows the thing it is for.

// The row structs are the vocabulary's: a body reads their fields only
// natively, and the generator only ever emits.
#![allow(dead_code)]

use ark::authoring::*;
use ark::eval::{self, Args, Ctx as EvalCtx};
use ark::hash::closure;
use ark::ir::module_value;
use ark::protocol::change_value;
use ark::store::{Change, MemoryStore, Row as StoreRow, Store};
use ark::value::Value;
use ark::view::{contract, hydrate, push_all, splice, Env, Patch};

use super::demo::id_n;
use super::json::{array, json, obj, quoted};
use super::{claim, Out};

pub struct Lib {
    pub playlist: Table<Playlist>,
    pub item: Table<Item>,
    pub person: Table<Person>,
    pub work: Table<Work>,
    pub movement: Table<Movement>,
    pub song: Table<Song>,
}
impl Tables for Lib {
    fn open() -> Self {
        Lib {
            playlist: table(),
            item: table(),
            person: table(),
            work: table(),
            movement: table(),
            song: table(),
        }
    }
}

pub struct Playlist {
    pub id: Id<Playlist>,
    pub name: Text,
    pub user_id: Text,
}
impl Row for Playlist {
    const NAME: &str = "playlist";
    type Key = (Id<Playlist>,);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::id)
            .text(Self::name)
            .text(Self::user_id)
            .key((Self::id,))
            .unique((Self::user_id, Self::name))
    }
}
#[allow(non_upper_case_globals)]
impl Playlist {
    pub const id: Col<Self, Id<Self>> = col("id");
    pub const name: Col<Self, Text> = col("name");
    pub const user_id: Col<Self, Text> = col("user_id");
}

pub struct Item {
    pub playlist_id: Id<Playlist>,
    pub track_id: Text,
    pub pos: Int,
}
impl Row for Item {
    const NAME: &str = "item";
    type Key = (Id<Playlist>, Text);
    fn columns() -> Columns<Self> {
        columns()
            .id(Self::playlist_id)
            .refs::<Playlist>()
            .text(Self::track_id)
            .int(Self::pos)
            .key((Self::playlist_id, Self::track_id))
            .unique((Self::playlist_id, Self::pos))
    }
}
#[allow(non_upper_case_globals)]
impl Item {
    pub const playlist_id: Col<Self, Id<Playlist>> = col("playlist_id");
    pub const track_id: Col<Self, Text> = col("track_id");
    pub const pos: Col<Self, Int> = col("pos");
}

pub struct Person {
    pub name: Text,
    pub born: Int,
}
impl Row for Person {
    const NAME: &str = "person";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns().text(Self::name).int(Self::born).key((Self::name,))
    }
}
#[allow(non_upper_case_globals)]
impl Person {
    pub const name: Col<Self, Text> = col("name");
    pub const born: Col<Self, Int> = col("born");
}

pub struct Work {
    pub id: Text,
    pub composer: Text,
    pub title: Text,
}
impl Row for Work {
    const NAME: &str = "work";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns()
            .text(Self::id)
            .text(Self::composer)
            .refs::<Person>()
            .text(Self::title)
            .key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl Work {
    pub const id: Col<Self, Text> = col("id");
    pub const composer: Col<Self, Text> = col("composer");
    pub const title: Col<Self, Text> = col("title");
}

pub struct Movement {
    pub id: Text,
    pub work_id: Text,
    pub no: Int,
    pub part: Text,
}
impl Row for Movement {
    const NAME: &str = "movement";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns()
            .text(Self::id)
            .text(Self::work_id)
            .refs::<Work>()
            .int(Self::no)
            .text(Self::part)
            .key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl Movement {
    pub const id: Col<Self, Text> = col("id");
    pub const work_id: Col<Self, Text> = col("work_id");
    pub const no: Col<Self, Int> = col("no");
    pub const part: Col<Self, Text> = col("part");
}

pub struct Song {
    pub id: Text,
    pub title: Text,
    pub creator: Text,
    pub movement_id: Opt<Text>,
    pub pos: Int,
}
impl Row for Song {
    const NAME: &str = "song";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns()
            .text(Self::id)
            .text(Self::title)
            .text(Self::creator)
            .text(Self::movement_id)
            .nullable()
            .refs::<Movement>()
            .int(Self::pos)
            .key((Self::id,))
    }
}
#[allow(non_upper_case_globals)]
impl Song {
    pub const id: Col<Self, Text> = col("id");
    pub const title: Col<Self, Text> = col("title");
    pub const creator: Col<Self, Text> = col("creator");
    pub const movement_id: Col<Self, Opt<Text>> = col("movement_id");
    pub const pos: Col<Self, Int> = col("pos");
}

/// A node that is a name and a number.
pub struct Tally {
    pub name: Text,
    pub n: Int,
}
impl Record for Tally {
    fn fields() -> Fields<Self> {
        fields().field("name", text()).field("n", int())
    }
}

/// A song as a listing shows it: its movement's part and its work's title.
pub struct Listed {
    pub part: Text,
    pub title: Text,
    pub work: Text,
}
impl Record for Listed {
    fn fields() -> Fields<Self> {
        fields().field("part", text()).field("title", text()).field("work", text())
    }
}

pub struct PlaylistId {
    pub playlist_id: Id<Playlist>,
}
impl Input for PlaylistId {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>())
    }
}

/// One query per vector, named as its file is.
fn lib() -> Router<Lib> {
    let r = router::<Lib>("views");
    r.routes((
        // The v3 vectors' first plan: a filter from the input, an order, a
        // limit, so a removal refills the window from the entries.
        r.input::<PlaylistId>().query("top-two-by-pos", |_ctx, db, input| {
            db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.asc()).limit(2)
        }),
        // The v3 vectors' second: a child list with its own order and limit
        // per parent, and no projection, so a child change is its parent's
        // update.
        r.query("playlist-with-items", |_ctx, db, _input: ()| {
            db.playlist
                .order_by(Playlist::name.asc())
                .each(|p, ()| db.item.order_by(Item::pos.desc()).limit(3).on(Item::playlist_id.eq(p.id)))
        }),
        r.query("projection", |_ctx, db, _input: ()| {
            db.song.order_by(Song::pos.asc()).map(|song, ()| Tally {
                name: song.title,
                n: song.pos,
            })
        }),
        // A work with no movement is a candidate and not an answer, until
        // one arrives.
        r.query("having-admits-on-a-child", |_ctx, db, _input: ()| {
            db.work
                .order_by(Work::title.asc())
                .each(|work, ()| db.movement.rows().on(Movement::work_id.eq(work.id)))
                .having(|_work, (movements,)| movements.len().gt(0))
                .map(|work, (movements,)| Tally {
                    name: work.title,
                    n: movements.len(),
                })
        }),
        r.query("group-source", |_ctx, db, _input: ()| {
            db.song.group_by(Song::creator).map(|creator, (songs,)| Tally {
                name: creator,
                n: songs.len(),
            })
        }),
        // A song's movement, and through it the movement's work.
        r.query("lookup-chain", |_ctx, db, _input: ()| {
            db.song
                .order_by(Song::pos.asc())
                .get(|song, ()| db.movement.by_opt(song.movement_id))
                .get(|_song, (movement,)| db.work.by_opt(movement.map(|row| row.work_id)))
                .map(|song, (movement, work)| Listed {
                    part: movement.map_or("", |row| row.part),
                    title: song.title,
                    work: work.map_or("", |row| row.title),
                })
        }),
        // `song.creator` is no key and no reference.
        r.query("related-on-a-non-key-column", |_ctx, db, _input: ()| {
            db.person
                .order_by(Person::name.asc())
                .each(|person, ()| db.song.order_by(Song::pos.asc()).on(Song::creator.eq(person.name)))
                .map(|person, (songs,)| Tally {
                    name: person.name,
                    n: songs.len(),
                })
        }),
        // The two works with the most movements.
        r.query("expression-order-under-a-limit", |_ctx, db, _input: ()| {
            db.work
                .each(|work, ()| db.movement.rows().on(Movement::work_id.eq(work.id)))
                .sort_by_desc(|_work, (movements,)| movements.len())
                .limit(2)
                .map(|work, (movements,)| Tally {
                    name: work.title,
                    n: movements.len(),
                })
        }),
        // People, their works, the works' movements, the songs of each: the
        // default node at every depth.
        r.query("related-three-deep", |_ctx, db, _input: ()| {
            db.person.each(|person, ()| {
                db.work
                    .each(|work, ()| {
                        db.movement
                            .each(|movement, ()| db.song.rows().on(Song::movement_id.eq(some(movement.id))))
                            .on(Movement::work_id.eq(work.id))
                    })
                    .on(Work::composer.eq(person.name))
            })
        }),
    ))
}

fn row(pairs: Vec<(&str, Value)>) -> StoreRow {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn t(s: &str) -> Value {
    Value::text(s)
}

fn person(name: &str, born: i64) -> StoreRow {
    row(vec![("name", t(name)), ("born", Value::Int(born))])
}

fn work(id: &str, composer: &str, title: &str) -> StoreRow {
    row(vec![("id", t(id)), ("composer", t(composer)), ("title", t(title))])
}

fn movement(id: &str, work_id: &str, no: i64, part: &str) -> StoreRow {
    row(vec![("id", t(id)), ("work_id", t(work_id)), ("no", Value::Int(no)), ("part", t(part))])
}

fn song(id: &str, title: &str, creator: &str, movement_id: Option<&str>, pos: i64) -> StoreRow {
    row(vec![
        ("id", t(id)),
        ("title", t(title)),
        ("creator", t(creator)),
        ("movement_id", movement_id.map(t).unwrap_or(Value::Null)),
        ("pos", Value::Int(pos)),
    ])
}

fn item(k: i64) -> StoreRow {
    row(vec![
        ("playlist_id", Value::Id(id_n(1))),
        ("track_id", t(&format!("t{k}"))),
        ("pos", Value::Int(k)),
    ])
}

fn add(table: &str, r: StoreRow) -> Change {
    Change::Add(table.into(), r)
}

fn remove(table: &str, r: StoreRow) -> Change {
    Change::Remove(table.into(), r)
}

fn edit(table: &str, old: StoreRow, new: StoreRow) -> Change {
    Change::Edit(table.into(), old, new)
}

/// Three people, three works, four movements, five songs — one song of no
/// movement, one creator with no person row — and no playlist yet.
fn library(st: &mut MemoryStore) {
    let rows = [
        add("person", person("Bach", 1685)),
        add("person", person("Gould", 1932)),
        add("person", person("Handel", 1685)),
        add("work", work("bwv1046", "Bach", "Brandenburg 1")),
        add("work", work("bwv988", "Bach", "Goldberg")),
        add("work", work("hwv349", "Handel", "Water Music")),
        add("movement", movement("bwv1046#1", "bwv1046", 1, "Allegro")),
        add("movement", movement("bwv988#1", "bwv988", 1, "Aria")),
        add("movement", movement("bwv988#2", "bwv988", 2, "Var. 1")),
        add("movement", movement("hwv349#1", "hwv349", 1, "Overture")),
        add("song", song("s1", "Aria", "Gould", Some("bwv988#1"), 3)),
        add("song", song("s2", "Variation 1", "Gould", Some("bwv988#2"), 1)),
        add("song", song("s3", "Allegro", "Pinnock", Some("bwv1046#1"), 2)),
        add("song", song("s4", "Hum", "Gould", None, 4)),
        add("song", song("s5", "Overture", "Pinnock", Some("hwv349#1"), 5)),
    ];
    st.apply_changes(&rows);
}

/// The batches every case is pushed, in order. Each is applied to the
/// store whole and then pushed as one (§1.5: the store is already at the
/// state after all of them).
fn batches() -> Vec<Vec<Change>> {
    let playlist = row(vec![("id", Value::Id(id_n(1))), ("name", t("Viewed")), ("user_id", t("alice"))]);
    let s2 = song("s2", "Variation 1", "Gould", Some("bwv988#2"), 1);
    let s3 = song("s3", "Allegro", "Pinnock", Some("bwv1046#1"), 2);
    let s9 = song("s9", "Sinfony", "Gould", Some("hwv56#1"), 8);
    let s10 = song("s10", "Passing", "Gould", None, 10);
    vec![
        vec![add("playlist", playlist)],
        vec![add("item", item(1))],
        vec![add("item", item(2))],
        // two at once
        vec![add("item", item(3)), add("item", item(4))],
        // the first leaves the window and the third is let in
        vec![remove("item", item(1))],
        vec![add("item", item(5))],
        // a work with no movement: a candidate, not an answer…
        vec![add("work", work("hwv56", "Handel", "Messiah"))],
        // …until its first movements arrive, which also put it level with
        // the work that had the most
        vec![
            add("movement", movement("hwv56#1", "hwv56", 1, "Sinfony")),
            add("movement", movement("hwv56#2", "hwv56", 2, "Comfort ye")),
        ],
        vec![
            add("song", s9.clone()),
            edit("song", s2.clone(), song("s2", "Variation 1", "Gould", Some("bwv988#2"), 9)),
        ],
        // reached through two lookups
        vec![edit(
            "work",
            work("bwv988", "Bach", "Goldberg"),
            work("bwv988", "Bach", "Goldberg Variations"),
        )],
        // a song moves from one group to another, and its new creator
        // gains a person row
        vec![
            edit("song", s3, song("s3", "Allegro", "Gould", Some("bwv1046#1"), 2)),
            add("person", person("Pinnock", 1946)),
        ],
        vec![remove("song", song("s1", "Aria", "Gould", Some("bwv988#1"), 3))],
        // added, edited and removed within one batch: nothing to say
        vec![
            add("song", s10.clone()),
            edit("song", s10, song("s10", "Passing", "Gould", None, 11)),
            remove("song", song("s10", "Passing", "Gould", None, 11)),
        ],
        // the work's movements go, and the song under one
        vec![
            remove("song", s9),
            remove("movement", movement("hwv56#1", "hwv56", 1, "Sinfony")),
            remove("movement", movement("hwv56#2", "hwv56", 2, "Comfort ye")),
        ],
    ]
}

fn patch_value(p: &Patch) -> Value {
    let t = |s: &str| ("t", Value::text(s));
    match p {
        Patch::Insert { at, node } => Value::record(vec![t("insert"), ("at", Value::Int(*at as i64)), ("node", node.clone())]),
        Patch::Remove { at } => Value::record(vec![t("remove"), ("at", Value::Int(*at as i64))]),
        Patch::Update { at, node } => Value::record(vec![t("update"), ("at", Value::Int(*at as i64)), ("node", node.clone())]),
    }
}

/// What each case must show, beside the contract every case keeps: the
/// patches after each batch, by batch.
fn shows(name: &str, steps: &[Vec<Patch>]) -> bool {
    let has = |i: usize, f: fn(&Patch) -> bool| steps[i].iter().any(f);
    let insert = |p: &Patch| matches!(p, Patch::Insert { .. });
    let remove = |p: &Patch| matches!(p, Patch::Remove { .. });
    let update = |p: &Patch| matches!(p, Patch::Update { .. });
    // Nothing moves in the batch that adds, edits and removes one key.
    steps[12].is_empty()
        && match name {
            // a removal, and the refill after it
            "top-two-by-pos" => has(4, remove) && has(4, insert),
            // an item arriving is its playlist's update
            "playlist-with-items" => has(2, update),
            // a moved song is a remove and an insert
            "projection" => has(8, remove) && has(8, insert),
            // Messiah appears with its movement and goes with it
            "having-admits-on-a-child" => steps[6].is_empty() && has(7, insert) && has(13, remove),
            // a song changing creator updates both groups
            "group-source" => steps[10].iter().filter(|p| update(p)).count() == 2,
            // a work retitled reaches the songs through two lookups
            "lookup-chain" => has(9, update),
            // Pinnock appears, already with songs
            "related-on-a-non-key-column" => has(10, insert),
            // Messiah enters the top two, evicting the last, and leaves it,
            // letting that one back in
            "expression-order-under-a-limit" => has(7, insert) && has(7, remove) && has(13, remove) && has(13, insert),
            // a song under a movement of a work is its composer's update
            "related-three-deep" => has(8, update),
            _ => false,
        }
}

pub fn views(out: &Out) {
    out.dir("views/");
    let module = Module::new((lib(),));
    let m = module.build();
    let sch = &m.schema;
    let mv = module_value(m);
    let ctx = EvalCtx::new("alice", "dev");
    let ctx_value = Value::record(vec![("user", t("alice")), ("session", t("dev"))]);
    let mut st0 = MemoryStore::empty(sch.clone());
    library(&mut st0);
    let batches = batches();
    for f in m.functions.iter() {
        let name = f.name.as_str();
        let plan = f.plan.clone().expect("a query is a plan");
        // The plan as the module writes it.
        let plan_value = mv
            .field("functions")
            .as_list()
            .into_iter()
            .find(|g| g.field("name") == t(name))
            .map(|g| g.field("plan"))
            .expect("the query in the module's value");
        let args: Args = if f.input.is_empty() {
            Args::new()
        } else {
            Args::from([("playlist_id".to_string(), Value::Id(id_n(1)))])
        };
        let c = closure(m, f);
        let (args_in, provided) = eval::middleware(sch, &c, &ctx, &args, &st0).expect("the middleware");
        let env = Env {
            helpers: c.helpers.clone(),
            ctx: ctx.clone(),
            args: args_in,
            provided,
        };
        let mut st = st0.clone();
        let mut view = hydrate(sch, &plan, env.clone(), &st).expect("hydrate");
        let rows_before = view.rows();
        let mut steps: Vec<(Vec<Patch>, Vec<Value>)> = Vec::new();
        for (i, batch) in batches.iter().enumerate() {
            let before = view.rows();
            st.apply_changes(batch);
            let patches = push_all(sch, &st, batch, &mut view).unwrap_or_else(|e| panic!("view {name}: batch {i}: {e:?}"));
            claim(&format!("view {name}: batch {i} keeps the contract"), contract(sch, &st, &view));
            let fresh = hydrate(sch, &plan, env.clone(), &st).expect("hydrate");
            claim(&format!("view {name}: batch {i} is a fresh hydrate"), view.rows() == fresh.rows());
            claim(
                &format!("view {name}: batch {i}'s patches splice"),
                splice(&patches, &before) == view.rows(),
            );
            steps.push((patches, view.rows()));
        }
        let patches: Vec<Vec<Patch>> = steps.iter().map(|(p, _)| p.clone()).collect();
        claim(
            &format!("view {name}: the patches do not show what the plan is for"),
            shows(name, &patches),
        );
        let parts = |steps: &[(Vec<Patch>, Vec<Value>)]| {
            vec![
                ("module", json(&mv)),
                ("query", quoted(name)),
                ("plan", json(&plan_value)),
                ("ctx", json(&ctx_value)),
                ("args", json(&Value::Struct(args.clone()))),
                ("store_before", json(&st0.store_value())),
                ("rows_before", json(&Value::List(rows_before.clone()))),
                (
                    "batches",
                    json(&Value::List(
                        batches.iter().map(|b| Value::List(b.iter().map(change_value).collect())).collect(),
                    )),
                ),
                (
                    "steps",
                    array(steps.iter().map(|(ps, rows)| {
                        obj(&[
                            ("patches", json(&Value::List(ps.iter().map(patch_value).collect()))),
                            ("rows", json(&Value::List(rows.clone()))),
                        ])
                    })),
                ),
            ]
        };
        out.write(&format!("views/{name}.json"), &obj(&parts(&steps)));
        if name == "top-two-by-pos" {
            // The same run with the refill dropped: the window left one
            // short after a removal.
            let mut wrong = steps.clone();
            wrong[4].0.retain(|p| !matches!(p, Patch::Insert { .. }));
            let mut parts = parts(&wrong);
            parts.push(("expect", quoted("fail")));
            out.write("views/falsify/top-two-by-pos-no-refill.json", &obj(&parts));
        }
    }
    println!("  {} batches through {} plans", batches.len(), m.functions.len());
    more(out);
}

// `docs/plan-db.md` D4 ------------------------------------------------------
//
// Two cases in a module of their own — its schema has a text index, and
// every file above carries its module whole, so adding them there would
// move every one of those files' bytes.

pub struct More {
    pub person: Table<Person>,
    pub track: Table<Track>,
}
impl Tables for More {
    fn open() -> Self {
        More {
            person: table(),
            track: table(),
        }
    }
}

/// Tracks with an index under the creator by position, which is what lets
/// `kept-max` keep its greatest, and a text index on the title, which is
/// what serves `has-search`.
pub struct Track {
    pub id: Text,
    pub title: Text,
    pub creator: Text,
    pub pos: Int,
}
impl Row for Track {
    const NAME: &str = "track";
    type Key = (Text,);
    fn columns() -> Columns<Self> {
        columns()
            .text(Self::id)
            .text(Self::title)
            .text(Self::creator)
            .int(Self::pos)
            .key((Self::id,))
            .index((Self::creator, Self::pos))
            .index_text(Self::title)
    }
}
#[allow(non_upper_case_globals)]
impl Track {
    pub const id: Col<Self, Text> = col("id");
    pub const title: Col<Self, Text> = col("title");
    pub const creator: Col<Self, Text> = col("creator");
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

fn more_lib() -> Router<More> {
    let r = router::<More>("views_d4");
    r.routes((
        // Each person's greatest position, kept as a value: an arrival above
        // it compares, its holder leaving with a twin behind changes
        // nothing, its holder leaving alone is read again through the index.
        r.query("kept-max", |_ctx, db, _input: ()| {
            db.person
                .order_by(Person::name.asc())
                .each(|person, ()| db.track.rows().on(Track::creator.eq(person.name)))
                .map(|person, (tracks,)| Tally {
                    name: person.name,
                    n: tracks.fold(-1, |acc: Int, x| acc.max(x.pos)),
                })
        }),
        // A search of the titles, folded, in position order: a row moves in
        // and out of it as its title is edited.
        r.input::<Needle>().query("has-search", |_ctx, db, input| {
            db.track
                .filter(Track::title.has(input.needle))
                .order_by(Track::pos.asc())
                .map(|track, ()| Tally {
                    name: track.title,
                    n: track.pos,
                })
        }),
    ))
}

fn track(id: &str, title: &str, creator: &str, pos: i64) -> StoreRow {
    row(vec![
        ("id", t(id)),
        ("title", t(title)),
        ("creator", t(creator)),
        ("pos", Value::Int(pos)),
    ])
}

fn more_batches() -> Vec<Vec<Change>> {
    let t6 = track("t6", "ARIA da capo", "Gould", 9);
    let t2 = track("t2", "Variation 1", "Gould", 7);
    let t3 = track("t3", "Variation 2", "Gould", 7);
    let t4 = track("t4", "Air", "Bach", 1);
    let t9 = track("t9", "Passing aria", "Gould", 20);
    vec![
        // above Gould's greatest, and capitals found by their lowercase
        vec![add("track", t6.clone())],
        // the greatest leaves: read again
        vec![remove("track", t6)],
        // the greatest leaves with a twin behind: nothing for the max
        vec![remove("track", t2)],
        // the last of the sevens moves down, and out of the search
        vec![edit("track", t3, track("t3", "Sarabande", "Gould", 2))],
        // Bach's one track moves up, and into the search
        vec![edit("track", t4, track("t4", "Fair aria", "Bach", 12))],
        // a person with no track
        vec![add("person", person("Nobody", 2000))],
        // added and removed within one batch: nothing to say
        vec![add("track", t9.clone()), remove("track", t9)],
    ]
}

fn more(out: &Out) {
    let module = Module::new((more_lib(),));
    let m = module.build();
    let sch = &m.schema;
    let mv = module_value(m);
    let ctx = EvalCtx::new("alice", "dev");
    let ctx_value = Value::record(vec![("user", t("alice")), ("session", t("dev"))]);
    let mut st0 = MemoryStore::empty(sch.clone());
    st0.apply_changes(&[
        add("person", person("Bach", 1685)),
        add("person", person("Gould", 1932)),
        add("person", person("Handel", 1685)),
        add("track", track("t1", "Aria", "Gould", 3)),
        add("track", track("t2", "Variation 1", "Gould", 7)),
        add("track", track("t3", "Variation 2", "Gould", 7)),
        add("track", track("t4", "Air", "Bach", 1)),
        add("track", track("t5", "Ariadne", "Handel", 2)),
    ]);
    let batches = more_batches();
    for f in m.functions.iter() {
        let name = f.name.as_str();
        let plan = f.plan.clone().expect("a query is a plan");
        let plan_value = mv
            .field("functions")
            .as_list()
            .into_iter()
            .find(|g| g.field("name") == t(name))
            .map(|g| g.field("plan"))
            .expect("the query in the module's value");
        let args: Args = if f.input.is_empty() {
            Args::new()
        } else {
            Args::from([("needle".to_string(), t("ari"))])
        };
        let c = closure(m, f);
        let (args_in, provided) = eval::middleware(sch, &c, &ctx, &args, &st0).expect("the middleware");
        let env = Env {
            helpers: c.helpers.clone(),
            ctx: ctx.clone(),
            args: args_in,
            provided,
        };
        let mut st = st0.clone();
        let mut view = hydrate(sch, &plan, env.clone(), &st).expect("hydrate");
        let kept = !view.shape.kept.is_empty();
        claim(&format!("view {name}: the max is kept"), name != "kept-max" || kept);
        let rows_before = view.rows();
        let mut steps: Vec<(Vec<Patch>, Vec<Value>)> = Vec::new();
        for (i, batch) in batches.iter().enumerate() {
            let before = view.rows();
            st.apply_changes(batch);
            let patches = push_all(sch, &st, batch, &mut view).unwrap_or_else(|e| panic!("view {name}: batch {i}: {e:?}"));
            claim(&format!("view {name}: batch {i} keeps the contract"), contract(sch, &st, &view));
            claim(
                &format!("view {name}: batch {i}'s patches splice"),
                splice(&patches, &before) == view.rows(),
            );
            steps.push((patches, view.rows()));
        }
        let has = |i: usize, f: fn(&Patch) -> bool| steps[i].0.iter().any(f);
        let update = |p: &Patch| matches!(p, Patch::Update { .. });
        let insert = |p: &Patch| matches!(p, Patch::Insert { .. });
        let remove = |p: &Patch| matches!(p, Patch::Remove { .. });
        let shows = steps[6].0.is_empty()
            && match name {
                // up with the arrival, back with its leaving, still with the
                // twin, down with the last seven, Bach up, Nobody in
                "kept-max" => has(0, update) && has(1, update) && steps[2].0.is_empty() && has(3, update) && has(4, update) && has(5, insert),
                // the capitals found, gone, a Variation gone, the
                // Sarabande out, the Fair aria in
                "has-search" => has(0, insert) && has(1, remove) && has(2, remove) && has(3, remove) && has(4, insert) && steps[5].0.is_empty(),
                _ => false,
            };
        claim(&format!("view {name}: the patches do not show what the plan is for"), shows);
        let parts = vec![
            ("module", json(&mv)),
            ("query", quoted(name)),
            ("plan", json(&plan_value)),
            ("ctx", json(&ctx_value)),
            ("args", json(&Value::Struct(args.clone()))),
            ("store_before", json(&st0.store_value())),
            ("rows_before", json(&Value::List(rows_before))),
            (
                "batches",
                json(&Value::List(
                    batches.iter().map(|b| Value::List(b.iter().map(change_value).collect())).collect(),
                )),
            ),
            (
                "steps",
                array(steps.iter().map(|(ps, rows)| {
                    obj(&[
                        ("patches", json(&Value::List(ps.iter().map(patch_value).collect()))),
                        ("rows", json(&Value::List(rows.clone()))),
                    ])
                })),
            ),
        ];
        out.write(&format!("views/{name}.json"), &obj(&parts));
    }
    println!("  {} batches through {} plans (D4)", batches.len(), m.functions.len());
}
