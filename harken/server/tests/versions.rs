//! Versions (`docs/plan-db.md` D1, on v5): which module may follow which,
//! and the fleet run across two of them.
//!
//! **The compat check** is `docs/plan-v5.md` §1.5's: §1.2's prefix check on
//! the schema plus one verify per new hash (`harken_server::hosted::compat`,
//! which `HARKEN_OLD_MODULES` is held to at start). Held here over harken's
//! module and three next releases written in this file: one whose
//! `create_playlist` moved (another hash, the same behaviour, the same
//! schema), one that grew a table and a mutator over it, and one that
//! grew a column on a table a function writes whole.
//!
//! **The fleet across versions.** A function whose body moved needs
//! nothing of the server in v5: an intent at a hash the server lacks is
//! held and its body sent (§1.11 `Held`), and one at a hash it holds from a
//! module run before is sequenced at once. A grown *schema* is another
//! matter: a log written under the narrower one is not read under the
//! wider ([`version_6_a_schema_grows_under_a_running_server`], a witness).
//!
//! Scenarios 1, 4 and 5 run a pinned previous revision beside this build's
//! (`nix/versions.nix`, which `checks.versions` hands over as
//! `HARKEN_OLD_<n>_SERVER` and `HARKEN_OLD_<n>_PEER`, [`olds`]). Without
//! them they say they are skipped and pass; `nix/versions.nix` is empty
//! until a shipped v5 revision is pinned, so they have not run on v5.
//!
//! `cargo test -p harken-server --test versions -- --nocapture` prints the
//! timings. Each test was falsified once; its doc comment says how.

mod support {
    pub mod fleet;
    pub mod proxy;
}

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use ark5::author::Domain;
use ark5::ir::{Expr, Function, Node, Op};
use ark5::module::{verify, Module};
use ark5::schema::{Column, Ref, Schema, Space, Table};
use ark5::ty::Ty;
use harken_server::hosted::{compat, write, Hosted};
use serde_json::json;
use support::fleet::*;

// -- the next releases ------------------------------------------------------------

/// harken's domain, its module changed by `f` over a schema `g` made.
fn release(g: impl FnOnce(&mut Schema), f: impl FnOnce(&mut Domain)) -> Domain {
    let mut d = harken_domain5::domain();
    let mut schema = (*d.schema).clone();
    g(&mut schema);
    d.module.schema = schema.clone();
    d.schema = Arc::new(schema);
    f(&mut d);
    d
}

/// `f` bound as `name` in `d`, checked over `d`'s schema.
fn bind(d: &mut Domain, name: &str, f: Function) {
    let h = d
        .lib
        .install(&d.schema, f.clone())
        .unwrap_or_else(|e| panic!("{name}: {}", e.0));
    d.bodies.insert(h, f.to_bytes());
    d.module.bindings.retain(|b| b.0 != name);
    d.module.bindings.push((name.into(), h));
    d.module.bindings.sort();
}

/// The function `name` is bound to, its body behind a check that always
/// holds: the same behaviour at another hash, what a release that touches
/// a mutator ships.
fn moved_fn(d: &Domain, name: &str) -> Function {
    let mut f = d.lib.get(&d.get(name)).unwrap().f.clone();
    let zero = || Box::new(Expr::Lit(Ty::Int, 0i64.to_bytes()));
    let never = Expr::Refuse(Box::new(Expr::Lit(
        Ty::Named("Text".into()),
        "never".to_string().to_bytes(),
    )));
    let check = Expr::If(
        Box::new(Expr::Op(Op::Eq, zero(), zero())),
        Box::new(Expr::Tuple(vec![])),
        Box::new(never),
    );
    f.body = Expr::Seq(vec![check, f.body]);
    f
}

/// `create_playlist` moved; nothing else.
fn moved() -> Domain {
    release(
        |_| {},
        |d| {
            bind(
                d,
                "playlists.create_playlist",
                moved_fn(d, "playlists.create_playlist"),
            )
        },
    )
}

