//! The schema a server last stood on, and the steps that read what it
//! wrote (`src/migrations.rs`).

use money_domain::domain;

/// The schema reads every row the one a server last stood on wrote
/// (`tests/schema.json`): as they are (`ark::schema::compat`), or through
/// the domain's migrations. A change that neither grows the schema nor
/// comes with a step that reads the rows before it is a server that does
/// not start. `ARK_WRITE_GEN=1` writes this schema there, once it is the
/// one the server runs.
#[test]
fn the_schema_reads_what_the_last_one_wrote() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/schema.json");
    let now = domain().schema.clone();
    if std::env::var_os("ARK_WRITE_GEN").is_some() {
        std::fs::write(path, serde_json::to_string_pretty(&*now).unwrap() + "\n").unwrap();
    }
    let was = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}; ARK_WRITE_GEN=1 writes it"));
    let was: ark::schema::Schema = serde_json::from_str(&was).unwrap();
    let read = ark::migrate::migrate(&was, &vec![], &now, &domain().migrations);
    assert!(read.is_ok(), "{read:?}");
}

/// A server older still — before credit cards, banks connected from the
/// app and formulas (`tests/older-schema.json`) — reads through the same
/// steps: its schema grows into the one the first step reads.
#[test]
fn an_older_server_reads_too() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/older-schema.json");
    let was: ark::schema::Schema = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let now = domain().schema.clone();
    let read = ark::migrate::migrate(&was, &vec![], &now, &domain().migrations);
    assert!(read.is_ok(), "{read:?}");
}
