//! The process fleet (`docs/plan-fleet.md` §2): the real `harken-server`,
//! real `harken-peer`s, and the network between them taken away in every
//! way a laptop lid and a bad hotel wifi take it away — then given back,
//! and every scenario ends on [`Fleet::converged`]: every replica's
//! confirmed state hashes as the server's log does, and every accepted
//! intent is in that log exactly once.
//!
//! `ark::sim` already holds the engine to this in one process. What only
//! processes can reach is everything outside it: the WebSocket and its
//! keepalive, a server killed and restarted from its data directory, a peer
//! killed between writing its intent and pumping, a connection cut in the
//! middle of a page, the sign-in before the first frame, the scanner
//! authoring while a client is away.
//!
//! Each scenario was falsified once by breaking what it holds; its doc
//! comment says how. A scenario that finds a bug in `rust/ark*` is not
//! fixed there: it stays, failing, `#[ignore = "witness: …"]`, and the
//! finding goes to the coordinator (plan §4, §5).
//!
//! `cargo test -p harken-server --test fleet -- --nocapture` prints the
//! timings; `FLEET_LONG=1` runs the seeded fuzz for 2,000 steps instead of
//! 20, and `FLEET_SEED=n` picks its seed.

mod common;
mod support {
    pub mod fleet;
    pub mod proxy;
}

// `arkc backup`, `restore` and `verify-log`, as the binary runs them
// (`docs/plan-db.md` D6): the same file, so the scenario below drives the
// code an operator runs against a real server's directory. Formatted as
// the crate it belongs to formats it (`rust/rustfmt.toml`), not as this one.
#[allow(dead_code)]
#[rustfmt::skip]
#[path = "../../../rust/ark/src/bin/ops.rs"]
mod ops;

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use ark::protocol::ClientMsg;
use ark::value::{Id, Value};
use support::fleet::*;

/// Any id, for a query argument whose value does not matter (`library`
/// asks which playlist to mark; none).
const NO_PLAYLIST: Id = [0; 16];

/// What a peer's library holds, as media ids in library order.
fn media(p: &mut PeerProc) -> Vec<Id> {
    ids_of(
        &p.query("library", vec![("playlist_id", Value::Id(NO_PLAYLIST))]),
        "id",
    )
}

/// The files a peer's library holds.
fn files(p: &mut PeerProc) -> BTreeSet<String> {
    p.query("library", vec![("playlist_id", Value::Id(NO_PLAYLIST))])
        .iter()
        .map(|r| match &r["file"] {
            Value::Text(t) => t.to_string(),
            other => panic!("{other:?}"),
        })
        .collect()
}

/// The caller's playlists on a peer: `(name, id)` in the order made.
fn playlists(p: &mut PeerProc) -> Vec<(String, Id)> {
    p.query("playlists", vec![])
        .iter()
        .map(|r| match (&r["name"], &r["id"]) {
            (Value::Text(n), Value::Id(i)) => (n.to_string(), *i),
            other => panic!("{other:?}"),
        })
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
        vec![("playlist_id", Value::Id(list)), ("media_id", Value::Id(m))],
    )
}

fn named(name: &str) -> Vec<(&'static str, Value)> {
    vec![("name", Value::text(name))]
}

/// The playlist a `create_playlist` entry made: its auto `id`.
fn made(e: &ark::log::Entry) -> Id {
    match e.autos.get("id") {
        Some(Value::Id(i)) => *i,
        other => panic!("create_playlist with no id auto: {other:?}"),
    }
}

