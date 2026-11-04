//! The schema a server last stood on, which this one grows from.

use money_domain::domain;

/// The schema reads every row the one a server last stood on wrote
/// (`tests/schema.json`), as they are: this one grows from it
/// (`ark::schema::compat`). A change that does not grow the schema is a
/// server that does not start. `ARK_WRITE_GEN=1` writes this schema there, once it is the
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
    let read = ark::schema::compat(&was, &now);
    assert!(read.is_ok(), "{read:?}");
}
