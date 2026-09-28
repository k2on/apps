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

struct Fleet {
    sim: Sim,
    procs: BTreeMap<String, (FnHash, Procedure)>,
    next: u32,
}

impl Fleet {
    fn new(seed: u64) -> Fleet {
        let m = module();
        let procs: BTreeMap<String, (FnHash, Procedure)> = m.procedures().into_iter().map(|(h, p)| (p.name().to_string(), (h, p))).collect();
        let mut sim = Sim::new(m.build().schema.clone(), ark::hash::closures(m.build()), 3, seed);
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
        let eid = self.fresh();
        let c = self.sim.clients.get_mut(&i).unwrap();
        if c.mutate(eid, &Ctx::new(user, "dev"), &fh, &autos, &a).is_ok() {
            let out = c.take_outgoing();
            if self.sim.conn.contains_key(&i) {
                self.sim.to_server.entry(i).or_default().extend(out);
            }
        }
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
    let mut f = Fleet::new(19);
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
