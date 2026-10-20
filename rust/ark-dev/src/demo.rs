//! The demo of `spec/AUTHORING.md`, in ArkLang: `demo.ark`, the four
//! mutators of `rust/ark/tests/demo.rs` (so its pinned states hold), a
//! query of a playlist's items and a scope; and `room.ark`, a room of live
//! state over volatile tables of its own (§1.9): who is here, which
//! `on_join` and `on_leave` write and `on_restore` keeps, a name a device
//! renames itself by, and a pointer each device moves, relayed as
//! ephemeral.

use std::collections::BTreeSet;

use ark::codec::bytes_enc;
use ark_client::{Live, Module};

/// The demo's text.
pub const DEMO: &str = include_str!("demo.ark");

/// The room's text.
pub const ROOM: &str = include_str!("room.ark");

/// The demo, built from [`DEMO`].
pub fn domain() -> ark::Domain {
    ark::sexp::build(DEMO).unwrap_or_else(|r| panic!("the demo: {r}"))
}

/// The room, built from [`ROOM`].
pub fn room() -> ark::Domain {
    ark::sexp::build(ROOM).unwrap_or_else(|r| panic!("the room: {r}"))
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
    Some([bytes_enc(who.session.as_bytes()), bytes_enc(name.as_bytes())].concat())
}
