//! The reference domains of the `views/` vectors (§1.10): each vector is
//! one view named here, its input, a store and batches of row changes, and
//! the patches after each. A second implementation runs the view of that
//! name it implements: the plan is this file's Rust, one per view.
//!
//! - `library`: the demo's playlists and items, people, works, movements,
//!   songs and tracks — v4's scenarios, their queries as typed plans;
//! - `colors` and `pairs`: the two fuzz findings on a maintained sum, each
//!   its own schema.
//!
//! Every operator is here: `filter` (`top_two_by_pos`, `has_search`,
//! `having_admits_on_a_child`), `map` (most, reading the store in
//! `lookup_chain` and the related ones), `sort`, `take` (`top_two_by_pos`,
//! `expression_order_under_a_limit`), `group` (`group_source`, the two
//! sums) and `join` (`playlist_join_item`).

use ark::prelude::*;
use ark::row::Bytes;

pub use crate::demo::{Item, Items, Playlist};

// * The library

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
pub struct Person {
    #[ark(key)]
    pub name: String,
    pub born: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
#[ark(index(composer))]
pub struct Work {
    #[ark(key)]
    pub id: String,
    #[ark(refs(Person))]
    pub composer: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
#[ark(index(work_id))]
pub struct Movement {
    #[ark(key)]
    pub id: String,
    #[ark(refs(Work))]
    pub work_id: String,
    pub no: i64,
    pub part: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
#[ark(index(creator), index(movement_id))]
pub struct Song {
    #[ark(key)]
    pub id: String,
    pub title: String,
    pub creator: String,
    #[ark(refs(Movement))]
    pub movement_id: Option<String>,
    pub pos: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
#[ark(index(creator, pos))]
pub struct Track {
    #[ark(key)]
    pub id: String,
    pub title: String,
    pub creator: String,
    pub pos: i64,
}

#[derive(Schema)]
pub struct Library {
    pub playlist: Playlist,
    pub item: Item,
    pub person: Person,
    pub work: Work,
    pub movement: Movement,
    pub song: Song,
    pub track: Track,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct Needle {
    pub needle: String,
}

type View<T> = Result<Plan<Library, T>, Refusal>;

/// A playlist's first two items by place.
#[view("library.top_two_by_pos")]
pub fn top_two_by_pos(_: &Ctx, _: &Db<Library>, input: Items) -> View<Item> {
    let p = input.playlist_id;
    Ok(scan::<Library, Item>()
        .filter(move |i| i.playlist_id == p)
        .sort(|i| (i.pos, i.playlist_id, i.track_id.clone()))
        .take(2))
}

/// Every playlist by name, with its last three items, last first.
#[view("library.playlist_with_items")]
pub fn playlist_with_items(_: &Ctx, _: &Db<Library>, _: ()) -> View<(Id<Playlist>, String, String, Vec<Item>)> {
    Ok(scan::<Library, Playlist>().sort(|p| (p.name.clone(), p.id)).map_with(|db, p| {
        let mut items = db.item.by(Item::KEY).from(p.id).all();
        items.sort_by_key(|i| std::cmp::Reverse(i.pos));
        items.truncate(3);
        (p.id, p.name.clone(), p.user_id.clone(), items)
    }))
}

/// Each song's place and title, by place.
#[view("library.projection")]
pub fn projection(_: &Ctx, _: &Db<Library>, _: ()) -> View<(i64, String)> {
    Ok(songs_by_pos().map(|s| (s.pos, s.title.clone())))
}

fn songs_by_pos() -> Plan<Library, Song> {
    scan::<Library, Song>().sort(|s| (s.pos, s.id.clone()))
}

/// Each work by title: how many movements it has, its title, its id.
fn works_with_counts() -> Plan<Library, (i64, String, String)> {
    scan::<Library, Work>().sort(|w| (w.title.clone(), w.id.clone())).map_with(|db, w| {
        (
            db.movement.by(Movement::WORK_ID).from(w.id.as_str()).count() as i64,
            w.title.clone(),
            w.id.clone(),
        )
    })
}

/// The works that have a movement: how many, and the title.
#[view("library.having_admits_on_a_child")]
pub fn having_admits_on_a_child(_: &Ctx, _: &Db<Library>, _: ()) -> View<(i64, String)> {
    Ok(works_with_counts().map(|x| (x.0, x.1.clone())).filter(|x| x.0 > 0))
}

/// How many songs each creator has, by creator.
#[view("library.group_source")]
pub fn group_source(_: &Ctx, _: &Db<Library>, _: ()) -> View<(String, i64)> {
    Ok(scan::<Library, Song>()
        .group(|s| s.creator.clone(), |_, k: &String, ms| (k.clone(), ms.len() as i64))
        .sort(|g| g.0.clone()))
}

/// Each song by place: its movement's part, its title, its work's title —
/// empty where there is none.
#[view("library.lookup_chain")]
pub fn lookup_chain(_: &Ctx, _: &Db<Library>, _: ()) -> View<(String, String, String)> {
    Ok(
        songs_by_pos().map_with(|db, s| match s.movement_id.as_ref().and_then(|m| db.movement.get(m.as_str())) {
            None => (String::new(), s.title.clone(), String::new()),
            Some(m) => {
                let work = db.work.get(m.work_id.as_str()).map_or(String::new(), |w| w.title);
                (m.part, s.title.clone(), work)
            }
        }),
    )
}

/// Each person: how many songs they made, and their name.
#[view("library.related_on_a_non_key_column")]
pub fn related_on_a_non_key_column(_: &Ctx, _: &Db<Library>, _: ()) -> View<(i64, String)> {
    Ok(scan::<Library, Person>().map_with(|db, p| (db.song.by(Song::CREATOR).from(p.name.as_str()).count() as i64, p.name.clone())))
}

/// The two works with the most movements, ties by id: how many, the title.
#[view("library.expression_order_under_a_limit")]
pub fn expression_order_under_a_limit(_: &Ctx, _: &Db<Library>, _: ()) -> View<(i64, String)> {
    Ok(works_with_counts().sort(|x| (Desc(x.0), x.2.clone())).take(2).map(|x| (x.0, x.1.clone())))
}

/// A movement, and its songs.
pub type Movements = (String, String, i64, String, Vec<Song>);
/// A work, and its movements.
pub type Works = (String, String, String, Vec<Movements>);

/// Each person, their works, each work's movements, each movement's songs.
#[view("library.related_three_deep")]
pub fn related_three_deep(_: &Ctx, _: &Db<Library>, _: ()) -> View<(String, i64, Vec<Works>)> {
    Ok(scan::<Library, Person>().map_with(|db, p| {
        let movement = |m: Movement| {
            let songs = db.song.by(Song::MOVEMENT_ID).from(Some(m.id.clone())).all();
            (m.id, m.work_id, m.no, m.part, songs)
        };
        let work = |w: Work| {
            let ms = db.movement.by(Movement::WORK_ID).from(w.id.as_str()).all();
            (w.id, w.composer, w.title, ms.into_iter().map(movement).collect())
        };
        let works = db.work.by(Work::COMPOSER).from(p.name.as_str()).all();
        (p.name.clone(), p.born, works.into_iter().map(work).collect())
    }))
}

/// Each playlist with each of its items.
#[view("library.playlist_join_item")]
pub fn playlist_join_item(_: &Ctx, _: &Db<Library>, _: ()) -> View<(Playlist, Item)> {
    Ok(scan::<Library, Playlist>().join(scan::<Library, Item>(), |p| p.id, |i| i.playlist_id))
}

/// Each person: the highest place among their tracks (-1 with none), and
/// their name.
#[view("library.kept_max")]
pub fn kept_max(_: &Ctx, _: &Db<Library>, _: ()) -> View<(i64, String)> {
    Ok(scan::<Library, Person>().map_with(|db, p| {
        let last = db.track.by(Track::CREATOR_POS).from(p.name.as_str()).last();
        (last.map_or(-1, |t| t.pos), p.name.clone())
    }))
}

/// The tracks whose title holds the needle, any case, by place: the place
/// and the title.
#[view("library.has_search")]
pub fn has_search(_: &Ctx, _: &Db<Library>, input: Needle) -> View<(i64, String)> {
    let needle = input.needle.to_lowercase();
    Ok(scan::<Library, Track>()
        .filter(move |t| t.title.to_lowercase().contains(&needle))
        .sort(|t| (t.pos, t.id.clone()))
        .map(|t| (t.pos, t.title.clone())))
}

/// The library, every view registered.
pub fn library() -> Domain {
    ark::register!(Domain::new::<Library>("library", "1");
        top_two_by_pos, playlist_with_items, projection, having_admits_on_a_child, group_source,
        lookup_chain, related_on_a_non_key_column, expression_order_under_a_limit,
        related_three_deep, playlist_join_item, kept_max, has_search)
}

/// A sum of integers taken whole and judged once (v4's §6.5 rule): an
/// `i64` overflow only where the total is one.
fn total(xs: impl Iterator<Item = i64>) -> i64 {
    i64::try_from(xs.map(i128::from).sum::<i128>()).expect("a group's sum fits")
}

// * `a-maintained-sum-answers-where-the-fold-overflows`

pub mod colors {
    use super::*;

