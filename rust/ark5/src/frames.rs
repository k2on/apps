//! §1.11 The frames: one `Sum` type, encoded by §1.1.
//!
//! ```text
//! Client: Hello { space, cursor, hash: Option<EntryHash>, device: Option<Text> }
//!         Push  { space, entry, body: Option<Bytes> }   -- the body when the authority asked
//!         Fetch { hashes }
//! Server: Ack   { id, seq, facts: Option<[Change]> }
//!         Reject{ id, why } | Held { id, fn }           -- Held: send the body
//!         Batch { space, from, logged: [Logged] }
//!         Snapshot { space, seq, hash, state, rows }
//!         Bodies { [(FnHash, Bytes)] }
//! Client: Join  { space, from, hash, logged: [Logged] } -- a chain made alone, handed over
//! ```
//!
//! The variants are numbered in that order, `Join` last. The type, the
//! encoder and the decoder are `#[derive(Node)]`'s (§1.4), and
//! `tests/frames.rs` holds every frame's bytes to [`frame_ty`].

use ark5_macros::Node;

use crate::hash::{EntryHash, FnHash};
use crate::ir::Node;
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
        device: Option<String>,
    },
    Push {
        space: Space,
        entry: Entry,
        body: Option<Vec<u8>>,
    },
    Fetch {
        hashes: Vec<FnHash>,
    },
    Ack {
        id: EntryId,
        seq: Seq,
        facts: Option<Vec<Change>>,
    },
    Reject {
        id: EntryId,
        why: String,
    },
    Held {
        id: EntryId,
        func: FnHash,
    },
    Batch {
        space: Space,
        from: Seq,
        logged: Vec<Logged>,
    },
    Snapshot {
        space: Space,
        snapshot: Snapshot,
    },
    Bodies {
        bodies: Vec<(FnHash, Vec<u8>)>,
    },
    Join {
        space: Space,
        from: Seq,
        hash: EntryHash,
        logged: Vec<Logged>,
    },
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
        "Frame" => Frame::ty(),
        _ => panic!("no frame type {name}"),
    }
}

pub fn frame_ty() -> Ty {
    Frame::ty()
}

impl Frame {
    pub fn enc(&self) -> Vec<u8> {
        self.to_bytes()
    }

    /// A frame from canonical bytes, whole, or why not.
    pub fn dec(b: &[u8]) -> Result<Frame, Refusal> {
        Frame::from_bytes(b)
    }
}