fn arg_id(e: &ark::log::Entry, k: &str) -> Id {
    match e.args.get(k) {
        Some(Value::Id(i)) => *i,
        other => panic!("{k}: {other:?}"),
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
                vec![
                    ("playlist_id", Value::Id(shared)),
                    ("media_id", Value::Id(m[0])),
                ],
            );
            c.author(
                "add_all_to_playlist",
                vec![("playlist_id", Value::Id(bobs))],
            );
        }
        if round == 5 {
            a.author(
                "remove_from_playlist",
                vec![
                    ("playlist_id", Value::Id(shared)),
                    ("media_id", Value::Id(m[6])),
                ],
            );
        }
    }
    let t = Instant::now();
    let done = f.converged(&mut [&mut a, &mut b, &mut c]);
    measured("three peers online converge", t.elapsed());

    // The model: the log, in order.
    let by_pos: Vec<Id> = {
        let mut rows = f.rows("media");
        rows.sort_by_key(|r| match r["pos"] {
            Value::Int(n) => n,
            _ => 0,
        });
        ids_of(&rows, "id")
    };
    let mut model: BTreeMap<Id, Vec<Id>> = BTreeMap::new();
    for (_, e) in &done.entries {
        match f.function(e).as_str() {
            "add_to_playlist" => {
                let list = model.entry(arg_id(e, "playlist_id")).or_default();
                let m = arg_id(e, "media_id");
                if !list.contains(&m) {
                    list.push(m);
                }
            }
            "remove_from_playlist" => {
                let m = arg_id(e, "media_id");
                model
                    .entry(arg_id(e, "playlist_id"))
                    .or_default()
                    .retain(|x| *x != m);
            }
            "add_all_to_playlist" => {
                let list = model.entry(arg_id(e, "playlist_id")).or_default();
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
        playlists_in_pos_order(&f.rows("playlist_item")),
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
    f.converged(&mut [&mut a, &mut b, &mut c]);
    measured("converge after a black hole of twenty intents", t.elapsed());

    let order = &playlists_in_pos_order(&f.rows("playlist_item"))[&mix];
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
/// nothing twice, and the log read back whole after the stop (`persist.rs`
/// renames a snapshot into place and appends a journal it reads to its
/// last whole record, so a stop never leaves half of either).
///
/// Falsified by starting again over an emptied data directory: what was
/// accepted before the stop is not in the log. (Before R6 it failed
/// earlier — the peers' cursors ahead of an empty log, the new server's
/// acks naming sequences they believed they were past, nothing pending
/// ever confirmed; that case is a scenario of its own now, 3c.)
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
    assert!(
        !f.server.data.join(".log.ark-log.tmp").exists(),
        "and nothing is left beside it"
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
    assert_eq!(done.head, 1 + 45, "one song and forty-five playlists");
    println!(
        "fleet: the log on disk (snapshot and journal) is {} bytes for {} entries ({} bytes an entry)",
        f.server.log_bytes(),
        done.head,
        f.server.log_bytes() / done.head as u64
    );
}

/// **3c. A server that lost its log re-bases everyone onto what it has.**
/// Three peers converge on a song and fifteen playlists; the server is
/// stopped, its log — snapshot and journal — deleted (the sessions are
/// kept: this is a lost log, not a lost server), and the peers author nine
/// more against nothing. Started again with an empty log, it answers each
/// peer's `Hello` — whose cursor, 16, is past its head, 0 — as it answers
/// one below its horizon: its store at the head as a snapshot
/// (`docs/plan-perf.md` R6). Each re-opens from it, its nine pending on top,
/// already pushed after its `Hello`; they are sequenced, acknowledged and
/// confirmed, and the fleet converges on a log of exactly those nine. What
/// the lost log held is gone from every peer — the song, the fifteen —
/// because there is one log and this is it. The server's own scanner peer,
/// whose replica is past the head too, is re-based the same way.
///
/// **And the half a cursor cannot tell** (`docs/plan-perf.md` Round 4):
/// the log is lost again, the three author two more each and go away, and
/// the server — started on a new, empty log — is not idle while they are
/// gone: a fourth device, `d`, sequences twelve on it. When the three come
/// back their cursors, 9, are *below* its head, 12, and a sequence number
/// alone says they are three entries behind on this log — which they never
/// held a byte of. Their `Hello` names the log they were confirmed on; this
/// one has another name, so each is answered with its snapshot at the head
/// as before, and the fleet converges on `d`'s twelve and the six. Nothing
/// of the second log — nor the first — is left on any peer.
///
/// Falsified by serving a cursor past the head nothing, as before
/// (`sent >= head` in `ark::protocol::Server::fanout`): not converged —
/// every peer stays at 16, ahead of a head of 9, its three pending. The
/// second half is falsified by a server that never compares the names
/// (`elsewhere` always false in `Server::recv`): the three are paged 10 to
/// 12 of the new log on top of the old one's nine, and are never
/// converged — their hashes are not the log's.
#[test]
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
    for gone in [
        ark_server::persist::path_of(&f.server.data),
        ark_server::persist::journal_path_of(&f.server.data),
    ] {
        std::fs::remove_file(&gone).unwrap_or_else(|e| panic!("{}: {e}", gone.display()));
    }
    burst(&mut [&mut a, &mut b, &mut c], 1, 3);
    assert_eq!(pending(&mut [&mut a, &mut b, &mut c]), 9);
    let t = Instant::now();
    f.server.start();
    f.lost(before.entries.iter().map(|(_, e)| e.id));
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
    for gone in [
        ark_server::persist::path_of(&f.server.data),
        ark_server::persist::journal_path_of(&f.server.data),
    ] {
        std::fs::remove_file(&gone).unwrap_or_else(|e| panic!("{}: {e}", gone.display()));
    }
    burst(&mut [&mut a, &mut b, &mut c], 2, 2);
    assert_eq!(pending(&mut [&mut a, &mut b, &mut c]), 6);
    for p in [&mut a, &mut b, &mut c] {
        p.quit();
    }
    let t = Instant::now();
    f.server.start();
    f.lost(done.entries.iter().map(|(_, e)| e.id));
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
/// What it holds is that an acknowledgement follows the write: the hub
/// appends what a message moved to `log.ark-journal` and syncs it before
/// it sends the `Ack` to the author or the `Batch` to anyone else (R3 in
/// `rust/ark-server`). It was a witness until then — `Hub::after` sent
/// first and rewrote the whole `log.ark-log` after, and a kill between
/// left peers confirmed past the file, the restarted server reusing their
/// sequences, the hashes different for ever and an acked intent stuck
/// pending.
///
/// Falsified by that code, before R3 (up to `a3ef488`): round 0 fails —
/// "a confirmed up to 1501 and the log on disk holds 1500" — three runs of
/// three.
#[test]
fn a_server_killed_mid_stream_loses_nothing() {
    let mut f = Fleet::seeded("server-kill", |s| {
        // Songs, not playlists: naming a playlist reads every other one of
        // its owner's, and 1,500 of them is a cubic seeding.
        for i in 0..1500 {
            s.author("carol", "add_song", song(&format!("seed-{i:04}")));
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
/// (b) Right after the server acked an intent and before the peer wrote
/// down that it had — made exact by restoring the peer's directory as it
/// was before the ack — the restarted peer pushes it again, the server
/// answers `Duplicate` with its sequence, and the peer's cursor still moves
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
/// through a peer, which is slow enough to be a finding of its own (see
/// "Measured" in the plan) — and a fresh peer joining it with its
/// connection cut after a third of the bytes a whole sync takes, and cut
/// again a third further on: torn frames, mid-page. It resumes each time
/// from its cursor, applies nothing twice (its state hashes as the log's),
/// and converges. A second fresh peer with nothing cut is what the sync
/// time is measured on.
///
/// Falsified by arming the second cut at twice a whole sync: it never
/// fires, because what is left is less — so the two cuts this asserts are
/// cuts in the middle of the sync, not after it. (The first version armed
/// the second cut once the first had fired, and under load a peer quick to
/// dial again had synced before it was armed: the proxy queues cuts now,
/// and each is a sixth of a sync rather than a third, for the reason the
/// code gives.)
#[test]
fn a_first_sync_cut_mid_page_resumes_at_its_cursor() {
    let long = std::env::var("FLEET_LONG").is_ok_and(|v| v == "1");
    let seed = |s: &mut Seeder| {
        let mut media = vec![];
        for i in 0..500 {
            media.push(s.author("alice", "add_song", song(&format!("page-{i:04}")))["id"].clone());
        }
        let lists: Vec<Value> = (0..10)
            .map(|i| {
                s.author("alice", "create_playlist", named(&format!("List {i}")))["id"].clone()
            })
            .collect();
        for i in 0..990 {
            s.author(
                "alice",
                "add_to_playlist",
                vec![
                    ("playlist_id", lists[i % 10].clone()),
                    ("media_id", media[i % 500].clone()),
                ],
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
        "fleet: the log on disk (snapshot and journal) is {} bytes for {head} entries ({} bytes an entry)",
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
    // byte after the first, and each a sixth of what a whole sync took —
    // about a page. Under load `whole` can come out larger than the sync
    // really is (a peer too busy applying a page to answer the keepalive
    // is dropped and fetches from its cursor again), and at a third the
    // second cut could find less left than its budget and never fire; a
    // sixth leaves room for `whole` to be more than twice the truth.
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

/// **6. Signing in later.** A peer nobody has signed in on, with the
/// network black-holed besides, adds a song, makes a playlist and puts the
/// song on it — as nobody. Given the network back, alice signs in on it:
/// everything it made is alice's on every peer (`user_id` on each row), and
/// every entry is in the log under alice and the session that sign-in
/// made.
///
/// Falsified by signing in as bob: the other peer, alice's, does not see
/// the playlist, and the actor assertion names bob.
#[test]
fn work_done_signed_out_is_the_signers_everywhere() {
    let f = Fleet::new("sign-in-later");
    let mut a = f.peer("a", Some("alice"));
    let mut s = f.peer("s", None);
    s.proxy.blackhole();
    let mine = vec![
        s.author("add_song", song("nobody's")),
        s.author("create_playlist", named("Made before signing in")),
    ];
    let list = playlist(&mut s, "Made before signing in");
    let m = media(&mut s);
    let mut mine = mine;
    mine.push(add(&mut s, list, m[0]));
    let st = s.status();
    assert!(st.user.is_empty() && st.pending == 3, "{st:?}");

    s.proxy.pass();
    let (user, session) = s.sign_in("alice");
    assert_eq!(user, "alice");
    let done = f.converged(&mut [&mut a, &mut s]);
    for (_, e) in done.entries.iter().filter(|(_, e)| mine.contains(&e.id)) {
        assert_eq!(
            (e.actor.as_str(), e.session.as_str()),
            ("alice", session.as_str()),
            "{}",
            f.function(e)
        );
    }
    assert_eq!(
        done.entries
            .iter()
            .filter(|(_, e)| mine.contains(&e.id))
            .count(),
        3
    );
    assert_eq!(
        playlist(&mut a, "Made before signing in"),
        list,
        "alice's other device has it"
    );
    for table in ["playlist", "playlist_item", "media"] {
        for row in f.rows(table) {
            assert_eq!(row["user_id"], Value::text("alice"), "{table}: {row:?}");
        }
    }
}

/// **7. The same playlist name, made on two black-holed devices.** Both
/// of alice's devices make "Favorites" with no network. Given it back, one
/// is "Favorites (1)" on every peer — and which one is decided by the log:
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
    let ia = a.author("create_playlist", named("Favorites"));
    let ib = b.author("create_playlist", named("Favorites"));
    assert_eq!(playlists(&mut a).len(), 1, "each sees only its own");
    b.proxy.pass();
    a.proxy.pass();
    let done = f.converged(&mut [&mut a, &mut b]);
    let seq = |id: Id| {
        done.entries
            .iter()
            .find(|(_, e)| e.id == id)
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
                ("Favorites".to_string(), first),
                ("Favorites (1)".to_string(), second)
            ],
            "{}",
            p.name
        );
    }
}

/// **8. Turned away.** Alice's device is linked and idle when its session
/// is revoked at the server (`/auth/logout` with its token). The server
/// closes that socket at once, saying why (`Denied` with
/// `ark_server::REVOKED`, `docs/plan-perf.md` R6): nothing cuts it, and
/// the peer does not have to dial again to learn it. It says `denied`,
/// keeps its store, stops dialling, and goes on authoring — three things,
/// pending under the revoked login. Alice's other device is another login
/// and is not touched. Alice signs in on it again, and what was pending —
/// authored under the revoked login — is taken under the new one's person
/// (`with_owns`) and converges, still carrying the session it was made in.
///
/// Then a sign-in into a black hole: signed out and signing in again with
/// the network black-holed, the answer is a refusal within the peer's
/// sign-in patience rather than a wait for ever; and a device with no
/// login yet, started behind a black hole, gives up and exits within it
/// (R6 — `login` had no timeout, which is why the fuzz could not do this).
///
/// Falsified three ways: by signing in again as bob (the authority
/// refuses the three as `not yours`, and `converged` fails on the
/// refusals — it first passed that, when `converged` forgave any refusal);
/// by the hub not closing a revoked session (`Hub::revoked` doing
/// nothing): the peer is still linked and not denied five seconds on; and
/// by the fleet's peers signing in on ark-auth's own patience (no
/// `--auth-patience-ms`): the refusal takes 21 s against a bound of five
/// — and before ark-auth had a timeout it was never answered at all.
#[test]
fn a_peer_turned_away_keeps_its_work_until_signed_in_again() {
    let f = Fleet::new("turned-away");
    let mut p = f.peer("p", Some("alice"));
    let mut q = f.peer("q", Some("alice"));
    p.author("add_song", song("kept"));
    f.converged(&mut [&mut p, &mut q]);
    let old = p.status().session;
    let dials = p.proxy.accepted();
    let token = p.token();
    let t = Instant::now();
    ureq::post(&format!("{}/auth/logout", f.server.url()))
        .set("Authorization", &format!("Bearer {token}"))
        .call()
        .expect("the session is revoked");
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
    assert_eq!(st.denied.as_deref(), Some(ark_server::REVOKED));
    assert_eq!(
        (
            st.linked,
            st.link.as_str(),
            p.proxy.cuts(),
            p.proxy.accepted()
        ),
        (false, "idle", 0, dials),
        "closed by the server, on the socket it had: {st:?}"
    );
    let other = q.status();
    assert!(
        other.linked && other.denied.is_none(),
        "the other login is not touched: {other:?}"
    );

    let list_id = p.author("create_playlist", named("Kept through a revocation"));
    let list = playlist(&mut p, "Kept through a revocation");
    let m = media(&mut p);
    let mine = [
        list_id,
        add(&mut p, list, m[0]),
        p.author("create_playlist", named("And this")),
    ];
    let (_, _, view) = p.hash();
    assert_eq!(p.status().pending, 3);
    assert_eq!(p.hash().2, view, "the store is kept");
    // Not dialling is an absence, so it is watched for a while: a second
    // is three backoffs at the fleet's pace.
    assert!(
        !eventually(Duration::from_secs(1), || p.proxy.accepted() > dials),
        "a denied peer does not dial again"
    );

    let (_, new) = p.sign_in("alice");
    assert_ne!(new, old);
    let done = f.converged(&mut [&mut p, &mut q]);
    let taken: Vec<_> = done
        .entries
        .iter()
        .filter(|(_, e)| mine.contains(&e.id))
        .collect();
    assert_eq!(taken.len(), 3, "all three were taken");
    for (_, e) in taken {
        assert_eq!(
            (e.actor.as_str(), e.session.as_str()),
            ("alice", old.as_str())
        );
    }

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
/// Copies under *new* names are new songs, by that same rule; the plan
/// said otherwise until R6 corrected it, and this asserts both halves.
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
                use ark::store::Store;
                l.state_at(l.head_seq())
                    .unwrap()
                    .scan("media")
                    .into_iter()
                    .filter_map(|r| match &r["file"] {
                        Value::Text(t) => Some(t.to_string()),
                        _ => None,
                    })
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
        .all(|(_, e)| e.actor == harken_server::library::ACCOUNT));

    // The same three again, rewritten in place: a watch event each, and no
    // song. An absence, so it is watched for as long as the scanner's own
    // settling takes, twice over.
    for name in ["one", "two", "three"] {
        common::wav(&music.join(format!("Fleet/{name}.wav")), 300);
    }
    let head = done.head;
    let moved = || f.server.log_on_disk().map_or(0, |l| l.head_seq()) != head;
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
            .map_or(0, |l| l.head_seq())
            != head),
        "a rescan authors nothing"
    );
    let done = f.converged(&mut [&mut p]);
    assert_eq!(done.head, head);

    // Under new names they are new songs: the path is the identity.
    for name in ["one", "two", "three"] {
        common::wav(&music.join(format!("Fleet/{name} (copy).wav")), 300);
    }
    assert!(eventually(PATIENCE, || songs(&f).len() == 6));
    f.converged(&mut [&mut p]);
    assert_eq!(files(&mut p).len(), 6);
}

/// **10. The keepalive.** A peer black-holed with no FIN: the server,
/// pinging every 300ms here, closes it after three go unanswered, and the
/// account's live room forgets the device (`/healthz`); the peer notices
/// the silence on its own and stops being linked. Given the network back
/// it dials again and converges what it made meanwhile.
///
/// Falsified by `HARKEN_KEEPALIVE_MISSED=1000`: the server never closes
/// the black-holed socket, and the room keeps two peers.
#[test]
fn a_black_holed_socket_is_closed_by_the_keepalive() {
    let f = Fleet::new("keepalive");
    let mut p = f.peer("p", Some("alice"));
    let mut q = f.peer("q", Some("alice"));
    f.converged(&mut [&mut p, &mut q]);
    assert!(
        eventually(PATIENCE, || f.server.room("alice") == 2),
        "two devices in alice's room"
    );
    let t = Instant::now();
    p.proxy.blackhole();
    p.author("create_playlist", named("Said into the void"));
    assert!(
        eventually(PATIENCE, || f.server.room("alice") == 1),
        "the server closes the socket and the room forgets it"
    );
    measured("the server closes a black-holed socket", t.elapsed());
    assert!(
        eventually(PATIENCE, || !p.status().linked),
        "the peer notices on its own"
    );
    measured("the peer notices a black hole", t.elapsed());
    let t = Instant::now();
    p.proxy.pass();
    f.converged(&mut [&mut p, &mut q]);
    measured(
        "converge after the keepalive closed a black hole",
        t.elapsed(),
    );
    assert!(
        eventually(PATIENCE, || f.server.room("alice") == 2),
        "and it is back in the room"
    );
}

/// **11. Replayed frames.** The last push a peer sent, sent again on the
/// same connection — the server dedupes it by entry id, and the
/// log holds the intent once — and a `Hello`, which on one connection is
/// the log paging, not the device leaving and arriving: the room still has
/// two devices, the peer is still linked on the same epoch, and it
/// converges.
///
/// Falsified by sending, in place of the replay, that `Push` with its entry
/// id edited (`Proxy::inject`): the log has an entry no peer accepted, and
/// `converged` names it.
#[test]
fn a_replayed_push_or_hello_changes_nothing() {
    let f = Fleet::new("replay");
    let mut p = f.peer("p", Some("alice"));
    let mut q = f.peer("q", Some("alice"));
    f.converged(&mut [&mut p, &mut q]);

    let once = p.author("create_playlist", named("Once"));
    assert!(p.settle(PATIENCE));
    let head = p.status().cursor;
    // The push, which a `Verify` follows once it is answered: a linked peer
    // verifies after every settle (`docs/plan-db.md` D3).
    let push = |b: &[u8]| {
        ark::canon::decode(b)
            .ok()
            .and_then(|v| ClientMsg::from_value(&v).ok())
            .is_some_and(|m| matches!(m, ClientMsg::Push { .. }))
    };
    let frame = p.proxy.last_payload_where(push).expect("a push was sent");
    let msg = ClientMsg::from_value(&ark::canon::decode(&frame).unwrap()).unwrap();
    assert!(
        matches!(&msg, ClientMsg::Push { entries } if entries.iter().any(|e| e.id == once)),
        "{msg:?}"
    );
    assert!(p.proxy.replay_where(push));
    // Whatever the peer says next travels behind the replay on the same
    // connection, so once it is answered the replay has been.
    p.author("create_playlist", named("After the replay"));
    let done = f.converged(&mut [&mut p, &mut q]);
    assert_eq!(
        done.head,
        head + 1,
        "the replayed push is not a second entry"
    );

    let epoch = p.status().epoch;
    p.proxy.cut();
    assert!(eventually(PATIENCE, || {
        let s = p.status();
        s.linked && s.epoch == epoch + 1
    }));
    assert!(p.settle(PATIENCE));
    let frame = p.proxy.last_payload().expect("a frame was sent");
    let msg = ClientMsg::from_value(&ark::canon::decode(&frame).unwrap()).unwrap();
    assert!(matches!(msg, ClientMsg::Hello { .. }), "{msg:?}");
    assert!(eventually(PATIENCE, || f.server.room("alice") == 2));
    assert!(p.proxy.replay_last());
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
        "a repeated hello is paging, not a departure"
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Net {
    Pass,
    Blackhole,
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
/// black hole or a stopped server as well, now that a sign-in gives up
/// (R6). Then everything is given back and the
/// fleet must converge. Twenty steps by default, 2,000 under
/// `FLEET_LONG=1`; `FLEET_SEED` picks the seed, and a failure prints it
/// with the schedule.
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
    let mut net = vec![Net::Pass; n];
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
                            vec![("playlist_id", Value::Id(list)), ("media_id", Value::Id(x))],
                        );
                        format!("{} add_to_playlist {list_name}: {}", p.name, r.is_ok())
                    } else if k < 85 {
                        let items = ids_of(
                            &p.query("playlist", vec![("playlist_id", Value::Id(list))]),
                            "id",
                        );
                        match items.first() {
                            Some(x) => {
                                let r = p.mutate(
                                    "remove_from_playlist",
                                    vec![
                                        ("playlist_id", Value::Id(list)),
                                        ("media_id", Value::Id(*x)),
                                    ],
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
                        let r = p.mutate(
                            "add_all_to_playlist",
                            vec![("playlist_id", Value::Id(list))],
                        );
                        format!("{} add_all_to_playlist {list_name}: {}", p.name, r.is_ok())
                    }
                }
            }
            50..=57 => {
                peers[i].proxy.blackhole();
                net[i] = Net::Blackhole;
                format!("{} blackhole", peers[i].name)
            }
            58..=67 => {
                peers[i].proxy.pass();
                net[i] = Net::Pass;
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
            // gives up within its patience (R6) and the process ends, which
            // is a step like any other. It was excluded while `login` had no
            // timeout and would have waited for ever.
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
                let got = peers[i].try_sign_in("alice");
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
    for (i, p) in peers.iter_mut().enumerate() {
        p.proxy.pass();
        net[i] = Net::Pass;
        if !p.running() {
            p.start();
        }
        if p.user.is_none() {
            p.sign_in("alice");
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
/// entries (`HARKEN_RETAIN_ENTRIES=50`) and holding the log for no session
/// that is not connected (`HARKEN_RETAIN_DAYS=0`), and three of alice's
/// devices. The third is black-holed and authors ten playlists into the
/// void while the other two author two hundred — so the server compacts
/// past where the third last was (`ark::retention`, `docs/plan-alone.md`
/// §3). Given the network back, it is below the horizon: it is served the
/// snapshot, rebases its ten onto it, pushes them, and converges; its ten
/// land after all two hundred.
///
/// An accepted intent whose entry was compacted away is in `log.ids`
/// rather than `log.entries`, and `converged` reads both.
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
        "the server closes the black hole: it no longer holds the log"
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
        before.entries.len() <= 75,
        "fifty kept, and half again at most: {}",
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
    for (id, who) in f.accepted() {
        assert!(
            log.seq_of(&id).is_some(),
            "{who} {} is in the log",
            ark::value::hex(&id)
        );
    }
    for id in &away {
        let n = log.seq_of(id).expect("sequenced");
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

// -- a peer without a server (`docs/plan-alone.md` §4) ----------------------------------

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
    playlists_in_pos_order(&f.rows("playlist_item"))
        .remove(&list)
        .unwrap_or_default()
}

/// **14. Alone, then a fresh server.** A peer that never had a server
/// authors fifty songs, a playlist and ten things on it with no server
/// dialled at all; then the same directory is started with `--server` —
/// the join — and a second device of the same person joins too. The
/// server ends with exactly those sixty-one entries, in the order they
/// were sequenced alone, all of them the person's, and every hash is the
/// server's.
///
/// Falsified by `Replica::fork_back` re-queuing nothing: the server ends
/// empty and the joined peer's hash is not its store's.
#[test]
fn alone_then_a_fresh_server() {
    let f = Fleet::new("alone-fresh");
    let mut p = alone_peer(&f, "p", "alice");
    let mut made = songs(&mut p, "alone", 50);
    made.push(p.author("create_playlist", named("Alone")));
    let list = playlist(&mut p, "Alone");
    for m in media(&mut p).into_iter().take(10) {
        made.push(add(&mut p, list, m));
    }
    let st = p.status();
    assert_eq!((st.cursor, st.pending, st.link.as_str()), (61, 0, "alone"));
    p.quit();
    p.alone = false;
    let t = Instant::now();
    p.start();
    let mut q = f.peer("q", Some("alice"));
    let done = f.converged(&mut [&mut p, &mut q]);
    measured("sixty-one local intents join a fresh server", t.elapsed());
    assert_eq!(done.head, 61);
    let order: Vec<Id> = done.entries.iter().map(|(_, e)| e.id).collect();
    assert_eq!(order, made, "in the order they were sequenced alone");
    assert!(done.entries.iter().all(|(_, e)| e.actor == "alice"));
    assert_eq!(on_playlist(&f, list).len(), 10);
}

/// **15. Alone beside two people using the server.** Alice's and bob's
/// devices author on the server while a third peer, never connected,
/// authors thirty of its own; told to `join` as alice, its local history
/// lands after everything theirs, once, and all three converge.
///
/// Falsified by `Peer::join` connecting without re-queuing (the
/// `fork_back_to` call skipped): the join answers with nothing pending.
#[test]
fn alone_beside_two_server_users_lands_after_theirs() {
    let f = Fleet::new("alone-beside");
    let mut a = f.peer("a", Some("alice"));
    let mut b = f.peer("b", Some("bob"));
    let mut m = alone_peer(&f, "m", "alice");
    let mine = songs(&mut m, "mine", 30);
    songs(&mut a, "a", 20);
    songs(&mut b, "b", 20);
    a.author("create_playlist", named("Theirs"));
    // Not `converged`: the thirty are owed fleet-wide and not yet pushed.
    assert!(a.settle(PATIENCE) && b.settle(PATIENCE));
    let head_before = f.server.log_on_disk().expect("a log").head_seq();
    let joined = m.ask(
        "join",
        vec![
            ("server", Value::text(m.proxy.url())),
            ("user", Value::text("alice")),
        ],
    );
    assert_eq!(
        joined,
        ark::json::decode(r#"{"ok":true,"pending":30,"cursor":0}"#).unwrap()
    );
    let done = f.converged(&mut [&mut a, &mut b, &mut m]);
    assert_eq!(done.head, head_before + 30);
    for id in &mine {
        let n = done.log.seq_of(id).expect("sequenced");
        assert!(n > head_before, "{n} is not after {head_before}");
    }
}

/// **16. Leave, more, and back — twice.** Alice's two devices share a
/// playlist; one `leave`s, puts three things on it alone, while the other
/// puts three on it online; the first `join`s again, and its three sit
/// after the other's — authored first, sequenced last, the rebase over a
/// fork. Then away and back once more.
///
/// Falsified by `Peer::leave` recording the fork at nothing: the rejoin's
/// `Hello` names no log at cursor 0 and is paged everything on top of a
/// store that already holds it — the peer never converges.
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
    assert_eq!(left.as_struct()["ok"], Value::Bool(true));
    assert_eq!(m.status().link, "alone");
    for x in &ms[1..4] {
        add(&mut m, list, *x);
    }
    for y in &ms[4..7] {
        add(&mut a, list, *y);
    }
    assert!(a.settle(PATIENCE));
    m.ask(
        "join",
        vec![
            ("server", Value::text(m.proxy.url())),
            ("user", Value::text("alice")),
        ],
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
        vec![
            ("server", Value::text(m.proxy.url())),
            ("user", Value::text("alice")),
        ],
    );
    let done = f.converged(&mut [&mut a, &mut m]);
    assert_eq!(on_playlist(&f, list).len(), 8);
    assert_eq!(done.head, 12 + 1 + 1 + 3 + 3 + 1);
}

/// **17. `kill -9` during a join.** A peer with four hundred local intents
/// joins to completion, which times a join; four more are told to `join`
/// and killed a tenth, a half, nine tenths and ninety-nine hundredths of
/// that time later — each a directory of its own — and started again with
/// `--server`. Whatever the kill cut, the restart finishes the join: the
/// transition writes the re-queued intents and the fork's replica before
/// it removes the local history, so a peer killed between the two reopens
/// with both (`docs/plan-alone.md` §4), and one killed before either
/// reopens alone-shaped and joins at open. Every intent is on the server
/// once. What each kill left is printed.
///
/// Falsified by `Peer::open` not joining over a local history (the
/// `fork_back_to` at open skipped): the restarted peers stay at their
/// local cursors and nothing converges. Removing the local history before
/// writing the re-queued intents was not caught here — that window is a
/// few file removals wide and no kill landed in it; `ark-client`'s
/// `a_join_killed_between_its_writes_finishes_on_reopen` walks every write.
#[test]
fn a_join_killed_with_kill9_finishes_on_restart() {
    let f = Fleet::new("alone-kill-join");
    let join = |p: &mut PeerProc| {
        let url = p.proxy.url();
        p.say(
            "join",
            vec![("server", Value::text(url)), ("user", Value::text("alice"))],
        );
    };
    let mut cal = alone_peer(&f, "cal", "alice");
    songs(&mut cal, "cal", 400);
    let t = Instant::now();
    join(&mut cal);
    let answered = eventually(PATIENCE, || !cal.answers().is_empty());
    assert!(answered, "the join answers");
    let whole = t.elapsed();
    measured("a join of 400 local intents, to its answer", whole);
    let mut peers = vec![cal];
    for (i, frac) in [0.1f64, 0.5, 0.9, 0.99].into_iter().enumerate() {
        let mut p = alone_peer(&f, &format!("k{i}"), "alice");
        songs(&mut p, &format!("k{i}"), 400);
        join(&mut p);
        std::thread::sleep(whole.mul_f64(frac));
        p.kill9();
        let replica = p.dir.join("replica");
        let pending = std::fs::metadata(replica.join("pending")).map_or(0, |m| m.len());
        println!(
            "fleet: killed at {:.0}% of a join: local history {}, pending record {pending} bytes",
            frac * 100.0,
            if replica.join("log").exists() {
                "present"
            } else {
                "gone"
            },
        );
        p.alone = false;
        p.start();
        peers.push(p);
    }
    let mut refs: Vec<&mut PeerProc> = peers.iter_mut().collect();
    let done = f.converged(&mut refs);
    assert_eq!(done.head, 5 * 400);
}

/// **18. A peer alone killed mid-append.** Three hundred songs are said to
/// a peer alone without waiting, and it is killed a hundred and twenty milliseconds in.
/// Started again alone, it is whole: nothing is pending, its confirmed
/// store is its view, and every song in its library is an entry of its
/// local history — the last whole one is where it stops. Then it joins,
/// and the server has what it kept, every intent it answered before the
/// kill among them.
///
/// Falsified by `mutate` alone not writing its page before it returns
/// (`persist_log` left to the pump): an answered intent is missing after
/// the restart.
#[test]
fn a_peer_alone_killed_mid_append_reopens_whole() {
    let f = Fleet::new("alone-kill-append");
    let mut p = alone_peer(&f, "p", "alice");
    for i in 0..300 {
        p.say(
            "mutate",
            vec![
                ("name", Value::text("add_song")),
                ("args", Value::record(song(&format!("burst-{i:04}")))),
            ],
        );
    }
    std::thread::sleep(Duration::from_millis(120));
    let answered: Vec<Id> = p
        .answers()
        .iter()
        .filter_map(|a| match &a.as_struct()["id"] {
            Value::Id(id) => Some(*id),
            _ => None,
        })
        .collect();
    p.kill9();
    p.start();
    let (cursor, hash, view) = p.hash();
    assert_eq!(hash, view, "the confirmed store is the view");
    assert_eq!(p.status().pending, 0);
    let kept = media(&mut p).len() as i64;
    assert_eq!(kept, cursor, "every song an entry, and every entry a song");
    assert!(cursor >= answered.len() as i64);
    println!(
        "fleet: killed mid-append: {} answered, {cursor} kept of 300",
        answered.len()
    );
    // What it kept and did not answer is owed too: the log has it.
    p.ask(
        "join",
        vec![
            ("server", Value::text(p.proxy.url())),
            ("user", Value::text("alice")),
        ],
    );
    assert!(p.settle(PATIENCE));
    let log = f.server.log_on_disk().expect("a log");
    assert_eq!(log.head_seq(), cursor);
    for id in &answered {
        assert!(
            log.seq_of(id).is_some(),
            "an intent answered before the kill is kept"
        );
    }
    for (e, _) in log.entries.values() {
        p.owe(e.id, "add_song, kept past the kill");
    }
    f.converged(&mut [&mut p]);
}

/// **19. Two thousand local intents join, timed.** A peer alone authors
/// two thousand songs, then is started with `--server`: the time from the
/// start to every intent on the server and the peer converged is printed
/// (`docs/plan-alone.md`, "Landed").
///
/// Falsified by `Replica::fork_back` queuing the first thousand only: the
/// thousand after them were accepted and are not in the log.
#[test]
fn two_thousand_local_intents_join_timed() {
    let f = Fleet::new("alone-2000");
    let mut p = alone_peer(&f, "p", "alice");
    let t = Instant::now();
    songs(&mut p, "big", 2_000);
    measured("a peer alone authors 2,000 songs", t.elapsed());
    p.quit();
    p.alone = false;
    let t = Instant::now();
    p.start();
    let done = f.converged(&mut [&mut p]);
    measured(
        "2,000 local intents: start with --server to converged",
        t.elapsed(),
    );
    assert_eq!(done.head, 2_000);
}

/// **D6. A backup taken while the server appends restores to its moment.**
/// Three peers converge on a song and nine playlists; then, while they push
/// round after round, another thread takes a backup of the running
/// server's data directory with `arkc backup`'s code the moment the log on
/// disk has moved, and the peers push one more round once it has. The copy
/// is checked with `verify-log`: its head is past what was converged before
/// and short of what was converged after, and its state hash is the full
/// log's at that head — it is the log at a moment, not a mixture.
///
/// The server is stopped, the backup restored into a new directory and the
/// server started on it; the three are away, and a fourth device, `d`,
/// sequences more on the restored log than the three ever confirmed, so
/// that when they come back their cursors are *below* its head — a place
/// in a history this log does not hold. `restore` wrote the log unnamed and
/// the server named it afresh, so every `Hello` names another log and is
/// answered with the snapshot (§12.4): the fleet converges on the restored
/// log, the entries sequenced after the backup's journal length are in no
/// replica and in no log, and `/healthz` asked for JSON says the head, the
/// new name and each session's place.
///
/// Falsified by restoring with the name kept (`ops::restore(.., true)`):
/// the three are paged `d`'s entries on top of the old history's and never
/// converge — their hashes are not the log's. And by `/healthz` answering
/// text whatever it is asked: the body does not decode as JSON.
#[test]
fn a_backup_taken_mid_stream_restores_to_its_moment() {
    let mut f = Fleet::new("backup");
    let mut a = f.peer("a", Some("alice"));
    let mut b = f.peer("b", Some("alice"));
    let mut c = f.peer("c", Some("bob"));
    a.author("add_song", song("before the backup"));
    burst(&mut [&mut a, &mut b, &mut c], 0, 3);
    let before = f.converged(&mut [&mut a, &mut b, &mut c]);

    let (data, out) = (f.server.data.clone(), f.root.path().join("backup"));
    let schema = domain().module().schema.clone();
    let at = before.head;
    let taker = std::thread::spawn(move || {
        let moved = eventually(PATIENCE, || {
            ark_server::persist::load(&data, &schema)
                .ok()
                .flatten()
                .is_some_and(|l| l.head_seq() > at)
        });
        assert!(moved, "the log never moved past {at}");
        let t = Instant::now();
        let taken = ops::backup(&data, &out).unwrap();
        (taken, t.elapsed())
    });
    let mut round = 1;
    while !taker.is_finished() {
        burst(&mut [&mut a, &mut b, &mut c], round, 2);
        round += 1;
    }
    let (taken, took) = taker.join().unwrap();
    measured("a backup of a server being pushed to", took);
    burst(&mut [&mut a, &mut b, &mut c], round, 2);
    let after = f.converged(&mut [&mut a, &mut b, &mut c]);
    assert!(
        before.head < taken.head && taken.head < after.head,
        "the backup is mid-stream: {} < {} < {}",
        before.head,
        taken.head,
        after.head
    );
    let backup = f.root.path().join("backup");
    let v = ops::verify_log(&backup, Some(domain().module())).unwrap();
    let want = ark::hash::state_hash(&after.log.state_at(taken.head).unwrap());
    assert_eq!((v.head, v.hash.as_ref()), (taken.head, Some(&want)));
    assert_eq!(v.log_id, after.log.id());
    assert_eq!(v.torn, 0);

    f.server.stop();
    for p in [&mut a, &mut b, &mut c] {
        p.quit();
    }
    let restored = f.root.path().join("restored");
    let r = ops::restore(&backup, &restored, false).unwrap();
    assert_eq!(r.head, taken.head);
    f.server.data = restored;
    let t = Instant::now();
    f.server.start();
    let gone: Vec<Id> = after
        .entries
        .iter()
        .filter(|(n, _)| *n > taken.head)
        .map(|(_, e)| e.id)
        .collect();
    f.lost(gone.iter().copied());

    let mut d = f.peer("d", Some("alice"));
    let more = (after.head - taken.head + 3) as usize;
    burst(&mut [&mut d], 100, more);
    let head = taken.head + more as i64;
    assert!(
        eventually(PATIENCE, || {
            let st = d.status();
            st.pending == 0 && st.cursor == head
        }),
        "d's are on the restored log: {:?}",
        d.status()
    );
    for p in [&mut a, &mut b, &mut c] {
        p.start();
    }
    let again = f.converged(&mut [&mut a, &mut b, &mut c, &mut d]);
    measured("a restored server: restart to converged", t.elapsed());
    assert_eq!(
        again.head, head,
        "the backup's entries and d's, nothing after the backup"
    );
    assert_ne!(
        again.log.id(),
        after.log.id(),
        "the restored log has a name of its own"
    );
    for id in &gone {
        assert_eq!(
            again.log.seq_of(id),
            None,
            "{} was after the backup",
            ark::value::hex(id)
        );
    }
    for (n, (e, _)) in &before.log.entries {
        assert_eq!(
            again.log.seq_of(&e.id),
            Some(*n),
            "what was before the backup is where it was"
        );
    }

    // `/healthz`, asked for JSON (D6): the head, the log's new name, and
    // every session with its place.
    let body = ureq::get(&format!("{}/healthz", f.server.url()))
        .set("Accept", "application/json")
        .call()
        .unwrap()
        .into_string()
        .unwrap();
    let h = ark::json::decode(&body).unwrap_or_else(|e| panic!("{body}: {e:?}"));
    let field = |k: &str| match &h {
        Value::Struct(m) => m.get(k).cloned().unwrap_or(Value::Null),
        _ => panic!("{body}"),
    };
    assert_eq!(field("head"), Value::int(head));
    let named = again.log.id().map(|i| Value::text(ark::value::hex(&i)));
    assert_eq!(Some(field("log")), named);
    let Value::List(sessions) = field("sessions") else {
        panic!("{body}")
    };
    let alice = sessions
        .iter()
        .find(|s| matches!(s, Value::Struct(m) if m.get("user") == Some(&Value::text("alice"))))
        .unwrap_or_else(|| panic!("no alice: {body}"));
    let Value::Struct(alice) = alice else {
        unreachable!()
    };
    // Recorded where a page started, at the lowest of the session's open
    // connections (`ark_server::retain`): at or below the head.
    assert!(
        matches!(alice.get("cursor"), Some(Value::Int(n)) if (0..=head).contains(n)),
        "{body}"
    );
    assert!(
        matches!(alice.get("heard_ms"), Some(Value::Int(n)) if *n > 0),
        "{body}"
    );
    assert!(
        matches!(alice.get("open"), Some(Value::Int(n)) if *n >= 1),
        "{body}"
    );
}
