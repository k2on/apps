//! The domain against a simulated fleet: `ark::sim`, the engine's own
//! seeded network that reorders, duplicates and drops — no sockets, no
//! threads, no sleeps, and a seed that reproduces the run exactly. The
//! server and one client apply entries natively; the others through the
//! interpreter over the emitted closures, so agreement is part of what
//! converging means here.

use std::collections::BTreeMap;

use ark::authoring::Procedure;
use ark::eval::{Args, Ctx};
use ark::hash::FnHash;
use ark::ir::Auto;
use ark::sim::Sim;
use ark::value::{Id, Value};
use harken_domain::module;

fn library() -> String {
    harken_domain::schema::LIBRARY.to_string()
}

struct Fleet {
    sim: Sim,
    procs: BTreeMap<String, (FnHash, Procedure)>,
    next: u32,
}

impl Fleet {
    fn new(seed: u64, n: i64) -> Fleet {
        let m = module();
        let procs: BTreeMap<String, (FnHash, Procedure)> = m.procedures().into_iter().map(|(h, p)| (p.name().to_string(), (h, p))).collect();
        let mut sim = Sim::new(m.build().schema.clone(), ark::hash::closures(m.build()), n, seed);
        sim.server.authority.hold(m.procedures());
        sim.clients.get_mut(&0).unwrap().replica.hold(m.procedures());
        // Client 2 is peer-0's second device: the same person, another login.
        sim.partition(2);
        sim.clients.get_mut(&2).unwrap().token = Some("peer-0".into());
        sim.heal(2);
        Fleet { sim, procs, next: 0 }
    }

    fn fresh(&mut self) -> Id {
        self.next += 1;
        let mut b = [0u8; 16];
        b[12..].copy_from_slice(&self.next.to_be_bytes());
        b
    }

    /// Author on client `i` as `user`; a refusal by the client's own view
    /// is dropped, as a screen would show it and move on.
    fn mutate(&mut self, i: i64, user: &str, name: &str, a: Args) {
        let _ = self.mutate_as(i, &Ctx::new(user, "dev"), name, a);
    }

    /// Author on client `i` as `ctx`, and answer the entry's first fresh id
    /// (a new row's), if it drew one.
    fn mutate_as(&mut self, i: i64, ctx: &Ctx, name: &str, a: Args) -> Option<Id> {
        let (fh, p) = self.procs[name].clone();
        let autos: Args = p
            .function()
            .autos
            .clone()
            .into_iter()
            .map(|(n, kind)| {
                let v = match kind {
                    Auto::NewId(_) => Value::Id(self.fresh()),
                    Auto::Now => Value::int(1_000 + self.next as i64),
                };
                (n, v)
            })
            .collect();
        let made = p
            .function()
            .autos
            .iter()
            .find(|(_, k)| matches!(k, Auto::NewId(_)))
            .map(|(n, _)| autos[n].as_id());
        let eid = self.fresh();
        let c = self.sim.clients.get_mut(&i).unwrap();
        if c.mutate(eid, ctx, &fh, &autos, &a).is_ok() {
            let out = c.take_outgoing();
            if self.sim.conn.contains_key(&i) {
                self.sim.to_server.entry(i).or_default().extend(out);
            }
        }
        made
    }

    /// What client `i` shows, as `user`: its optimistic view.
    fn query(&self, i: i64, user: &str, name: &str, a: Args) -> Vec<Value> {
        let (_, p) = &self.procs[name];
        p.query(&Ctx::new(user, "dev"), &a, &self.sim.clients[&i].replica.view)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"))
            .as_list()
    }
}

