//! The domain against a simulated fleet (`harken/domain/tests/
//! converge.rs`): one authority and clients, every one the one peer
//! machine (§1.7), over the simulator's seeded network that reorders,
//! duplicates and drops — no sockets, no threads, no sleeps, and a seed
//! that reproduces the run exactly. Every peer runs the domain through the
//! interpreter; converging is one chain and one state on every peer.

mod common;
mod support {
    pub mod fleet;
}

use std::collections::BTreeMap;

use ark::ir::Node;
use ark::log::EntryId;
use ark::peer::{Ctx, Who};
use common::{domain, full, song};
use harken_domain::domain::host::LIBRARY;
use harken_domain::domain::Id;
use harken_domain::domain::*;
use support::fleet::{who, Net};

struct Fleet {
    net: Net,
    now: i64,
}

impl Fleet {
    fn new(seed: u64, logins: Vec<Who>) -> Fleet {
        Fleet {
            net: Net::new(domain().schema.clone(), domain().cas(), logins, seed),
            now: 1_000,
        }
    }

    /// Author on client `i` as whoever it believes it is.
    fn mutate(&mut self, i: i64, name: &str, input: &impl Node) {
        self.now += 1;
        self.net.mutate(i, domain().get(&full(name)), input.to_bytes(), self.now);
    }

    /// What client `i` shows, as `user`: its optimistic view.
    fn query<T: Node>(&self, i: i64, user: &str, name: &str, input: &impl Node) -> T {
        let ctx = Ctx {
            id: EntryId::of("q", 0),
            actor: user.into(),
            session: "q".into(),
            roles: vec![],
            now: 0,
        };
        let b = domain()
            .ask(&self.net.clients[&i].store, &ctx, &full(name), &input.to_bytes())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        T::from_bytes(&b).unwrap()
    }

    fn converged(&self) {
        self.net.converged().unwrap_or_else(|e| panic!("{e}"));
    }
}

fn track(title: &str) -> AddSong {
    song(title, "someone", "")
}

/// Three peers, one of them peer-0's second device, adding songs through
/// a partition, and both of peer-0's devices putting the whole library on
/// one playlist while apart: after settling, one chain and one state, 17
/// songs at positions 1..17 and 17 items on the playlist, each once, at
/// 1..17 — the positions recomputed by replay rather than merged.
/// Falsified by `add_to_playlist` not skipping an item already there: the
/// second device's adds were refused by the server.
#[test]
fn the_domain_converges_when_peers_go_dark_and_come_back() {
    let login = |i: i64| {
        let user = if i == 2 { "peer-0".to_string() } else { format!("peer-{i}") };
        who(&user, &format!("dev-{i}"), &[LIBRARY])
    };
    let mut f = Fleet::new(19, (0..3).map(login).collect());
    for round in 0..3 {
        for i in 0..3 {
            f.mutate(i, "add_song", &track(&format!("c{i}-{round}")));
            f.net.step();
        }
    }
    f.net.partition(2);
    for round in 0..4 {
        f.mutate(2, "add_song", &track(&format!("dark-{round}")));
        f.mutate(0, "add_song", &track(&format!("lit-{round}")));
        f.net.step();
    }
    f.mutate(0, "create_playlist", &CreatePlaylist { name: "Favorites".into() });
    f.net.settle();

    let favs = f.query::<Vec<Playlist>>(0, "peer-0", "playlists", &())[0].id;
    f.net.partition(2);
    for i in [0, 2] {
        let lib: Vec<LibraryEntry> = f.query(i, "peer-0", "library", &Library { playlist_id: favs });
        for e in lib {
            f.mutate(
                i,
                "add_to_playlist",
                &AddToPlaylist {
                    playlist_id: favs,
                    media_id: e.id,
                },
            );
        }
        f.net.step();
    }
    // Somebody else's playlist is refused by their own view, and never sent.
    f.mutate(1, "add_all_to_playlist", &AddAllToPlaylist { playlist_id: favs });
    assert_eq!(f.net.refused, [(1, "not your playlist".to_string())]);
    assert!(f.net.clients[&1].pending.is_empty());
    f.net.refused.clear();
    f.net.settle();

    f.converged();
    assert!(f.net.refused.is_empty(), "{:?}", f.net.refused);
    let lib: Vec<LibraryEntry> = f.query(1, "peer-1", "library", &Library { playlist_id: favs });
    assert_eq!(lib.len(), 17, "9 shared + 4 dark + 4 lit, none lost and none duplicated");
    let mut pos: Vec<i64> = lib.iter().map(|r| r.pos).collect();
    pos.sort_unstable();
    assert_eq!(pos, (1..=17).collect::<Vec<_>>(), "library positions are 1..n after the replay");
    let on: Vec<LibraryEntry> = f.query(0, "peer-0", "playlist", &PlaylistInput { playlist_id: favs });
    assert_eq!(on.len(), 17, "every item once, though both devices added every one");
    assert_eq!(
        on.iter().map(|r| r.playlist_pos.unwrap()).collect::<Vec<_>>(),
        (1..=17).collect::<Vec<_>>()
    );
}

