//! The process fleet (`docs/plan-fleet.md` §2): the real `harken-server`,
//! real `harken-peer`s, and the network between them taken away in every
//! way a laptop lid and a bad hotel wifi take it away — then given back,
//! and every scenario ends on [`Fleet::converged`]: every replica's
//! confirmed state hashes as the server's log does, and every accepted
//! intent is in that log exactly once.
//!
//! `arkdb`'s simulator already holds the machines to this in one
//! process. What only processes can reach is everything outside it: the
//! WebSocket and its keepalive, a server killed and restarted from its data
//! directory, a peer killed between writing its intent and pumping, a
//! connection cut in the middle of a page, the sign-in before the first
//! frame, the scanner authoring while a client is away.
//!
//! Each scenario was falsified once by breaking what it holds; its doc
//! comment says how. On v5 every scenario was run again and those whose
//! means of falsifying moved say how they were falsified here. A scenario
//! that finds a bug in `rust/ark*` is not fixed there: it stays, failing,
//! `#[ignore = "witness: …"]`, and the finding goes to the coordinator.
//!
//! `cargo test -p harken-server --test fleet -- --nocapture` prints the
//! timings; `FLEET_LONG=1` runs the seeded fuzz for 2,000 steps instead of
//! 20, and `FLEET_SEED=n` picks its seed.

mod common;
mod support {
    pub mod fleet;
    pub mod proxy;
}

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use ark::format::Encode;
use ark::frames::Frame;
use ark::log::Logged;
use harken_server::domain::playlists::favorites_of;
use harken_server::domain::{AddAllToPlaylist, AddToPlaylist, CreatePlaylist, RemoveFromPlaylist};
use harken_server::domain::{Media, Playlist, PlaylistItem};
use serde_json::{json, Value};
use support::fleet::*;

/// Any id, for a query argument whose value does not matter (`library`
/// asks which playlist to mark; none).
const NO_PLAYLIST: Id = [0; 16];

/// What a peer's library holds, as media ids in library order.
fn media(p: &mut PeerProc) -> Vec<Id> {
    ids_of(
        &p.query("library", vec![("playlist_id", id(NO_PLAYLIST))]),
        "id",
    )
}

/// The files a peer's library holds.
fn files(p: &mut PeerProc) -> BTreeSet<String> {
    p.query("library", vec![("playlist_id", id(NO_PLAYLIST))])
        .iter()
        .map(|r| r["file"].as_str().unwrap().to_string())
        .collect()
}

/// The caller's playlists on a peer: `(name, id)` in the order made.
fn playlists(p: &mut PeerProc) -> Vec<(String, Id)> {
    p.query("playlists", vec![])
        .iter()
        .map(|r| (r["name"].as_str().unwrap().to_string(), id_of(&r["id"])))
        .collect()
}

fn playlist(p: &mut PeerProc, name: &str) -> Id {
    playlists(p)
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("{} has no playlist {name}", p.name))
        .1
}

fn add(p: &mut PeerProc, list: Id, m: Id) -> Id {
    p.author(
        "add_to_playlist",
        vec![("playlist_id", id(list)), ("media_id", id(m))],
    )
}

fn named(name: &str) -> Vec<(&'static str, Value)> {
    vec![("name", json!(name))]
}

/// The playlist a `create_playlist` entry made: the row its facts add.
fn made(e: &Logged) -> Id {
    added::<Playlist>(e, "playlist")
        .first()
        .unwrap_or_else(|| panic!("{} made no playlist", function(e)))
        .id
        .0
}

/// `(playlist, media)` an entry names, by its function's input.
fn named_ids(e: &Logged) -> (Id, Option<Id>) {
    let i = &e.entry.input;
    match function(e).as_str() {
        "add_to_playlist" => {
            let a = AddToPlaylist::from_bytes(i).unwrap();
            (a.playlist_id.0, Some(a.media_id.0))
        }
        "remove_from_playlist" => {
            let a = RemoveFromPlaylist::from_bytes(i).unwrap();
            (a.playlist_id.0, Some(a.media_id.0))
        }
        "add_all_to_playlist" => (AddAllToPlaylist::from_bytes(i).unwrap().playlist_id.0, None),
        other => panic!("{other} names no playlist"),
    }
}

/// **1. Three peers online.** Two of alice's devices and one of bob's make
/// playlists and move things on and off them, interleaved, with nothing
/// taken away. Converged — and every playlist's `pos` order is the log's
/// order, replayed here by a model of the four verbs: an add appends what
/// is not already there, a remove takes it off, add-all appends the library
/// in its order.
///
/// Falsified by reversing the model's append for `add_to_playlist` (a
/// prepend): the playlists' order no longer matches.
#[test]
fn three_peers_online_agree_and_a_playlist_is_in_log_order() {
    let f = Fleet::new("online");
    let mut a = f.peer("a", Some("alice"));
    let mut b = f.peer("b", Some("alice"));
    let mut c = f.peer("c", Some("bob"));
    for i in 0..8 {
        a.author("add_song", song(&format!("online-{i}")));
    }
    a.author("create_playlist", named("Shared"));
    c.author("create_playlist", named("Bob's"));
    f.converged(&mut [&mut a, &mut b, &mut c]);
    let m = media(&mut b);
    assert_eq!(m.len(), 8);
    let (shared, bobs) = (playlist(&mut b, "Shared"), playlist(&mut c, "Bob's"));
    for round in 0..8 {
        add(&mut a, shared, m[round]);
        add(&mut b, shared, m[7 - round]);
        add(&mut c, bobs, m[(round * 3) % 8]);
        if round == 3 {
            b.author(
                "remove_from_playlist",
                vec![("playlist_id", id(shared)), ("media_id", id(m[0]))],
            );
            c.author("add_all_to_playlist", vec![("playlist_id", id(bobs))]);
        }
        if round == 5 {
            a.author(
                "remove_from_playlist",
                vec![("playlist_id", id(shared)), ("media_id", id(m[6]))],
            );
        }
    }
    let t = Instant::now();
    let done = f.converged(&mut [&mut a, &mut b, &mut c]);
    measured("three peers online converge", t.elapsed());

    // The model: the log, in order.
    let by_pos: Vec<Id> = {
        let mut rows: Vec<Media> = done.log.rows("media");
        rows.sort_by_key(|r| r.pos);
        rows.into_iter().map(|r| r.id.0).collect()
    };
    let mut model: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
    for (_, e) in &done.entries {
        match function(e).as_str() {
            "add_to_playlist" => {
                let (l, m) = named_ids(e);
                let list = model.entry(l).or_default();
                let m = m.unwrap();
                if !list.contains(&m) {
                    list.push(m);
                }
            }
            "remove_from_playlist" => {
                let (l, m) = named_ids(e);
                model.entry(l).or_default().retain(|x| Some(*x) != m);
            }
            "add_all_to_playlist" => {
                let list = model.entry(named_ids(e).0).or_default();
                for m in &by_pos {
                    if !list.contains(m) {
                        list.push(*m);
                    }
                }
            }
            _ => {}
        }
    }
    model.retain(|_, v| !v.is_empty());
    assert_eq!(
        playlists_in_pos_order(&done.log.rows::<PlaylistItem>("playlist_item")),
        model,
        "pos order is log order"
    );
}

/// **2. One peer black-holed.** Alice's third device loses the network —
/// bytes accepted and never answered, nothing closed — and puts twenty
/// things on the shared playlist while her other two put twenty on it
/// online. Given the network back, it converges, and every one of its adds
/// sits after all of theirs: authored first, sequenced last, which is the
/// rebase made visible.
///
/// Falsified by not black-holing the third peer: its twenty are not
/// pending; with that assertion taken out too, its adds interleave with the
/// others' and the ordering assertion fails.
#[test]
fn a_black_holed_peer_lands_after_everyone_else() {
    let f = Fleet::new("blackhole");
    let mut a = f.peer("a", Some("alice"));
    let mut b = f.peer("b", Some("alice"));
    let mut c = f.peer("c", Some("alice"));
    for i in 0..40 {
        a.author("add_song", song(&format!("bh-{i}")));
    }
    a.author("create_playlist", named("Mix"));
    f.converged(&mut [&mut a, &mut b, &mut c]);
    let m = media(&mut c);
    let mix = playlist(&mut c, "Mix");

    c.proxy.blackhole();
    let mut away = BTreeSet::new();
    for i in 0..20 {
        away.insert(m[20 + i]);
        add(&mut c, mix, m[20 + i]);
        if i % 2 == 0 {
            add(&mut a, mix, m[i]);
        } else {
            add(&mut b, mix, m[i]);
        }
    }
    assert!(a.settle(PATIENCE) && b.settle(PATIENCE));
    assert_eq!(
        c.status().pending,
        20,
        "the twenty are pending behind the black hole"
    );

    let t = Instant::now();
    c.proxy.pass();
    let done = f.converged(&mut [&mut a, &mut b, &mut c]);
    measured("converge after a black hole of twenty intents", t.elapsed());

    let order = &playlists_in_pos_order(&done.log.rows::<PlaylistItem>("playlist_item"))[&mix];
    assert_eq!(order.len(), 40);
    let first_away = order.iter().position(|x| away.contains(x)).unwrap();
    assert!(
        order[first_away..].iter().all(|x| away.contains(x)) && first_away == 20,
        "the black-holed peer's adds land after everyone else's: {:?}",
        order.iter().map(|x| away.contains(x)).collect::<Vec<_>>()
    );
}