fn song(title: &str) -> Args {
    [
        ("title", Value::text(title)),
        ("artist", Value::text("someone")),
        ("album", Value::text("")),
        ("duration_ms", Value::int(0)),
        ("file", Value::text("")),
        ("track", Value::int(0)),
        ("part", Value::text("")),
        ("catalogue", Value::text("")),
        ("performer", Value::text("")),
        ("bpm", Value::int(0)),
        ("album_art", Value::text("")),
        ("artist_art", Value::text("")),
        ("disc", Value::int(0)),
        ("work_title", Value::text("")),
        ("movement_no", Value::int(0)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

#[test]
fn the_domain_converges_when_peers_go_dark_and_come_back() {
    let mut f = Fleet::new(19, 3);
    let user = |i: i64| if i == 2 { "peer-0".to_string() } else { format!("peer-{i}") };
    for round in 0..3 {
        for i in 0..3 {
            f.mutate(i, &user(i), "add_song", song(&format!("c{i}-{round}")));
            f.sim.step();
        }
    }

    f.sim.partition(2);
    for round in 0..4 {
        f.mutate(2, "peer-0", "add_song", song(&format!("dark-{round}")));
        f.mutate(0, "peer-0", "add_song", song(&format!("lit-{round}")));
        f.sim.step();
    }
    f.mutate(0, "peer-0", "create_playlist", [("name".to_string(), Value::text("Favorites"))].into());
    f.sim.settle();

    // Both devices put the whole library on the same playlist while apart,
    // so the positions are recomputed by replay rather than merged.
    let favs = f.query(0, "peer-0", "playlists", Args::new())[0].field("id");
    f.sim.partition(2);
    for i in [0, 2] {
        let ids: Vec<Value> = f
            .query(i, "peer-0", "library", [("playlist_id".to_string(), favs.clone())].into())
            .iter()
            .map(|r| r.field("id"))
            .collect();
        for id in ids {
            f.mutate(
                i,
                "peer-0",
                "add_to_playlist",
                [("playlist_id".to_string(), favs.clone()), ("media_id".to_string(), id)].into(),
            );
        }
        f.sim.step();
    }
    // Somebody else's playlist is refused by their own view, and never sent.
    f.mutate(1, "peer-1", "add_all_to_playlist", [("playlist_id".to_string(), favs.clone())].into());
    // `settle` reconnects every client itself: the dark work is recovered
    // only because a reconnecting client re-offers what it still has pending.
    f.sim.settle();

    let server = f.sim.server_hash();
    for (i, n, h) in f.sim.client_hashes() {
        assert_eq!((n, h), server, "client {i} disagrees with the server");
    }
    for i in 0..3 {
        assert!(
            f.sim.clients[&i].replica.rejections.is_empty(),
            "client {i}: {:?}",
            f.sim.clients[&i].replica.rejections
        );
    }
    let lib = f.query(1, "peer-1", "library", [("playlist_id".to_string(), favs.clone())].into());
    assert_eq!(lib.len(), 17, "9 shared + 4 dark + 4 lit, none lost and none duplicated");
    let mut pos: Vec<i64> = lib.iter().map(|r| r.field("pos").as_int()).collect();
    pos.sort_unstable();
    assert_eq!(pos, (1..=17).collect::<Vec<_>>(), "library positions are 1..n after the replay");
    let on = f.query(0, "peer-0", "playlist", [("playlist_id".to_string(), favs)].into());
    let places: Vec<i64> = on.iter().map(|r| r.field("playlist_pos").as_int()).collect();
    assert_eq!(on.len(), 17, "every item once, though both devices added every one");
    assert_eq!(places, (1..=17).collect::<Vec<_>>(), "every position on the playlist distinct, in order");
}

/// Two devices of one person each make "Favorites" while apart, and a
/// third peer used by nobody makes one too and then signs in as that
/// person: after sync there are three playlists, named in log order
/// "Favorites", "Favorites (1)", "Favorites (2)", each still holding what
/// was put on it under its own id. Another person's "Favorites" is theirs
/// and plain.
#[test]
fn same_named_playlists_made_apart_are_numbered_in_log_order() {
    let mut f = Fleet::new(23, 4);
    for k in 0..6 {
        f.mutate(0, "peer-0", "add_song", song(&format!("t{k}")));
    }
    f.sim.settle();
    let none = Value::Id([0; 16]);
    let lib = f.query(1, "peer-1", "library", [("playlist_id".to_string(), none)].into());
    let ids: Vec<Value> = lib.iter().map(|r| r.field("id")).collect();
    assert_eq!(ids.len(), 6);

    for i in 0..3 {
        f.sim.partition(i);
    }
    let me = |i: i64| if i == 1 { Ctx::nobody() } else { Ctx::new("peer-0", "dev") };
    let mut made = BTreeMap::new();
    for i in 0..3 {
        let fav = f
            .mutate_as(i, &me(i), "create_playlist", [("name".to_string(), Value::text("Favorites"))].into())
            .expect("a playlist id");
        let on = f.query(i, &me(i).user, "playlists", Args::new());
        assert_eq!(on.len(), 1, "apart, each device sees only its own");
        assert_eq!(on[0].field("name"), Value::text("Favorites"), "and calls it what it was called");
        for k in [2 * i, 2 * i + 1] {
            let media = ids[k as usize].clone();
            f.mutate_as(
                i,
                &me(i),
                "add_to_playlist",
                [("playlist_id".to_string(), Value::Id(fav)), ("media_id".to_string(), media)].into(),
            );
        }
        made.insert(fav, vec![format!("t{}", 2 * i), format!("t{}", 2 * i + 1)]);
    }
    f.mutate(3, "peer-3", "create_playlist", [("name".to_string(), Value::text("Favorites"))].into());
    // The peer nobody had signed in on: its work becomes peer-0's.
    f.sim
        .clients
        .get_mut(&1)
        .unwrap()
        .sign_in(&Ctx::new("peer-0", "dev"), Some("peer-0".into()));
    f.sim.settle();

    let server = f.sim.server_hash();
    for (i, n, h) in f.sim.client_hashes() {
        assert_eq!((n, h), server, "client {i} disagrees with the server");
    }
    for i in 0..4 {
        assert!(
            f.sim.clients[&i].replica.rejections.is_empty(),
            "client {i}: {:?}",
            f.sim.clients[&i].replica.rejections
        );
    }
    let mine = f.query(1, "peer-0", "playlists", Args::new());
    assert_eq!(
        texts(&mine, "name"),
        ["Favorites", "Favorites (1)", "Favorites (2)"],
        "numbered in log order"
    );
    for p in &mine {
        let id = p.field("id");
        let on = f.query(2, "peer-0", "playlist", [("playlist_id".to_string(), id.clone())].into());
        assert_eq!(texts(&on, "title"), made[&id.as_id()], "{:?} keeps what was put on it", p.field("name"));
    }
    assert_eq!(
        texts(&f.query(3, "peer-3", "playlists", Args::new()), "name"),
        ["Favorites"],
        "another person's names are theirs"
    );
}

fn texts(rows: &[Value], field: &str) -> Vec<String> {
    rows.iter().map(|r| r.field(field).as_text().to_string()).collect()
}

/// A track on two people's playlists leaves the library: the library's
/// login takes both items off with it, and nobody's other items go. Nothing
/// is declared about who may write a playlist item (`docs/plan-guards.md`
/// G1), so `remove_media` reaches every playlist holding the track.
#[test]
fn removing_a_track_takes_it_off_everybodys_playlists() {
    let mut f = Fleet::new(31, 4);
    // Client 0 authors as the library, holding its role as the scanner
    // does; 1 and 3 are two people holding none.
    let lib = Ctx::new("peer-0", "dev").with_roles([library()]);
    let person = |i: i64| Ctx::new(format!("peer-{i}"), "dev");
    for t in ["removed", "kept"] {
        f.mutate_as(0, &lib, "add_song", song(t));
    }
    f.sim.settle();
    let none = Value::Id([0; 16]);
    let ids: Vec<Value> = f
        .query(0, "peer-0", "library", [("playlist_id".to_string(), none)].into())
        .iter()
        .map(|r| r.field("id"))
        .collect();
    let (removed, kept) = (ids[0].clone(), ids[1].clone());
    for i in [1, 3] {
        let list = f
            .mutate_as(i, &person(i), "create_playlist", [("name".to_string(), Value::text("Mine"))].into())
            .expect("a playlist id");
        for media in [&removed, &kept] {
            let a: Args = [("playlist_id".to_string(), Value::Id(list)), ("media_id".to_string(), media.clone())].into();
            f.mutate_as(i, &person(i), "add_to_playlist", a);
        }
    }
    f.sim.settle();
    let items = |f: &Fleet| -> Vec<(Value, Value)> {
        use ark::store::Store;
        f.sim
            .server
            .authority
            .store
            .scan("playlist_item")
            .iter()
            .map(|r| (r.get("user_id").unwrap().clone(), r.get("media_id").unwrap().clone()))
            .collect()
    };
    assert_eq!(items(&f).len(), 4, "two tracks on each of two people's playlists");
    let remove = |id: &Value| -> Args { [("id".to_string(), id.clone())].into() };

    // The library: both items go with the track, and nothing else does.
    f.mutate_as(0, &lib, "remove_media", remove(&removed));
    f.sim.settle();
    let left = items(&f);
    assert_eq!(left.len(), 2, "{left:?}");
    assert!(left.iter().all(|(_, m)| *m == kept), "no item names the removed track: {left:?}");
    let mut owners: Vec<Value> = left.into_iter().map(|(u, _)| u).collect();
    owners.sort();
    assert_eq!(
        owners,
        [Value::text("peer-1"), Value::text("peer-3")],
        "each person keeps their other one"
    );

    for i in [0, 1, 3] {
        let none = &f.sim.clients[&i].replica.rejections;
        assert!(none.is_empty(), "client {i}: {none:?}");
    }
    let server = f.sim.server_hash();
    for (i, n, h) in f.sim.client_hashes() {
        assert_eq!((n, h), server, "client {i} disagrees with the server");
    }
}
