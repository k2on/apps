//! §1.2 A schema grows under a log: an authority reopened under a
//! prefix-extension of the schema its records were written under keeps its
//! head, its chain and its rows, the rows widened and the store's digests
//! this schema's. And builds of other schemas meet it (§1.11): an input an
//! older or a newer build wrote is read as a row is; a function a server no
//! longer runs is refused as needing an update, so the queue moves on; and
//! a build whose schema neither grows from its server's nor into it is told
//! once to update, and is not filled.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod wire;
}

use std::sync::Arc;

use ark::frames::{Frame, Verdict, UPDATE};
use ark::log::{EntryId, Origin, Record};
use ark::peer::{Ctx, Input, Peer, Role};
use ark::row::Type;
use ark::schema::{Column, Schema, Table};
use ark::store::{Change, Store};
use demo::{create_input, pair_input, ADD, CREATE};
use support::wire::*;

/// The demo's schema with a column appended to `playlist` (an `Option`) and
/// a table appended, `tag`.
fn wider() -> Arc<Schema> {
    let mut s = demo::schema();
    s.tables[0].columns.push(Column {
        name: "note".into(),
        ty: Type::Option(Box::new(Type::Text)),
        default: None,
    });
    s.tables.push(Table::new("tag").col("name", Type::Text));
    ark::schema::compat(&demo::schema(), &s).unwrap();
    Arc::new(s)
}

/// Tags a row in the appended table: what only the wider schema can log.
fn tag_it(store: &mut Store, _: &Ctx, input: &[u8]) -> Result<Vec<Change>, String> {
    let c = Change::Add {
        table: "tag".into(),
        row: input.to_vec(),
    };
    store.apply(&c)?;
    Ok(vec![c])
}

const TAG: &str = "grow.tag";

fn durable(p: &Peer) -> Vec<Record> {
    vec![Record::Rows(p.confirmed_rows())]
}

fn wide_authority(d: Vec<Record>) -> Peer {
    let cas = ark::Domain {
        schema: wider(),
        ..demo::demo().run(TAG, Arc::new(tag_it))
    };
    let mut p = Peer::new(Arc::new(cas), Role::authority(), who("server"));
    p.step(Input::Open(d));
    p
}

/// An authority reopened under an appended column and an appended table
/// keeps its head, its chain entry for entry and its rows, the playlist
/// row widened with `None`; its store hashes as the wider schema's from
/// those rows; a `Hello` from the start is paged every entry, the old
/// schema's among them; reopened again, and after compacting, it is the
/// same. Falsified by `open` deriving each entry's
/// state again (cutting the chain where it differs): the head was 0.
#[test]
fn a_log_reopened_under_a_wider_schema_keeps_head_chain_and_rows() {
    let mut server = authority(demo::cas());
    let mut alice = replica("alice", demo::cas());
    for (f, input) in [
        (CREATE, create_input("Road")),
        (ADD, pair_input(&EntryId::of(Origin::named("alice-session"), 1).draw(0), "t1")),
    ] {
        let o = alice.step(mutate(f, input, 1));
        let (_, fan) = deliver_up(&mut server, &who("alice"), &o);
        alice.step(down(&fan[0]));
    }
    assert_eq!(server.log().head().0, 2);
    let mut wide = wide_authority(durable(&server));
    assert_eq!(wide.log(), server.log(), "head and chain kept");
    let rows = |p: &Peer, t: &str| p.store.rows(t).map(<[u8]>::to_vec).collect::<Vec<_>>();
    let widened: Vec<Vec<u8>> = rows(&server, "playlist")
        .into_iter()
        .map(|r| {
            let mut v = ark::dynamic::row(&demo::schema().tables[0], &r).unwrap();
            v["note"] = serde_json::Value::Null;
            ark::dynamic::row_bytes(&wider().tables[0], &v).unwrap()
        })
        .collect();
    assert_eq!(rows(&wide, "playlist"), widened, "a row widened with None");
    assert_eq!(rows(&wide, "item"), rows(&server, "item"));
    let fresh = Store::from_rows(wider(), ark::schema::Space::Durable, &wide.store.snapshot_rows()).unwrap();
    assert_eq!(wide.store.state_hash(), fresh.state_hash(), "the digests are the wider schema's");
    assert_ne!(wide.store.state_hash(), server.store.state_hash());

    let o = wide.step(Input::Mutate {
        func: TAG.into(),
        input: ark::dynamic::row_bytes(&wider().tables[2], &serde_json::json!({"name": "loud"})).unwrap(),
        now: 3,
        cause: None,
    });
    assert_eq!(fanouts(&o).len(), 1, "logged");
    let s = sends(&wide.step(up(
        &who("bob"),
        &Frame::Hello {
            space: ark::schema::Space::Durable,
            cursor: 0,
            hash: Default::default(),
            fill: None,
            most: ark::peer::PART as u64,
            schema: Default::default(),
        },
    )));
    assert!(matches!(&s[..], [Frame::Page { from: 0, logged, .. }] if logged.len() == 3), "{s:?}");

    let again = wide_authority(durable(&wide));
    assert_eq!((again.log(), &again.store), (wide.log(), &wide.store));
    wide.compact(3);
    let compacted = wide_authority(durable(&wide));
    assert_eq!(compacted.log().head(), wide.log().head());
    assert_eq!(compacted.store, wide.store);
}