/// A table appended, `tag` — a playlist's labels — and `grown.tag_playlist`
/// writing it.
fn grown() -> Domain {
    let col = |name: &str, decl: &str| Column {
        name: name.into(),
        decl: decl.into(),
        ty: Ty::Bytes,
        default: None,
    };
    let tag = Table {
        columns: vec![col("playlist_id", "Id<playlist>"), col("tag", "Text")],
        key: 2,
        refs: vec![Ref {
            column: 0,
            table: "playlist".into(),
        }],
        ..Table::new("tag")
    };
    release(
        |s| s.tables.push(tag),
        |d| {
            let id = d
                .module
                .types
                .iter()
                .find(|t| t.0 == "Id<playlist>")
                .unwrap()
                .clone();
            let f = Function {
                types: vec![id],
                params: vec![Ty::Tuple(vec![
                    Ty::Named("Id<playlist>".into()),
                    Ty::Named("Text".into()),
                ])],
                ret: Ty::unit(),
                effect: ark5::ir::Effect::Write,
                body: Expr::Insert("tag".into(), Box::new(Expr::Var(0))),
            };
            bind(d, "grown.tag_playlist", f)
        },
    )
}

/// `schema` with `note`, an `Option<Text>`, appended to `playlist_item`.
fn with_note(s: &mut Schema) {
    let t = s
        .tables
        .iter_mut()
        .find(|t| t.name == "playlist_item")
        .unwrap();
    t.columns.push(Column {
        name: "note".into(),
        decl: "Option<Text>".into(),
        ty: Ty::option(Ty::Bytes),
        default: None,
    });
}

fn refused(r: Result<usize, ark5::Refusal>) -> String {
    r.expect_err("refused").0
}

/// **The compat check.** A module follows itself with nothing new; the
/// release whose `create_playlist` moved follows harken's with one new
/// hash, verified; the release that grew a table follows it with one, and
/// harken's does not follow that one back (a table removed). Falsified by
/// a `compat` that skipped the schema's prefix check: harken's followed the
/// grown one.
#[test]
fn a_module_follows_another_by_its_schema_and_a_verify_per_new_hash() {
    let h = harken_domain5::domain();
    assert_eq!(compat(&h.module, &h.module, &h.lib).map_err(|e| e.0), Ok(0));
    let m = moved();
    verify(&m.module, &m.lib).unwrap();
    assert_ne!(
        m.get("playlists.create_playlist"),
        h.get("playlists.create_playlist")
    );
    assert_eq!(compat(&h.module, &m.module, &m.lib).map_err(|e| e.0), Ok(1));
    let g = grown();
    verify(&g.module, &g.lib).unwrap();
    assert_eq!(compat(&h.module, &g.module, &g.lib).map_err(|e| e.0), Ok(1));
    assert_eq!(
        refused(compat(&g.module, &h.module, &h.lib)),
        "a table was removed"
    );
}

