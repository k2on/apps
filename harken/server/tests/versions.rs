//! Versions (`docs/plan-db.md` D1, on §1.16): which schema may follow
//! which, and the fleet run across two builds.
//!
//! **The compat check** is §1.2's prefix check on the schema
//! (`ark::schema::compat`): a release appends tables and columns, never
//! removes, reorders or retypes. Held here over harken's schema as its
//! structs derive it and the next releases written in this file. A domain
//! is this build's Rust (§1.16): a release that moves a function is a new
//! build, and an intent naming a function a server's build does not
//! register is held — the kernel's own tests hold that (`ark/tests`).
//!
//! **The fleet across versions.** Scenarios 1, 4 and 5 run a pinned
//! previous revision beside this build's (`nix/versions.nix`, which
//! `checks.versions` hands over as `HARKEN_OLD_<n>_SERVER` and
//! `HARKEN_OLD_<n>_PEER`, [`olds`]). Without them they say they are skipped
//! and pass; `nix/versions.nix` is empty until a shipped revision is
//! pinned, so they have not run.
//!
//! `cargo test -p harken-server --test versions -- --nocapture` prints the
//! timings. Each test was falsified once; its doc comment says how.

mod support {
    pub mod fleet;
    pub mod proxy;
}

use std::path::PathBuf;
use std::time::Instant;

use ark::row::Type;
use ark::schema::{compat, Column, Ref, Schema, Space, Table};
use serde_json::json;
use support::fleet::*;

// -- the next releases ------------------------------------------------------------

/// harken's schema, as its structs derive it.
fn harken() -> Schema {
    (*harken_domain::domain().schema).clone()
}

/// A table appended, `tag` — a playlist's labels.
fn grown() -> Schema {
    let col = |name: &str, ty: Type| Column {
        name: name.into(),
        ty,
        default: None,
    };
    let mut s = harken();
    s.tables.push(Table {
        columns: vec![
            col("playlist_id", Type::Id("playlist".into())),
            col("tag", Type::Text),
        ],
        key: 2,
        refs: vec![Ref {
            column: 0,
            table: "playlist".into(),
        }],
        ..Table::new("tag")
    });
    s
}

/// `schema` with `note`, an `Option<Text>`, appended to `playlist_item`.
fn with_note(mut s: Schema) -> Schema {
    let t = s
        .tables
        .iter_mut()
        .find(|t| t.name == "playlist_item")
        .unwrap();
    t.columns.push(Column {
        name: "note".into(),
        ty: Type::Option(Box::new(Type::Text)),
        default: None,
    });
    s
}

fn refused(r: Result<(), ark::Refusal>) -> String {
    r.expect_err("refused").0
}

/// **The compat check.** A schema follows itself; the release that grew a
/// table follows harken's, and harken's does not follow it back (a table
/// removed); a column appended as an `Option` follows. Falsified by a
/// `compat` that skipped the table count: harken's followed the grown one.
#[test]
fn a_schema_follows_another_by_its_prefix() {
    let h = harken();
    assert_eq!(compat(&h, &h), Ok(()));
    assert_eq!(compat(&h, &grown()), Ok(()));
    assert_eq!(refused(compat(&grown(), &h)), "a table was removed");
    assert_eq!(
        compat(&h, &with_note(harken())),
        Ok(()),
        "an Option appended follows"
    );
}

/// **What is not additive is refused**: a column reordered, retyped, or
/// appended with neither an `Option` type nor a default (with a default it
/// follows); a table rekeyed or moved to the volatile space. Falsified by
/// a `compat` that let a column be appended with no default: `rating`
/// followed.
#[test]
fn what_is_not_additive_is_refused() {
    let h = harken();
    let with = |g: &dyn Fn(&mut Table)| {
        let mut s = h.clone();
        g(s.tables.iter_mut().find(|t| t.name == "playlist").unwrap());
        s
    };
    let msg = |s: &Schema| refused(compat(&h, s));
    assert!(msg(&with(&|t| t.columns.swap(1, 4))).contains("reordered or retyped"));
    assert!(msg(&with(&|t| t.columns[2].ty = Type::Bytes)).contains("reordered or retyped"));
    assert!(msg(&with(&|t| t.key = 2)).contains("rekeyed"));
    assert!(msg(&with(&|t| t.space = Space::Volatile)).contains("another space"));
    let rating = |default: Option<serde_json::Value>| {
        with(&move |t: &mut Table| {
            t.columns.push(Column {
                name: "rating".into(),
                ty: Type::Int,
                default: default.clone(),
            })
        })
    };
    assert!(msg(&rating(None)).contains("rating is appended with neither"));
    assert_eq!(compat(&h, &rating(Some(json!(0)))), Ok(()));
}

// -- the fleet across versions ----------------------------------------------------

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

/// The head `/healthz` says.
fn head(f: &Fleet) -> i64 {
    let h = f.server.health().unwrap_or_default();
    h.lines()
        .find_map(|l| l.strip_prefix("head ")?.trim().parse().ok())
        .expect("a head in /healthz")
}

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
