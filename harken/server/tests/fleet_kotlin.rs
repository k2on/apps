//! The fleet, mixed across runtimes (`docs/plan-phones.md` round 4,
//! `docs/plan-fleet.md`): Kotlin peers — the Kotlin client's
//! `harken-peer-kt`, speaking `harken-peer`'s lines over the same flags —
//! beside Rust ones, against this build's `harken-server` or a pinned
//! revision's. Every scenario ends where the Rust fleet's do: every
//! replica's confirmed state is the log's, every accepted intent is in it
//! once — so a Kotlin peer that disagrees with a Rust one fails here, and
//! which of the two is wrong is the next question, not this file's.
//!
//! The Kotlin peer is `FLEET_KOTLIN_PEER` ([`kotlin_peer`]), which
//! `checks.fleet-kotlin` builds and sets; without it every scenario says it
//! is skipped and passes, so `cargo test` on a laptop is unchanged. The
//! version shapes want `HARKEN_OLD_<n>_*` too ([`olds`]), as
//! `checks.versions` does.
//!
//! Each scenario was falsified once, with a Kotlin peer broken on purpose;
//! its doc comment says how.
//!
//! `cargo test -p harken-server --test fleet_kotlin -- --nocapture` prints
//! the timings.

mod support {
    pub mod fleet;
    pub mod proxy;
}

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ark::hash::state_hash;
use ark::ir::{self, CmpOp, Expr, FnKind, Hold, Pred, Projection};
use ark::value::{hex, Id, Value};
use support::fleet::*;

const NO_PLAYLIST: Id = [0; 16];

