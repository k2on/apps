//! What a live space holds as it is used (§1.9): `N` calls of the room's
//! `room.point` from `C` devices in turn, through a server's hub, every
//! frame carried by hand. Counted, not measured: the entries each live
//! machine's log holds above the row it stands on, and the verdicts the
//! server's live authority keeps — both must stay bounded however many
//! calls are made, as the server's live log is folded at harken-server's
//! `keep` of 256.
//!
//! Falsified: on the tree before a device's live instance folded and the
//! volatile verdicts were capped, `live_held_is_bounded` failed with every
//! device holding all 1,203 entries (the calls and three `on_join`s) and
//! the server 1,203 verdicts (C = 3).
//! `live_held_measured` (ignored; `--ignored --nocapture`) prints the same
//! counts and the per-call time at 10,000 calls for C = 2 and C = 10.
//!
//! And the live space follows the log's rules (§1.8, §1.9): a connection
//! fed by a scope holds, of the live rows, only what the scope gives it
//! (`a_scoped_connection_holds_only_its_live_rows`). A device of the live
//! space alone (`Config::live_only`) is fed none of the log
//! (`a_live_only_device_is_fed_the_live_space_alone`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use ark::format::Encode;
use ark::schema::Space;
use ark_peer::{dev, Authority, Config, Memory, Peer};
use arkdb::demo::demo as domain;
use arkdb::room::{Arrival, Point};

/// harken-server's live `keep`.
const KEEP: usize = 256;

fn hub() -> Authority {
    scoped_hub(Arc::new(|_| None))
}

fn scoped_hub(feed: ark_peer::Scoping) -> Authority {
    let c = ark_peer::ServerConfig {
        live_keep: Some(KEEP),
        live: Some(Arc::new(arkdb::room::room())),
        feed,
        ..ark_peer::ServerConfig::of(Arc::new(domain()))
    };
    Authority::serve(c, Box::new(Memory::default()), 0).unwrap()
}

fn device(k: usize) -> Peer {
    let c = Config {
        device: Some(Arrival { name: format!("d{k}") }.to_bytes()),
        live: Some(Arc::new(arkdb::room::room())),
        ..Config::of(Arc::new(domain()))
    };
    let who = dev(&format!("u{k}@d{k}")).unwrap();
    let mut p = Peer::open(c, who, Box::new(Memory::default()));
    p.linked = true;
    p
}

/// Every frame each side has, carried until none is left: the frames the
/// devices were sent, each to its connection.
fn carry(hub: &mut Authority, ds: &mut [(Peer, u64)]) -> Vec<(u64, Vec<u8>)> {
    let mut sent = vec![];
    loop {
        let mut q = vec![];
        for (d, c) in ds.iter_mut() {
            for f in d.take_frames() {
                q.extend(hub.frame(*c, f, 0));
            }
        }
        if q.is_empty() {
            return sent;
        }
        for (to, b) in q {
            if let Some((d, _)) = ds.iter_mut().find(|(_, c)| *c == to) {
                d.receive(b.clone());
            }
            sent.push((to, b));
        }
    }
}

/// What the live space holds after `n` calls from `c` devices in turn:
/// the entries of the most any device holds, the server's entries, the
/// server's live verdicts; and the mean time of a call.
struct Held {
    device: usize,
    server: usize,
    verdicts: usize,
    per_call: Duration,
}

fn live(p: &ark_peer::Core) -> &ark::peer::Peer {
    p.machine(Space::Volatile).expect("a live machine")
}

fn run(c: usize, n: u64) -> Held {
    let mut hub = hub();
    let mut ds: Vec<(Peer, u64)> = (0..c)
        .map(|k| {
            let d = device(k);
            let conn = hub.accept(d.me().clone());
            (d, conn)
        })
        .collect();
    for (d, _) in ds.iter_mut() {
        d.connected();
    }
    carry(&mut hub, &mut ds);
    let t = Instant::now();
    for i in 0..n {
        let k = i as usize % c;
        let x = Point { x: i as i64 }.to_bytes();
        ds[k].0.mutate_at("room.point", x, i as i64).unwrap();
        carry(&mut hub, &mut ds);
    }
    let per_call = t.elapsed() / n as u32;
    // Every device stands where the server does, nothing pending.
    for (d, _) in &ds {
        assert_eq!(live(d).log().head(), live(&hub).log().head());
        assert_eq!(live(d).store, live(&hub).store);
        assert!(live(d).pending().is_empty());
    }
    let journal = live(&hub).journal().expect("a server's live machine is an authority");
    let verdicts = journal.refused.len() + journal.last.len();
    Held {
        device: ds.iter().map(|(d, _)| live(d).log().entries().len()).max().unwrap_or(0),
        server: live(&hub).log().entries().len(),
        verdicts,
        per_call,
    }
}

