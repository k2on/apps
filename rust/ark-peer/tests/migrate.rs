//! A log under a schema its server's does not grow from (`ark_peer::migrate`):
//! the server migrates its rows as it starts, keeping what they were, and
//! is ahead of every replica; a replica's own disk of the schema before
//! starts empty and is sent the rows again.

mod support;

use std::path::Path;
use std::sync::Arc;

use ark::migrate::{Migration, Tables};
use ark::prelude::*;
use ark::schema::Space;
use ark_peer::link::{bell, wait};
use ark_peer::{dev, Config, Files, Peer, Server};
use support::named;

/// The demo with `playlist.name` called `title`, and the step that reads
/// the demo's rows into it when `step`.
fn retitled(step: bool) -> Config {
    let mut d = arkdb::demo::demo();
    let was = serde_json::to_string(&*d.schema).unwrap();
    let mut s = (*d.schema).clone();
    let t = s.tables.iter_mut().find(|t| t.name == "playlist").unwrap();
    t.columns.iter_mut().find(|c| c.name == "name").unwrap().name = "title".into();
    d.schema = Arc::new(s);
    fn rename(t: &mut Tables) -> Result<(), Refusal> {
        for p in t.get_mut("playlist").into_iter().flatten() {
            let name = p.remove("name").ok_or(Refusal::new("a playlist without a name"))?;
            p.insert("title".into(), name);
        }
        Ok(())
    }
    if step {
        d = d.migration(Migration::new("playlist.title", &was, rename));
    }
    Config::of(Arc::new(d))
}

fn server(config: Config, data: &Path) -> anyhow::Result<ark_peer::Handle> {
    let mut s = Server::new(config);
    s.data = Some(data.to_path_buf());
    Ok(s.build()?.0)
}

