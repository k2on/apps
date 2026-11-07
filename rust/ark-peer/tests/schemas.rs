//! A schema only grows (§1.2, `ark_peer::disk::own`): a server opens a disk
//! of any schema its own grows from, and refuses one it does not; every
//! device syncs with every server, a build older or newer, its rows and
//! facts read under its own schema; a device's disk of a schema its own
//! does not grow from starts empty, its pending intents kept; and a device
//! of a schema its server's neither grows from nor into is told to update,
//! and says so.

mod support;

use std::path::Path;
use std::sync::Arc;

use ark::schema::Table;
use ark::store::Change;
use ark_peer::link::{bell, wait};
use ark_peer::{dev, Config, Files, Peer, Server};
use support::named;

/// The demo with `playlist.name` called `title`: not a schema it grows.
fn retitled() -> Config {
    let mut d = arkdb::demo::demo();
    let mut s = (*d.schema).clone();
    let t = s.tables.iter_mut().find(|t| t.name == "playlist").unwrap();
    t.columns.iter_mut().find(|c| c.name == "name").unwrap().name = "title".into();
    d.schema = Arc::new(s);
    Config::of(Arc::new(d))
}

/// The demo with a table appended, `tag`: a schema it grows.
fn grown() -> Config {
    let mut d = arkdb::demo::demo();
    let mut s = (*d.schema).clone();
    s.tables.push(Table::new("tag").col("name", ark::row::Type::Text));
    d.schema = Arc::new(s);
    Config::of(Arc::new(d))
}

fn server(config: Config, data: &Path) -> anyhow::Result<ark_peer::Handle<ark_peer::Authority>> {
    let mut s = Server::new(config);
    s.data = Some(data.to_path_buf());
    Ok(s.build()?.0)
}

/// A device of `config` on `disk`, linked to `hub`, pumped until `f` holds.
fn synced(config: Config, disk: &Path, who: &str, hub: &ark_peer::Handle<ark_peer::Authority>, author: &[&str], f: impl Fn(&Peer) -> bool) -> Peer {
    let mut p = Peer::open(config, dev(who).unwrap(), Box::new(Files::new(disk).unwrap()));
    let (wake, rx) = bell();
    p.set_waker(wake);
    p.set_token(Some(who.into()));
    p.connect_with("", hub.dialer(Arc::new(dev)));
    for name in author {
        p.mutate_at("demo.create_playlist", named(name), 1).unwrap();
    }
    let held = ark_peer::link::until(std::time::Duration::from_secs(5), std::time::Duration::ZERO, || {
        p.pump();
        f(&p) || {
            wait(&p, &rx, std::time::Duration::from_millis(5));
            false
        }
    });
    assert!(held, "never synced");
    p
}

fn playlists(p: &Peer) -> usize {
    p.log.store.rows("playlist").count()
}

/// A server reopened under a schema that grows its own keeps its rows,
/// and writes a row of the table it appended; a device of the build before
/// and one of the new build both sync with it, each authoring an intent
/// the server sequences, and only the new one holds the tag. Falsified by
/// a replica applying facts of a table it lacks as they are: the old
/// device diverged at the tag, and filled for ever.
#[test]
fn every_device_syncs_with_a_server_whose_schema_grew() {
    let dir = tempfile::tempdir().unwrap();
    let (data, old, new) = (dir.path().join("server"), dir.path().join("old"), dir.path().join("new"));
    let hub = server(support::config(), &data).unwrap();
    drop(synced(support::config(), &old, "alice@laptop", &hub, &["Hymns"], |p| {
        p.log.pending().is_empty() && playlists(p) == 1
    }));
    hub.stop();
    let hub = server(grown(), &data).unwrap();
    let schema = grown().log.schema.clone();
    let (_, tag) = schema.table("tag").unwrap();
    let row = ark::dynamic::row_bytes(tag, &serde_json::json!({"name": "loud"})).unwrap();
    hub.write(vec![Change::Add { table: "tag".into(), row }]).unwrap().unwrap();
    let a = synced(support::config(), &old, "alice@laptop", &hub, &["Psalms"], |p| {
        p.log.pending().is_empty() && playlists(p) == 2
    });
    let b = synced(grown(), &new, "bob@phone", &hub, &["Carols"], |p| {
        p.log.pending().is_empty() && playlists(p) == 3
    });
    assert!(
        a.log.diverged() == 0 && a.log.store.schema_arc().table("tag").is_none(),
        "the old device has no tags"
    );
    assert_eq!(b.log.store.rows("tag").count(), 1, "the new one holds it");
    assert_eq!(hub.with(|p| p.log.store.rows("playlist").count()).unwrap(), 3);
    hub.stop();
}