#[test]
fn live_held_is_bounded() {
    let (c, n) = (3, 1_200);
    let h = run(c, n);
    eprintln!(
        "C {c} N {n}: device entries {}, server entries {}, verdicts {}",
        h.device, h.server, h.verdicts
    );
    assert!(h.device <= 64, "a device holds {} live entries", h.device);
    assert!(h.server <= 2 * KEEP, "the server holds {} live entries", h.server);
    // The last 64 of each device's origin, and of the server's own (its
    // `on_join`s).
    assert!(h.verdicts <= 64 * (c + 1), "the server keeps {} live verdicts", h.verdicts);
}

#[test]
#[ignore]
fn live_held_measured() {
    for c in [2, 10] {
        let n = 10_000;
        let h = run(c, n);
        eprintln!(
            "C {c} N {n}: device entries {}, server entries {}, verdicts {}, {:.1} µs a call",
            h.device,
            h.server,
            h.verdicts,
            h.per_call.as_secs_f64() * 1e6
        );
    }
}

/// Of the room's pointers, a connection fed by the demo's scope holds only
/// its own (`arkdb::room::room_top_two`), as it is filled and as the
/// pointers move; one fed whole holds every one. Falsified by the hub
/// before the live machine was given the feed: `u0`'s device held `u1`'s
/// pointer.
#[test]
fn a_scoped_connection_holds_only_its_live_rows() {
    let mut hub = scoped_hub(Arc::new(|w: &ark::peer::Ctx| (w.actor == "u0").then(|| "demo.top_two".to_string())));
    // `u1` points before `u0` comes, so `u0` is filled; then each points.
    let mut ds = vec![];
    for k in [1, 0] {
        let mut d = device(k);
        let conn = hub.accept(d.me().clone());
        d.connected();
        ds.push((d, conn));
        carry(&mut hub, &mut ds);
        ds.last_mut()
            .unwrap()
            .0
            .mutate_at("room.point", Point { x: k as i64 }.to_bytes(), 1)
            .unwrap();
        carry(&mut hub, &mut ds);
    }
    ds[0].0.mutate_at("room.point", Point { x: 7 }.to_bytes(), 2).unwrap();
    carry(&mut hub, &mut ds);
    let pointers = |p: &ark_peer::Core| -> Vec<String> {
        let rows = live(p)
            .store
            .rows("pointer")
            .map(|r| arkdb::room::Pointer::from_bytes(r).unwrap().session);
        rows.collect()
    };
    assert_eq!(pointers(&hub), ["u0@d0", "u1@d1"]);
    assert_eq!(pointers(&ds[0].0), ["u0@d0", "u1@d1"], "fed whole");
    assert_eq!(pointers(&ds[1].0), ["u0@d0"], "fed by its scope");
    assert!(live(&ds[1].0).pending().is_empty());
}

/// A device of the live space alone (a server's speaker) stands in the room
/// and points there as any device does, and holds none of the log: it is
/// not filled, and the entry another device's playlist was is not
/// taken in. Falsified by saying `Hello` for the log all the same: it was
/// answered; and by taking in the log's frames: it held the playlist.
#[test]
fn a_live_only_device_is_fed_the_live_space_alone() {
    let mut hub = hub();
    let mut ds = vec![];
    for k in [0, 1] {
        let mut d = device(k);
        d.config.live_only = k == 1;
        let conn = hub.accept(d.me().clone());
        d.connected();
        ds.push((d, conn));
    }
    let list = arkdb::demo::CreatePlaylist { name: "Road".into() };
    ds[0].0.mutate_at("demo.create_playlist", list.to_bytes(), 1).unwrap();
    ds[1].0.mutate_at("room.point", Point { x: 3 }.to_bytes(), 1).unwrap();
    let sent = carry(&mut hub, &mut ds);
    let log = |c: u64| {
        let of_log = |b: &[u8]| ark::frames::Frame::dec(b).is_ok_and(|f| f.space() == Space::Durable);
        sent.iter().filter(|(to, b)| *to == c && of_log(b)).count()
    };
    assert!(
        log(ds[1].1) < log(ds[0].1),
        "the speaker said no `Hello` for the log, and is answered none"
    );
    let playlists = |p: &ark_peer::Core| p.log.store.rows("playlist").count();
    assert_eq!((playlists(&hub), playlists(&ds[0].0)), (1, 1));
    assert_eq!(playlists(&ds[1].0), 0, "the speaker holds none of the log");
    assert_eq!(live(&ds[1].0).store, live(&hub).store, "and the room as the server has it");
    assert!(live(&ds[1].0).pending().is_empty());
}
