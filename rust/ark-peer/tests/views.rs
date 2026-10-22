//! A view a peer keeps is made again by the peer, under its number, when
//! it cannot be kept up (§1.10): no driver views it again.

use std::sync::Arc;

use ark::format::Encode;
use ark::prelude::*;
use ark::schema::Space;
use ark_dev::demo::{add_to_playlist, create_playlist, rename_playlist, AddToPlaylist, CreatePlaylist, Demo, Item, Playlist, RenamePlaylist};
use ark_peer::{Config, Memory, Peer, Update};

/// The items of the playlist of this name of the reader's: refused when
/// there is none, which is read as the view is made.
#[view("demo.named")]
pub fn named(ctx: &Ctx, db: &Db<Demo>, input: CreatePlaylist) -> Result<Plan<Demo, Item>, Refusal> {
    let key = (ctx.user().to_string(), input.name);
    let p: Playlist = db
        .playlist
        .by(Playlist::USER_ID_NAME)
        .from(key)
        .first()
        .ok_or_else(|| Refusal::new("no such playlist"))?;
    Ok(scan::<Demo, Item>().filter(move |i| i.playlist_id == p.id))
}

fn author(p: &mut Peer, name: &str, input: Vec<u8>) {
    p.author(Space::Durable, name, input, 1).unwrap();
}

/// A view whose making read a playlist that is renamed away cannot be
/// kept up: the peer makes it again, which refuses, and says `Reset`; the
/// playlist named so again, it is made again under the same number and
/// its rows are read. Falsified by `changed` dropping a view that cannot
/// be kept up, as it did: the second `Reset` never came and the view had
/// no rows.
#[test]
fn a_view_that_cannot_be_kept_up_is_made_again_under_its_number() {
    let domain = ark::register!(Domain::new::<Demo>("demo", "1"); create_playlist, add_to_playlist, rename_playlist, named);
    let mut p = Peer::open_as(Config::of(Arc::new(domain)), Box::new(Memory::default()), None);
    p.alone();
    author(&mut p, "demo.create_playlist", CreatePlaylist { name: "Road".into() }.to_bytes());
    let road = p.log.log().entries()[0].entry.id.draw(0);
    let track = AddToPlaylist {
        playlist_id: Id::new(road),
        track_id: "t1".into(),
    };
    author(&mut p, "demo.add_to_playlist", track.to_bytes());
    let v = p
        .watch(Space::Durable, "demo.named", &CreatePlaylist { name: "Road".into() }.to_bytes(), p.ctx())
        .unwrap();
    assert_eq!(p.values(v).map(|r| r.len()), Some(1));
    let rename = |name: &str| {
        RenamePlaylist {
            playlist_id: Id::new(road),
            name: name.into(),
        }
        .to_bytes()
    };
    author(&mut p, "demo.rename_playlist", rename("Sea"));
    assert_eq!((p.update(v), p.values(v)), (Update::Reset, None), "it cannot be made");
    author(&mut p, "demo.rename_playlist", rename("Road"));
    assert_eq!((p.update(v), p.values(v).map(|r| r.len())), (Update::Reset, Some(1)), "made again");
}
