//! The demo of `spec/AUTHORING.md` Appendix B, its mutators written by hand
//! as `Procedure`s until the interpreter (L1) runs them from their IR — and
//! the vectors these tests pin must not move when it does.
//!
//! `playlist(id: Id<playlist>, name: Text, user_id: Text)` keyed by `id`,
//! unique `(user_id, name)`; `item(playlist_id: Id<playlist> → playlist,
//! track_id: Text, pos: Int)` keyed by `(playlist_id, track_id)`, unique
//! `(playlist_id, pos)`. The mutators: `create_playlist(name)`, the name
//! trimmed and at least one character ("a playlist needs a name"), its id
//! the entry's first draw; `add_to_playlist(playlist_id, track_id)`, the
//! playlist existing and the track at least one character, at one past the
//! playlist's last `pos`. Beside them, for the rebase's other change kinds,
//! `rename_playlist(id, name)` (an `Edit`) and `remove_from_playlist(id,
//! track_id)` (a `Remove`); and dave's wrong build of `add_to_playlist`,
//! which steps by two.
//!
//! Included by the other tests with `#[path]`; it has no tests of its own.

#![allow(dead_code)]

use std::sync::Arc;

use ark5::codec::{bytes_enc, parse, put_bytes, put_int, Reader};
use ark5::hash::FnHash;
use ark5::peer::{Cas, Ctx, Procedure};
use ark5::schema::{Schema, Table};
use ark5::store::{Change, Dir, Read, Scan};
use ark5::ty::Ty;
use ark5::Refusal;

pub fn schema() -> Schema {
    let id = || Ty::Bytes;
    Schema {
        tables: vec![
            Table::new("playlist")
                .col("id", "Id<playlist>", id())
                .col("name", "Text", Ty::Bytes)
                .col("user_id", "Text", Ty::Bytes)
                .index(&["user_id", "name"], true),
            Table::new("item")
                .col("playlist_id", "Id<playlist>", id())
                .col("track_id", "Text", Ty::Bytes)
                .col("pos", "Int", Ty::Int)
                .keyed(2)
                .index(&["playlist_id", "pos"], true)
                .refs("playlist_id", "playlist"),
        ],
    }
}

pub fn shared() -> Arc<Schema> {
    Arc::new(schema())
}

pub fn playlist_row(id: &[u8], name: &str, user: &str) -> Vec<u8> {
    let mut b = bytes_enc(id);
    put_bytes(&mut b, name.as_bytes());
    put_bytes(&mut b, user.as_bytes());
    b
}

pub fn item_row(pid: &[u8], track: &str, pos: i64) -> Vec<u8> {
    let mut b = bytes_enc(pid);
    put_bytes(&mut b, track.as_bytes());
    put_int(&mut b, pos);
    b
}

/// An `item` row read back: `(playlist_id, track_id, pos)`.
pub fn item_of(row: &[u8]) -> (Vec<u8>, String, i64) {
    let mut r = Reader::new(row);
    (r.bytes().unwrap(), r.text().unwrap(), r.int().unwrap())
}

/// The `pos` of a track on a playlist, as a store holds it.
pub fn pos_of(store: &dyn Read, pid: &[u8], track: &str) -> Option<i64> {
    let mut key = bytes_enc(pid);
    put_bytes(&mut key, track.as_bytes());
    store.get("item", &key).map(|r| item_of(&r).2)
}

fn texts(input: &[u8], n: usize) -> Result<Vec<String>, Refusal> {
    parse(&Ty::Tuple(vec![Ty::Bytes; n]), input)?;
    let mut r = Reader::new(input);
    (0..n).map(|_| r.text()).collect()
}

fn id_and_text(input: &[u8]) -> Result<(Vec<u8>, String), Refusal> {
    parse(&Ty::Tuple(vec![Ty::Bytes, Ty::Bytes]), input)?;
    let mut r = Reader::new(input);
    Ok((r.bytes()?, r.text()?))
}

pub fn create_input(name: &str) -> Vec<u8> {
    bytes_enc(name.as_bytes())
}

pub fn pair_input(id: &[u8], text: &str) -> Vec<u8> {
    let mut b = bytes_enc(id);
    put_bytes(&mut b, text.as_bytes());
    b
}