/// A log reopened under the schema it was written under is its own, from
/// its rows whole or from its entries appended: the same log and the same
/// state. Falsified by `open` not applying the appended entries' facts.
#[test]
fn a_log_reopened_under_its_own_schema_is_its_own() {
    let mut server = authority(demo::cas());
    let mut alice = replica("alice", demo::cas());
    for (f, input) in [
        (CREATE, create_input("Road")),
        (ADD, pair_input(&EntryId::of(Origin::named("alice-session"), 1).draw(0), "t1")),
    ] {
        let o = alice.step(mutate(f, input, 1));
        let (_, fan) = deliver_up(&mut server, &who("alice"), &o);
        alice.step(down(&fan[0]));
    }
    let mut whole = authority(demo::cas());
    whole.step(Input::Open(durable(&server)));
    let mut base = durable(&authority(demo::cas()));
    base.extend(server.log().entries().into_iter().map(Record::Logged));
    let mut appended = authority(demo::cas());
    appended.step(Input::Open(base));
    for p in [&whole, &appended] {
        assert_eq!(p.log(), server.log());
        assert_eq!(p.store.state_hash(), server.store.state_hash());
    }
}

// * Builds of other schemas

/// The demo with `playlist`'s columns the other way round: a schema that
/// neither grows from the demo's nor into it — as harken's albums-by-folder
/// is for the builds before it.
fn reordered() -> Arc<ark::Domain> {
    let mut s = demo::schema();
    s.tables[0].columns.swap(1, 2);
    Arc::new(ark::Domain {
        schema: Arc::new(s),
        ..demo::demo()
    })
}

/// A newer build's `create_playlist`: its input with a note appended.
#[derive(Clone, Debug, PartialEq, ark::prelude::Serialize, ark::prelude::Deserialize, ark::prelude::Input)]
pub struct CreateNoted {
    pub name: String,
    pub note: Option<String>,
}

#[ark::prelude::mutator("demo.create_playlist")]
fn create_noted(ctx: &Ctx, db: &mut ark::prelude::Db<demo::Demo>, input: CreateNoted) -> Result<(), String> {
    demo::create_playlist(ctx, db, demo::CreatePlaylist { name: input.name })
}

/// Each step's pushes delivered to `server` as alice's, and what it said
/// to all and back carried to her; its verdicts.
fn carried(server: &mut Peer, alice: &mut Peer, o: &[ark::peer::Output]) -> Vec<Verdict> {
    let (back, fan) = deliver_up(server, &who("alice"), o);
    fan.iter().chain(&back).for_each(|f| _ = alice.step(down(f)));
    verdicts(&back).into_iter().flatten().collect()
}

