//! harken's domain on ArkDB v5 (`docs/plan-v5.md` §1.15): v4's domain,
//! function for function, written with the Rust builders (`ark::author`)
//! over derived rows, records and sums — each function a `static`, two of
//! them with a line of Ark text (`listening::SCALED`, `MOVED_PLACE`) — and
//! assembled into one module by [`module::domain`].
//!
//! [`schema`] is the tables; [`library`] what the scanner authors and every
//! peer reads, and the helpers that derive the keys; [`playlists`] what
//! people do; [`listening`] the listening session, live state over
//! volatile tables, and the plays; [`keys`] the derived keys on the host;
//! [`view`] what a screen holds of a library list.

pub mod keys;
pub mod library;
pub mod listening;
pub mod module;
pub mod playlists;
pub mod schema;
pub mod view;

pub use module::domain;