pub struct CreatePlaylist;
impl Procedure for CreatePlaylist {
    fn run(&self, _: &dyn Read, ctx: &Ctx, input: &[u8]) -> Result<Vec<Change>, Refusal> {
        let name = texts(input, 1)?.remove(0);
        let name = name.trim();
        if name.is_empty() {
            return Err(Refusal::new("a playlist needs a name"));
        }
        let row = playlist_row(&ctx.new_id(0), name, &ctx.actor);
        Ok(vec![Change::Add { table: "playlist".into(), row }])
    }
}

/// `add_to_playlist`, stepping `pos` by `step`: 1 is the demo's, 2 dave's.
pub struct AddToPlaylist(pub i64);
impl Procedure for AddToPlaylist {
    fn run(&self, store: &dyn Read, _: &Ctx, input: &[u8]) -> Result<Vec<Change>, Refusal> {
        let (pid, track) = id_and_text(input)?;
        if store.get("playlist", &bytes_enc(&pid)).is_none() {
            return Err(Refusal::new("playlist_id: no such playlist"));
        }
        if track.is_empty() {
            return Err(Refusal::new("track_id: at least 1 character"));
        }
        let last = store.scan("item", &Scan::prefix(Some(0), &bytes_enc(&pid), Dir::Desc, 1));
        let pos = last.first().map_or(0, |r| item_of(r).2).checked_add(self.0).ok_or_else(|| Refusal::new("pos overflows"))?;
        Ok(vec![Change::Add {
            table: "item".into(),
            row: item_row(&pid, &track, pos),
        }])
    }
}

pub struct RenamePlaylist;
impl Procedure for RenamePlaylist {
    fn run(&self, store: &dyn Read, _: &Ctx, input: &[u8]) -> Result<Vec<Change>, Refusal> {
        let (id, name) = id_and_text(input)?;
        let old = store.get("playlist", &bytes_enc(&id)).ok_or_else(|| Refusal::new("no such playlist"))?;
        let mut r = Reader::new(&old);
        let (_, _, user) = (r.bytes()?, r.text()?, r.text()?);
        let new = playlist_row(&id, name.trim(), &user);
        Ok(vec![Change::Edit { table: "playlist".into(), old, new }])
    }
}

pub struct RemoveFromPlaylist;
impl Procedure for RemoveFromPlaylist {
    fn run(&self, store: &dyn Read, _: &Ctx, input: &[u8]) -> Result<Vec<Change>, Refusal> {
        let (pid, track) = id_and_text(input)?;
        let mut key = bytes_enc(&pid);
        put_bytes(&mut key, track.as_bytes());
        Ok(store.get("item", &key).map(|row| Change::Remove { table: "item".into(), row }).into_iter().collect())
    }
}

/// A hand-written function's hash: of its name, standing for its bytes
/// until the IR has some (which is what a body sent in a frame is here).
pub fn h(name: &str) -> FnHash {
    FnHash::of(name.as_bytes())
}

pub const CREATE: &str = "demo.create_playlist";
pub const ADD: &str = "demo.add_to_playlist";
pub const RENAME: &str = "demo.rename_playlist";
pub const REMOVE: &str = "demo.remove_from_playlist";

pub fn procedure(name: &str) -> Option<Arc<dyn Procedure>> {
    Some(match name {
        CREATE => Arc::new(CreatePlaylist),
        ADD => Arc::new(AddToPlaylist(1)),
        RENAME => Arc::new(RenamePlaylist),
        REMOVE => Arc::new(RemoveFromPlaylist),
        _ => return None,
    })
}

/// Every demo function, as a peer that authors all of them holds them.
pub fn cas() -> Cas {
    let mut c = Cas::default();
    for n in [CREATE, ADD, RENAME, REMOVE] {
        c.insert(h(n), procedure(n).unwrap());
    }
    c
}

/// Dave's build: `add_to_playlist`'s hash, stepping by two.
pub fn daves_cas() -> Cas {
    let mut c = cas();
    c.insert(h(ADD), Arc::new(AddToPlaylist(2)));
    c
}