/// An input an older build wrote — its fields a prefix of this build's —
/// is read with the fields appended since empty, and one a newer build
/// wrote without the fields this one lacks, as a row is: each intent is
/// sequenced. Falsified by inputs read strictly by position: each was
/// refused.
#[test]
fn an_input_of_an_older_or_a_newer_build_is_read() {
    let noted = ark::functions!(ark::Domain::new::<demo::Demo>("demo", "2"));
    let mut server = authority(Arc::new(noted));
    let mut alice = replica("alice", demo::cas());
    let o = alice.step(mutate(CREATE, create_input("Road"), 1));
    let said = carried(&mut server, &mut alice, &o);
    assert!(matches!(&said[..], [Verdict::At { seq: 1, .. }]), "an older build's: {said:?}");
    let mut server = authority(demo::cas());
    let mut bob = replica("bob", Arc::new(server.domain.as_ref().clone()));
    let note = CreateNoted {
        name: "Sea".into(),
        note: Some("blue".into()),
    };
    let o = bob.step(mutate(CREATE, ark::prelude::Encode::to_bytes(&note), 1));
    let said = carried(&mut server, &mut bob, &o);
    assert!(matches!(&said[..], [Verdict::At { seq: 1, .. }]), "a newer build's: {said:?}");
    let names: Vec<String> = ark::db::read::<demo::Demo, _>(&server.store, |db| db.playlist.scan())
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(names, ["Sea"]);
}

/// A function its server's build no longer runs, named by an intent of a
/// build no newer — the same schema here — is refused as needing an update,
/// and the intents after it are judged: the device's queue moves on.
/// Falsified by holding it whatever the build, as before: the playlist
/// after it was held behind it, for ever.
#[test]
fn a_retired_function_is_refused_and_the_queue_moves_on() {
    let mut server = authority(demo::only(&[CREATE]));
    let mut alice = replica("alice", demo::cas());
    let road = EntryId::of(Origin::named("alice-session"), 1).draw(0);
    let mut said = vec![];
    for (f, input) in [
        (CREATE, create_input("Road")),
        (ADD, pair_input(&road, "t1")),
        (CREATE, create_input("Sea")),
    ] {
        let o = alice.step(mutate(f, input, 1));
        said.extend(carried(&mut server, &mut alice, &o));
    }
    assert!(
        matches!(&said[..], [Verdict::At { seq: 1, .. }, Verdict::Refused { why }, Verdict::At { seq: 2, .. }] if why.starts_with(UPDATE)),
        "{said:?}"
    );
    assert!(alice.pending().is_empty() && alice.store == server.store);
}

/// A build whose schema neither grows from its server's nor into it is
/// told once to update, as its `Hello` is answered, and sent no fill; its
/// pushes are held, not judged. Told, it sends nothing and takes nothing it
/// is sent, its rows and its intents as they were. Falsified by serving it
/// (judging its schema by nothing): its `Hello` was answered as any
/// build's.
#[test]
fn a_build_its_server_cannot_serve_is_told_to_update() {
    let mut server = authority(demo::cas());
    let mut old = replica("alice", reordered());
    old.step(mutate(CREATE, create_input("Road"), 1));
    let (back, fan) = deliver_up(&mut server, &who("alice"), &old.step(Input::Connected));
    assert!(fan.is_empty() && server.log().head().0 == 0, "nothing judged");
    assert!(
        matches!(&back[..], [Frame::Update { why, .. }, Frame::Verdict { verdict: Verdict::At { last: 0, .. }, .. }] if why.starts_with(UPDATE)),
        "{back:?}"
    );
    back.iter().for_each(|f| _ = old.step(down(f)));
    assert!(old.update_needed().is_some_and(|w| w.starts_with(UPDATE)));
    assert_eq!(
        (old.pending().len(), old.store.rows("playlist").count()),
        (1, 1),
        "its intent and its preview kept"
    );
    assert!(sends(&old.step(Input::Connected)).is_empty(), "it asks nothing more");
    assert!(sends(&old.step(mutate(CREATE, create_input("Sea"), 2))).is_empty());
    let o = server.step(mutate(CREATE, create_input("Mix"), 3));
    old.step(down(&fanouts(&o)[0]));
    assert_eq!((old.log().head().0, old.store.rows("playlist").count()), (0, 2), "nor takes anything");
}