/// A burst of `n` playlists from each peer, named by round.
fn burst(peers: &mut [&mut PeerProc], round: usize, n: usize) {
    for i in 0..n {
        for p in peers.iter_mut() {
            let name = format!("{}-{round}-{i}", p.name);
            p.author("create_playlist", named(&name));
        }
    }
}

/// The pending intents of every peer, together.
fn pending(peers: &mut [&mut PeerProc]) -> i64 {
    peers.iter_mut().map(|p| p.status().pending).sum()
}

/// **3a. The server stopped mid-stream.** Three peers push bursts of
/// intents; the server is sent `SIGTERM` with frames in flight, the peers
/// go on authoring against nothing, and it is started again from its data
/// directory — the peers find it on their own. Converged, nothing lost and
/// nothing twice, and the log read back whole after the stop (the chain on
/// disk is appended a record at a time, the snapshot renamed into place).
///
/// Falsified by starting again over an emptied data directory: what was
/// accepted before the stop is not in the log.
#[test]
fn a_server_stopped_mid_stream_loses_nothing() {
    let mut f = Fleet::new("server-stop");
    let mut a = f.peer("a", Some("alice"));
    let mut b = f.peer("b", Some("alice"));
    let mut c = f.peer("c", Some("bob"));
    a.author("add_song", song("stop-song"));
    f.converged(&mut [&mut a, &mut b, &mut c]);
    burst(&mut [&mut a, &mut b, &mut c], 0, 10);
    f.server.stop();
    assert!(
        f.server.log_on_disk().is_some(),
        "the log after a stop is whole"
    );
    let beside: Vec<String> = std::fs::read_dir(f.server.log_dir())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains("tmp") || n.contains("partial"))
        .collect();
    assert!(
        beside.is_empty(),
        "and nothing is left beside it: {beside:?}"
    );
    burst(&mut [&mut a, &mut b, &mut c], 1, 5);
    let before = pending(&mut [&mut a, &mut b, &mut c]);
    assert!(before > 0, "the burst after the stop is pending");
    let t = Instant::now();
    f.server.start();
    let listening = t.elapsed();
    assert!(
        eventually(PATIENCE, || pending(&mut [&mut a, &mut b, &mut c]) < before),
        "nothing was acked after the restart"
    );
    measured("server restart to listening", listening);
    measured("server restart to first re-ack", t.elapsed());
    let done = f.converged(&mut [&mut a, &mut b, &mut c]);
    measured("converge after a server restart", t.elapsed());
    assert_eq!(done.work(), 1 + 45, "one song and forty-five playlists");
    println!(
        "fleet: the log on disk is {} bytes for {} entries ({} bytes an entry)",
        f.server.log_bytes(),
        done.head,
        f.server.log_bytes() / done.head as u64
    );
}

/// The server's log, deleted where it lies: a lost log, not a lost server
/// (the sign-ins are kept).
fn lose_the_log(f: &Fleet) {
    std::fs::remove_dir_all(f.server.log_dir())
        .unwrap_or_else(|e| panic!("{}: {e}", f.server.log_dir().display()));
}

/// **3c. A server that lost its log re-bases everyone onto what it has.**
/// Three peers converge on a song and fifteen playlists; the server is
/// stopped, its log deleted (the sessions are kept: this is a lost log, not
/// a lost server), and the peers author nine more against nothing. Started
/// again with an empty log, it answers each peer's `Hello` — at 16, past
/// its head, with a hash it does not have — with its snapshot at the head.
/// Each re-opens from it, its nine pending on top, and the fleet converges
/// on a log of exactly those nine. What the lost log held is gone from
/// every peer, because there is one log and this is it. The server's own
/// scanner, whose replica is past the head too, is re-based the same way.
///
/// **And the half a cursor cannot tell**: the log is lost again, the three
/// author two more each and go away, and the server — on a new, empty log —
/// is not idle while they are gone: a fourth device, `d`, sequences twelve
/// on it. When the three come back their cursors, 9, are *below* its head,
/// 12, and a sequence number alone says they are three entries behind on
/// this log — which they never held a byte of. Their `Hello` names the
/// entry hash they were confirmed at, which this chain does not have at 9,
/// so each is answered with the snapshot as before, and the fleet converges
/// on `d`'s twelve and the six.
///
/// Falsified on v5 by keeping the log (no `lose_the_log`): the head is
/// sixteen and more, not nine.
#[test]
#[ignore = "witness: a server whose log was lost serves its empty log at seq 0, which every replica holds as its own base and ignores as a late copy; their pushes past the server`s `last` for their session are held as a gap, and nothing converges (ark `Peer::reset`, `push`)"]
fn a_server_that_lost_its_log_re_bases_everyone_onto_what_it_has() {
    let mut f = Fleet::new("log-lost");
    let mut a = f.peer("a", Some("alice"));
    let mut b = f.peer("b", Some("alice"));
    let mut c = f.peer("c", Some("bob"));
    a.author("add_song", song("lost with the log"));
    burst(&mut [&mut a, &mut b, &mut c], 0, 5);
    let before = f.converged(&mut [&mut a, &mut b, &mut c]);
    assert_eq!(before.head, 16);
    f.server.stop();
    lose_the_log(&f);
    burst(&mut [&mut a, &mut b, &mut c], 1, 3);
    assert_eq!(pending(&mut [&mut a, &mut b, &mut c]), 9);
    let t = Instant::now();
    f.server.start();
    f.lost(before.entries.iter().map(|(_, e)| e.entry.id.0));
    let done = f.converged(&mut [&mut a, &mut b, &mut c]);
    measured(
        "a server that lost its log: restart to converged",
        t.elapsed(),
    );
    assert_eq!(
        done.head, 9,
        "the nine pending, and nothing the lost log held"
    );
    assert!(media(&mut a).is_empty(), "the song went with the log");
    for p in [&mut a, &mut b, &mut c] {
        let names: Vec<String> = playlists(p).into_iter().map(|(n, _)| n).collect();
        assert!(
            !names.is_empty() && names.iter().all(|n| n.split('-').nth(1) == Some("1")),
            "{}: only the second burst's: {names:?}",
            p.name
        );
    }

    // The other half: the log lost again, and sequenced on before the
    // peers that held it come back.
    f.server.stop();
    lose_the_log(&f);
    burst(&mut [&mut a, &mut b, &mut c], 2, 2);
    assert_eq!(pending(&mut [&mut a, &mut b, &mut c]), 6);
    for p in [&mut a, &mut b, &mut c] {
        p.quit();
    }
    let t = Instant::now();
    f.server.start();
    f.lost(done.entries.iter().map(|(_, e)| e.entry.id.0));
    let mut d = f.peer("d", Some("alice"));
    burst(&mut [&mut d], 3, 12);
    assert!(
        eventually(PATIENCE, || {
            let st = d.status();
            st.pending == 0 && st.cursor == 12
        }),
        "d's twelve are the new log: {:?}",
        d.status()
    );
    for p in [&mut a, &mut b, &mut c] {
        p.start();
        assert!(
            p.status().cursor == 9,
            "{} comes back at 9 of the old log",
            p.name
        );
    }
    let again = f.converged(&mut [&mut a, &mut b, &mut c, &mut d]);
    measured(
        "a lost log sequenced on before its peers returned: restart to converged",
        t.elapsed(),
    );
    assert_eq!(
        again.head, 18,
        "d's twelve and the six, and nothing either lost log held"
    );
    for p in [&mut a, &mut b, &mut c, &mut d] {
        let names: Vec<String> = playlists(p).into_iter().map(|(n, _)| n).collect();
        // Bob's are his two; alice's are her three devices' — a's and b's
        // two each, and d's twelve.
        let (count, rounds) = if p.name == "c" {
            (2, &["2"][..])
        } else {
            (16, &["2", "3"][..])
        };
        assert!(
            names.len() == count
                && names
                    .iter()
                    .all(|n| n.split('-').nth(1).is_some_and(|r| rounds.contains(&r))),
            "{}: only what was authored since the second loss: {names:?}",
            p.name
        );
    }
}

