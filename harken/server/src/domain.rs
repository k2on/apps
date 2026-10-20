//! harken's domain as this crate calls it: the bindings `build.rs` writes
//! from `harken/domain` (`ark::gen`), and the host's Rust beside them.

include!(concat!(env!("OUT_DIR"), "/domain.rs"));

// host.rs is formatted where it lives, by harken/domain's width; this
// crate's rustfmt (the default width) leaves it alone.
#[rustfmt::skip]
#[path = "../../domain/host.rs"]
pub mod host;