    #[allow(non_camel_case_types)]
    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Describe)]
    pub enum Color {
        c0,
        c1,
        c2,
    }

    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
    #[ark(table = "t0", index(c02))]
    pub struct T0 {
        #[ark(key)]
        pub id: Id<T0>,
        pub c00: String,
        pub c01: Bytes,
        pub c02: Option<String>,
    }

    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
    #[ark(table = "t1")]
    pub struct T1 {
        #[ark(key)]
        pub k1: i64,
        pub c10: String,
        pub c11: String,
        pub c12: Option<Color>,
    }

    #[derive(Schema)]
    pub struct Colors {
        pub t0: T0,
        pub t1: T1,
    }

    /// A colour, how many rows have it, the sum of their `k1`.
    pub type Sum = (Option<Color>, i64, i64);

    /// Per colour, last first: the colour, how many, the sum of `k1`.
    #[view("colors.kept_sums")]
    pub fn kept_sums(_: &Ctx, _: &Db<Colors>, _: ()) -> Result<Plan<Colors, Sum>, Refusal> {
        Ok(scan::<Colors, T1>()
            .group(
                |r| r.c12.clone(),
                |_, k: &Option<Color>, ms| (k.clone(), ms.len() as i64, total(ms.map(|m| m.k1))),
            )
            .sort(|g| Desc(g.0.clone())))
    }