/// **3b. The server killed with `-9` mid-stream.**
///
/// Over a log of 1,500 entries, so that writing it down takes long enough
/// to land in: three peers push bursts and the server is killed in the
/// middle of each, eight times. After each kill, **no peer may have
/// confirmed a sequence the log on disk does not hold** — a peer that
/// has, has an entry the restarted server would give a different one at
/// the same sequence. Then restarted, converged.
///
/// What it holds is that a delivery follows the write: the hub writes what
/// a step made durable before it delivers anything the step said
/// (`ark_peer`'s server role, "Persist to the disk first").
///
/// Falsified on v5 by asserting the log on disk holds less than every
/// cursor (`st.cursor < on_disk`): round 0 fails, every peer at the head.
#[test]
fn a_server_killed_mid_stream_loses_nothing() {
    let mut f = Fleet::seeded("server-kill", |s| {
        for i in 0..1500 {
            s.author(
                "carol",
                "library.add_song",
                &song_input(&format!("seed-{i:04}")),
            );
        }
    });
    let mut a = f.peer("a", Some("alice"));
    let mut b = f.peer("b", Some("alice"));
    let mut c = f.peer("c", Some("bob"));
    f.converged(&mut [&mut a, &mut b, &mut c]);
    for round in 0..8 {
        burst(&mut [&mut a, &mut b, &mut c], round, 6);
        f.server.kill9();
        let on_disk = f
            .server
            .log_on_disk()
            .expect("the log after a kill is whole")
            .head_seq();
        // Whatever was on its way has arrived once every link is down.
        assert!(eventually(PATIENCE, || [&mut a, &mut b, &mut c]
            .into_iter()
            .all(|p| !p.status().linked)));
        for p in [&mut a, &mut b, &mut c] {
            let st = p.status();
            assert!(
                st.cursor <= on_disk,
                "round {round}: {} confirmed up to {} and the log on disk holds {on_disk}: the server said it before it wrote it",
                p.name,
                st.cursor
            );
        }
        f.server.start();
        f.converged(&mut [&mut a, &mut b, &mut c]);
    }
}

