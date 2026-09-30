//! The JSON the vectors are written in is `ark::json`: one copy of the
//! dialect, shared with `harken-peer` and the fleet that drives it
//! (`docs/plan-fleet.md` §1). What a vector file looks like is decided
//! there, and so is every byte of it.

pub use ark::json::{array, json, obj, quoted};
