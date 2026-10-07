//! The fleet, run mixed (`docs/plan-db.md` D1): an older `harken-peer` or
//! `harken-server` beside this build's, a server upgraded in place under
//! running peers, a client upgraded in place over its directory, and a
//! domain that grew. Every scenario ends on
//! [`Fleet::converged_across`] — every peer at the log's head with nothing
//! pending, and what each one's user sees equal to the log's state — since
//! two revisions' state hashes are not comparable.
//!
//! The older binaries are pinned previous revisions of this repository
//! (`nix/versions.nix`), which `checks.versions` builds and hands over as
//! `HARKEN_OLD_<n>_SERVER` and `HARKEN_OLD_<n>_PEER` ([`olds`]). Without
//! them, scenarios 1–5 say they are skipped and pass, so `cargo test` on a
//! laptop is unchanged. Scenarios 6 and 7 need no older binary: "the next
//! release" there is this build hosting the grown domain
//! (`harken_server::grown`, `HARKEN_MODULE` and `harken-peer --module`),
//! since no pinned revision has a different schema yet.
//!
//! Its own test target over the fleet's support, rather than more of
//! `tests/fleet.rs`: `checks.versions` builds and runs exactly these.
//!
//! `cargo test -p harken-server --test versions -- --nocapture` prints the
//! timings. Each scenario was falsified once; its doc comment says how.

mod support {
    pub mod fleet;
    pub mod proxy;
}

use std::path::Path;
use std::time::Instant;

use ark::value::{hex, Id, Value};
use support::fleet::*;

const NO_PLAYLIST: Id = [0; 16];

fn media(p: &mut PeerProc) -> Vec<Id> {
    ids_of(
        &p.query("library", vec![("playlist_id", Value::Id(NO_PLAYLIST))]),
        "id",
    )
}