/// **4. A peer killed with `-9`.** (a) Right after `mutate` answered and
/// before any pump could push it — the network black-holed so none can —
/// the intent is on disk as pending, and a restart pushes it. The same
/// with the network up, where the kill races the pump: either way, once.
/// (b) Right after the server confirmed an intent and before the peer
/// wrote down that it had — made exact by restoring the peer's directory as
/// it was before — the restarted peer pushes it again, the server answers
/// it from its verdict with its sequence, and the peer's cursor still moves
/// past it.
///
/// Falsified by restoring the directory in (b) with its `pending` record
/// removed: "the peer is back before the ack" fails. Without that
/// assertion the peer would never re-push and `converged` would be
/// satisfied, which is why it is there.
#[test]
fn a_peer_killed_after_mutate_or_after_the_ack_converges() {
    let f = Fleet::new("peer-kill");
    let mut p = f.peer("p", Some("alice"));
    let mut q = f.peer("q", Some("alice"));
    p.author("add_song", song("durable"));
    f.converged(&mut [&mut p, &mut q]);

    // (a)
    p.proxy.blackhole();
    p.author("create_playlist", named("Written before the kill"));
    p.kill9();
    p.proxy.pass();
    p.start();
    assert_eq!(p.status().pending, 1, "the intent was on disk");
    f.converged(&mut [&mut p, &mut q]);
    p.author("create_playlist", named("Racing the pump"));
    p.kill9();
    p.start();
    f.converged(&mut [&mut p, &mut q]);

    // (b)
    p.proxy.blackhole();
    p.author("create_playlist", named("Acked, then forgotten"));
    p.quit();
    let saved = f.root.path().join("peer-p-before-ack");
    copy_dir(&p.dir, &saved);
    p.proxy.pass();
    p.start();
    f.converged(&mut [&mut p, &mut q]);
    let head = q.status().cursor;
    p.kill9();
    std::fs::remove_dir_all(&p.dir).unwrap();
    copy_dir(&saved, &p.dir);
    p.proxy.blackhole();
    p.start();
    let st = p.status();
    assert!(
        st.pending == 1 && st.cursor < head,
        "the peer is back before the ack: {st:?}"
    );
    p.proxy.pass();
    let done = f.converged(&mut [&mut p, &mut q]);
    assert_eq!(
        done.head, head,
        "the re-push was a duplicate, not a new entry"
    );
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

/// **5. Cut mid-page during the first sync.** A log of 1,500 entries —
/// written before the server starts, or under `FLEET_LONG=1` pushed
/// through a peer — and a fresh peer joining it with its connection cut
/// after a sixth of the bytes a whole sync takes, and cut again a sixth
/// further on: torn frames, mid-page. It resumes each time from its cursor,
/// applies nothing twice (its state hashes as the log's), and converges. A
/// second fresh peer with nothing cut is what the sync time is measured
/// on.
///
/// Falsified by arming the second cut at twice a whole sync: it never
/// fires, because what is left is less — so the two cuts this asserts are
/// cuts in the middle of the sync, not after it.
#[test]
fn a_first_sync_cut_mid_page_resumes_at_its_cursor() {
    let long = std::env::var("FLEET_LONG").is_ok_and(|v| v == "1");
    let seed = |s: &mut Seeder| {
        let mut media = vec![];
        for i in 0..500 {
            let facts = s.author(
                "alice",
                "library.add_song",
                &song_input(&format!("page-{i:04}")),
            );
            media.push(facts.iter().find_map(media_of).expect("a media row"));
        }
        let lists: Vec<[u8; 16]> = (0..10)
            .map(|i| {
                let facts = s.author(
                    "alice",
                    "playlists.create_playlist",
                    &CreatePlaylist {
                        name: format!("List {i}"),
                    },
                );
                facts.iter().find_map(playlist_of).expect("a playlist row")
            })
            .collect();
        for i in 0..990 {
            s.author(
                "alice",
                "playlists.add_to_playlist",
                &AddToPlaylist {
                    playlist_id: harken_server::domain::Id::new(lists[i % 10]),
                    media_id: harken_server::domain::Id::new(media[i % 500]),
                },
            );
        }
    };
    let f = if long {
        Fleet::new("mid-page")
    } else {
        Fleet::seeded("mid-page", seed)
    };
    let mut s = f.peer("seeder", Some("alice"));
    if long {
        let t = Instant::now();
        for i in 0..500 {
            s.author("add_song", song(&format!("page-{i:04}")));
        }
        for i in 0..10 {
            s.author("create_playlist", named(&format!("List {i}")));
        }
        assert!(s.settle(PATIENCE));
        let lists: Vec<Id> = playlists(&mut s).into_iter().map(|(_, i)| i).collect();
        let m = media(&mut s);
        for i in 0..990 {
            add(&mut s, lists[i % 10], m[i % m.len()]);
        }
        assert!(s.settle(PATIENCE * 10));
        measured("seed 1,500 entries through one peer", t.elapsed());
    }
    let head = 1500;
    assert!(s.wait(head, PATIENCE), "the seeder has the log");
    println!(
        "fleet: the log on disk is {} bytes for {head} entries ({} bytes an entry)",
        f.server.log_bytes(),
        f.server.log_bytes() / head as u64
    );

    let mut g = f.peer_stopped("fresh", Some("alice"));
    let t = Instant::now();
    g.start();
    assert!(g.wait(head, PATIENCE), "a fresh peer reaches the head");
    measured(
        "a fresh peer syncs 1,500 entries over a real socket",
        t.elapsed(),
    );
    let whole = g.proxy.down_bytes();
    println!("fleet: a whole first sync of {head} entries is {whole} bytes from the server");

    let mut h = f.peer_stopped("cut", Some("alice"));
    // Both cuts armed before the first byte, the second counting from the
    // byte after the first, and each a sixth of what a whole sync took.
    h.proxy.cut_after(whole / 6);
    h.proxy.cut_after(whole / 6);
    let t = Instant::now();
    h.start();
    let cut = eventually(PATIENCE * 3, || {
        h.proxy.cuts() == 2 || h.status().cursor >= head
    });
    assert!(
        cut && h.proxy.cuts() == 2,
        "both cuts, each before the sync was done: {} of 2 fired, {} of {whole} bytes down, cursor {}",
        h.proxy.cuts(),
        h.proxy.down_bytes(),
        h.status().cursor
    );
    assert!(h.wait(head, PATIENCE), "the cut peer reaches the head");
    measured(
        "a fresh peer syncs 1,500 entries, cut twice mid-page",
        t.elapsed(),
    );
    assert!(h.proxy.accepted() >= 3, "it dialled again after each cut");
    f.converged(&mut [&mut s, &mut g, &mut h]);
}

/// The id of a media row a change adds.
fn media_of(c: &ark::store::Change) -> Option<[u8; 16]> {
    match c {
        ark::store::Change::Add { table, row } if table == "media" => {
            Some(Media::from_bytes(row).unwrap().id.0)
        }
        _ => None,
    }
}

/// The id of a playlist row a change adds.
fn playlist_of(c: &ark::store::Change) -> Option<[u8; 16]> {
    match c {
        ark::store::Change::Add { table, row } if table == "playlist" => {
            Some(Playlist::from_bytes(row).unwrap().id.0)
        }
        _ => None,
    }
}

/// **6. Signing in later.** A peer nobody has signed in on, with the
/// network black-holed besides, adds a song, makes a playlist and puts the
/// song on it — as nobody. Given the network back, alice signs in on it:
/// everything it made is alice's on every peer (`user_id` on each row), and
/// every entry is in the log under alice and the session that sign-in
/// made, under the ids the peer answered with signed out (§1.6: an id is
/// its replica's origin's, whoever signs in).
///
/// Falsified by signing in as bob: the other peer, alice's, does not see
/// the playlist, and the actor assertion names bob.
#[test]
fn work_done_signed_out_is_the_signers_everywhere() {
    let f = Fleet::new("sign-in-later");
    let mut a = f.peer("a", Some("alice"));
    let mut s = f.peer("s", None);
    s.proxy.blackhole();
    let mut mine = vec![
        s.author("add_song", song("nobody's")),
        s.author("create_playlist", named("Made before signing in")),
    ];
    let list = playlist(&mut s, "Made before signing in");
    let m = media(&mut s);
    mine.push(add(&mut s, list, m[0]));
    let st = s.status();
    assert!(st.user.is_empty() && st.pending == 3, "{st:?}");
    // The login's now, under the ids they were made with (§1.6).
    let _ = mine;

    s.proxy.pass();
    let (user, session) = s.sign_in("alice");
    assert_eq!(user, "alice");
    let head = a.status().cursor;
    let done = f.converged(&mut [&mut a, &mut s]);
    let theirs: Vec<&Logged> = done
        .entries
        .iter()
        .filter(|(n, e)| *n > head && !is_sign_in(e) && !is_look(e))
        .map(|(_, e)| e)
        .collect();
    assert_eq!(theirs.len(), 3);
    for e in theirs {
        assert_eq!(
            (e.entry.actor.as_str(), e.entry.session.as_str()),
            ("alice", session.as_str()),
            "{}",
            function(e)
        );
    }
    let list = playlist(&mut s, "Made before signing in");
    assert_eq!(
        playlist(&mut a, "Made before signing in"),
        list,
        "alice's other device has it"
    );
    for p in done.log.rows::<Playlist>("playlist") {
        assert_eq!(p.user_id, "alice", "{p:?}");
    }
    for p in done.log.rows::<PlaylistItem>("playlist_item") {
        assert_eq!(p.user_id, "alice", "{p:?}");
    }
    for p in done.log.rows::<Media>("media") {
        assert_eq!(p.user_id, "alice", "{p:?}");
    }
}

/// **7. The same playlist name, made on two black-holed devices.** Both
/// of alice's devices make "Road trip" with no network. Given it back, one
/// is "Road trip (1)" on every peer, after the Favorites her first sign-in
/// made — and which one is decided by the log:
/// the earlier entry keeps the name.
///
/// Falsified by asserting the later entry keeps the name: it fails on
/// every peer.
#[test]
fn the_same_name_twice_is_numbered_in_log_order() {
    let f = Fleet::new("same-name");
    let mut a = f.peer("a", Some("alice"));
    let mut b = f.peer("b", Some("alice"));
    f.converged(&mut [&mut a, &mut b]);
    a.proxy.blackhole();
    b.proxy.blackhole();
    let ia = a.author("create_playlist", named("Road trip"));
    let ib = b.author("create_playlist", named("Road trip"));
    assert_eq!(
        playlists(&mut a).len(),
        2,
        "each sees only its own, and Favorites"
    );
    b.proxy.pass();
    a.proxy.pass();
    let done = f.converged(&mut [&mut a, &mut b]);
    let seq = |i: Id| {
        done.entries
            .iter()
            .find(|(_, e)| e.entry.id.0 == i)
            .map(|(n, e)| (*n, made(e)))
            .unwrap()
    };
    let (first, second) = {
        let (sa, sb) = (seq(ia), seq(ib));
        if sa.0 < sb.0 {
            (sa.1, sb.1)
        } else {
            (sb.1, sa.1)
        }
    };
    for p in [&mut a, &mut b] {
        let lists = playlists(p);
        assert_eq!(
            lists,
            vec![
                ("Favorites".to_string(), favorites_of("alice").0),
                ("Road trip".to_string(), first),
                ("Road trip (1)".to_string(), second)
            ],
            "{}",
            p.name
        );
    }
}

/// **8. Turned away.** Alice's device is linked and idle when its session
/// is revoked by her other device (`session.revoke`). The server closes
/// that socket at once (a revoked session's connections close); the peer
/// dials again, is turned away (4401), and says `denied`; it keeps its store, stops
/// dialling, and goes on authoring — three things, pending. Alice's other
/// device is another login and is not touched. Alice signs in on it again,
/// and what was pending is the new login's, under the ids it was made with,
/// and converges.
///
/// Then a sign-in into a black hole: signed out and signing in again with
/// the network black-holed, the answer is a refusal within the peer's
/// sign-in patience rather than a wait for ever; and a device with no
/// login yet, started behind a black hole, gives up and exits within it.
///
/// Falsified on v5 by the peer not asking why a connection closed before a
/// word (`ask_why` never called): it never says `denied`, and dials on.
#[test]
fn a_peer_turned_away_keeps_its_work_until_signed_in_again() {
    let f = Fleet::new("turned-away");
    let mut p = f.peer("p", Some("alice"));
    let mut q = f.peer("q", Some("alice"));
    p.author("add_song", song("kept"));
    f.converged(&mut [&mut p, &mut q]);
    let old = p.status().session;
    let t = Instant::now();
    q.author("session.revoke", vec![("id", json!(old))]);
    assert!(
        eventually(Duration::from_secs(5), || p.status().denied.is_some()),
        "the peer is told at once: {:?}",
        p.status()
    );
    measured(
        "a revoked session's socket closed, to the peer denied",
        t.elapsed(),
    );
    let st = p.status();
    assert!(!st.linked, "closed by the server: {st:?}");
    let dials = p.proxy.accepted();
    let other = q.status();
    assert!(
        other.linked && other.denied.is_none(),
        "the other login is not touched: {other:?}"
    );

    p.author("create_playlist", named("Kept through a revocation"));
    let list = playlist(&mut p, "Kept through a revocation");
    let m = media(&mut p);
    add(&mut p, list, m[0]);
    p.author("create_playlist", named("And this"));
    let (_, _, view) = p.hash();
    assert_eq!(p.status().pending, 3);
    assert_eq!(p.hash().2, view, "the store is kept");
    // Not dialling is an absence, so it is watched for a while: a second
    // is three backoffs at the fleet's pace.
    assert!(
        !eventually(Duration::from_secs(1), || p.proxy.accepted() > dials),
        "a denied peer does not dial again"
    );

    // The new login's, under the ids they were made with (§1.6).
    let (_, new) = p.sign_in("alice");
    assert_ne!(new, old);
    let done = f.converged(&mut [&mut p, &mut q]);
    let taken: Vec<_> = done
        .entries
        .iter()
        .filter(|(_, e)| e.entry.session == new)
        .collect();
    assert_eq!(taken.len(), 3, "all three were taken");

    // A sign-in into a black hole is answered, within the patience.
    let bound = Duration::from_millis(AUTH_PATIENCE_MS) * 2 + Duration::from_secs(1);
    p.ask("sign_out", vec![]);
    p.proxy.blackhole();
    let t = Instant::now();
    let refused = p
        .try_sign_in("alice")
        .expect_err("a black hole signs nobody in");
    assert!(t.elapsed() < bound, "{:?}: {refused}", t.elapsed());
    measured("a sign-in into a black hole, refused", t.elapsed());
    let mut r = f.peer_stopped("r", Some("alice"));
    r.proxy.blackhole();
    let t = Instant::now();
    let why = r
        .try_start()
        .expect_err("a first sign-in into a black hole ends the start");
    assert!(t.elapsed() < bound, "{:?}: {why}", t.elapsed());
    assert!(!r.running());
    measured("a first sign-in into a black hole, given up", t.elapsed());
}

/// **9. The scanner, and a peer that was away.** Files dropped into
/// `media/music` while a peer is black-holed are authored by the server's
/// scanner; the peer comes back and has them. The same files written again
/// under the same names are not new songs, nor is anything after a
/// restart's rescan — the identity of a song is its path, in `add_song`.
/// Copies under *new* names are new songs, by that same rule.
///
/// Falsified by giving the rewritten files new names: "rewriting a file is
/// not a new song" fails.
#[test]
fn the_scanner_authors_while_a_peer_is_away() {
    let mut f = Fleet::new("scanner");
    let mut p = f.peer("p", Some("alice"));
    f.converged(&mut [&mut p]);
    p.proxy.blackhole();
    let music = f.server.media.join("music");
    for name in ["one", "two", "three"] {
        common::wav(&music.join(format!("Fleet/{name}.wav")), 300);
    }
    let want: BTreeSet<String> = ["one", "two", "three"]
        .iter()
        .map(|n| format!("music/Fleet/{n}.wav"))
        .collect();
    let songs = |f: &Fleet| -> BTreeSet<String> {
        f.server
            .log_on_disk()
            .map(|l| {
                l.rows::<Media>("media")
                    .into_iter()
                    .map(|m| m.file)
                    .collect()
            })
            .unwrap_or_default()
    };
    assert!(
        eventually(PATIENCE, || songs(&f) == want),
        "the scanner authored the three: {:?}",
        songs(&f)
    );
    assert!(files(&mut p).is_empty(), "the peer away has none of them");
    p.proxy.pass();
    let done = f.converged(&mut [&mut p]);
    assert_eq!(files(&mut p), want);
    assert!(done
        .entries
        .iter()
        .filter(|(_, e)| !is_sign_in(e))
        .all(|(_, e)| e.entry.actor == harken_server::library::ACCOUNT));

    // The same three again, rewritten in place: a watch event each, and no
    // song. An absence, so it is watched for as long as the scanner's own
    // settling takes, twice over.
    for name in ["one", "two", "three"] {
        common::wav(&music.join(format!("Fleet/{name}.wav")), 300);
    }
    let work = done.work();
    let moved = || f.server.log_on_disk().map_or(0, |l| l.work()) != work;
    assert!(
        !eventually(Duration::from_secs(2), moved),
        "rewriting a file is not a new song"
    );
    f.server.stop();
    f.server.start();
    assert!(
        !eventually(Duration::from_secs(2), || f
            .server
            .log_on_disk()
            .map_or(0, |l| l.work())
            != work),
        "a rescan authors nothing"
    );
    let done = f.converged(&mut [&mut p]);
    assert_eq!(done.work(), work);

    // Under new names they are new songs: the path is the identity.
    for name in ["one", "two", "three"] {
        common::wav(&music.join(format!("Fleet/{name} (copy).wav")), 300);
    }
    assert!(eventually(PATIENCE, || songs(&f).len() == 6));
    f.converged(&mut [&mut p]);
    assert_eq!(files(&mut p).len(), 6);
}

/// **10. The keepalive.** A peer black-holed with no FIN: the server,
/// pinging every 300ms here, closes it after three go unanswered, and
/// `/healthz` no longer counts it among alice's connections. Given the
/// network back — which cuts what was black-holed — it dials again and
/// converges what it made meanwhile. (On v5 the peer's link does not ping:
/// it learns of the black hole when the network comes back and cuts it,
/// not on its own.)
///
/// Falsified on v5 by `HARKEN_KEEPALIVE_MS=600000`: the server never closes
/// the black-holed socket, and alice keeps two connections.
#[test]
fn a_black_holed_socket_is_closed_by_the_keepalive() {
    let f = Fleet::new("keepalive");
    let mut p = f.peer("p", Some("alice"));
    let mut q = f.peer("q", Some("alice"));
    f.converged(&mut [&mut p, &mut q]);
    assert!(
        eventually(PATIENCE, || f.server.room("alice") == 2),
        "two of alice's connections"
    );
    let t = Instant::now();
    p.proxy.blackhole();
    p.author("create_playlist", named("Said into the void"));
    assert!(
        eventually(PATIENCE, || f.server.room("alice") == 1),
        "the server closes the socket"
    );
    measured("the server closes a black-holed socket", t.elapsed());
    let t = Instant::now();
    p.proxy.pass();
    f.converged(&mut [&mut p, &mut q]);
    measured(
        "converge after the keepalive closed a black hole",
        t.elapsed(),
    );
    assert!(
        eventually(PATIENCE, || f.server.room("alice") == 2),
        "and it is back"
    );
}

/// Whether a payload the peer sent is a frame of this kind (its first
/// message on a connection is its token, which is no frame).
fn is_push(b: &[u8]) -> bool {
    matches!(Frame::dec(b), Ok(Frame::Page { logged, .. }) if logged.first().is_some_and(|l| l.seq == 0))
}

fn is_hello(b: &[u8]) -> bool {
    matches!(Frame::dec(b), Ok(Frame::Hello { .. }))
}

/// **11. Replayed frames.** The last push a peer sent, sent again on the
/// same connection — the server answers it from its verdict, and the log
/// holds the intent once — and a `Hello` sent again on one connection,
/// answered from the cursor it names: the server still counts two of
/// alice's connections, the peer is still linked on the same connection,
/// and it converges.
///
/// Falsified by sending, in place of the replay, that push as a new
/// intent of the same session at the next `local`, its id made to match
/// (`Proxy::inject`): the server sequenced it as "Once (1)", and the
/// peer's own next intent, at that `local`, was answered as it — no "After
/// the replay" in the log.
#[test]
fn a_replayed_push_or_hello_changes_nothing() {
    let f = Fleet::new("replay");
    let mut p = f.peer("p", Some("alice"));
    let mut q = f.peer("q", Some("alice"));
    f.converged(&mut [&mut p, &mut q]);

    let once = p.author("create_playlist", named("Once"));
    assert!(p.settle(PATIENCE));
    // The work in the log, the scanner's looks apart (they land when
    // they land).
    let work = f.server.log_on_disk().map_or(0, |l| l.work());
    let frame = p
        .proxy
        .last_payload_where(is_push)
        .expect("a push was sent");
    let msg = Frame::dec(&frame).unwrap();
    assert!(
        matches!(&msg, Frame::Page { logged, .. } if logged[0].entry.id.0 == once),
        "{msg:?}"
    );
    assert!(p.proxy.replay_where(is_push));
    // Whatever the peer says next travels behind the replay on the same
    // connection, so once it is answered the replay has been.
    p.author("create_playlist", named("After the replay"));
    let done = f.converged(&mut [&mut p, &mut q]);
    assert_eq!(
        done.work(),
        work + 1,
        "the replayed push is not a second entry"
    );
    let names: BTreeSet<String> = done
        .log
        .rows::<Playlist>("playlist")
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(
        names,
        ["After the replay", "Favorites", "Once"]
            .map(String::from)
            .into(),
        "and what came after it is the next entry"
    );

    let epoch = p.status().epoch;
    p.proxy.cut();
    assert!(eventually(PATIENCE, || {
        let s = p.status();
        s.linked && s.epoch == epoch + 1
    }));
    assert!(p.settle(PATIENCE));
    let frame = p
        .proxy
        .last_payload_where(is_hello)
        .expect("a hello was sent");
    assert!(matches!(Frame::dec(&frame), Ok(Frame::Hello { .. })));
    assert!(eventually(PATIENCE, || f.server.room("alice") == 2));
    assert!(p.proxy.replay_where(is_hello));
    p.author("create_playlist", named("After the second hello"));
    f.converged(&mut [&mut p, &mut q]);
    let s = p.status();
    assert!(
        s.linked && s.epoch == epoch + 1,
        "the same connection: {s:?}"
    );
    assert_eq!(
        f.server.room("alice"),
        2,
        "a repeated hello is not a second connection"
    );
}

// -- 12. the seeded fuzz ---------------------------------------------------------

/// xorshift64*: a schedule that is the seed's and nothing else's.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

/// Prints the schedule when the fuzz panics, so a failure says how to get
/// back to it.
struct Schedule {
    seed: u64,
    steps: Vec<String>,
}

impl Drop for Schedule {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "fleet fuzz: FLEET_SEED={} failed after {} steps:",
                self.seed,
                self.steps.len()
            );
            for (i, s) in self.steps.iter().enumerate() {
                eprintln!("  {i:4} {s}");
            }
        }
    }
}

