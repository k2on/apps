//! harken's domain (`docs/plan-v5.md` §1.15) is the `.ark` files beside
//! this crate — the schema, the types, the library, the playlists and the
//! listening session, in ArkLang — and nothing here is a second copy of
//! it. The crate is where its tests live: [`DIR`], the module [`text`]
//! builds from it, and the bindings an app's `build.rs` writes for it
//! ([`domain`], with the host's Rust beside them, `host.rs`).

/// The domain's directory: every `.ark` file under it is the module.
pub const DIR: &str = env!("CARGO_MANIFEST_DIR");

/// The module, built from [`DIR`].
pub fn text() -> ark::Domain {
    ark::sexp::build_dir(std::path::Path::new(DIR)).unwrap_or_else(|r| panic!("harken's domain: {r}"))
}

/// The domain's bindings (`ark::gen`), as `harken-server` and
/// `harken-iced` generate them, and `host.rs` beside them.
#[allow(clippy::all)]
pub mod domain;
