//! The demo of `spec/AUTHORING.md` written with the Rust builders: the four
//! mutators of `rust/ark/tests/demo.rs`, byte for byte (so its pinned
//! states hold), a query of a playlist's items and a scope; and a room of
//! live state over volatile tables of its own (§1.9): who is here, which
//! `on_join` and `on_leave` write and `on_restore` keeps, a name a device
//! renames itself by, and a pointer each device moves, relayed as
//! ephemeral.

use std::collections::BTreeSet;

use ark::author::*;
use ark_client::{Live, Module};

#[derive(Row)]
#[ark(unique(user_id, name))]
pub struct Playlist {
    #[ark(key)]
    pub id: Id<Playlist>,
    pub name: Text,
    pub user_id: Text,
}

#[derive(Row)]
#[ark(table = "item", unique(playlist_id, pos))]
pub struct Item {
    #[ark(key, refs = Playlist)]
    pub playlist_id: Id<Playlist>,
    #[ark(key)]
    pub track_id: Text,
    pub pos: Int,
}

#[derive(Schema)]
pub struct Demo {
    pub playlist: Table<Playlist>,
    pub item: Table<Item>,
}

#[derive(Input)]
#[ark(trim, non_empty, why = "a playlist needs a name")]
pub struct Name(pub Text);

#[derive(Input)]
#[ark(len = "1..")]
pub struct TrackId(pub Text);

/// A playlist made, its id the entry's first draw; nothing when the person
/// has one of that name.
pub static CREATE_PLAYLIST: Def1<Name, ()> = Def1 {
    name: "demo.create_playlist",
    kind: Kind::Mutator,
    body: |name| {
        let db = Demo::db();
        let mine = db.playlist.filter(|p| p.f().user_id.eq(Ctx.user()).and(p.f().name.eq(&name)));
        let row = PlaylistEx {
            id: Ctx.new_id::<Playlist>(),
            name,
            user_id: Ctx.user(),
        };
        if_(mine.len().eq(0), db.playlist.insert(row), unit())
    },
};

/// A track put on a playlist, at one past its last `pos`; nothing when it
/// is on it already.
pub static ADD_TO_PLAYLIST: Def2<Id<Playlist>, TrackId, ()> = Def2 {
    name: "demo.add_to_playlist",
    kind: Kind::Mutator,
    body: |pid, tid| {
        let db = Demo::db();
        let last = db.item.filter(|r| r.f().playlist_id.eq(&pid)).sort(|r| desc(r.f().pos)).take(1).first();
        let add = let_(last, |last| {
            let pos = last.map_or(0, |r| r.f().pos) + 1;
            db.item.insert(ItemEx {
                playlist_id: pid.clone(),
                track_id: tid.clone(),
                pos,
            })
        });
        let on = db.item.get((pid.clone(), tid.clone())).is_some();
        if_(
            db.playlist.get(&pid).is_some(),
            if_(on, unit(), add),
            refuse("playlist_id: no such playlist"),
        )
    },
};

pub static RENAME_PLAYLIST: Def2<Id<Playlist>, Name, ()> = Def2 {
    name: "demo.rename_playlist",
    kind: Kind::Mutator,
    body: |id, name| {
        let db = Demo::db();
        let update = db.playlist.update(&id, |p| PlaylistEx { name, ..p.f() }.into());
        if_(db.playlist.get(&id).is_some(), update, refuse("no such playlist"))
    },
};

pub static REMOVE_FROM_PLAYLIST: Def2<Id<Playlist>, Text, ()> = Def2 {
    name: "demo.remove_from_playlist",
    kind: Kind::Mutator,
    body: |pid, tid| Demo::db().item.delete((pid, tid)),
};

/// A playlist's items in order.
pub static ITEMS: Def1<Id<Playlist>, Vec<Item>> = Def1 {
    name: "demo.items",
    kind: Kind::Query,
    body: |pid| Demo::db().item.filter(|i| i.f().playlist_id.eq(&pid)).sort(|i| i.f().pos),
};

/// Every playlist, and the first two places of each.
pub static TOP_TWO: Def1<DemoRow, bool> = Def1 {
    name: "demo.top_two",
    kind: Kind::Scope,
    body: |row| DemoRowEx::match_(&row, |_| bool(true), |i| i.f().pos.le(2)),
};