/// **12. The seeded fuzz.** Three to five peers — alice twice, bob, one
/// nobody signs in on until the schedule says so, and sometimes bob again —
/// and a seeded schedule of mutating, black-holing, releasing, cutting,
/// pausing, killing and restarting peers, killing and restarting the
/// server, and signing in late — at start or on a running peer, under a
/// black hole or a stopped server as well. Then everything is given back
/// and the fleet must converge. Twenty steps by default, 2,000 under
/// `FLEET_LONG=1`; `FLEET_SEED` picks the seed, and a failure prints it
/// with the schedule. A peer signed in late authors what it had pending
/// again under the login, so what it accepted before is owed by its new
/// ids from then on.
///
/// Falsified by leaving the last black hole in place before `converged`
/// (skipping the release): the black-holed peer's cursor is behind the
/// head and it does not converge.
#[test]
fn the_seeded_fuzz_converges() {
    let long = std::env::var("FLEET_LONG").is_ok_and(|v| v == "1");
    let steps = if long { 2_000 } else { 20 };
    let seed: u64 = std::env::var("FLEET_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(if long {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64
                | 1
        } else {
            0x5eed_f1ee7
        });
    fuzz(seed, steps);
}

/// A late sign-in: what the peer had pending stays owed by the ids it was
/// made with (§1.6).
fn sign_in_late(_f: &Fleet, p: &mut PeerProc) -> Result<(), String> {
    p.try_sign_in("alice")?;
    Ok(())
}