/// **What is not additive is refused**: a column reordered, retyped, or
/// appended with neither an `Option` type nor a default (with a default it
/// follows); a table rekeyed or moved to the volatile space; and a column
/// appended to a table a function writes whole — the schema follows, and
/// the function must move with it, so its new hash is verified over the
/// wider row and refused, by name. Falsified by a `compat` that verified
/// no new hash: the moved `add_to_playlist` followed.
#[test]
fn what_is_not_additive_is_refused() {
    let h = harken_domain5::domain();
    let with = |g: &dyn Fn(&mut Table)| {
        let mut s = h.module.schema.clone();
        g(s.tables.iter_mut().find(|t| t.name == "playlist").unwrap());
        Module {
            schema: s,
            ..h.module.clone()
        }
    };
    let follows = |m: &Module| compat(&h.module, m, &h.lib).map_err(|e| e.0);
    let msg = |m: &Module| refused(compat(&h.module, m, &h.lib));
    assert!(msg(&with(&|t| t.columns.swap(1, 4))).contains("reordered or retyped"));
    assert!(msg(&with(&|t| t.columns[2].ty = Ty::Bytes)).contains("reordered or retyped"));
    assert!(msg(&with(&|t| t.key = 2)).contains("rekeyed"));
    assert!(msg(&with(&|t| t.space = Space::Volatile)).contains("another space"));
    let rating = |default: Option<Vec<u8>>| {
        with(&move |t: &mut Table| {
            t.columns.push(Column {
                name: "rating".into(),
                decl: "Int".into(),
                ty: Ty::Int,
                default: default.clone(),
            })
        })
    };
    assert!(msg(&rating(None)).contains("rating is appended with neither"));
    assert_eq!(follows(&rating(Some(0i64.to_bytes()))), Ok(0));

    let noted = release(with_note, |_| {});
    assert_eq!(
        compat(&h.module, &noted.module, &noted.lib).map_err(|e| e.0),
        Ok(0),
        "the schema follows"
    );
    let e = verify(&noted.module, &noted.lib).unwrap_err().0;
    assert!(
        e.starts_with("playlists.add_all_to_playlist:"),
        "a function writing the row whole no longer checks: {e}"
    );
    // Moved, its body over the narrower row: checked where it was written.
    let f = moved_fn(&h, "playlists.add_to_playlist");
    noted.lib.install(&h.schema, f.clone()).unwrap();
    let mut module = noted.module.clone();
    module
        .bindings
        .iter_mut()
        .find(|b| b.0 == "playlists.add_to_playlist")
        .unwrap()
        .1 = f.hash();
    let e = refused(compat(&h.module, &module, &noted.lib));
    assert!(e.starts_with("playlists.add_to_playlist:"), "{e}");
}

/// **The server holds a module run before only if it may follow it**
/// (`HARKEN_OLD_MODULES`): the grown release holds harken's — and runs
/// every body of it — and harken's refuses to start holding the grown
/// one, saying why. Falsified by `Hosted::hold` not asking `compat`: it
/// was refused only when a grown body did not check (`no table tag`), not
/// as a module it may not follow.
#[test]
fn a_server_holds_an_old_module_it_follows() {
    let dir = tempfile::tempdir().unwrap();
    let (old, new) = (
        dir.path().join("harken.module"),
        dir.path().join("grown.module"),
    );
    write(harken_domain5::domain(), &old).unwrap();
    write(grown(), &new).unwrap();
    let (held, n) = Hosted::read(&new).unwrap().hold(&old).unwrap();
    assert_eq!(n, harken_domain5::domain().bodies.len());
    assert!(held
        .module
        .cas
        .get(&harken_domain5::domain().get("playlists.create_playlist"))
        .is_some());
    let e = Hosted::read(&old)
        .unwrap()
        .hold(&new)
        .err()
        .expect("refused")
        .to_string();
    assert!(
        e.contains("may not follow") && e.contains("a table was removed"),
        "{e}"
    );
}

// -- the fleet across versions ----------------------------------------------------

/// A module file of `d` under `dir`.
fn module_file(dir: &Path, name: &str, d: Domain) -> PathBuf {
    let p = dir.join(format!("{name}.module"));
    write(d, &p).unwrap();
    p
}

const NO_PLAYLIST: Id = [0; 16];

fn media(p: &mut PeerProc) -> Vec<Id> {
    ids_of(
        &p.query("library", vec![("playlist_id", id(NO_PLAYLIST))]),
        "id",
    )
}

fn playlist(p: &mut PeerProc, name: &str) -> Id {
    let rows = p.query("playlists", vec![]);
    let hit = rows.iter().find(|r| r["name"] == json!(name));
    id_of(&hit.unwrap_or_else(|| panic!("{} has no playlist {name}", p.name))["id"])
}

fn add(p: &mut PeerProc, list: Id, m: Id) {
    p.author(
        "add_to_playlist",
        vec![("playlist_id", id(list)), ("media_id", id(m))],
    );
}