/// Two devices of one person each make "Favorites" while apart, and a
/// third peer used by nobody makes one too and then signs in as that
/// person: after sync there are three playlists, named in log order
/// "Favorites", "Favorites (1)", "Favorites (2)", each holding what was
/// put on it under its own id. Signing in is the connection's login: the
/// authority stamps nobody's entries with it. Another person's "Favorites"
/// is theirs and plain. Falsified by `create_playlist` reading every
/// playlist's names, not the person's: peer-3's came out "Favorites (3)".
#[test]
fn same_named_playlists_made_apart_are_numbered_in_log_order() {
    let logins = vec![
        who("peer-0", "dev-0", &[LIBRARY]),
        who("", "dev-1", &[]),
        who("peer-0", "dev-2", &[]),
        who("peer-3", "dev-3", &[]),
    ];
    let mut f = Fleet::new(23, logins);
    for k in 0..6 {
        f.mutate(0, "add_song", &track(&format!("t{k}")));
    }
    f.net.settle();
    let none = Id::new([0; 16]);
    let ids: Vec<Id<Media>> = f
        .query::<Vec<LibraryEntry>>(1, "", "library", &Library { playlist_id: none })
        .into_iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(ids.len(), 6);

    for i in 0..3 {
        f.net.partition(i);
    }
    let mut made = BTreeMap::new();
    for i in 0..3 {
        let me = f.net.clients[&i].me.actor.clone();
        f.mutate(i, "create_playlist", &CreatePlaylist { name: "Favorites".into() });
        let on: Vec<Playlist> = f.query(i, &me, "playlists", &());
        assert_eq!(on.len(), 1, "apart, each device sees only its own");
        assert_eq!(on[0].name, "Favorites", "and calls it what it was called");
        let fav = on[0].id;
        for k in [2 * i, 2 * i + 1] {
            f.mutate(
                i,
                "add_to_playlist",
                &AddToPlaylist {
                    playlist_id: fav,
                    media_id: ids[k as usize],
                },
            );
        }
        made.insert(fav, vec![format!("t{}", 2 * i), format!("t{}", 2 * i + 1)]);
    }
    f.mutate(3, "create_playlist", &CreatePlaylist { name: "Favorites".into() });
    // The peer nobody had signed in on: its connection is peer-0's now.
    let signed = who("peer-0", "dev-1", &[]);
    f.net.logins.insert(1, signed.clone());
    f.net.clients.get_mut(&1).unwrap().me = signed;
    f.net.settle();

    f.converged();
    assert!(f.net.refused.is_empty(), "{:?}", f.net.refused);
    let mine: Vec<Playlist> = f.query(1, "peer-0", "playlists", &());
    assert_eq!(
        mine.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["Favorites", "Favorites (1)", "Favorites (2)"],
        "numbered in log order"
    );
    for p in &mine {
        let on: Vec<LibraryEntry> = f.query(2, "peer-0", "playlist", &PlaylistInput { playlist_id: p.id });
        assert_eq!(
            on.iter().map(|e| e.title.clone()).collect::<Vec<_>>(),
            made[&p.id],
            "{} keeps what was put on it",
            p.name
        );
    }
    let theirs: Vec<Playlist> = f.query(3, "peer-3", "playlists", &());
    assert_eq!(
        theirs.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["Favorites"],
        "another person's names are theirs"
    );
}