fn fuzz(seed: u64, steps: usize) {
    let mut rng = Rng(seed);
    let mut log = Schedule {
        seed,
        steps: vec![],
    };
    println!("fleet fuzz: FLEET_SEED={seed}, {steps} steps");
    let mut f = Fleet::new("fuzz");
    let music = f.server.media.join("music");
    for i in 0..6 {
        common::wav(&music.join(format!("Fuzz/{i}.wav")), 200);
    }
    let n = 3 + rng.below(3);
    let users: [Option<&str>; 5] = [Some("alice"), Some("alice"), Some("bob"), None, Some("bob")];
    let mut peers: Vec<PeerProc> = (0..n).map(|i| f.peer(&format!("p{i}"), users[i])).collect();
    let names = ["Favorites", "Road trip", "Focus"];
    let started = Instant::now();

    for step in 0..steps {
        let i = rng.below(n);
        let roll = rng.below(100);
        let what = match roll {
            0..=49 if peers[i].running() => {
                let p = &mut peers[i];
                let lists = playlists(p);
                let m = media(p);
                if lists.is_empty() || m.is_empty() || rng.chance(15) {
                    let name = names[rng.below(names.len())];
                    let r = p.mutate("create_playlist", named(name));
                    format!("{} create_playlist {name}: {}", p.name, r.is_ok())
                } else {
                    let (list_name, list) = lists[rng.below(lists.len())].clone();
                    let k = rng.below(100);
                    if k < 60 {
                        let x = m[rng.below(m.len())];
                        let r = p.mutate(
                            "add_to_playlist",
                            vec![("playlist_id", id(list)), ("media_id", id(x))],
                        );
                        format!("{} add_to_playlist {list_name}: {}", p.name, r.is_ok())
                    } else if k < 85 {
                        let items =
                            ids_of(&p.query("playlist", vec![("playlist_id", id(list))]), "id");
                        match items.first() {
                            Some(x) => {
                                let r = p.mutate(
                                    "remove_from_playlist",
                                    vec![("playlist_id", id(list)), ("media_id", id(*x))],
                                );
                                format!(
                                    "{} remove_from_playlist {list_name}: {}",
                                    p.name,
                                    r.is_ok()
                                )
                            }
                            None => format!("{} nothing to remove from {list_name}", p.name),
                        }
                    } else {
                        let r = p.mutate("add_all_to_playlist", vec![("playlist_id", id(list))]);
                        format!("{} add_all_to_playlist {list_name}: {}", p.name, r.is_ok())
                    }
                }
            }
            50..=57 => {
                peers[i].proxy.blackhole();
                format!("{} blackhole", peers[i].name)
            }
            58..=67 => {
                peers[i].proxy.pass();
                format!("{} release", peers[i].name)
            }
            68..=73 => {
                peers[i].proxy.cut();
                format!("{} cut", peers[i].name)
            }
            74..=78 => {
                let ms = 100 + rng.below(500) as u64;
                peers[i].proxy.pause(Duration::from_millis(ms));
                format!("{} pause {ms}ms", peers[i].name)
            }
            79..=84 if peers[i].running() => {
                peers[i].kill9();
                format!("{} kill -9", peers[i].name)
            }
            // A peer that never finished signing in signs in as it starts —
            // into a black hole, or a server that is down, too: the sign-in
            // gives up within its patience and the process ends, which is a
            // step like any other.
            85..=91 if !peers[i].running() => match peers[i].try_start() {
                Ok(()) => format!("{} restart", peers[i].name),
                Err(_) => format!("{} restart: its sign-in gave up", peers[i].name),
            },
            92..=94 if f.server.running() => {
                f.server.kill9();
                "server kill -9".to_string()
            }
            95..=98 if !f.server.running() => {
                f.server.start();
                "server restart".to_string()
            }
            // Signing in late, whatever the network: refused within the
            // patience when there is no server to answer.
            99 if peers[i].user.is_none() && peers[i].running() => {
                let got = sign_in_late(&f, &mut peers[i]);
                format!("{} sign_in alice: {}", peers[i].name, got.is_ok())
            }
            _ => "nothing".to_string(),
        };
        log.steps.push(format!("{step}: {what}"));
    }

    // Everything given back.
    log.steps.push("-- release everything --".into());
    if !f.server.running() {
        f.server.start();
    }
    for p in peers.iter_mut() {
        p.proxy.pass();
        if !p.running() {
            p.start();
        }
        if p.user.is_none() {
            sign_in_late(&f, p).unwrap_or_else(|e| panic!("{}: {e}", p.name));
        }
    }
    let t = Instant::now();
    let mut refs: Vec<&mut PeerProc> = peers.iter_mut().collect();
    let done = f.converged_allowing_refusals(&mut refs);
    measured(
        &format!("fuzz of {steps} steps, {n} peers, converge at the end"),
        t.elapsed(),
    );
    measured(
        &format!("fuzz of {steps} steps, whole run"),
        started.elapsed(),
    );
    println!(
        "fleet fuzz: FLEET_SEED={seed}: {} entries, {} accepted, head {} hash {}",
        done.entries.len(),
        f.accepted().len(),
        done.head,
        &done.hash[..12]
    );
}

/// **13. Retention: a peer away past the horizon.** A server keeping fifty
/// entries above its snapshot (`HARKEN_RETAIN_ENTRIES=50`), and three of
/// alice's devices. The third is black-holed and authors ten playlists into
/// the void while the other two author two hundred — so the server
/// compacts past where the third last was. Given the network back, it is
/// below the horizon: it is served the snapshot, rebases its ten onto it,
/// pushes them, and converges; its ten land after all two hundred.
///
/// Falsified by `HARKEN_RETAIN_ENTRIES=100000`: nothing is compacted, and
/// the horizon assertion names 0.
#[test]
fn a_peer_away_past_the_horizon_is_served_the_snapshot() {
    let f = Fleet::with_env(
        "retention",
        vec![("HARKEN_RETAIN_ENTRIES", "50"), ("HARKEN_RETAIN_DAYS", "0")],
    );
    let mut a = f.peer("a", Some("alice"));
    let mut b = f.peer("b", Some("alice"));
    let mut c = f.peer("c", Some("alice"));
    a.author("create_playlist", named("Before"));
    f.converged(&mut [&mut a, &mut b, &mut c]);
    let left_at = c.status().cursor;

    c.proxy.blackhole();
    let mut away = vec![];
    for i in 0..10 {
        away.push(c.author("create_playlist", named(&format!("away-{i}"))));
    }
    assert!(
        eventually(PATIENCE, || f.server.room("alice") == 2),
        "the server closes the black hole"
    );
    let t = Instant::now();
    for i in 0..100 {
        a.author("create_playlist", named(&format!("a-{i}")));
        b.author("create_playlist", named(&format!("b-{i}")));
    }
    assert!(a.settle(PATIENCE) && b.settle(PATIENCE));
    measured(
        "two peers author 200 entries on a retaining server",
        t.elapsed(),
    );
    assert_eq!(
        c.status().pending,
        10,
        "the ten are pending behind the black hole"
    );
    let before = f.server.log_on_disk().expect("a log");
    assert!(
        before.horizon() > left_at,
        "compacted past where the third peer was: horizon {}, it left at {left_at}",
        before.horizon()
    );
    assert!(
        before.entries.len() <= 100,
        "fifty kept, and as many again at most: {}",
        before.entries.len()
    );
    let head_before = before.head_seq();

    let t = Instant::now();
    c.proxy.pass();
    assert!(c.settle(PATIENCE), "the returning peer settles");
    measured(
        "a peer below the horizon: snapshot, rebase, push, confirm",
        t.elapsed(),
    );
    let log = f.server.log_on_disk().expect("a log");
    for i in &away {
        let n = log.seq_of(i).expect("sequenced");
        assert!(
            n > head_before,
            "the away peer's intent landed after the two hundred: {n} <= {head_before}"
        );
    }
    let done = f.converged(&mut [&mut a, &mut b, &mut c]);
    assert_eq!(done.head, head_before + 10);
    println!(
        "fleet: retention kept {} of {} entries (horizon {})",
        done.log.entries.len(),
        done.head,
        done.log.horizon()
    );
}