fn create(p: &mut PeerProc, name: &str) {
    p.author("create_playlist", vec![("name", json!(name))]);
}

fn songs(p: &mut PeerProc, tag: &str, n: usize) {
    for i in 0..n {
        p.author("add_song", song(&format!("{tag}-{i}")));
    }
}

/// A peer of this build running the module in `file`, started.
fn peer_on(f: &Fleet, file: &Path, name: &str, user: &str) -> PeerProc {
    let mut p = f.peer_stopped(name, Some(user));
    p.args = vec!["--module".into(), file.display().to_string()];
    p.start();
    p
}

/// The head `/healthz` says.
fn head(f: &Fleet) -> i64 {
    let h = f.server.health().unwrap_or_default();
    h.lines()
        .find_map(|l| l.strip_prefix("head ")?.trim().parse().ok())
        .expect("a head in /healthz")
}

/// The function each entry of the log names, by hash.
fn hashes(f: &Fleet) -> Vec<ark5::hash::FnHash> {
    f.server
        .log_on_disk()
        .unwrap()
        .entries
        .iter()
        .map(|l| l.entry.func)
        .collect()
}

/// **3. A peer authors at a hash the server never had.** The server runs
/// harken's module; a peer of the release whose `create_playlist` moved
/// authors it at its new hash. The server holds it and asks for the body;
/// the peer sends it, and it is sequenced — no upgrade needed, where v4's
/// old server refused it as unknown until upgraded. Beside a peer of
/// harken's own, everyone converges, and the log names both hashes.
/// Falsified by the peer not sending a body when asked (`--module` not
/// handed to it, so it authored harken's hash): the log named one hash.
#[test]
fn version_3_a_peer_authors_at_a_hash_the_server_never_had() {
    let f = Fleet::new("v3-held");
    let next = module_file(f.root.path(), "moved", moved());
    let mut a = f.peer("a", Some("alice"));
    let mut n = peer_on(&f, &next, "next", "alice");
    let t = Instant::now();
    create(&mut a, "Before");
    create(&mut n, "Ahead");
    f.converged(&mut [&mut a, &mut n]);
    playlist(&mut a, "Ahead");
    let hs = hashes(&f);
    assert!(
        hs.contains(&moved().get("playlists.create_playlist")),
        "the moved hash is in the log"
    );
    assert!(hs.contains(&harken_domain5::domain().get("playlists.create_playlist")));
    measured("v3 a moved hash, held and sent, to converged", t.elapsed());
}

/// **3b. What a peer authors behind a held intent lands** — a witness. As
/// in 3, a peer of the moved release authors `create_playlist` at a hash
/// the server never had, and then `add_song`. The server holds the first
/// and asks for its body; the second arrives meanwhile, past what the
/// session has sequenced, and is dropped as a gap — "pushed again after a
/// Hello" (`ark5::peer::Peer::push`) — but the peer pushes nothing again
/// until it reconnects: the first lands with its body, the second stays
/// pending. Run with `--ignored`: `converged` times out with one pending.
#[test]
#[ignore = "witness: an intent pushed behind a held one is dropped as a gap and not pushed again until the link reconnects (ark5 Peer::push)"]
fn version_3b_what_a_peer_authors_behind_a_held_intent_lands() {
    let f = Fleet::new("v3b-behind-held");
    let next = module_file(f.root.path(), "moved", moved());
    let mut n = peer_on(&f, &next, "next", "alice");
    create(&mut n, "Ahead");
    songs(&mut n, "v3b", 1);
    f.converged(&mut [&mut n]);
}