/// A track on two people's playlists leaves the library: the library's
/// login takes both items off with it, and nobody's other items go. A
/// login without the role may not do the same: refused on its own device
/// by `is_library`, and by the server when the device believes a role its
/// login was never given — and since the authority stamps, no entry in
/// the log carries that belief. Falsified by `remove_media` leaving the
/// items: all six were left.
#[test]
fn the_library_takes_a_removed_track_off_everybodys_playlists() {
    let logins = vec![
        who("peer-0", "dev-0", &[LIBRARY]),
        who("peer-1", "dev-1", &[]),
        who("peer-2", "dev-2", &[LIBRARY]),
        who("peer-3", "dev-3", &[]),
    ];
    let mut f = Fleet::new(31, logins);
    for t in ["removed", "attempted", "kept"] {
        f.mutate(0, "add_song", &track(t));
    }
    f.net.settle();
    let none = Id::new([0; 16]);
    let ids: Vec<Id<Media>> = f
        .query::<Vec<LibraryEntry>>(0, "peer-0", "library", &Library { playlist_id: none })
        .into_iter()
        .map(|e| e.id)
        .collect();
    let (removed, attempted) = (ids[0], ids[1]);
    for i in [1, 3] {
        f.mutate(i, "create_playlist", &CreatePlaylist { name: "Mine".into() });
        let list = f.query::<Vec<Playlist>>(i, &format!("peer-{i}"), "playlists", &())[0].id;
        for media in &ids {
            f.mutate(
                i,
                "add_to_playlist",
                &AddToPlaylist {
                    playlist_id: list,
                    media_id: *media,
                },
            );
        }
    }
    f.net.settle();
    let items = |f: &Fleet| -> Vec<PlaylistItem> {
        f.net
            .server
            .store
            .rows("playlist_item")
            .map(|r| PlaylistItem::from_bytes(r).unwrap())
            .collect()
    };
    assert_eq!(items(&f).len(), 6, "three tracks on each of two people's playlists");

    f.mutate(0, "remove_media", &RemoveMedia { id: removed });
    f.net.settle();
    let left = items(&f);
    assert_eq!(left.len(), 4, "{left:?}");
    assert!(left.iter().all(|i| i.media_id != removed), "no item names the removed track");
    let mut owners: Vec<String> = left.into_iter().map(|i| i.user_id).collect();
    owners.sort();
    assert_eq!(owners, ["peer-1", "peer-1", "peer-3", "peer-3"], "each person keeps their other two");

    // Somebody without the role: refused on the device, never sent…
    f.mutate(1, "remove_media", &RemoveMedia { id: attempted });
    assert_eq!(f.net.refused, [(1, "only the library writes the library".to_string())]);
    assert!(f.net.clients[&1].pending.is_empty(), "nothing pending");
    f.net.refused.clear();
    // …and by the server, from a device that believes a role it was never given.
    f.net.clients.get_mut(&1).unwrap().me.roles = vec![LIBRARY.into()];
    f.mutate(1, "remove_media", &RemoveMedia { id: attempted });
    f.net.settle();
    assert_eq!(f.net.refused, [(1, "only the library writes the library".to_string())]);
    assert_eq!(items(&f).len(), 4, "nothing moved");
    for l in &f.net.server.log().entries() {
        let want: Vec<String> = if l.entry.actor == "peer-0" { vec![LIBRARY.into()] } else { vec![] };
        assert_eq!(l.entry.roles, want, "entry {} by {}", l.seq, l.entry.actor);
    }
    f.converged();
}