// -- a peer without a server ----------------------------------------------------------

/// A peer started `--alone` over its own directory, as `user` once it
/// joins a server.
fn alone_peer(f: &Fleet, name: &str, user: &str) -> PeerProc {
    let mut p = f.peer_stopped(name, Some(user));
    p.alone = true;
    p.start();
    p
}

/// `n` songs authored, each with a title of its own.
fn songs(p: &mut PeerProc, tag: &str, n: usize) -> Vec<Id> {
    (0..n)
        .map(|i| p.author("add_song", song(&format!("{tag}-{i:04}"))))
        .collect()
}

/// The media on a playlist, in `pos` order.
fn on_playlist(f: &Fleet, list: Id) -> Vec<Id> {
    playlists_in_pos_order(&f.rows::<PlaylistItem>("playlist_item"))
        .remove(&list)
        .unwrap_or_default()
}

/// What a peer alone authored is its login's when it joins, under the ids
/// it was made with (§1.6): nothing is let go of before the join…
fn joining(_f: &Fleet, _p: &PeerProc) {}

/// …and nothing new is owed after it.
fn joined(_p: &mut PeerProc, _n: i64) {}

/// **14. Alone, then a fresh server.** A peer that never had a server
/// authors fifty songs, a playlist and ten things on it with no server
/// dialled at all; then the same directory is started with `--server` —
/// the join — and a second device of the same person joins too. The
/// server ends with exactly those sixty-one entries, in the order they
/// were sequenced alone, all of them the person's, and every hash is the
/// server's.
///
/// Falsified on v5 by the peer opening an alone directory with `--server`
/// and not joining (the `join` at open skipped): it stays alone at 61 and
/// the server ends empty.
#[test]
fn alone_then_a_fresh_server() {
    let f = Fleet::new("alone-fresh");
    let mut p = alone_peer(&f, "p", "alice");
    songs(&mut p, "alone", 50);
    p.author("create_playlist", named("Alone"));
    let list = playlist(&mut p, "Alone");
    for m in media(&mut p).into_iter().take(10) {
        add(&mut p, list, m);
    }
    let st = p.status();
    assert_eq!((st.cursor, st.pending, st.link.as_str()), (61, 0, "alone"));
    p.quit();
    joining(&f, &p);
    p.alone = false;
    let t = Instant::now();
    p.start();
    joined(&mut p, 61);
    let mut q = f.peer("q", Some("alice"));
    let done = f.converged(&mut [&mut p, &mut q]);
    measured("sixty-one local intents join a fresh server", t.elapsed());
    assert_eq!(done.work(), 61);
    let fns: Vec<String> = done
        .entries
        .iter()
        .filter(|(_, e)| !is_sign_in(e) && !is_look(e))
        .map(|(_, e)| function(e))
        .collect();
    assert!(
        fns[..50].iter().all(|f| f == "add_song")
            && fns[50] == "create_playlist"
            && fns[51..].iter().all(|f| f == "add_to_playlist"),
        "in the order they were sequenced alone: {fns:?}"
    );
    assert!(done
        .entries
        .iter()
        .filter(|(_, e)| !is_sign_in(e) && !is_look(e))
        .all(|(_, e)| e.entry.actor == "alice"));
    assert_eq!(on_playlist(&f, list).len(), 10);
}

/// **15. Alone beside two people using the server.** Alice's and bob's
/// devices author on the server while a third peer, never connected,
/// authors thirty of its own; told to `join` as alice, its local history
/// lands after everything theirs, once, and all three converge.
///
/// Falsified on v5 by `join` answering before the peer joined (the
/// `client.join()` skipped): it answers with nothing pending.
#[test]
fn alone_beside_two_server_users_lands_after_theirs() {
    let f = Fleet::new("alone-beside");
    let mut a = f.peer("a", Some("alice"));
    let mut b = f.peer("b", Some("bob"));
    let mut m = alone_peer(&f, "m", "alice");
    songs(&mut m, "mine", 30);
    songs(&mut a, "a", 20);
    songs(&mut b, "b", 20);
    a.author("create_playlist", named("Theirs"));
    // Not `converged`: the thirty are owed and not yet pushed.
    assert!(a.settle(PATIENCE) && b.settle(PATIENCE));
    let before = f.server.log_on_disk().expect("a log");
    let (head_before, work_before) = (before.head_seq(), before.work());
    joining(&f, &m);
    let url = m.proxy.url();
    let joined = m.ask(
        "join",
        vec![("server", json!(url)), ("user", json!("alice"))],
    );
    assert_eq!(joined, json!({"ok": true, "pending": 30, "cursor": 0}));
    self::joined(&mut m, 30);
    let done = f.converged(&mut [&mut a, &mut b, &mut m]);
    assert_eq!(done.work(), work_before + 30);
    let mine = done
        .entries
        .iter()
        .filter(|(_, e)| function(e) == "add_song" && e.entry.actor == "alice")
        .count();
    assert_eq!(mine, 50, "alice's twenty and the thirty");
    assert!(
        done.entries
            .iter()
            .filter(|(n, e)| *n > head_before && !is_sign_in(e) && !is_look(e))
            .all(|(_, e)| e.entry.actor == "alice"),
        "the thirty after theirs"
    );
}

/// **16. Leave, more, and back — twice.** Alice's two devices share a
/// playlist; one `leave`s, puts three things on it alone, while the other
/// puts three on it online; the first `join`s again, and its three sit
/// after the other's — authored first, sequenced last, the rebase over a
/// fork. Then away and back once more.
///
/// Falsified on v5 by `leave` going alone without the client's `alone`
/// (only the link dropped): its three were pending, not sequenced, and the
/// status said `offline`, not `alone`.
#[test]
fn leave_more_intents_and_join_again() {
    let f = Fleet::new("alone-leave");
    let mut a = f.peer("a", Some("alice"));
    let mut m = f.peer("m", Some("alice"));
    songs(&mut a, "s", 12);
    a.author("create_playlist", named("Shared"));
    f.converged(&mut [&mut a, &mut m]);
    let list = playlist(&mut m, "Shared");
    let ms = media(&mut m);
    add(&mut a, list, ms[0]);
    f.converged(&mut [&mut a, &mut m]);

    let left = m.ask("leave", vec![]);
    assert_eq!(left["ok"], json!(true));
    assert_eq!(m.status().link, "alone");
    for x in &ms[1..4] {
        add(&mut m, list, *x);
    }
    for y in &ms[4..7] {
        add(&mut a, list, *y);
    }
    assert!(a.settle(PATIENCE));
    // The directory's own login, kept: its session's chain, as it is.
    let url = m.proxy.url();
    m.ask(
        "join",
        vec![("server", json!(url)), ("user", json!("alice"))],
    );
    f.converged(&mut [&mut a, &mut m]);
    let want: Vec<Id> = [&ms[0..1], &ms[4..7], &ms[1..4]].concat();
    assert_eq!(
        on_playlist(&f, list),
        want,
        "the away three after the other's"
    );

    m.ask("leave", vec![]);
    add(&mut m, list, ms[7]);
    m.ask(
        "join",
        vec![("server", json!(url)), ("user", json!("alice"))],
    );
    let done = f.converged(&mut [&mut a, &mut m]);
    assert_eq!(on_playlist(&f, list).len(), 8);
    assert_eq!(done.work(), 12 + 1 + 1 + 3 + 3 + 1);
}