fn playlist(p: &mut PeerProc, name: &str) -> Id {
    p.query("playlists", vec![])
        .iter()
        .find_map(|r| match (&r["name"], &r["id"]) {
            (Value::Text(n), Value::Id(i)) if &**n == name => Some(*i),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{} has no playlist {name}", p.name))
}

fn add(p: &mut PeerProc, list: Id, m: Id) {
    p.author(
        "add_to_playlist",
        vec![("playlist_id", Value::Id(list)), ("media_id", Value::Id(m))],
    );
}

fn create(p: &mut PeerProc, name: &str) {
    p.author("create_playlist", vec![("name", Value::text(name))]);
}

fn songs(p: &mut PeerProc, tag: &str, n: usize) {
    for i in 0..n {
        p.author("add_song", song(&format!("{tag}-{i}")));
    }
}

/// A peer of a pinned revision, started.
fn old_peer(f: &Fleet, o: &Old, name: &str, user: &str) -> PeerProc {
    let mut p = f.peer_stopped(name, Some(user));
    p.old(&o.peer);
    p.start();
    p
}

/// A peer of this build running the grown domain, started.
fn grown_peer(f: &Fleet, grown: &Path, name: &str, user: &str) -> PeerProc {
    let mut p = f.peer_stopped(name, Some(user));
    p.args = vec!["--module".into(), grown.display().to_string()];
    p.start();
    p
}

/// Wait until `p` is at the log's head as the server has it.
fn caught_up(f: &Fleet, p: &mut PeerProc) {
    let head = f.server.log_on_disk().map_or(0, |l| l.head_seq());
    assert!(p.wait(head, PATIENCE), "{} did not reach {head}", p.name);
}

/// The head an old server's `/healthz` says.
fn old_head(f: &Fleet) -> i64 {
    f.server
        .health()
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("head ").and_then(|n| n.trim().parse().ok()))
        .expect("a head in /healthz")
}

/// The identity the snapshot on the disk names, read without checking its
/// hash.
fn log_named(f: &Fleet) -> Option<Id> {
    let bytes = std::fs::read(ark_server::persist::path_of(&f.server.data)).ok()?;
    let v = ark::canon::decode(&bytes).ok()?;
    let Value::Struct(m) = v else { return None };
    let Some(Value::Struct(base)) = m.get("base") else {
        return None;
    };
    match base.get("log") {
        Some(Value::Id(i)) => Some(*i),
        _ => None,
    }
}

fn module_flag(grown: &Path) -> Vec<(&'static str, String)> {
    vec![("HARKEN_MODULE", grown.display().to_string())]
}

fn upgrade_to(f: &mut Fleet, env: Vec<(&'static str, String)>) {
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    f.server.upgrade(env);
}

/// **1. Old peer, new server.** A peer of each pinned revision signs in,
/// syncs and authors beside two peers of this build, and all of them
/// converge. Its intents are at the hashes its own module shipped: the
/// playlist mutators' are this server's too, and `add_song`'s moved when
/// the library gained its guard (`docs/plan-guards.md` D1) — so the
/// server, fresh, is told it ran that module ([`Fleet::beside`]) and
/// sequences them through its closures, unguarded, as they always ran.
/// Without that, `checks.versions` found every old `add_song` held for
/// ever and the old peer never settled.
///
/// Falsified once: with `converged_across` comparing the old peer's
/// playlists against bob's rather than alice's, it failed naming them.
#[test]
fn version_1_an_old_peer_and_a_new_server() {
    for o in olds_or_skip("1 old peer, new server") {
        let f = Fleet::beside(&format!("v1-{}", o.name), &o);
        let mut a = old_peer(&f, &o, "old", "alice");
        let mut b = f.peer("new", Some("bob"));
        let mut c = f.peer("new-alice", Some("alice"));
        let t = Instant::now();
        songs(&mut a, "v1", 6);
        create(&mut a, "Old");
        assert!(a.settle(PATIENCE));
        let m = media(&mut a);
        let old = playlist(&mut a, "Old");
        for x in &m[..3] {
            add(&mut a, old, *x);
        }
        caught_up(&f, &mut b);
        caught_up(&f, &mut c);
        create(&mut b, "New");
        let new = playlist(&mut b, "New");
        add(&mut b, new, m[4]);
        add(&mut c, old, m[5]);
        let took = f.converged_across(&mut [(&mut a, "alice"), (&mut b, "bob"), (&mut c, "alice")]);
        assert_eq!(
            c.status().behind,
            Some(false),
            "the server's module is this build's"
        );
        measured(
            &format!("v1 {} old peer beside new ones, to converged", o.name),
            t.elapsed(),
        );
        measured(&format!("v1 {} …the last settle", o.name), took);
    }
}

/// **2. Old peer, new server, a mutator whose body changed.** The server
/// runs this build's module, then is upgraded in place to the grown one,
/// whose `create_playlist` is another hash. A peer of each pinned revision
/// authors `create_playlist` at the hash it was built with: applied — no
/// verdict, which an older client would take as final — because the
/// server keeps the closures of every module it has run. `/healthz` lists
/// both.
///
/// Falsified once: with `Authority::ran` recording a module without
/// holding its closures, the old peer's `create_playlist` was answered
/// `held` — a frame its revision cannot read — and stayed pending, and
/// `converged_across` timed out naming it.
#[test]
fn version_2_an_old_peer_authors_a_mutator_whose_body_moved() {
    for o in olds_or_skip("2 old peer, changed mutator") {
        let mut f = Fleet::beside(&format!("v2-{}", o.name), &o);
        let grown = grown_module(f.root.path());
        let mut a = old_peer(&f, &o, "old", "alice");
        songs(&mut a, "v2", 3);
        create(&mut a, "Before");
        assert!(a.settle(PATIENCE));
        let t = Instant::now();
        upgrade_to(&mut f, module_flag(&grown));
        let ms = f.server.modules();
        // This build's and the grown one, and the old peer's own, which the
        // fresh server was told it ran (`Fleet::beside`).
        let want = if o.module.is_some() { 3 } else { 2 };
        assert_eq!(ms.len(), want, "every module run: {ms:?}");
        assert!(ms.iter().any(|(_, current)| *current));
        create(&mut a, "After");
        let mut g = grown_peer(&f, &grown, "grown", "alice");
        caught_up(&f, &mut g);
        let took = f.converged_across(&mut [(&mut a, "alice"), (&mut g, "alice")]);
        let after = playlist(&mut g, "After");
        measured(
            &format!("v2 {} upgrade to the grown module, to converged", o.name),
            t.elapsed(),
        );
        measured(&format!("v2 {} …the last settle", o.name), took);
        assert!(f
            .server
            .log_on_disk()
            .unwrap()
            .entries
            .values()
            .any(|(e, _)| e.autos.get("id") == Some(&Value::Id(after))));
    }
}

/// **3. New peer, old server.** A peer of this build running the grown
/// domain authors at hashes no pinned server ever had: `create_playlist`,
/// whose body the grown domain changed, and `add_song`, which moved when
/// the library gained its guard (`docs/plan-guards.md` D1). The old
/// server refuses each as an unknown function; this build reads that as
/// the hold it is — every one of them pending and held, three here — and
/// when the server is upgraded in place to this build with the grown
/// module, they are pushed again on the reconnect and land.
///
/// Falsified once: with `Client::recv` taking every `reject` as a verdict
/// (the unknown function not read as a hold), the intent was dropped at
/// the old server's answer and `held` never reached 1.
#[test]
fn version_3_a_new_peer_is_held_by_an_old_server_until_it_is_upgraded() {
    for o in olds_or_skip("3 new peer, old server") {
        let mut f = Fleet::old(&format!("v3-{}", o.name), &o.server);
        let grown = grown_module(f.root.path());
        let mut n = grown_peer(&f, &grown, "new", "alice");
        songs(&mut n, "v3", 2);
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
        upgrade_to(&mut f, module_flag(&grown));
        let took = f.converged_across(&mut [(&mut n, "alice")]);
        assert_eq!(n.status().held, Some(0));
        playlist(&mut n, "Ahead");
        measured(
            &format!("v3 {} upgraded under a held intent, to landed", o.name),
            t.elapsed(),
        );
        measured(&format!("v3 {} …the last settle", o.name), took);
    }
}

/// **4. Server upgraded in place.** A data directory written by each
/// pinned server — snapshot, journal, cursors, sessions — is opened by
/// this build's: every peer (one of each revision) reconnects on its own,
/// nobody is turned away, the log keeps its identity and its head, and
/// they go on and converge.
///
/// Falsified once: with the first start of the new module not re-homing
/// the snapshot (`persist::rehome` skipped), this build refused the old
/// server's log — `the snapshot's hash does not match its rows`, the hash
/// `docs/plan-db.md` D3 redefined — and never listened.
#[test]
fn version_4_a_server_upgraded_in_place() {
    for o in olds_or_skip("4 server upgraded in place") {
        let mut f = Fleet::old(&format!("v4-{}", o.name), &o.server);
        let mut a = old_peer(&f, &o, "old", "alice");
        let mut b = f.peer("new", Some("bob"));
        let mut c = f.peer("new-alice", Some("alice"));
        songs(&mut a, "v4", 4);
        create(&mut a, "Kept");
        create(&mut b, "Bobs");
        // Read off the old server rather than its files: this build cannot
        // check an older snapshot's hash (`docs/plan-db.md` D3 moved its
        // definition), which is what the upgrade's re-homing is for.
        for p in [&mut a, &mut b, &mut c] {
            assert!(p.settle(PATIENCE), "{} settled", p.name);
        }
        let head = old_head(&f);
        for p in [&mut a, &mut b, &mut c] {
            assert!(p.wait(head, PATIENCE), "{} at {head}", p.name);
        }
        let named = log_named(&f);
        let t = Instant::now();
        f.server.upgrade(vec![]);
        let after = f.server.log_on_disk().unwrap();
        assert_eq!(
            (after.id(), after.head_seq()),
            (named, head),
            "the log's identity and head survive the upgrade"
        );
        let kept = playlist(&mut c, "Kept");
        let m = media(&mut c);
        add(&mut a, kept, m[0]);
        add(&mut c, kept, m[1]);
        create(&mut b, "After");
        let took = f.converged_across(&mut [(&mut a, "alice"), (&mut b, "bob"), (&mut c, "alice")]);
        for p in [&mut a, &mut b, &mut c] {
            assert_eq!(p.status().denied, None, "{} was turned away", p.name);
        }
        measured(
            &format!("v4 {} upgrade in place, to converged", o.name),
            t.elapsed(),
        );
        measured(&format!("v4 {} …the last settle", o.name), took);
    }
}

/// **5. Client upgraded in place.** A directory written by each pinned
/// `harken-peer` — its replica and pending pages, intents made behind a
/// black hole — is opened by this build's, and the pending lands; and one
/// used `--alone` by the old peer, opened by this build with a server,
/// joins it, its local history landing.
///
/// Falsified once: with the upgraded peer started over a fresh directory
/// rather than the old one, the old peer's intents never reached the log
/// and `converged_across` named them.
#[test]
fn version_5_a_client_upgraded_in_place() {
    for o in olds_or_skip("5 client upgraded in place") {
        let f = Fleet::beside(&format!("v5-{}", o.name), &o);
        let mut a = old_peer(&f, &o, "old", "alice");
        let mut b = f.peer("new", Some("alice"));
        songs(&mut a, "v5", 3);
        create(&mut a, "Shared");
        assert!(a.settle(PATIENCE));
        let shared = playlist(&mut a, "Shared");
        let m = media(&mut a);
        a.proxy.blackhole();
        create(&mut a, "Away");
        add(&mut a, shared, m[2]);
        assert_eq!(a.status().pending, 2, "pending behind the black hole");
        a.quit();
        a.proxy.pass();
        let t = Instant::now();
        a.upgrade();
        f.converged_across(&mut [(&mut a, "alice"), (&mut b, "alice")]);
        measured(
            &format!(
                "v5 {} pending kept through the upgrade, to converged",
                o.name
            ),
            t.elapsed(),
        );
    }
}

/// The pinned revisions from before `docs/plan-alone.md`, whose `--alone`
/// kept no history ([`version_5c_an_alone_directory_from_before_plan_alone`]).
const BEFORE_ALONE: [&str; 1] = ["v4-journal"];

/// One pinned revision's alone directory opened by this build with a
/// server: its local history joins and lands.
fn alone_upgraded(o: &Old) {
    let f = Fleet::beside(&format!("v5b-{}", o.name), o);
    let mut b = f.peer("new", Some("alice"));
    songs(&mut b, "v5b", 2);
    let mut d = f.peer_stopped("alone", Some("alice"));
    d.old(&o.peer);
    d.alone = true;
    d.start();
    create(&mut d, "Local");
    songs(&mut d, "v5b-alone", 2);
    d.quit();
    d.alone = false;
    let t = Instant::now();
    d.upgrade();
    let took = f.converged_across(&mut [(&mut b, "alice"), (&mut d, "alice")]);
    playlist(&mut b, "Local");
    measured(
        &format!(
            "v5b {} alone history joined after the upgrade, to converged",
            o.name
        ),
        t.elapsed(),
    );
    measured(&format!("v5b {} …the last settle", o.name), took);
}

/// **5b. An alone directory upgraded in place.** A directory a pinned
/// `harken-peer` used `--alone`, opened by this build with a server: it
/// joins, and its local history lands. The alone log's snapshot was hashed
/// by the state hash before `docs/plan-db.md` D3 redefined it, and says so
/// by having no `hashing`; this build checks it the way it was written and
/// holds it hashed the new way (`ark::journal::log_from_value`). Every
/// pinned revision but those from before `docs/plan-alone.md`, which are
/// 5c's.
///
/// Falsified once, against `v4-rows`, by `ark::journal` checking every
/// snapshot by this build's construction alone: `storage: reading log: the
/// snapshot's hash does not match its rows`, the directory not opened.
#[test]
fn version_5b_an_alone_directory_upgraded_in_place() {
    for o in olds_or_skip("5b alone directory upgraded in place") {
        if BEFORE_ALONE.contains(&o.name.as_str()) {
            println!("fleet: 5b: {} is before plan-alone, and is 5c's", o.name);
            continue;
        }
        alone_upgraded(&o);
    }
}

/// **5c. An alone directory from before plan-alone** — a witness, ignored.
/// Before `docs/plan-alone.md` a peer alone kept no history, only its
/// store under a cursor of its own sequence. Opened with a server, that
/// store is taken as the fork and paged on top of: the peer reaches the
/// head with nothing pending, and its playlists are not the log's — its
/// alone work never reaches the server, and the server's lands on a store
/// that was never its. **Accepted as a limitation** (`docs/plan-db.md`
/// D1): those directories predate the fork, so there is no history to hand
/// over, and nothing will be built to recover one. Kept, ignored, so that
/// what such a directory does is written down and can be run with
/// `--ignored` under `HARKEN_OLD_<n>_*`.
#[test]
#[ignore = "accepted limitation: an alone directory from before plan-alone predates the fork and has no history to hand over"]
fn version_5c_an_alone_directory_from_before_plan_alone() {
    for o in olds_or_skip("5c alone directory from before plan-alone") {
        if BEFORE_ALONE.contains(&o.name.as_str()) {
            alone_upgraded(&o);
        }
    }
}

/// **6. Schema grows under running peers.** A server of this build is
/// upgraded in place to the grown domain — a nullable `playlist_item.note`,
/// a table `tag` — under a peer that stays on harken's own module. The
/// grown peer writes the column and the table; the facts reach the other
/// peer, which applies them projected, says `behind`, stays usable (it
/// authors on top of what the grown peer made, and `create_playlist` at
/// its own hash is applied from the closure the server kept), and
/// converges on everything it can see — its confirmed state hashing as the
/// log's does, projected to its schema. The grown peer sees the column,
/// and hashes as the log does under the grown schema.
///
/// Needs no pinned revision: "the next release" is the grown domain.
///
/// Falsified once: with `Client::heard_module` never setting the
/// replica's `behind`, the narrower peer stored the grown rows as they
/// came — `note` and all — and its hash was not the projected log's.
#[test]
fn version_6_a_schema_grows_under_running_peers() {
    use ark::store::{project_row, Change, MemoryStore, Store};
    let mut f = Fleet::new("v6-grows");
    let grown = grown_module(f.root.path());
    let mut n = f.peer("narrow", Some("alice"));
    songs(&mut n, "v6", 4);
    create(&mut n, "Mine");
    assert!(n.settle(PATIENCE));
    let mine = playlist(&mut n, "Mine");
    let m = media(&mut n);
    add(&mut n, mine, m[0]);
    let t = Instant::now();
    upgrade_to(&mut f, module_flag(&grown));
    let mut g = grown_peer(&f, &grown, "grown", "alice");
    caught_up(&f, &mut g);
    add(&mut g, mine, m[1]);
    g.author(
        "set_item_note",
        vec![
            ("playlist_id", Value::Id(mine)),
            ("media_id", Value::Id(m[0])),
            ("note", Value::text("for the drive")),
        ],
    );
    g.author(
        "tag_playlist",
        vec![
            ("playlist_id", Value::Id(mine)),
            ("tag", Value::text("summer")),
        ],
    );
    create(&mut g, "Grown");
    assert!(g.settle(PATIENCE));
    caught_up(&f, &mut n);
    let grown_list = playlist(&mut n, "Grown");
    add(&mut n, grown_list, m[2]);
    add(&mut n, mine, m[3]);
    create(&mut n, "Still");
    let took = f.converged_across(&mut [(&mut n, "alice"), (&mut g, "alice")]);
    assert_eq!(
        n.status().behind,
        Some(true),
        "the narrower peer says it is behind"
    );
    assert_eq!(g.status().behind, Some(false));
    let notes: Vec<Value> = g
        .query("item_notes", vec![("playlist_id", Value::Id(mine))])
        .into_iter()
        .map(|r| r["note"].clone())
        .collect();
    assert!(
        notes.contains(&Value::text("for the drive")),
        "the grown peer sees the column: {notes:?}"
    );

    // The log under each schema: the grown one as it is, harken's own
    // projected — which is what each peer's confirmed store must hash as.
    let wide = harken_server::grown::domain().module().schema.clone();
    let log = ark_server::persist::load(&f.server.data, &wide)
        .unwrap()
        .unwrap();
    let at_head = ark_server::persist::widen(&log.state_at(log.head_seq()).unwrap());
    let narrow = domain().module().schema.clone();
    let mut projected = MemoryStore::empty(narrow.clone());
    for tbl in narrow.tables() {
        for r in at_head.scan(&tbl.name) {
            projected.apply_change(&Change::Add(
                tbl.name.clone(),
                project_row(tbl, &r).unwrap(),
            ));
        }
    }
    let hash = |st: &MemoryStore| hex(&ark::hash::state_hash(st));
    assert_eq!(g.hash().1, hash(&at_head), "the grown peer is the log");
    assert_eq!(
        n.hash().1,
        hash(&projected),
        "the narrower peer is the log, projected to its schema"
    );
    measured(
        "v6 upgrade to a grown schema under a peer, to converged",
        t.elapsed(),
    );
    measured("v6 …the last settle", took);
}

/// **7. Module update over a retained log.** A server of this build with
/// a log of harken's own module, restarted with the grown module: it
/// replays its log — the same identity, the same head — and an intent at
/// `create_playlist`'s old hash, made behind a black hole before the
/// restart and pushed after it, is applied from the closure the server
/// retained, though the module it runs no longer ships that hash; a
/// fresh peer of the grown domain syncs the whole log and converges.
///
/// Needs no pinned revision.
///
/// Falsified once: with `Authority::ran` recording a module without
/// holding its closures, the pushed intent was `held`, `pending` stayed 1,
/// and `converged_across` timed out naming it.
#[test]
fn version_7_a_module_update_over_a_retained_log() {
    let mut f = Fleet::new("v7-retained");
    let grown = grown_module(f.root.path());
    let mut a = f.peer("a", Some("alice"));
    let mut c = f.peer("c", Some("alice"));
    songs(&mut a, "v7", 3);
    create(&mut a, "One");
    f.converged_across(&mut [(&mut a, "alice"), (&mut c, "alice")]);
    c.proxy.blackhole();
    create(&mut c, "Offline");
    let before = f.server.log_on_disk().unwrap();
    let t = Instant::now();
    upgrade_to(&mut f, module_flag(&grown));
    let after = f.server.log_on_disk().unwrap();
    assert_eq!(
        (after.id(), after.head_seq()),
        (before.id(), before.head_seq())
    );
    let shipped = harken_server::grown::domain();
    let old_create = after
        .entries
        .values()
        .map(|(e, _)| e.fn_hash.clone())
        .find(|h| !shipped.closures().contains_key(h))
        .expect("an entry naming a hash the grown module does not ship");
    let ms = f.server.modules();
    assert_eq!(ms.len(), 2, "{ms:?}");
    c.proxy.pass();
    let mut d = grown_peer(&f, &grown, "fresh", "alice");
    let took = f.converged_across(&mut [(&mut a, "alice"), (&mut c, "alice"), (&mut d, "alice")]);
    assert_eq!(c.status().held, Some(0));
    playlist(&mut d, "Offline");
    println!(
        "fleet: v7 an entry at {} is replayed by a module that ships no such hash",
        &hex(&old_create)[..12]
    );
    measured("v7 restart with a newer module, to converged", t.elapsed());
    measured("v7 …the last settle", took);
}
