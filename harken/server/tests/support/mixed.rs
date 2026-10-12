//! The fleet mixed across runtimes (`docs/plan-phones.md` rounds 4 and 7,
//! `docs/plan-fleet.md` §6): six scenarios, each run with another runtime's
//! headless peer — the Kotlin client's `harken-peer-kt`, the Swift client's
//! `harken-peer-swift`, each speaking `harken-peer`'s lines over the same
//! flags — beside Rust ones, against this build's `harken-server` or a
//! pinned revision's. Every scenario ends where the Rust fleet's do: every
//! replica's confirmed state is the log's, every accepted intent is in it
//! once — so a peer of another runtime that disagrees with a Rust one fails
//! here, and which of the two is wrong is the next question, not this
//! file's.
//!
//! One body per scenario, whatever the runtime: `tests/fleet_kotlin.rs` and
//! `tests/fleet_swift.rs` each run all six with their runtime's binary
//! ([`Runtime::or_skip`]), and say there how each was falsified for theirs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ark::hash::state_hash;
use ark::ir::{self, CmpOp, Expr, FnKind, Hold, Pred, Projection};
use ark::value::{hex, Id, Value};

use super::fleet::*;

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

/// The other runtime's peer, started `--alone`: its own authority, dialling nothing.
fn foreign_alone(f: &Fleet, rt: &Foreign, user: &str) -> PeerProc {
    let mut p = f.peer_stopped(rt.tag, Some(user));
    p.foreign(&rt.bin);
    p.alone = true;
    p.start();
    p
}

/// **1. Two Rust peers and another runtime's, through partitions and
/// restarts.** Alice on a Rust peer and on the other runtime's, bob on a
/// Rust one. The other runtime's peer is black-holed and puts ten things on
/// a shared playlist while alice's Rust device puts five on it online and
/// bob makes three of his own: given the network back, everything converges
/// and its ten sit after alice's five — authored first, sequenced last, the
/// rebase made visible on another runtime. Then it is killed with `-9`
/// holding five more behind another black hole and started again over its
/// directory, the Rust peer's connection is cut mid-stream and it is
/// restarted, and the server is killed with `-9` and restarted; each time
/// the fleet converges, every hash the log's.
pub fn partitions_and_restarts(rt: &Foreign) {
    let mut f = Fleet::new(&format!("{}1-mixed", rt.tag));
    let mut a = f.peer("a", Some("alice"));
    let mut b = f.peer("b", Some("bob"));
    let t = Instant::now();
    let mut k = f.foreign_peer(&rt.bin, rt.tag, Some("alice"));
    measured(&rt.said(1, "a {} peer started and signed in"), t.elapsed());
    songs(&mut a, "k1", 40);
    create(&mut a, "Mix");
    let t = Instant::now();
    let first = f.converged(&mut [&mut a, &mut b, &mut k]);
    measured(
        &rt.said(1, "a {} peer's first sync of 41 entries"),
        t.elapsed(),
    );
    assert_eq!(first.head, 41);
    let m = media(&mut k);
    assert_eq!(m.len(), 40, "the {} peer reads the library", rt.name);
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
        &rt.said(1, "converge after a {} peer's black hole of ten"),
        t.elapsed(),
    );
    let order = &playlists_in_pos_order(&f.rows("playlist_item"))[&mix];
    assert_eq!(order.len(), 15);
    assert!(
        order[..5].iter().all(|x| !away.contains(x)) && order[5..].iter().all(|x| away.contains(x)),
        "the {} peer's adds land after alice's online ones: {:?}",
        rt.name,
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
    measured(
        &rt.said(1, "converge after a kill -9 of each kind"),
        t.elapsed(),
    );
    assert_eq!(done.head, 41 + 10 + 5 + 3 + 5 + 6 + 6);
    assert_eq!(on(&f, mix).len(), 20);
}

/// The media on a playlist in the log, in `pos` order.
fn on(f: &Fleet, list: Id) -> Vec<Id> {
    playlists_in_pos_order(&f.rows("playlist_item"))
        .remove(&list)
        .unwrap_or_default()
}

/// **2. Another runtime's peer alone, then joining.** A peer that never had
/// a server authors thirty songs, a playlist and ten things on it with no
/// server dialled; then the same directory is started with `--server` —
/// the join — beside a Rust device of the same person and one of bob's that
/// authored meanwhile. The log ends with the forty-one in the order they
/// were sequenced alone, after everything theirs, all of them alice's. Then
/// it `leave`s, puts three more on the playlist alone while the Rust device
/// puts three on it online, and `join`s again without a restart: its three
/// sit after the other's.
pub fn alone_then_joining(rt: &Foreign) {
    let f = Fleet::new(&format!("{}2-alone", rt.tag));
    let mut k = foreign_alone(&f, rt, "alice");
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
        &rt.said(2, "forty-one local intents of a {} peer join"),
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
        "the {} peer's three, authored first, are sequenced last",
        rt.name
    );
    assert_eq!(on(&f, list).len(), 16);
}

