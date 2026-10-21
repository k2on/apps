//! §1.11 The frames: one `Sum` type, encoded by §1.1. **The protocol is
//! pages of a chain**: a peer sends pages of its chain; an authority
//! answers with verdicts and its own pages.
//!
//! ```text
//! Hello   { space, cursor, hash: Option<EntryHash>, device: Option<Bytes>, schema, version }
//! Page    { space, from, logged: [Logged] }
//! Snapshot{ space, snapshot }
//! Verdict { space, id, verdict: Ok { seq } | Refused { why } | Held { func: name } }
//! ```
//!
//! An authority's `Page` is its log from `from`, sequenced. A peer's `Page`
//! is one of two things, told by its entries: a chain it sequenced alone
//! from its fork at `from` (each entry at its sequence, hashed — what a
//! join hands over), or its intents not yet sequenced, each at sequence 0
//! with no facts and zero hashes ([`Logged::intent`]) — a push being a page
//! of one. A `Verdict` answers one intent: sequenced at `seq`, refused, or
//! `Held` — a function the authority does not run.
//!
//! **No code crosses the wire.** An entry names its function by name; an
//! authority runs exactly the functions its program was built with, and
//! `Held` says an intent names one it does not run, and the intent stays pending on the device, pushed
//! again on its next connection. Nothing a client sends is ever loaded.
//!
//! The variants are numbered in that order. Every frame names its space, so
//! two machines share a connection (§1.9): a verdict's id is `(origin,
//! local)`'s, and each space counts its own. `device` is what a device says
//! it is, the bytes the authority authors its `on_join` from (the driver's
//! to read); `schema` and `version` are the domain the peer runs, its
//! schema's hash and its version string. The type, the encoder and the decoder are `#[derive(Node)]`'s
//! (§1.4), and `tests/frames.rs` holds every frame's bytes to
//! [`frame_ty`].

use ark_macros::Node;

use crate::codec::Node;
use crate::hash::{EntryHash, SchemaHash};
use crate::log::{Entry, EntryId, Logged, Record, Seq, Snapshot};
use crate::schema::Space;
use crate::store::Change;
use crate::ty::Ty;
use crate::Refusal;

#[derive(Clone, Debug, PartialEq, Eq, Node)]
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
}

/// An authority's answer to one intent; what its journal keeps of each it
/// judged (never `Held`, which is not a judgment).
#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub enum Verdict {
    Ok { seq: Seq },
    Refused { why: String },
    Held { func: String },
}

/// The types, by name, as the derive writes them.
pub fn ty(name: &str) -> Ty {
    match name {
        "Space" => Space::ty(),
        "Entry" => Entry::ty(),
        "Change" => Change::ty(),
        "Logged" => Logged::ty(),
        "Snapshot" => Snapshot::ty(),
        "Record" => Record::ty(),
        "Verdict" => Verdict::ty(),
        "Frame" => Frame::ty(),
        _ => panic!("no frame type {name}"),
    }
}

pub fn frame_ty() -> Ty {
    Frame::ty()
}

impl Frame {
    /// The space whose log the frame is about.
    pub fn space(&self) -> Space {
        match self {
            Frame::Hello { space, .. } | Frame::Page { space, .. } | Frame::Snapshot { space, .. } | Frame::Verdict { space, .. } => *space,
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
