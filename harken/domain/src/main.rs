//! harken's domain, as a program that emits it.
//!
//! Nothing here runs on a phone or a server. Run, it writes `harken.ark`:
//! the schema, four mutators and three queries as Ark IR in canonical CBOR,
//! which `arkc verify` checks and `arkc gen` turns into Rust, Swift and
//! Kotlin — the server with everything, each client with only what it
//! calls (`--only`), the rest arriving as facts. The builder's types keep
//! the IR well-formed as it is written; the verifier holds it to the
//! schema.
//!
//! One file per concern: `schema`, `library` (what the scanner authors),
//! `playlists` (what people do), `queries` (what screens read).

mod library;
mod playlists;
mod queries;
mod schema;

use ark_builder::ModuleBuilder;

/// The functions a client program calls; everything else it applies by
/// facts. Passed to `arkc gen --only`.
pub const CLIENT_FUNCTIONS: &[&str] = &[
    "create_playlist",
    "add_to_playlist",
    "remove_from_playlist",
    "library",
    "playlists",
    "playlist_items",
];

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "harken.ark".to_string());
    let module = harken();
    module.write(&out).expect("write harken.ark");
    println!("{out}  {} functions", module.functions().len());
}

/// The whole domain.
pub fn harken() -> ModuleBuilder {
    let mut m = ModuleBuilder::new();
    schema::schema(&mut m);
    library::library(&mut m);
    playlists::playlists(&mut m);
    queries::queries(&mut m);
    m
}
