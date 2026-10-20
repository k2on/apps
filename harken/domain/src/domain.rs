//! The domain's bindings, written by `build.rs` (`ark::gen`), and the
//! host's Rust beside them.

include!(concat!(env!("OUT_DIR"), "/domain.rs"));

#[path = "../host.rs"]
pub mod host;
