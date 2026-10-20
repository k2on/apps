//! §1.11 The frames: one `Sum` type, encoded by §1.1.
//!
//! ```text
//! Client: Hello { space, cursor, hash: Option<EntryHash>, device: Option<Bytes> }
//!         Push  { space, entry }
//! Server: Ack   { space, id, seq, facts: Option<[Change]> }
//!         Reject{ space, id, why } | Held { space, id, fn }  -- Held: a function it does not run
//!         Batch { space, from, logged: [Logged] }
//!         Snapshot { space, seq, hash, state, rows }
//! Client: Join  { space, from, hash, logged: [Logged] } -- a chain made alone, handed over
//! ```
//!
//! **No body crosses the wire.** An authority runs exactly the functions
//! bound in the modules its operator loaded; `Held` says an intent names
//! one it does not run, and the intent stays pending on the device, pushed
//! again on its next connection. Nothing a client sends is ever loaded.
//!
//! The variants are numbered in that order, `Join` last. Every frame about
//! one space's log names it, so two machines share a connection (§1.9): a
//! verdict's id is `(session, local)`'s, and each space counts its own.
//! `device` is what a device says it is, the bytes the authority authors
//! its `on_join` from (the driver's to read). The type, the
//! encoder and the decoder are `#[derive(Node)]`'s (§1.4), and
//! `tests/frames.rs` holds every frame's bytes to [`frame_ty`].

use ark_macros::Node;

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
        device: Option<Vec<u8>>,
    },
    Push {
        space: Space,
        entry: Entry,
    },
    Ack {
        space: Space,
        id: EntryId,
        seq: Seq,
        facts: Option<Vec<Change>>,
    },
    Reject {
        space: Space,
        id: EntryId,
        why: String,
    },
    Held {
        space: Space,
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
    /// The space whose log the frame is about.
    pub fn space(&self) -> Space {
        match self {
            Frame::Hello { space, .. } | Frame::Push { space, .. } | Frame::Ack { space, .. } | Frame::Reject { space, .. } => *space,
            Frame::Held { space, .. } | Frame::Batch { space, .. } | Frame::Snapshot { space, .. } | Frame::Join { space, .. } => *space,
        }
    }

    pub fn enc(&self) -> Vec<u8> {
        self.to_bytes()
    }

    /// A frame from canonical bytes, whole, or why not.
    pub fn dec(b: &[u8]) -> Result<Frame, Refusal> {
        Frame::from_bytes(b)
    }
}
