//! A view a peer keeps is made again by the peer, under its number, when
//! it cannot be kept up (§1.10): no driver views it again.

use std::sync::Arc;

use ark::format::Encode;
use ark::prelude::*;
use ark::schema::Space;
use ark_peer::{Config, Memory, Patch, Peer};
use arkdb::demo::{AddToPlaylist, CreatePlaylist, Demo, Item, Playlist, RenamePlaylist};

/// The items of the playlist of this name of the reader's: refused when
/// there is none, which is read as the view is made.
pub fn named(ctx: &Ctx, db: &Db<Demo>, input: CreatePlaylist) -> Result<Plan<Demo, Item>, String> {
    let key = (ctx.user().to_string(), input.name);
    let p: Playlist = db
        .playlist
        .by(Playlist::USER_ID_NAME)
        .from(key)
        .first()
        .ok_or_else(|| String::from("no such playlist"))?;
    Ok(scan::<Demo, Item>().filter(move |i| i.playlist_id == p.id))
}

fn author(p: &mut Peer, name: &str, input: Vec<u8>) {
    p.mutate_at(name, input, 1).unwrap();
}

/// A view whose making read a playlist that is renamed away cannot be
/// kept up: the peer makes it again, which refuses, and says its row
/// removed; the playlist named so again, it is made again under the same
/// number and says its row inserted. Falsified by `changed` dropping a view
/// that cannot be kept up, as it did: the insert never came and the view
/// had no rows.
#[test]
fn a_view_that_cannot_be_kept_up_is_made_again_under_its_number() {
    let domain = arkdb::demo::demo().view("demo.named", named);
    let mut p = Peer::open_as(Config::of(Arc::new(domain)), Box::new(Memory::default()), None);
    author(&mut p, "demo.create_playlist", CreatePlaylist { name: "Road".into() }.to_bytes());
    let road = p.log.pending()[0].entry.id.draw(0);
    let track = AddToPlaylist {
        playlist_id: Id::new(road),
        track_id: "t1".into(),
    };
    author(&mut p, "demo.add_to_playlist", track.to_bytes());
    let v = p
        .watch(Space::Durable, "demo.named", &CreatePlaylist { name: "Road".into() }.to_bytes(), p.ctx())
        .unwrap();
    assert_eq!(p.rows(v).map(|r| r.len()), Some(1));
    let rename = |name: &str| {
        RenamePlaylist {
            playlist_id: Id::new(road),
            name: name.into(),
        }
        .to_bytes()
    };
    author(&mut p, "demo.rename_playlist", rename("Sea"));
    assert_eq!((p.update(v), p.rows(v)), (vec![Patch::Remove { at: 0 }], None), "it cannot be made");
    author(&mut p, "demo.rename_playlist", rename("Road"));
    let (again, rows) = (p.update(v), p.rows(v).unwrap_or_default());
    assert_eq!(
        again,
        vec![Patch::Insert {
            at: 0,
            node: rows[0].clone()
        }],
        "made again"
    );
}

/// A UI's view (`Watched`) keeps its rows as its own type: refused at open
/// it is no rows, made again when anything moved; once it answers its rows
/// are inserted, then patched; and the peer, which keeps it for whoever is
/// signed in, makes it again for the next login, its rows removed and read
/// afresh — alice's intents, pending still, are bob's now, authored again
/// from his origin, so his Road is another. Falsified by a sign-in that
/// does not make the views again (`rewatch` taken out of `sign_in`): the
/// view, still alice's, held none of bob's Road's tracks.
#[test]
fn a_watched_view_keeps_its_rows_typed_and_follows_the_login() {
    let domain = arkdb::demo::demo().view("demo.named", named);
    let mut p = Peer::open_as(Config::of(Arc::new(domain)), Box::new(Memory::default()), None);
    p.sign_in(ark::peer::Ctx::who("alice", "a", vec![]));
    let road = CreatePlaylist { name: "Road".into() };
    let mut w: ark_peer::Watched<Item> = ark_peer::Watched::open(&mut p, ark_peer::Func::new("demo.named").on(&road));
    assert_eq!((p.rows(w.view()), w.rows.len()), (None, 0), "refused: no rows");
    author(&mut p, "demo.create_playlist", road.to_bytes());
    let id = Id::new(p.log.pending()[0].entry.id.draw(0));
    let add = |t: &str| AddToPlaylist {
        playlist_id: id,
        track_id: t.into(),
    };
    author(&mut p, "demo.add_to_playlist", add("t1").to_bytes());
    assert!(
        matches!(&w.settle(&mut p)[..], [Patch::Insert { at: 0, .. }]),
        "it answers now: its row inserted"
    );
    author(&mut p, "demo.add_to_playlist", add("t2").to_bytes());
    assert!(!w.settle(&mut p).is_empty());
    let tracks: Vec<&str> = w.rows.iter().map(|i| i.track_id.as_str()).collect();
    assert_eq!(tracks, ["t1", "t2"]);
    p.sign_in(ark::peer::Ctx::who("bob", "b", vec![]));
    let gone = vec![Patch::Remove { at: 1 }, Patch::Remove { at: 0 }];
    assert_eq!(w.settle(&mut p), gone, "opened again for bob: alice's rows removed");
    // His Road is drawn from his origin; alice's adds name hers.
    assert!(w.rows.is_empty(), "bob's Road has no tracks");
    let his = Id::new(p.log.pending()[0].entry.id.draw(0));
    let t3 = AddToPlaylist {
        playlist_id: his,
        track_id: "t3".into(),
    };
    author(&mut p, "demo.add_to_playlist", t3.to_bytes());
    w.settle(&mut p);
    let tracks: Vec<&str> = w.rows.iter().map(|i| i.track_id.as_str()).collect();
    assert_eq!(tracks, ["t3"], "the view is bob's: his Road's tracks");
}