pub fn domain() -> ark::Domain {
    let fns: [&'static dyn Fun; 6] = [
        &CREATE_PLAYLIST,
        &ADD_TO_PLAYLIST,
        &RENAME_PLAYLIST,
        &REMOVE_FROM_PLAYLIST,
        &ITEMS,
        &TOP_TWO,
    ];
    assemble(Demo::schema(), &fns).unwrap_or_else(|r| panic!("the demo: {r}"))
}

// * The room

#[derive(Row)]
#[ark(volatile)]
pub struct Here {
    #[ark(key)]
    pub session: Text,
    pub name: Text,
    pub since: Int,
}

#[derive(Row)]
#[ark(volatile)]
pub struct Pointer {
    #[ark(key)]
    pub session: Text,
    pub x: Int,
}

#[derive(Schema)]
pub struct Room {
    pub here: Table<Here>,
    pub pointer: Table<Pointer>,
}

/// A device as `on_join` and `on_leave` are told of it.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct Arrival {
    pub session: Text,
    pub name: Text,
}

pub static ON_JOIN: Def1<Arrival, ()> = Def1 {
    name: "room.on_join",
    kind: Kind::Live,
    body: |a| {
        let db = Room::db();
        let here = HereEx {
            session: a.f().session,
            name: a.f().name,
            since: Ctx.now(),
        };
        seq([db.here.delete(a.f().session)], db.here.insert(here))
    },
};

pub static ON_LEAVE: Def1<Arrival, ()> = Def1 {
    name: "room.on_leave",
    kind: Kind::Live,
    body: |a| {
        let db = Room::db();
        seq([db.pointer.delete(a.f().session)], db.here.delete(a.f().session))
    },
};

pub static RENAME: Def1<Text, ()> = Def1 {
    name: "room.rename",
    kind: Kind::Live,
    body: |name| {
        let db = Room::db();
        let update = db.here.update(Ctx.session(), |h| HereEx { name, ..h.f() }.into());
        if_(db.here.get(Ctx.session()).is_some(), update, refuse_("not here"))
    },
};

pub static POINT: Def1<Int, ()> = Def1 {
    name: "room.point",
    kind: Kind::Live,
    body: |x| {
        let db = Room::db();
        seq(
            [db.pointer.delete(Ctx.session())],
            db.pointer.insert(PointerEx { session: Ctx.session(), x }),
        )
    },
};

/// After a restart: who was here is kept, every pointer let go.
pub static ON_RESTORE: Def0<()> = Def0 {
    name: "room.on_restore",
    kind: Kind::Live,
    body: || {
        let db = Room::db();
        db.pointer.map(|p| db.pointer.delete(p.f().session)).s()
    },
};

pub static PRESENT: Def0<Vec<Here>> = Def0 {
    name: "room.present",
    kind: Kind::Query,
    body: || Room::db().here.filter(|h| h.f().since.ge(0)),
};

pub fn room() -> ark::Domain {
    let fns: [&'static dyn Fun; 6] = [&ON_JOIN, &ON_LEAVE, &ON_RESTORE, &RENAME, &POINT, &PRESENT];
    assemble(Room::schema(), &fns).unwrap_or_else(|r| panic!("the room: {r}"))
}

/// A domain as a client or a hub holds it.
pub fn module(d: &ark::Domain) -> Module {
    Module {
        schema: d.schema.clone(),
        cas: d.cas(),
        lib: d.lib.clone(),
    }
}

/// The room as a client of it is configured: the device it says it is,
/// its name; `point` relayed every `every_ms`.
pub fn live(name: &str, every_ms: i64) -> Live {
    let r = room();
    Live {
        module: module(&r),
        device: Some(name.as_bytes().to_vec()),
        ephemeral: BTreeSet::from([r.get("room.point")]),
        every_ms,
    }
}

/// What a device's `Hello` says, made the input of `on_join`: its session
/// is the login's, whatever it says; its name is what it says.
pub fn arrival(who: &ark::peer::Who, said: &[u8]) -> Option<Vec<u8>> {
    let name = String::from_utf8(said.to_vec()).ok()?;
    Some(
        Arrival {
            session: who.session.clone(),
            name,
        }
        .to_bytes(),
    )
}