/// **17. `kill -9` during a join.** A peer with four hundred local intents
/// joins to completion, which times a join; four more are told to `join`
/// and killed a tenth, a half, nine tenths and ninety-nine hundredths of
/// that time later — each a directory of its own — and started again with
/// `--server`. Whatever the kill cut, the restart finishes the join: the
/// client keeps the chain as pending intents until the server answers each,
/// and one killed before it wrote that reopens alone and joins at open. Every intent is on the server once.
///
/// Falsified on v5 by the peer opening an alone directory with `--server`
/// and not joining: the ones killed before the join was written stay
/// alone at 400 and nothing converges.
#[test]
fn a_join_killed_with_kill9_finishes_on_restart() {
    let f = Fleet::new("alone-kill-join");
    let join = |p: &mut PeerProc| {
        let url = p.proxy.url();
        p.say(
            "join",
            vec![("server", json!(url)), ("user", json!("alice"))],
        );
    };
    let mut cal = alone_peer(&f, "cal", "alice");
    songs(&mut cal, "cal", 400);
    joining(&f, &cal);
    let t = Instant::now();
    join(&mut cal);
    let answered = eventually(PATIENCE, || !cal.answers().is_empty());
    assert!(answered, "the join answers");
    joined(&mut cal, 400);
    let whole = t.elapsed();
    measured("a join of 400 local intents, to its answer", whole);
    let mut peers = vec![cal];
    for (i, frac) in [0.1f64, 0.5, 0.9, 0.99].into_iter().enumerate() {
        let mut p = alone_peer(&f, &format!("k{i}"), "alice");
        songs(&mut p, &format!("k{i}"), 400);
        joining(&f, &p);
        join(&mut p);
        std::thread::sleep(whole.mul_f64(frac));
        p.kill9();
        let replica = p.dir.join("replica");
        println!(
            "fleet: killed at {:.0}% of a join: its own state {} B",
            frac * 100.0,
            ["local.kv", "local.log"]
                .iter()
                .map(|n| std::fs::metadata(replica.join(n)).map_or(0, |m| m.len()))
                .sum::<u64>(),
        );
        p.alone = false;
        p.start();
        joined(&mut p, 400);
        peers.push(p);
    }
    let mut refs: Vec<&mut PeerProc> = peers.iter_mut().collect();
    let done = f.converged(&mut refs);
    assert_eq!(done.work(), 5 * 400);
}

/// **18. A peer alone killed mid-append.** Three hundred songs are said to
/// a peer alone without waiting, and it is killed a hundred and twenty
/// milliseconds in. Started again alone, it is whole: nothing is pending,
/// its confirmed store is its view, and every song in its library is an
/// entry of its local history — the last whole one is where it stops. Then
/// it joins, and the server has what it kept, every intent it answered
/// before the kill among them.
///
/// Falsified on v5 by the peer alone keeping its replica in memory rather
/// than on disk: it came back at 0, short of what it had answered. A torn
/// last record is `ark-client`'s storage tests' to hold; no kill here was
/// seen to land inside one.
#[test]
fn a_peer_alone_killed_mid_append_reopens_whole() {
    let f = Fleet::new("alone-kill-append");
    let mut p = alone_peer(&f, "p", "alice");
    for i in 0..300 {
        p.say(
            "mutate",
            vec![
                ("name", json!("add_song")),
                ("args", args(song(&format!("burst-{i:04}")))),
            ],
        );
    }
    std::thread::sleep(Duration::from_millis(120));
    let answered = p
        .answers()
        .iter()
        .filter(|a| a["ok"] == json!(true))
        .count() as i64;
    p.kill9();
    p.start();
    let (cursor, hash, view) = p.hash();
    assert_eq!(hash, view, "the confirmed store is the view");
    assert_eq!(p.status().pending, 0);
    let kept = media(&mut p).len() as i64;
    assert_eq!(kept, cursor, "every song an entry, and every entry a song");
    assert!(cursor >= answered, "{cursor} kept, {answered} answered");
    println!("fleet: killed mid-append: {answered} answered, {cursor} kept of 300");
    joining(&f, &p);
    let url = p.proxy.url();
    p.ask(
        "join",
        vec![("server", json!(url)), ("user", json!("alice"))],
    );
    assert!(p.settle(PATIENCE));
    let log = f.server.log_on_disk().expect("a log");
    assert_eq!(log.work(), cursor, "the server has what it kept");
    for l in log.entries.iter().filter(|l| !is_sign_in(l) && !is_look(l)) {
        p.owe(l.entry.id.0, "add_song, kept past the kill");
    }
    f.converged(&mut [&mut p]);
}

/// **19. Two thousand local intents join, timed.** A peer alone authors
/// two thousand songs, then is started with `--server`: the time from the
/// start to every intent on the server and the peer converged is printed.
///
/// Falsified on v5 as 14 is: no join at open, and the server ends empty.
///
/// A debug build waits ten times [`PATIENCE`] to converge: the join takes
/// about 10 s in release, as it did before K2 (11 s at `0179f02`), and
/// about 100 s unoptimised, so in debug this is a convergence test and only
/// the release number is a measurement.
#[test]
fn two_thousand_local_intents_join_timed() {
    let f = Fleet::new("alone-2000");
    let mut p = alone_peer(&f, "p", "alice");
    let t = Instant::now();
    songs(&mut p, "big", 2_000);
    measured("a peer alone authors 2,000 songs", t.elapsed());
    p.quit();
    joining(&f, &p);
    p.alone = false;
    let t = Instant::now();
    p.start();
    joined(&mut p, 2_000);
    let patience = PATIENCE * if cfg!(debug_assertions) { 10 } else { 1 };
    let done = f.converged_within(&mut [&mut p], patience);
    measured(
        "2,000 local intents: start with --server to converged",
        t.elapsed(),
    );
    assert_eq!(done.work(), 2_000);
}

/// **The library's guard, judged at the server.** No role configured:
/// bob's song is refused on his own device by `is_library`. The server
/// restarts with `HARKEN_ROLES=library=alice`; signed in again, alice's
/// device is told she holds it and her song is sequenced — logged with the
/// role the server stamped. Restarted once more with the role revoked, her
/// device still believes it and takes her next song; the server, which
/// stamps the entry with the roles it now gives her login, refuses it with
/// the guard's own words, and it is in the log nowhere.
///
/// Falsified on v5 by the fleet's peers carrying `--roles library` here:
/// bob's song is taken on his own device.
#[test]
fn a_role_granted_by_a_restart_and_revoked_by_another() {
    let mut f = Fleet::with_env("roles", vec![("HARKEN_ROLES", "")]);
    let peer = |f: &Fleet, name: &str| {
        let mut p = f.peer_stopped(name, Some(name));
        p.roles = vec![];
        p.start();
        p
    };
    let mut alice = peer(&f, "alice");
    let mut bob = peer(&f, "bob");
    let refused = bob
        .mutate("add_song", song("bob's"))
        .expect_err("a library write by a login without the role");
    assert!(
        refused.contains("only the library writes the library"),
        "refused on bob's own device by the guard: {refused}"
    );
    alice.author("create_playlist", named("Alice's"));

    // Granted by configuration, at a restart; a fresh login holds it.
    f.server.upgrade(vec![("HARKEN_ROLES", "library=alice")]);
    alice.sign_in("alice");
    let kept = alice.author("add_song", song("alice's"));
    let done = f.converged(&mut [&mut alice, &mut bob]);
    assert_eq!(
        done.log.rows::<Media>("media").len(),
        1,
        "alice's song is in the log, bob's nowhere"
    );
    let at = done.log.seq_of(&kept).expect("sequenced");
    let e = &done.log.entries.iter().find(|l| l.seq == at).unwrap().entry;
    assert_eq!(
        e.roles,
        ["library".to_string()],
        "logged with the role the server stamped"
    );

    // Revoked at another restart, while her device believes it still
    // holds it: the device takes the song, the authority does not.
    f.server.upgrade(vec![("HARKEN_ROLES", "")]);
    let late = alice.author("add_song", song("too late"));
    assert!(
        eventually(PATIENCE, || alice.rejections().iter().any(|(i, why)| *i
            == late
            && why.contains("only the library writes the library"))),
        "the server refused the song with the guard's words: {:?}",
        alice.rejections()
    );
    let done = f.converged_allowing_refusals(&mut [&mut alice, &mut bob]);
    assert_eq!(
        done.log.rows::<Media>("media").len(),
        1,
        "and it landed nowhere"
    );
    assert!(done.log.seq_of(&kept).is_some());
    assert!(done.log.seq_of(&late).is_none());
    for l in &done.log.entries {
        if l.entry.actor == "alice" && l.seq > at {
            assert!(
                l.entry.roles.is_empty(),
                "entry {}: after the revocation alice's login holds nothing",
                l.seq
            );
        }
    }
}
