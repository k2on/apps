//! The demo domain of `spec/AUTHORING.md` Appendix B, which every vector
//! is over: `playlist(id, name, user_id)` and `item(playlist_id →
//! playlist, track_id, pos)`, the mutators `create_playlist` and
//! `add_to_playlist`, the query `items`.
//!
//! A copy, of `rust/ark-client/src/demo.rs` and
//! `rust/ark/tests/demo_authoring.rs`: this crate cannot depend on
//! `ark-client`, and a binary cannot reach a test file. The copies are held
//! together by the vectors themselves — `module/demo.json` is what this one
//! emits, and `the_demo_emits_what_the_spec_records` holds the test's copy
//! to those bytes.

// The row structs are the vocabulary's: a body reads their fields only
// natively, and the generator only ever emits.
#![allow(dead_code)]

use ark::authoring::*;
use ark::hash::FnHash;
use ark::value::Id as IdBytes;

pub struct Demo {
    pub playlist: Table<Playlist>,
    pub item: Table<Item>,
}
impl Tables for Demo {
    fn open() -> Self {
        Demo {
            playlist: table(),
            item: table(),
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
    pub const item: Rel<Self, Item> = rel("item");
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

pub struct CreatePlaylist {
    pub name: Text,
}
impl Input for CreatePlaylist {
    fn schema() -> Object<Self> {
        object().field("name", text().trim().min(1).why("a playlist needs a name"))
    }
}

pub struct AddToPlaylist {
    pub playlist_id: Id<Playlist>,
    pub track_id: Text,
}
impl Input for AddToPlaylist {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>().exists()).field("track_id", text().min(1))
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

pub fn demo() -> Router<Demo> {
    let demo = router::<Demo>("demo");
    demo.routes((
        demo.input::<CreatePlaylist>().mutation("create_playlist", |ctx, db, input| {
            db.playlist
                .insert(Playlist {
                    id: ctx.new_id("id"),
                    name: input.name,
                    user_id: ctx.user,
                })
                .on((Playlist::user_id, Playlist::name))
        }),
        demo.input::<AddToPlaylist>().mutation("add_to_playlist", |_ctx, db, input| {
            let last = db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.desc()).first();
            db.item.insert(Item {
                playlist_id: input.playlist_id,
                track_id: input.track_id,
                pos: last.map_or(0, |row| row.pos).add(1),
            })
        }),
        demo.input::<PlaylistId>().query("items", |_ctx, db, input| {
            db.item.filter(Item::playlist_id.eq(input.playlist_id)).order_by(Item::pos.asc())
        }),
    ))
}

pub struct Feature {
    pub name: Text,
}
impl Input for Feature {
    fn schema() -> Object<Self> {
        object().field("name", text().trim().min(1))
    }
}

/// A second router over the demo's tables whose one mutation is guarded by
/// a role (`docs/plan-guards.md` D1): `is_curator` refuses an author who
/// does not hold `curator`, and `feature`'s body reads `editor` to decide
/// whose the playlist is. What `module/guarded.json` and
/// `eval/has-role.json` are over; the demo's own module, and every vector
/// of it, is untouched.
pub fn curated() -> Router<Demo> {
    let curated = router::<Demo>("curated");
    let is_curator = curated.guard("is_curator", |ctx, _db| {
        unless(ctx.has_role("curator"), || refuse("only a curator features a playlist"))
    });
    curated.routes((is_curator.input::<Feature>().mutation("feature", |ctx, db, input| {
        db.playlist
            .insert(Playlist {
                id: ctx.new_id("id"),
                name: input.name,
                user_id: pick(ctx.has_role("editor"), "editors", "curators"),
            })
            .on((Playlist::user_id, Playlist::name))
    }),))
}

/// A router over the demo's tables that says what each person holds
/// (`docs/plan-guards.md` D2): on the router, `mine` — a person's own
/// playlists, every playlist for a curator, and a playlist that has the
/// person's name among its tracks (the `exists` form) — and on a chain,
/// `tracks` — every item, without its position. Two queries, one over each.
/// What `module/scoped.json` and the D2 protocol vectors are over.
pub fn shared() -> Router<Demo> {
    let shared = router::<Demo>("shared");
    let mine = shared.server("mine", |ctx, db| {
        db.playlist.filter(
            Playlist::user_id
                .eq(ctx.user)
                .or(Pred::when(ctx.has_role("curator")))
                .or(exists(Item::playlist_id, Item::track_id.eq(ctx.user))),
        )
    });
    let tracks = mine.server("tracks", |_ctx, db| db.item.rows().exclude(Item::pos));
    shared.routes((
        mine.client("playlists", |_ctx, db, ()| db.playlist.rows().order_by(Playlist::name.asc())),
        tracks
            .input::<PlaylistId>()
            .client("tracks_of", |_ctx, db, input| db.item.filter(Item::playlist_id.eq(input.playlist_id))),
    ))
}

/// A router over the demo's tables whose one mutation has a server half
/// (`docs/plan-guards.md` D3): `create_audited` makes a playlist, as
/// `create_playlist` does, and its private block — run by the authority
/// alone, after the body — files an `audit` item on it, or refuses the name
/// `forbidden`. What `module/private.json` and the D3 protocol vectors are
/// over.
pub fn audited() -> Router<Demo> {
    let audited = router::<Demo>("audited");
    audited.routes((audited.input::<CreatePlaylist>().mutation("create_audited", |ctx, db, input| {
        let id: Id<Playlist> = ctx.new_id("id");
        let name = input.name;
        ctx.private(move |db: &Demo| {
            when(name.eq("forbidden"), || refuse("the server keeps that name"));
            db.item.insert(Item {
                playlist_id: id,
                track_id: "audit".into(),
                pos: 0.into(),
            })
        });
        db.playlist
            .insert(Playlist { id, name, user_id: ctx.user })
            .on((Playlist::user_id, Playlist::name))
    }),))
}

/// The demo with [`audited`] beside it, verified: the server's module,
/// private block and all.
pub fn private() -> ark::ir::Module {
    Module::new((demo(), audited())).build().clone()
}

/// [`shared`] alone, verified.
pub fn scoped() -> ark::ir::Module {
    Module::new((shared(),)).build().clone()
}

/// The demo with [`curated`] beside it, verified.
pub fn guarded() -> ark::ir::Module {
    Module::new((demo(), curated())).build().clone()
}

/// The module, verified: orders completed and every function normalised,
/// which is the form every hash is of.
pub fn module() -> ark::ir::Module {
    Module::new((demo(),)).build().clone()
}

/// The id whose last byte is `k` and every other byte zero: the ids the
/// scripted vectors use.
pub fn id_n(k: u8) -> IdBytes {
    let mut id = [0u8; 16];
    id[15] = k;
    id
}

/// The hash of a function's closure, by its name.
pub fn hash_of(m: &ark::ir::Module, name: &str) -> FnHash {
    ark::hash::closures(m)
        .into_iter()
        .find(|(_, c)| c.function.name == name)
        .map(|(h, _)| h)
        .unwrap_or_else(|| panic!("the demo has no function {name}"))
}
