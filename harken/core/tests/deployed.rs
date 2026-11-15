//! The server the owner runs, and the clients built now (ui/docs/app.md,
//! Servers Already Deployed): `tests/deployed.json` is that server's
//! catalog — its schema and every function it registers — as the revision
//! it was built from (k2on/apps 77da9807, the rev k2on/os pins) wrote it
//! (`ark::domain::Catalog::of`). A client is that schema, and sends only
//! intents that server runs: one it does not have is held, pending, for
//! ever, and another schema's state hash is never the server's.
//!
//! With the `push` feature the push table is appended and `session.push`
//! registered, which that server has neither of; the tests say so, and
//! are the ones to move when the server is deployed again
//! (docs/releasing.md).
// A test reads its fixture: the domain's purity (clippy.toml) is its
// library's, not its tests'.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use ark::domain::{Catalog, Kind};

/// The deployed server's `/healthz` names its domain by this hash.
const DEPLOYED: &str = "b0f869fb912d16806b2340366f5b5f865e1ebfa4b6fd852050f1c0c35602249c";

fn deployed() -> Catalog {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/deployed.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Intents a client sends the server: those it judges, durable or live.
fn sent(k: Kind) -> bool {
    matches!(k, Kind::Mutator | Kind::Live)
}

/// The schema is the deployed server's, table for table: its hash is the
/// one that server's `/healthz` says, so a phone finds it the same
/// domain, and every state hash an entry carries is the one the client
/// works out. Falsified by appending the push table to the schema, as
/// the `push` feature does (`push_appends_its_table_and_one_intent`).
#[test]
#[cfg(not(feature = "push"))]
fn the_schema_is_the_deployed_servers() {
    let (now, was) = (harken_domain::domain(), deployed());
    assert_eq!(*now.schema, was.schema, "the schema is not the deployed server's");
    assert_eq!(was.schema.hash().hex(), DEPLOYED);
    assert_eq!(now.schema_hash().hex(), DEPLOYED);
}

/// Every intent a client can send is one the deployed server registers,
/// with the same input: none is held for want of a function, nor refused
/// for an input it cannot read. Falsified by `session.push` registered
/// (the `push` feature).
#[test]
#[cfg(not(feature = "push"))]
fn every_intent_is_one_the_deployed_server_runs() {
    let (now, was) = (Catalog::of(&harken_domain::domain()), deployed());
    let mut missing = vec![];
    for f in now.functions.iter().filter(|f| sent(f.kind)) {
        match was.get(&f.name) {
            Some(w) if w.kind == f.kind && w.input == f.input => {}
            Some(_) => missing.push(format!("{} (its input or kind differs)", f.name)),
            None => missing.push(format!("{} (not there)", f.name)),
        }
    }
    assert!(missing.is_empty(), "intents the deployed server does not run: {missing:?}");
}

/// With push on, the schema grows by the push table alone — appended, as
/// a schema grows (§1.2) — and `session.push` is the one intent added: what
/// a server deployed with push serves.
#[test]
#[cfg(feature = "push")]
fn push_appends_its_table_and_one_intent() {
    let (now, was) = (Catalog::of(&harken_domain::domain()), deployed());
    assert_eq!(was.schema.hash().hex(), DEPLOYED);
    let (n, w) = (&now.schema.tables, &was.schema.tables);
    assert_eq!(&n[..w.len()], &w[..], "the tables before it are as they were");
    assert_eq!(n[w.len()..].iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["push"]);
    let added: Vec<&str> = now
        .functions
        .iter()
        .filter(|f| sent(f.kind) && was.get(&f.name).is_none())
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(added, [ark::auth::PUSH]);
}

/// Every change a screen sends (`HarkenChange`) is a mutator the deployed
/// server registers by that name: falsified by a change named for a
/// function there is not (`playlists.create`).
#[test]
#[cfg(not(feature = "push"))]
fn every_change_a_screen_sends_is_a_deployed_mutator() {
    let was = deployed();
    for c in harken_domain::HarkenChange::every() {
        let f = was.get(c.name()).unwrap_or_else(|| panic!("{} is not the deployed server's", c.name()));
        assert_eq!(f.kind, Kind::Mutator, "{} is not a mutator", c.name());
    }
    assert_eq!(
        harken_domain::HarkenChange::CreatePlaylist(Default::default()).problem().as_deref(),
        Some("a playlist needs a name")
    );
}