/// A server under a schema its own does not grow from does not start, and
/// its disk is as it was. Falsified by `own` comparing nothing: the server
/// started, its rows unread.
#[test]
fn a_server_under_a_schema_not_grown_from_does_not_start() {
    let dir = tempfile::tempdir().unwrap();
    let hub = server(support::config(), dir.path()).unwrap();
    hub.with(|p| p.mutate_at("demo.create_playlist", named("Hymns"), 1)).unwrap().unwrap();
    hub.stop();
    let kv = std::fs::read(dir.path().join("log/kv")).ok();
    let why = server(retitled(), dir.path()).err().expect("refused").to_string();
    assert!(why.contains("does not grow from"), "{why}");
    assert_eq!(std::fs::read(dir.path().join("log/kv")).ok(), kv);
}

/// A device's disk of a schema its own does not grow from holds no row it
/// was sent and no entry, and its pending intent still, run again on top;
/// reopened again it is the same. Falsified by `own` reading any schema as
/// its own: the head was 1.
#[test]
fn a_replica_of_a_schema_not_grown_from_starts_empty_its_pending_kept() {
    let dir = tempfile::tempdir().unwrap();
    let (data, phone) = (dir.path().join("server"), dir.path().join("phone"));
    let hub = server(support::config(), &data).unwrap();
    let mut p = synced(support::config(), &phone, "alice@laptop", &hub, &["Hymns"], |p| {
        p.log.pending().is_empty() && p.log.log().head().0 >= 1
    });
    hub.stop();
    p.mutate_at("demo.create_playlist", named("Psalms"), 1).unwrap();
    drop(p);
    for _ in 0..2 {
        let p = Peer::open(retitled(), dev("alice@laptop").unwrap(), Box::new(Files::new(&phone).unwrap()));
        assert_eq!(p.log.log().head().0, 0, "no entry it was sent");
        assert_eq!(p.log.pending().len(), 1, "its intent, pending");
        assert_eq!(playlists(&p), 1, "and run on top");
    }
}

/// A device of a schema its server's neither grows from nor into is told,
/// as it connects, that it is to update: its status says so, its link is
/// closed until it is, and it keeps its rows and its intent, nothing sent.
/// Falsified by the server serving it: it was filled, and its intent
/// judged.
#[test]
fn a_device_its_server_cannot_serve_says_it_is_to_update() {
    let dir = tempfile::tempdir().unwrap();
    let (data, phone) = (dir.path().join("server"), dir.path().join("phone"));
    let hub = server(support::config(), &data).unwrap();
    hub.with(|p| p.mutate_at("demo.create_playlist", named("Hymns"), 1)).unwrap().unwrap();
    let p = synced(retitled(), &phone, "alice@laptop", &hub, &["Psalms"], |p| {
        p.link_state() == "update needed"
    });
    assert!(p.update_needed().is_some_and(|w| w.starts_with(ark::frames::UPDATE)));
    assert!(!p.linked, "its link closed");
    assert_eq!(
        (p.log.pending().len(), playlists(&p), p.log.log().head().0),
        (1, 1, 0),
        "its own, and nothing sent"
    );
    assert_eq!(hub.with(|h| h.log.store.rows("playlist").count()).unwrap(), 1, "nothing judged");
    hub.stop();
}
