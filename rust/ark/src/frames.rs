//! §1.11 The frames: one `Sum` type, encoded by §1.1. **The protocol is
//! pages**: a peer sends pages of its intents; an authority answers with
//! verdicts and pages of its chain.
//!
//! ```text
//! Hello   { space, cursor, hash: Option<EntryHash>, device: Option<Bytes>, schema, version }
//! Page    { space, from, logged: [Logged] }
//! Snapshot{ space, snapshot }
//! Verdict { space, id, verdict: Ok { seq } | Refused { why } | Held { func: name } }
//! Telemetry { events: [Event] }
//! ```
//!
//! An authority's `Page` is its log from `from`, sequenced. A peer's `Page`
//! is its intents not yet sequenced, each at sequence 0 with no facts and
//! zero hashes ([`Logged::intent`]) — a push being a page of one; a chain
//! it made alone is pushed so too, as intents (§1.7). A `Verdict` answers one intent: sequenced at `seq`, refused, or
//! `Held` — a function the authority does not run.
//!
//! `Telemetry` is no machine's: what a device says of itself
//! (docs/plan-observe.md), sent to its server beside the frames, which a
//! server collects and a machine never reads.
//!
//! **No code crosses the wire.** An entry names its function by name; an
//! authority runs exactly the functions its program was built with, and
//! `Held` says an intent names one it does not run, and the intent stays pending on the device, pushed
//! again on its next connection. Nothing a client sends is ever loaded.
//!
//! Every frame names its space, so two machines share a connection (§1.9):
//! a verdict's id is `(origin, local)`'s, and each space counts its own.
//! `device` is what a device says it is, the bytes the authority authors
//! its `on_join` from (the driver's to read); `schema` and `version` are the
//! domain the peer runs, its schema's hash and its version string. A frame
//! is its bytes in the value format, serde's derived form (`crate::format`;
//! `tests/frames.rs` holds every frame to
//! reading back as itself).

use serde::{Deserialize, Serialize};

use crate::format::Encode;
use crate::hash::{EntryHash, SchemaHash};
use crate::log::{Entry, EntryId, Logged, Seq, Snapshot};
use crate::schema::Space;
use crate::Refusal;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Frame {
    Hello {
        space: Space,
        cursor: Seq,
        hash: Option<EntryHash>,
        device: Option<Vec<u8>>,
        schema: SchemaHash,
        version: String,
    },
    Page {
        space: Space,
        from: Seq,
        logged: Vec<Logged>,
    },
    Snapshot {
        space: Space,
        snapshot: Snapshot,
    },
    Verdict {
        space: Space,
        id: EntryId,
        verdict: Verdict,
    },
    Telemetry {
        events: Vec<Event>,
    },
}

/// One thing a peer said of itself: when (its clock, ms), which peer, how
/// loud (`error`, `warn`, `info`, `debug`, `trace`), what (`msg`), and its
/// fields as text, the spans around it first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub at: i64,
    pub peer: String,
    pub level: String,
    pub msg: String,
    pub fields: Vec<(String, String)>,
}

/// An authority's answer to one intent; what its journal keeps of each it
/// judged (never `Held`, which is not a judgment).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    Ok { seq: Seq },
    Refused { why: String },
    Held { func: String },
}

impl Frame {
    /// The space whose log the frame is about.
    pub fn space(&self) -> Space {
        match self {
            Frame::Hello { space, .. } | Frame::Page { space, .. } | Frame::Snapshot { space, .. } | Frame::Verdict { space, .. } => *space,
            // About no log: the log's space, whose machine passes it by.
            Frame::Telemetry { .. } => Space::Durable,
        }
    }

    /// A peer's intents, not yet sequenced, as a page.
    pub fn push(space: Space, entries: impl IntoIterator<Item = Entry>) -> Frame {
        let logged = entries.into_iter().map(Logged::intent).collect();
        Frame::Page { space, from: 0, logged }
    }

    pub fn enc(&self) -> Vec<u8> {
        self.to_bytes()
    }

    /// A frame from canonical bytes, whole, or why not.
    pub fn dec(b: &[u8]) -> Result<Frame, Refusal> {
        Frame::from_bytes(b)
    }
}