    pub fn domain() -> Domain {
        ark::register!(Domain::new::<Colors>("colors", "1"); kept_sums)
    }
}

// * `a-maintained-sum-overflows-where-the-fold-answers`

pub mod pairs {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
    #[ark(table = "t0", index(c00))]
    pub struct T0 {
        #[ark(key)]
        pub p0: String,
        #[ark(key)]
        pub n0: i64,
        pub c00: i64,
    }

    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
    #[ark(table = "t1")]
    pub struct T1 {
        #[ark(key)]
        pub p1: String,
        #[ark(key)]
        pub n1: i64,
        pub c10: Option<bool>,
        pub c11: bool,
        pub c12: i64,
        pub c13: Option<String>,
    }

    #[derive(Schema)]
    pub struct Pairs {
        pub t0: T0,
        pub t1: T1,
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
    pub struct Other {
        pub f_p0: String,
    }

    /// The rows of `t0` whose `p0` is not the input's, per `c00`: those of
    /// two or more, each its `c00`, how many, the sum of `n0`.
    #[view("pairs.kept_sums")]
    pub fn kept_sums(_: &Ctx, _: &Db<Pairs>, input: Other) -> Result<Plan<Pairs, (i64, i64, i64)>, Refusal> {
        Ok(scan::<Pairs, T0>()
            .filter(move |r| r.p0 != input.f_p0)
            .group(|r| r.c00, |_, k: &i64, ms| (*k, ms.len() as i64, total(ms.map(|m| m.n0))))
            .filter(|g| g.1 >= 2))
    }

    pub fn domain() -> Domain {
        ark::register!(Domain::new::<Pairs>("pairs", "1"); kept_sums)
    }
}

/// A reference domain by its name.
pub fn named(name: &str) -> Option<Domain> {
    match name {
        "library" => Some(library()),
        "colors" => Some(colors::domain()),
        "pairs" => Some(pairs::domain()),
        _ => None,
    }
}