/// Alice's device on `disk`, linked to `hub`, pumped until `f` holds.
fn synced(config: Config, disk: &Path, hub: &ark_peer::Handle, author: &[&str], f: impl Fn(&Peer) -> bool) -> Peer {
    let mut p = Peer::open(config, dev("alice@laptop").unwrap(), Box::new(Files::new(disk).unwrap()), false);
    let (wake, rx) = bell();
    p.set_waker(wake);
    p.set_token(Some("alice@laptop".into()));
    p.connect_with("", hub.dialer(Arc::new(dev)));
    for name in author {
        p.author(Space::Durable, "demo.create_playlist", named(name), 1).unwrap();
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

/// The playlists' titles a store holds under the retitled schema.
fn titles(store: &ark::store::Store) -> Vec<String> {
    let (_, t) = store.schema_arc().table("playlist").unwrap();
    let rows = store.snapshot_rows().into_iter().find(|(n, _)| n == "playlist").unwrap().1;
    let mut out: Vec<String> = rows
        .iter()
        .map(|r| ark::dynamic::row(t, r).unwrap()["title"].as_str().unwrap().into())
        .collect();
    out.sort();
    out
}

/// A server reopened under the retitled schema migrates its rows: a
/// playlist's name is its title, its head one past where it stood, what it
/// stood on kept beside it, and its schema the new one. Alice's device,
/// its disk of the schema before, opens under the new one empty, is sent
/// the rows, and stands where the server does. Falsified by the snapshot
/// at the head it migrated rather than one past it: the head was where it
/// stood.
#[test]
fn a_server_migrates_its_rows_and_its_replicas_are_sent_them() {
    let dir = tempfile::tempdir().unwrap();
    let (data, phone) = (dir.path().join("server"), dir.path().join("phone"));
    let hub = server(support::config(), &data).unwrap();
    let before = synced(support::config(), &phone, &hub, &["Hymns", "Psalms"], |p| {
        p.log.pending.is_empty() && p.log.log().head().0 >= 2
    });
    let head = before.log.log().head().0;
    drop(before);
    hub.stop();

    let hub = server(retitled(true), &data).unwrap();
    let (at, store) = hub.with(|p| (p.log.log().head().0, p.log.store.clone())).unwrap();
    assert_eq!(at, head + 1, "one past the head it stood at");
    assert_eq!(titles(&store), ["Hymns", "Psalms"]);
    let was = &arkdb::demo::demo().schema.hash().hex()[..12];
    assert!(data.join(format!("log/log.{was}")).exists(), "what it stood on, kept");
    let schema: ark::schema::Schema = serde_json::from_slice(&std::fs::read(data.join("log/schema.json")).unwrap()).unwrap();
    assert_eq!(schema.hash(), retitled(true).log.schema.hash());

    let alice = synced(retitled(true), &phone, &hub, &[], |p| p.log.log().head().0 == at);
    assert_eq!(titles(&alice.log.store), ["Hymns", "Psalms"]);
    hub.stop();
}

/// A server under a schema no step reads its rows into does not start,
/// and its disk is as it was. Falsified by `migrate` reading the rows as
/// the new schema's: the server started.
#[test]
fn a_server_no_step_migrates_does_not_start() {
    let dir = tempfile::tempdir().unwrap();
    let hub = server(support::config(), dir.path()).unwrap();
    hub.with(|p| p.author(Space::Durable, "demo.create_playlist", named("Hymns"), 1))
        .unwrap()
        .unwrap();
    hub.stop();
    let kv = std::fs::read(dir.path().join("log/kv")).ok();
    let why = server(retitled(false), dir.path()).err().expect("refused").to_string();
    assert!(why.contains("no migration reads rows of schema"), "{why}");
    assert_eq!(std::fs::read(dir.path().join("log/kv")).ok(), kv);
}

/// A device's disk of the schema before, reopened under the retitled one,
/// holds no row it was sent and no entry, and its pending intent still,
/// run again on top; reopened again it is the same, its disk now the new
/// schema's. Falsified by `kept` reading a schema not grown from as kept:
/// the head was 1.
#[test]
fn a_replica_of_the_schema_before_starts_empty_its_pending_kept() {
    let dir = tempfile::tempdir().unwrap();
    let (data, phone) = (dir.path().join("server"), dir.path().join("phone"));
    let hub = server(support::config(), &data).unwrap();
    let mut p = synced(support::config(), &phone, &hub, &["Hymns"], |p| {
        p.log.pending.is_empty() && p.log.log().head().0 >= 1
    });
    hub.stop();
    p.author(Space::Durable, "demo.create_playlist", named("Psalms"), 1).unwrap();
    drop(p);
    for _ in 0..2 {
        let p = Peer::open(retitled(true), dev("alice@laptop").unwrap(), Box::new(Files::new(&phone).unwrap()), false);
        assert_eq!(p.log.log().head().0, 0, "no entry it was sent");
        assert_eq!(p.log.pending.len(), 1, "its intent, pending");
        assert_eq!(titles(&p.log.store), ["Psalms"], "and run on top");
    }
}

/// Whether a replica's rows are kept: an empty disk's, and those of a
/// schema this one grows from; not those of one it does not, nor rows with
/// no schema beside them. Falsified by rows without a schema read as kept.
#[test]
fn a_replica_keeps_rows_of_a_schema_it_grows_from() {
    use ark_peer::migrate::kept;
    use ark_peer::{Disk, Memory};
    let (demo, new) = (support::config().log.schema, retitled(true).log.schema);
    assert!(kept(&mut Memory::default(), &demo).unwrap(), "an empty disk");
    let mut d = Memory::default();
    d.put("kv", b"rows").unwrap();
    assert!(!kept(&mut d, &demo).unwrap(), "rows, and no schema beside them");
    assert!(kept(&mut d, &demo).unwrap(), "rows of this schema");
    assert!(!kept(&mut d, &new).unwrap(), "rows of a schema not grown from");
}