/// **7. Module update over a retained log.** A server of harken's module,
/// restarted with the release whose `create_playlist` moved, holding
/// harken's as run before (`HARKEN_OLD_MODULES`): the log keeps its head;
/// an intent at the old hash made behind a black hole before the restart
/// is pushed after it and sequenced from the body the server holds; a
/// fresh peer of the new release syncs the whole log and converges with
/// the old one; `/healthz` names the module it runs and the one it holds.
/// Falsified by `start` not reading `HARKEN_OLD_MODULES`: `/healthz` named
/// no module held.
#[test]
fn version_7_a_module_update_over_a_retained_log() {
    let mut f = Fleet::new("v7-retained");
    let old = module_file(f.root.path(), "harken", harken_domain5::domain());
    let next = module_file(f.root.path(), "moved", moved());
    let mut a = f.peer("a", Some("alice"));
    let mut c = f.peer("c", Some("alice"));
    songs(&mut a, "v7", 3);
    create(&mut a, "One");
    f.converged(&mut [&mut a, &mut c]);
    c.proxy.blackhole();
    create(&mut c, "Offline");
    let before = head(&f);
    let t = Instant::now();
    let (next_s, old_s) = (next.display().to_string(), old.display().to_string());
    f.server.upgrade(vec![
        ("HARKEN_MODULE", &next_s),
        ("HARKEN_OLD_MODULES", &old_s),
    ]);
    assert_eq!(head(&f), before, "the log keeps its head");
    let h = f.server.health().unwrap();
    assert!(
        h.contains(&format!("module {} current\n", moved().module.hash())),
        "{h}"
    );
    assert!(
        h.contains(&format!(
            "module {}\n",
            harken_domain5::domain().module.hash()
        )),
        "it holds harken's: {h}"
    );
    c.proxy.pass();
    let mut d = peer_on(&f, &next, "fresh", "alice");
    create(&mut d, "Next");
    let took = f.converged(&mut [&mut a, &mut c, &mut d]).took;
    playlist(&mut d, "Offline");
    let m = media(&mut d);
    let next_list = playlist(&mut a, "Next");
    add(&mut a, next_list, m[0]);
    f.converged(&mut [&mut a, &mut c, &mut d]);
    assert!(hashes(&f).contains(&moved().get("playlists.create_playlist")));
    measured("v7 restart with a newer module, to converged", t.elapsed());
    measured("v7 …the last settle", took);
}

/// **6. A schema grows under a running server** — a witness. The server is
/// upgraded in place to the release that appended the table `tag`; §1.2
/// says a row written under the narrower schema is read under the wider
/// by completing it, and the log should keep its head. It does not: the
/// hub opens its chain under the new schema, every entry's state hash
/// covers the schema's hash, so the first entry does not check and the
/// chain is cut there (`ark5::peer::Peer::open`); `Table::complete` and
/// `Table::prefix` are not called anywhere. Run with `--ignored`: the head
/// after the upgrade is 0.
#[test]
#[ignore = "witness: a log written under a narrower schema is cut at its first entry when opened under a wider one (ark5 Peer::open: the state hash covers the schema hash; Table::complete is not wired)"]
fn version_6_a_schema_grows_under_a_running_server() {
    let mut f = Fleet::new("v6-grows");
    let next = module_file(f.root.path(), "grown", grown());
    let mut a = f.peer("a", Some("alice"));
    create(&mut a, "Mine");
    f.converged(&mut [&mut a]);
    let before = head(&f);
    let next_s = next.display().to_string();
    f.server.upgrade(vec![("HARKEN_MODULE", &next_s)]);
    assert_eq!(
        head(&f),
        before,
        "the log keeps its head under the grown schema"
    );
}

// -- a pinned previous revision -----------------------------------------------------

/// A pinned previous revision: its two binaries.
pub struct Old {
    pub name: String,
    pub server: PathBuf,
    pub peer: PathBuf,
}

/// The pinned revisions `checks.versions` hands over, `HARKEN_OLD_1_*`
/// upward until one is missing.
pub fn olds() -> Vec<Old> {
    let var = |n: usize, k: &str| {
        std::env::var(format!("HARKEN_OLD_{n}_{k}"))
            .ok()
            .filter(|v| !v.is_empty())
    };
    (1..)
        .map_while(|n| {
            Some(Old {
                name: var(n, "NAME").unwrap_or_else(|| format!("old-{n}")),
                server: var(n, "SERVER")?.into(),
                peer: var(n, "PEER")?.into(),
            })
        })
        .collect()
}