fn media(p: &mut PeerProc) -> Vec<Id> {
    ids_of(
        &p.query("library", vec![("playlist_id", Value::Id(NO_PLAYLIST))]),
        "id",
    )
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

fn create(p: &mut PeerProc, name: &str) -> Id {
    p.author("create_playlist", vec![("name", Value::text(name))])
}

fn songs(p: &mut PeerProc, tag: &str, n: usize) -> Vec<Id> {
    (0..n)
        .map(|i| p.author("add_song", song(&format!("{tag}-{i:04}"))))
        .collect()
}

/// A burst of `n` playlists from each peer, named by round.
fn burst(peers: &mut [&mut PeerProc], round: usize, n: usize) {
    for i in 0..n {
        for p in peers.iter_mut() {
            let name = format!("{}-{round}-{i}", p.name);
            create(p, &name);
        }
    }
}

fn pending(peers: &mut [&mut PeerProc]) -> i64 {
    peers.iter_mut().map(|p| p.status().pending).sum()
}

/// A Kotlin peer, started `--alone`: its own authority, dialling nothing.
fn kotlin_alone(f: &Fleet, kt: &Path, name: &str, user: &str) -> PeerProc {
    let mut p = f.peer_stopped(name, Some(user));
    p.kotlin(kt);
    p.alone = true;
    p.start();
    p
}

/// **K1. Two Rust peers and a Kotlin one, through partitions and
/// restarts.** Alice on a Rust peer and on a Kotlin one, bob on a Rust one.
/// The Kotlin peer is black-holed and puts ten things on a shared playlist
/// while alice's Rust device puts five on it online and bob makes three of
/// his own: given the network back, everything converges and the Kotlin
/// peer's ten sit after alice's five — authored first, sequenced last, the
/// rebase made visible on another runtime. Then the Kotlin peer is killed
/// with `-9` holding five more behind another black hole and started again
/// over its directory, the Rust peer's connection is cut mid-stream and it
/// is restarted, and the server is killed with `-9` and restarted; each
/// time the fleet converges, every hash the log's.
///
/// Falsified with a Kotlin client that did not push what was pending when
/// a connection opened (`Client.connected` without its `Push`): the ten
/// pushed into the black hole were never pushed again, and `converged`
/// timed out with the Kotlin peer holding them. (A client that ignored
/// every `ack` was tried first and passed — every phase here reconnects,
/// and a page of its own entries confirms them as an ack would.)
#[test]
fn k1_two_rust_peers_and_a_kotlin_one_through_partitions_and_restarts() {
    let Some(kt) = kotlin_or_skip("k1 mixed partitions and restarts") else {
        return;
    };
    let mut f = Fleet::new("k1-mixed");
    let mut a = f.peer("a", Some("alice"));
    let mut b = f.peer("b", Some("bob"));
    let t = Instant::now();
    let mut k = f.kotlin_peer(&kt, "k", Some("alice"));
    measured("k1 a Kotlin peer started and signed in", t.elapsed());
    songs(&mut a, "k1", 40);
    create(&mut a, "Mix");
    let t = Instant::now();
    let first = f.converged(&mut [&mut a, &mut b, &mut k]);
    measured("k1 a Kotlin peer's first sync of 41 entries", t.elapsed());
    assert_eq!(first.head, 41);
    let m = media(&mut k);
    assert_eq!(m.len(), 40, "the Kotlin peer reads the library");
    let mix = playlist(&mut k, "Mix");

    k.proxy.blackhole();
    let mut away = BTreeSet::new();
    for x in &m[20..30] {
        away.insert(*x);
        add(&mut k, mix, *x);
    }
    for x in &m[0..5] {
        add(&mut a, mix, *x);
    }
    burst(&mut [&mut b], 0, 3);
    assert!(a.settle(PATIENCE) && b.settle(PATIENCE));
    assert_eq!(
        k.status().pending,
        10,
        "the ten are pending behind the black hole"
    );
    let t = Instant::now();
    k.proxy.pass();
    f.converged(&mut [&mut a, &mut b, &mut k]);
    measured(
        "k1 converge after a Kotlin peer's black hole of ten",
        t.elapsed(),
    );
    let order = &playlists_in_pos_order(&f.rows("playlist_item"))[&mix];
    assert_eq!(order.len(), 15);
    assert!(
        order[..5].iter().all(|x| !away.contains(x)) && order[5..].iter().all(|x| away.contains(x)),
        "the Kotlin peer's adds land after alice's online ones: {:?}",
        order.iter().map(|x| away.contains(x)).collect::<Vec<_>>()
    );

    // Killed holding five, started again over its directory.
    k.proxy.blackhole();
    for x in &m[30..35] {
        add(&mut k, mix, *x);
    }
    k.kill9();
    k.proxy.pass();
    k.start();
    // The Rust peer's connection cut mid-stream, and it restarted.
    a.proxy.cut();
    burst(&mut [&mut a, &mut k], 1, 3);
    a.restart();
    // The server killed and restarted under all three.
    f.server.restart();
    burst(&mut [&mut a, &mut b, &mut k], 2, 2);
    let t = Instant::now();
    let done = f.converged(&mut [&mut a, &mut b, &mut k]);
    measured("k1 converge after a kill -9 of each kind", t.elapsed());
    assert_eq!(done.head, 41 + 10 + 5 + 3 + 5 + 6 + 6);
    assert_eq!(on(&f, mix).len(), 20);
}

/// The media on a playlist in the log, in `pos` order.
fn on(f: &Fleet, list: Id) -> Vec<Id> {
    playlists_in_pos_order(&f.rows("playlist_item"))
        .remove(&list)
        .unwrap_or_default()
}

/// **K2. A Kotlin peer alone, then joining.** A Kotlin peer that never had
/// a server authors thirty songs, a playlist and ten things on it with no
/// server dialled; then the same directory is started with `--server` —
/// the join — beside a Rust device of the same person and one of bob's that
/// authored meanwhile. The log ends with the forty-one in the order they
/// were sequenced alone, after everything theirs, all of them alice's. Then
/// the Kotlin peer `leave`s, puts three more on the playlist alone while
/// the Rust device puts three on it online, and `join`s again without a
/// restart: its three sit after the other's.
///
/// Falsified with a Kotlin `Peer.join` that re-queued nothing (its
/// `forkBackTo` skipped): the `join` without a restart answered pending 0,
/// cursor 49, where three were owed. (The join by restart opens through
/// `Peer.open`, which re-queues on its own, and passed.)
#[test]
fn k2_a_kotlin_peer_alone_then_joining() {
    let Some(kt) = kotlin_or_skip("k2 alone, then joining") else {
        return;
    };
    let f = Fleet::new("k2-alone");
    let mut k = kotlin_alone(&f, &kt, "k", "alice");
    let mut made = songs(&mut k, "alone", 30);
    made.push(create(&mut k, "Alone"));
    let list = playlist(&mut k, "Alone");
    for x in media(&mut k).into_iter().take(10) {
        made.push(add(&mut k, list, x));
    }
    let st = k.status();
    assert_eq!((st.cursor, st.pending, st.link.as_str()), (41, 0, "alone"));
    let mut a = f.peer("a", Some("alice"));
    let mut b = f.peer("b", Some("bob"));
    songs(&mut b, "b", 5);
    assert!(b.settle(PATIENCE));
    let head_before = f.server.log_on_disk().map_or(0, |l| l.head_seq());
    k.quit();
    k.alone = false;
    let t = Instant::now();
    k.start();
    let done = f.converged(&mut [&mut k, &mut a, &mut b]);
    measured(
        "k2 forty-one local intents of a Kotlin peer join",
        t.elapsed(),
    );
    assert_eq!(done.head, head_before + 41);
    let theirs: Vec<Id> = done
        .entries
        .iter()
        .filter(|(n, _)| *n > head_before)
        .map(|(_, e)| e.id)
        .collect();
    assert_eq!(
        theirs, made,
        "in the order they were sequenced alone, after bob's"
    );
    assert!(done
        .entries
        .iter()
        .filter(|(n, _)| *n > head_before)
        .all(|(_, e)| e.actor == "alice"));

    // Leave, more, and back without a restart.
    let left = k.ask("leave", vec![]);
    assert_eq!(left.as_struct()["ok"], Value::Bool(true), "{left:?}");
    let m = media(&mut a);
    let mut mine = BTreeSet::new();
    // Library order is bob's five, then the thirty; the first ten of the
    // thirty are on the playlist already.
    for x in &m[20..23] {
        mine.insert(add(&mut k, list, *x));
    }
    for x in &m[23..26] {
        add(&mut a, list, *x);
    }
    assert!(a.settle(PATIENCE));
    let joined = k.ask(
        "join",
        vec![
            ("server", Value::text(k.proxy.url())),
            ("user", Value::text("alice")),
        ],
    );
    assert_eq!(joined.as_struct()["ok"], Value::Bool(true), "{joined:?}");
    assert_eq!(joined.as_struct()["pending"], Value::int(3), "{joined:?}");
    let done = f.converged(&mut [&mut k, &mut a, &mut b]);
    let tail: Vec<Id> = done
        .entries
        .iter()
        .rev()
        .take(3)
        .map(|(_, e)| e.id)
        .collect();
    assert!(
        tail.iter().all(|id| mine.contains(id)),
        "the Kotlin peer's three, authored first, are sequenced last"
    );
    assert_eq!(on(&f, list).len(), 16);
}

/// **K3. A Kotlin peer through a server restart and a lost log.** Two Rust
/// peers and a Kotlin one push bursts; the server is stopped with frames in
/// flight, they go on authoring, and it starts again from its data
/// directory: converged, nothing lost and nothing twice. Then the server
/// loses its log — snapshot and journal deleted, sessions kept — while the
/// three author nine more; started on an empty log it answers each
/// `Hello`, whose cursor is past its head, with its snapshot, and every
/// peer, the Kotlin one among them, is re-based onto exactly those nine.
///
/// Falsified with a Kotlin client that ignored every snapshot
/// (`Client.recvSnapshot` returning at once): after the lost log it stayed
/// at cursor 25, past a head of 9, its intents never confirmed, and
/// `converged` timed out.
#[test]
fn k3_a_kotlin_peer_through_a_server_restart_and_a_lost_log() {
    let Some(kt) = kotlin_or_skip("k3 server restart and lost log") else {
        return;
    };
    let mut f = Fleet::new("k3-restart");
    let mut a = f.peer("a", Some("alice"));
    let mut k = f.kotlin_peer(&kt, "k", Some("alice"));
    let mut c = f.peer("c", Some("bob"));
    a.author("add_song", song("k3-song"));
    f.converged(&mut [&mut a, &mut k, &mut c]);
    burst(&mut [&mut a, &mut k, &mut c], 0, 5);
    f.server.stop();
    burst(&mut [&mut a, &mut k, &mut c], 1, 3);
    assert!(pending(&mut [&mut a, &mut k, &mut c]) > 0);
    let t = Instant::now();
    f.server.start();
    let before = f.converged(&mut [&mut a, &mut k, &mut c]);
    measured(
        "k3 a server restart under a Kotlin peer, to converged",
        t.elapsed(),
    );
    assert_eq!(before.head, 1 + 15 + 9);

    f.server.stop();
    for gone in [
        ark_server::persist::path_of(&f.server.data),
        ark_server::persist::journal_path_of(&f.server.data),
    ] {
        std::fs::remove_file(&gone).unwrap_or_else(|e| panic!("{}: {e}", gone.display()));
    }
    burst(&mut [&mut a, &mut k, &mut c], 2, 3);
    assert_eq!(pending(&mut [&mut a, &mut k, &mut c]), 9);
    let t = Instant::now();
    f.server.start();
    f.lost(before.entries.iter().map(|(_, e)| e.id));
    let done = f.converged(&mut [&mut a, &mut k, &mut c]);
    measured(
        "k3 a lost log under a Kotlin peer, restart to converged",
        t.elapsed(),
    );
    assert_eq!(
        done.head, 9,
        "the nine pending, and nothing the lost log held"
    );
    assert!(media(&mut k).is_empty(), "the song went with the log");
    let names: Vec<String> = playlists(&mut k).into_iter().map(|(n, _)| n).collect();
    assert!(
        names.len() == 6 && names.iter().all(|n| n.split('-').nth(1) == Some("2")),
        "the Kotlin peer holds only alice's of the third burst: {names:?}"
    );
}

/// **K4. New peer, old server — a Kotlin one.** A Kotlin peer running the
/// grown domain authors at hashes no pinned server ever had; the old
/// server answers each as unknown, the Kotlin client reads that as the hold
/// it is — pending and held, three — and when the server is upgraded in
/// place to this build with the grown module they land
/// (`version_3_…` in `tests/versions.rs`, with the Rust peer swapped out).
///
/// Falsified with a Kotlin client that took every `reject` as a verdict
/// (the unknown function not read as a hold): `held` never reached 3 and
/// the scenario failed waiting for it.
#[test]
fn k4_new_kotlin_peer_old_server() {
    let Some(kt) = kotlin_or_skip("k4 new Kotlin peer, old server") else {
        return;
    };
    for o in olds_or_skip("k4 new Kotlin peer, old server") {
        let mut f = Fleet::old(&format!("k4-{}", o.name), &o.server);
        let grown = grown_module(f.root.path());
        let mut n = f.peer_stopped("new", Some("alice"));
        n.kotlin(&kt);
        n.args = vec!["--module".into(), grown.display().to_string()];
        n.start();
        songs(&mut n, "k4", 2);
        create(&mut n, "Ahead");
        assert!(
            eventually(PATIENCE, || {
                let s = n.status();
                s.held == Some(3) && s.pending == 3
            }),
            "held by the old server: {:?}",
            n.status()
        );
        assert!(n.rejections().is_empty(), "nothing was refused");
        let t = Instant::now();
        f.server
            .upgrade(vec![("HARKEN_MODULE", grown.to_str().unwrap())]);
        let took = f.converged_across(&mut [(&mut n, "alice")]);
        assert_eq!(n.status().held, Some(0));
        playlist(&mut n, "Ahead");
        measured(
            &format!(
                "k4 {} a Kotlin peer held, upgraded under, to landed",
                o.name
            ),
            t.elapsed(),
        );
        measured(&format!("k4 {} …the last settle", o.name), took);
    }
}

/// **K5. Old peer, new server — a Kotlin one.** A Kotlin peer running a
/// pinned revision's own `harken.ark` (`HARKEN_OLD_<n>_MODULE`) signs in,
/// syncs and authors beside two Rust peers of this build, against a fresh
/// server of this build told it ran that module; everyone converges
/// (`version_1_…`, with the old peer's runtime swapped for Kotlin's — a
/// phone that has not been updated runs its old module, whatever it is
/// interpreted by).
///
/// The Kotlin peer is held to having run the old module: it reports itself
/// behind this server, and the log holds its six songs at the hash that
/// module shipped for `add_song`, which this build's moved.
///
/// Falsified with a `harken-peer-kt` that ignored `--module` and ran the
/// harken.ark it carries: it was not behind, and its songs were at this
/// build's hash.
#[test]
fn k5_old_kotlin_peer_new_server() {
    let Some(kt) = kotlin_or_skip("k5 old Kotlin peer, new server") else {
        return;
    };
    for o in olds_or_skip("k5 old Kotlin peer, new server") {
        let Some(module) = &o.module else {
            println!(
                "fleet: k5 {}: skipped — no HARKEN_OLD_{}_MODULE",
                o.name, o.n
            );
            continue;
        };
        let f = Fleet::beside(&format!("k5-{}", o.name), &o);
        let mut a = f.peer_stopped("old", Some("alice"));
        a.kotlin(&kt);
        a.args = vec!["--module".into(), module.display().to_string()];
        a.start();
        let mut b = f.peer("new", Some("bob"));
        let mut c = f.peer("new-alice", Some("alice"));
        let t = Instant::now();
        songs(&mut a, "k5", 6);
        create(&mut a, "Old");
        assert!(a.settle(PATIENCE));
        let m = media(&mut a);
        let old = playlist(&mut a, "Old");
        for x in &m[..3] {
            add(&mut a, old, *x);
        }
        create(&mut b, "New");
        let new = playlist(&mut b, "New");
        add(&mut b, new, m[4]);
        add(&mut c, old, m[5]);
        let took = f.converged_across(&mut [(&mut a, "alice"), (&mut b, "bob"), (&mut c, "alice")]);
        // The Kotlin peer ran the old module: behind this server's, and its
        // songs are at the hash that module shipped, which moved since.
        assert_eq!(
            a.status().behind,
            Some(true),
            "the old module is not the server's"
        );
        assert_eq!(c.status().behind, Some(false));
        let old_add_song = ark_client::Domain::from_bytes(&std::fs::read(module).unwrap(), vec![])
            .unwrap()
            .mutator("add_song")
            .unwrap()
            .0
            .clone();
        assert_ne!(
            &old_add_song,
            domain().mutator("add_song").unwrap().0,
            "add_song moved"
        );
        let log = f.server.log_on_disk().expect("a log");
        let at_old = log
            .entries
            .values()
            .filter(|(e, _)| e.fn_hash == old_add_song)
            .count();
        assert_eq!(at_old, 6, "the six songs at the old module's hash");
        measured(
            &format!(
                "k5 {} an old Kotlin peer beside new ones, to converged",
                o.name
            ),
            t.elapsed(),
        );
        measured(&format!("k5 {} …the last settle", o.name), took);
    }
}

/// harken's module with one scope: a person holds the playlists that are
/// theirs and nobody else's (`docs/plan-guards.md` D2) — harken declares
/// none, so the fleet adds this one to have a partial client to hold. The
/// scope is on the `playlists` router and the `playlists` query lists it;
/// every other table is held whole.
fn scoped_module() -> ir::Module {
    let mut m = harken_domain::module().build().clone();
    let router = m
        .functions
        .iter()
        .find(|f| f.name == "playlists")
        .and_then(|f| f.router.clone())
        .expect("the playlists query is on a router");
    // Declared before the procedures that run it, as every middleware is.
    m.functions.insert(
        0,
        ir::Function {
            name: "own_playlists".into(),
            kind: FnKind::Scope,
            router: None,
            uses: vec![],
            autos: vec![],
            input: vec![],
            refine: vec![],
            ret: None,
            body: vec![],
            plan: None,
            holds: vec![Hold {
                table: "playlist".into(),
                filter: Some(Pred::Cmp("user_id".into(), CmpOp::Eq, Expr::CtxUser)),
                columns: Projection::All,
            }],
            private: false,
            names: BTreeMap::new(),
        },
    );
    for r in &mut m.routers {
        if r.name == router {
            r.uses.push("own_playlists".into());
        }
    }
    for f in &mut m.functions {
        if f.name == "playlists" {
            f.uses.push("own_playlists".into());
        }
    }
    ark::verify::verify(&m).unwrap_or_else(|es| panic!("the scoped module does not verify: {es:?}"))
}

fn scoped_file(dir: &Path) -> (PathBuf, ir::Module) {
    let m = scoped_module();
    let path = dir.join("scoped.ark");
    std::fs::write(&path, ark::canon::encode(&ir::module_value(&m))).unwrap();
    (path, m)
}

/// **K6. A Kotlin peer partial under a scoped module.** The server hosts
/// harken with a scope — a person holds only their own playlists — and
/// alice runs it on a Rust peer and on a Kotlin one, bob on a Rust one.
/// Songs, playlists of both people and items on them. Converged: every
/// peer at the head with nothing pending, and each one's confirmed state
/// hashing as the person's union of the log's state does (§8.1 over the
/// rows they hold, `scope::Holdings::digest`) — not as the whole store,
/// which holds bob's playlists. The two alice peers, two runtimes, hash
/// the same; bob's does not.
///
/// Falsified with a Kotlin client that opened a partial snapshot whole (its
/// `held` dropped in `Client.recvSnapshot`): every partial page after it
/// was refused as another kind of replica's, and the Kotlin peer stayed at
/// the head with an intent pending for good.
#[test]
fn k6_a_kotlin_peer_partial_under_a_scoped_module() {
    let Some(kt) = kotlin_or_skip("k6 partial under a scope") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (scoped, m) = scoped_file(dir.path());
    let flag = scoped.display().to_string();
    let f = Fleet::with_env("k6-scoped", vec![("HARKEN_MODULE", &flag)]);
    let module = |p: &mut PeerProc| p.args = vec!["--module".into(), flag.clone()];
    let mut a = f.peer_stopped("a", Some("alice"));
    module(&mut a);
    a.start();
    let mut b = f.peer_stopped("b", Some("bob"));
    module(&mut b);
    b.start();
    let mut k = f.peer_stopped("k", Some("alice"));
    k.kotlin(&kt);
    module(&mut k);
    k.start();
    songs(&mut a, "k6", 6);
    create(&mut a, "Alice's");
    create(&mut b, "Bob's");
    create(&mut k, "Alice's phone");
    assert!(a.settle(PATIENCE) && b.settle(PATIENCE) && k.settle(PATIENCE));
    let ms = media(&mut k);
    let theirs = playlist(&mut b, "Bob's");
    let mine = playlist(&mut k, "Alice's phone");
    for x in &ms[..3] {
        add(&mut k, mine, *x);
        add(&mut b, theirs, *x);
    }
    let t = Instant::now();
    let scopes = ark::scope::Scopes::of(&m);
    let deadline = Instant::now() + PATIENCE;
    let (hashes, head) = loop {
        for p in [&mut a, &mut b, &mut k] {
            p.settle(Duration::from_secs(2));
        }
        let log = f.server.log_on_disk().expect("a log");
        let head = log.head_seq();
        let state = log.state_at(head).expect("a state at the head");
        let whole = hex(&state_hash(&state));
        let mut off = vec![];
        let mut hashes = BTreeMap::new();
        for (p, user) in [(&mut a, "alice"), (&mut b, "bob"), (&mut k, "alice")] {
            let s = p.status();
            let (cursor, hash, view) = p.hash();
            let roles: BTreeSet<String> = ["library".to_string()].into();
            let want = hex(&scopes
                .holdings(ark::scope::Who {
                    user,
                    session: &s.session,
                    roles: &roles,
                })
                .digest(&state));
            if cursor != head || s.pending != 0 || hash != want || view != want {
                off.push(format!(
                    "{}: cursor {cursor} (head {head}) pending {} hash {} view {} union {} whole {}",
                    p.name,
                    s.pending,
                    &hash[..12],
                    &view[..12],
                    &want[..12],
                    &whole[..12]
                ));
            }
            hashes.insert(p.name.clone(), (hash, whole.clone()));
        }
        if off.is_empty() {
            break (hashes, head);
        }
        if Instant::now() >= deadline {
            panic!(
                "k6: not converged:\n{}\n--- k ---\n{}",
                off.join("\n"),
                tail(&k.stderr, 20)
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    measured(
        "k6 a partial Kotlin peer beside partial Rust ones, to converged",
        t.elapsed(),
    );
    assert_eq!(head, 6 + 3 + 6);
    assert_eq!(
        hashes["a"].0, hashes["k"].0,
        "alice's two runtimes hold the same union"
    );
    assert_ne!(
        hashes["k"].0, hashes["k"].1,
        "and not the whole store: bob's playlist is not hers"
    );
    assert_ne!(hashes["b"].0, hashes["k"].0, "bob's union is not alice's");
}