/// **3. Another runtime's peer through a server restart and a lost log.**
/// Two Rust peers and the other runtime's push bursts; the server is stopped
/// with frames in flight, they go on authoring, and it starts again from its
/// data directory: converged, nothing lost and nothing twice. Then the
/// server loses its log — snapshot and journal deleted, sessions kept —
/// while the three author nine more; started on an empty log it answers
/// each `Hello`, whose cursor is past its head, with its snapshot, and every
/// peer is re-based onto exactly those nine.
pub fn server_restart_and_lost_log(rt: &Foreign) {
    let mut f = Fleet::new(&format!("{}3-restart", rt.tag));
    let mut a = f.peer("a", Some("alice"));
    let mut k = f.foreign_peer(&rt.bin, rt.tag, Some("alice"));
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
        &rt.said(3, "a server restart under a {} peer, to converged"),
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
        &rt.said(3, "a lost log under a {} peer, restart to converged"),
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
        "the {} peer holds only alice's of the third burst: {names:?}",
        rt.name
    );
}

/// **4. New peer, old server — another runtime's.** A peer running the
/// grown domain authors at hashes no pinned server ever had; the old server
/// answers each as unknown, the client reads that as the hold it is —
/// pending and held, three — and when the server is upgraded in place to
/// this build with the grown module they land (`version_3_…` in
/// `tests/versions.rs`, with the Rust peer swapped out).
pub fn new_peer_old_server(rt: &Foreign) {
    for o in olds_or_skip(&rt.said(4, "new {} peer, old server")) {
        let mut f = Fleet::old(&format!("{}4-{}", rt.tag, o.name), &o.server);
        let grown = grown_module(f.root.path());
        let mut n = f.peer_stopped("new", Some("alice"));
        n.foreign(&rt.bin);
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
                "{}4 {} a {} peer held, upgraded under, to landed",
                rt.tag, o.name, rt.name
            ),
            t.elapsed(),
        );
        measured(&format!("{}4 {} …the last settle", rt.tag, o.name), took);
    }
}

/// **5. Old peer, new server — another runtime's.** A peer running a pinned
/// revision's own `harken.ark` (`HARKEN_OLD_<n>_MODULE`) signs in, syncs
/// and authors beside two Rust peers of this build, against a fresh server
/// of this build told it ran that module; everyone converges (`version_1_…`,
/// with the old peer's runtime swapped — a phone that has not been updated
/// runs its old module, whatever it is interpreted by). It is held to having
/// run the old module: it reports itself behind this server, and the log
/// holds its six songs at the hash that module shipped for `add_song`,
/// which this build's moved.
pub fn old_peer_new_server(rt: &Foreign) {
    for o in olds_or_skip(&rt.said(5, "old {} peer, new server")) {
        let Some(module) = &o.module else {
            println!(
                "fleet: {}5 {}: skipped — no HARKEN_OLD_{}_MODULE",
                rt.tag, o.name, o.n
            );
            continue;
        };
        let f = Fleet::beside(&format!("{}5-{}", rt.tag, o.name), &o);
        let mut a = f.peer_stopped("old", Some("alice"));
        a.foreign(&rt.bin);
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
        // The other runtime's peer ran the old module: behind this server's, and its
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
                "{}5 {} an old {} peer beside new ones, to converged",
                rt.tag, o.name, rt.name
            ),
            t.elapsed(),
        );
        measured(&format!("{}5 {} …the last settle", rt.tag, o.name), took);
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

/// **6. Another runtime's peer partial under a scoped module.** The server
/// hosts harken with a scope — a person holds only their own playlists —
/// and alice runs it on a Rust peer and on the other runtime's, bob on a
/// Rust one. Songs, playlists of both people and items on them. Converged:
/// every peer at the head with nothing pending, and each one's confirmed
/// state hashing as the person's union of the log's state does (§8.1 over
/// the rows they hold, `scope::Holdings::digest`) — not as the whole store,
/// which holds bob's playlists. The two alice peers, two runtimes, hash the
/// same; bob's does not.
pub fn partial_under_a_scope(rt: &Foreign) {
    let dir = tempfile::tempdir().unwrap();
    let (scoped, m) = scoped_file(dir.path());
    let flag = scoped.display().to_string();
    let f = Fleet::with_env(
        &format!("{}6-scoped", rt.tag),
        vec![("HARKEN_MODULE", &flag)],
    );
    let module = |p: &mut PeerProc| p.args = vec!["--module".into(), flag.clone()];
    let mut a = f.peer_stopped("a", Some("alice"));
    module(&mut a);
    a.start();
    let mut b = f.peer_stopped("b", Some("bob"));
    module(&mut b);
    b.start();
    let mut k = f.peer_stopped(rt.tag, Some("alice"));
    k.foreign(&rt.bin);
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
                "{}6: not converged:\n{}\n--- {} ---\n{}",
                rt.tag,
                off.join("\n"),
                rt.tag,
                tail(&k.stderr, 20)
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    measured(
        &rt.said(
            6,
            "a partial {} peer beside partial Rust ones, to converged",
        ),
        t.elapsed(),
    );
    assert_eq!(head, 6 + 3 + 6);
    assert_eq!(
        hashes["a"].0, hashes[rt.tag].0,
        "alice's two runtimes hold the same union"
    );
    assert_ne!(
        hashes[rt.tag].0, hashes[rt.tag].1,
        "and not the whole store: bob's playlist is not hers"
    );
    assert_ne!(
        hashes["b"].0, hashes[rt.tag].0,
        "bob's union is not alice's"
    );
}
