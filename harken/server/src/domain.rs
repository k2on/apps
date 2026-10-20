//! harken's domain as this crate calls it: the bindings `build.rs` writes
//! from `harken/domain` (`ark::gen`), and the host's Rust beside them.

include!(concat!(env!("OUT_DIR"), "/domain.rs"));

#[path = "../../domain/host.rs"]
pub mod host;