/// [`olds`], or none and a line saying the scenario is skipped.
fn olds_or_skip(scenario: &str) -> Vec<Old> {
    let o = olds();
    if o.is_empty() {
        println!("fleet: {scenario}: skipped, no HARKEN_OLD_1_SERVER and HARKEN_OLD_1_PEER (nix/versions.nix pins none)");
    }
    o
}

/// A peer of a pinned revision, started.
fn old_peer(f: &Fleet, o: &Old, name: &str, user: &str) -> PeerProc {
    let mut p = f.peer_stopped(name, Some(user));
    p.bin = Some(o.peer.clone());
    p.start();
    p
}

/// **1. Old peer, new server.** A peer of each pinned revision signs in,
/// syncs and authors beside two peers of this build, and all of them
/// converge. Not run on v5: no revision is pinned.
#[test]
fn version_1_an_old_peer_and_a_new_server() {
    for o in olds_or_skip("1 old peer, new server") {
        let f = Fleet::new(&format!("v1-{}", o.name));
        let mut a = old_peer(&f, &o, "old", "alice");
        let mut b = f.peer("new", Some("bob"));
        let mut c = f.peer("new-alice", Some("alice"));
        let t = Instant::now();
        songs(&mut a, "v1", 6);
        create(&mut a, "Old");
        f.converged(&mut [&mut a, &mut b, &mut c]);
        let m = media(&mut a);
        let old = playlist(&mut a, "Old");
        for x in &m[..3] {
            add(&mut a, old, *x);
        }
        create(&mut b, "New");
        add(&mut c, old, m[5]);
        let took = f.converged(&mut [&mut a, &mut b, &mut c]).took;
        measured(
            &format!("v1 {} old peer beside new ones, to converged", o.name),
            t.elapsed(),
        );
        measured(&format!("v1 {} …the last settle", o.name), took);
    }
}

/// **4. Server upgraded in place.** A data directory written by each
/// pinned server is opened by this build's: every peer (one of each
/// revision) reconnects on its own, nobody is turned away, the log keeps
/// its head, and they go on and converge. Not run on v5: no revision is
/// pinned.
#[test]
fn version_4_a_server_upgraded_in_place() {
    for o in olds_or_skip("4 server upgraded in place") {
        let mut f = Fleet::old(&format!("v4-{}", o.name), &o.server);
        let mut a = old_peer(&f, &o, "old", "alice");
        let mut b = f.peer("new", Some("bob"));
        songs(&mut a, "v4", 4);
        create(&mut a, "Kept");
        create(&mut b, "Bobs");
        f.converged(&mut [&mut a, &mut b]);
        let before = head(&f);
        let t = Instant::now();
        f.server.upgrade(vec![]);
        assert_eq!(
            head(&f),
            before,
            "the log keeps its head through the upgrade"
        );
        let kept = playlist(&mut a, "Kept");
        let m = media(&mut b);
        add(&mut a, kept, m[0]);
        create(&mut b, "After");
        let took = f.converged(&mut [&mut a, &mut b]).took;
        for p in [&mut a, &mut b] {
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
/// `harken-peer` — its replica and intents made behind a black hole — is
/// opened by this build's, and the pending lands. Not run on v5: no
/// revision is pinned.
#[test]
fn version_5_a_client_upgraded_in_place() {
    for o in olds_or_skip("5 client upgraded in place") {
        let f = Fleet::new(&format!("v5-{}", o.name));
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
        a.quit();
        a.proxy.pass();
        let t = Instant::now();
        a.bin = None;
        a.start();
        f.converged(&mut [&mut a, &mut b]);
        measured(
            &format!(
                "v5 {} pending kept through the upgrade, to converged",
                o.name
            ),
            t.elapsed(),
        );
    }
}
