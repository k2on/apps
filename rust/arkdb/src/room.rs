//! The demo's room (§1.9): live state over volatile tables of its own —
//! who is here, which `on_join` and `on_leave` write and `on_restore`
//! keeps, a name a device renames itself by, and a pointer each device
//! moves, relayed as ephemeral.

use std::collections::BTreeSet;

use ark::prelude::*;
use ark_peer::Config;

/// Who is here: what `on_join` writes and `on_leave` takes away.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
#[ark(volatile)]
pub struct Here {
    #[ark(key)]
    pub session: String,
    pub name: String,
    pub since: i64,
}

/// A pointer a device moves, relayed as ephemeral.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Row)]
#[ark(volatile)]
pub struct Pointer {
    #[ark(key)]
    pub session: String,
    pub x: i64,
}

#[derive(Schema)]
pub struct Room {
    pub here: Here,
    pub pointer: Pointer,
}

/// A device as its `Hello` says it, and `on_join` and `on_leave` are told
/// of it, as its login: its session is the login's, whatever it says.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct Arrival {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct Point {
    pub x: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
pub struct Rename {
    pub name: String,
}

#[mutator("room.on_join")]
pub fn on_join(ctx: &Ctx, db: &mut Db<Room, Volatile>, a: Arrival) -> Result<(), String> {
    db.here.delete(ctx.device())?;
    db.here.insert(Here {
        session: ctx.device().into(),
        name: a.name,
        since: ctx.now,
    })
}

#[mutator("room.on_leave")]
pub fn on_leave(ctx: &Ctx, db: &mut Db<Room, Volatile>, _: Arrival) -> Result<(), String> {
    db.pointer.delete(ctx.device())?;
    db.here.delete(ctx.device())
}

/// After a restart: who was here is kept, every pointer let go.
#[mutator("room.on_restore")]
pub fn on_restore(_: &Ctx, db: &mut Db<Room, Volatile>, _: ()) -> Result<(), String> {
    for p in db.pointer.scan() {
        db.pointer.remove(&p)?;
    }
    Ok(())
}

#[mutator("room.point")]
pub fn point(ctx: &Ctx, db: &mut Db<Room, Volatile>, input: Point) -> Result<(), String> {
    db.pointer.delete(ctx.device())?;
    db.pointer.insert(Pointer {
        session: ctx.device().into(),
        x: input.x,
    })
}

#[mutator("room.rename")]
pub fn rename(ctx: &Ctx, db: &mut Db<Room, Volatile>, input: Rename) -> Result<(), String> {
    let one = db.here.get(ctx.device()).ok_or_else(|| String::from("not here"))?;
    db.here.update(Here { name: input.name, ..one })
}

#[view("room.present")]
pub fn present(_: &Ctx, _: &Db<Room>, _: ()) -> Result<Plan<Room, Here>, String> {
    Ok(scan::<Room, Here>().filter(|h| h.since >= 0))
}

/// The room as a connection the demo's scope feeds holds it (§1.8, §1.9:
/// the live space is fed by the log's rules, its scope of the same name):
/// who is here, and of the pointers its own.
#[scope("demo.top_two")]
pub fn room_top_two() -> Scope<Room> {
    Scope::new().on::<Here>(|_, _| true).on::<Pointer>(|ctx, p| p.session == ctx.device())
}

/// The room, every function registered.
pub fn room() -> Domain {
    ark::functions!(Domain::new::<Room>("room", "1"))
}

/// The room as a client of it is configured: the device it says it is,
/// `config` with the room as its live space: a device's, named `name`,
/// relaying its pointer at most every `every_ms`.
pub fn device(config: Config, name: &str, every_ms: i64) -> Config {
    Config {
        device: Some(Arrival { name: name.into() }.to_bytes()),
        ephemeral: BTreeSet::from(["room.point".to_string()]),
        every_ms,
        ..hub(config)
    }
}

/// `config` with the room as its live space: a server's, authoring its
/// `on_join`, `on_leave` and `on_restore`.
pub fn hub(config: Config) -> Config {
    Config {
        live: Some(std::sync::Arc::new(room())),
        ..config
    }
}
