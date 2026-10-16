//! §14 Live state (`docs/plan-live.md`): a state machine per scope,
//! sequenced per instance beside the log, on the one socket — the frames, the
//! server's sequencer and the client's confirmed-plus-pending view, all in
//! [`state`].
//!
//! It superseded opaque rooms — a room per account, frames the engine never
//! looked inside (`say` and `heard`), an app's machine told who arrived,
//! spoke and left — which harken's listening session rode until it became a
//! live state (`docs/plan-live.md` phase 2). They are gone; what is left of
//! them is on the wire alone: a `say` from a peer of an older revision is
//! read and dropped, and so is a `heard` from an older server (§14).

pub mod state;

/// A connection, as the transport numbers them; never reused within one
/// authority's life.
pub type ConnId = i64;
