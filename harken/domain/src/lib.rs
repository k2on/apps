//! harken's domain on ArkDB v5 (`docs/plan-v5.md` §1.12): the same domain
//! as `harken/domain`, function for function, written as `#[ark::*]`
//! functions over derived rows, records and sums, and assembled into one
//! module by [`module::domain`].
//!
//! [`schema`] is the tables; [`library`] what the scanner authors and every
//! peer reads, and the helpers that derive the keys; [`playlists`] what
//! people do; [`listening`] the listening session, live state over
//! volatile tables, and the plays; [`keys`] the derived keys on the host;
//! [`view`] what a screen holds of a library list.

pub mod built;
pub mod keys;
pub mod library;
pub mod listening;
pub mod module;
pub mod playlists;
pub mod schema;
pub mod view;

pub use module::domain;
