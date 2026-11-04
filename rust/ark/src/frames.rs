//! §1.11 The frames: one `Sum` type, encoded by §1.1. **The protocol is
//! pages**: a peer sends pages of its intents; an authority answers with
//! verdicts and pages of its chain.
//!
//! ```text
//! Hello   { space, cursor, hash: Option<EntryHash>, device: Option<Bytes>, schema, most }
//! Page    { space, from, logged: [Logged] }
//! Verdict { space, id, verdict: Ok { seq } | Refused { why } | Held { last } }
//! Part    { space, part: { seq, hash, state, first, ranges: [{ table, from, to, rows }], next } }
//! More    { space, from: { table, key }, most }
//! Call    { id, name, input }
//! Reply   { id, reply: Ok { output } | Refused { why } }
//! Offer   { hash, size }
//! Want    { hash, at }
//! Chunk   { hash, at, size, bytes }
//! Have    { hash }
//! Absent  { hash, why }
//! ```
//!
//! An authority's `Page` is its log from `from`, sequenced. A peer's `Page`
//! is its intents not yet sequenced, each at sequence 0 with no facts and
//! zero hashes ([`Logged::intent`]) — a push being a page of one; what it
//! did with no server is pushed so too, as intents (§1.7). A `Verdict` answers
//! one intent: sequenced at `seq`, refused, or `Held` — not judged, past
//! one of its origin's the authority has not judged or naming a function
//! it does not run — with the last of its origin it has.
//!
//! A `Part` is a part of a fill (§1.7): the authority's answer to a
//! `Hello` its log cannot page — the first, from its rows' start — and to
//! each `More`, the part from where it says, of rows as they stand when
//! the authority answers. A replica asks for each part once it holds the
//! one before. A `Hello` and a `More` say how big a replica would have the
//! part they ask for, `most` bytes of rows at most — one that takes a part
//! on the thread it draws on asks for small ones, and is sent more of
//! them — within [`crate::peer::MIN_PART`] and [`crate::peer::PART`].
//!
//! `Call` and `Reply` are no machine's either: a device's call of a
//! function its server's program registered by name (ark-peer's
//! `Config::calls`) and the server's answer, matched by the device's `id`.
//! A call is never an entry: its input is not logged, journaled, recorded
//! or told to any other connection, so it can carry what must stay off the
//! log (a password for the server alone). It is answered once, while the
//! connection that made it is up, and is not sent again. What a device says
//! of itself is shipped as one ([`EVENTS`]), which every server collects
//! and answers nothing.
//!
//! `Offer`, `Want`, `Chunk`, `Have` and `Absent` are no machine's either:
//! the bytes of the files rows name (`crate::file`, docs/plan-files.md),
//! which never enter the log. A device offers a file it made; a `Want`
//! asks for a file's bytes from `at`, answered by one `Chunk` of them (or
//! `Absent`: not for this connection, or not on the server yet); `Have`
//! says the server holds a file whole — an upload done, or one wanted
//! arrived.
//!
//! **No code crosses the wire.** An entry names its function by name; an
//! authority runs exactly the functions its program was built with, and
//! an intent naming one it does not run is `Held`: it stays pending on the
//! device, pushed again on its next connection. Nothing a client sends is ever loaded.
//!
//! Every frame names its space, so two machines share a connection (§1.9):
//! a verdict's id is `(origin, local)`'s, and each space counts its own.
//! `device` is what a device says it is, the bytes the authority authors
//! its `on_join` from (the driver's to read); `schema` is the hash of the
//! schema the peer runs. A frame
//! is its bytes in the value format, serde's derived form (`crate::format`;
//! `tests/frames.rs` holds every frame to
//! reading back as itself).

use serde::{Deserialize, Serialize};

use crate::format::Encode;
use crate::hash::{EntryHash, SchemaHash};
use crate::log::{Cursor, Entry, EntryId, Logged, Part, Seq};
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
        most: u64,
    },
    Page {
        space: Space,
        from: Seq,
        logged: Vec<Logged>,
    },
    Verdict {
        space: Space,
        id: EntryId,
        verdict: Verdict,
    },
    Part {
        space: Space,
        part: Part,
    },
    More {
        space: Space,
        from: Cursor,
        most: u64,
    },
    Call {
        id: u64,
        name: String,
        input: Vec<u8>,
    },
    Reply {
        id: u64,
        reply: Reply,
    },
    Offer {
        hash: String,
        size: u64,
    },
    Want {
        hash: String,
        at: u64,
    },
    Chunk {
        hash: String,
        at: u64,
        size: u64,
        bytes: Vec<u8>,
    },
    Have {
        hash: String,
    },
    Absent {
        hash: String,
        why: String,
    },
}

/// The call a device ships what it said of itself by (docs/plan-observe.md):
/// its input a page of [`Event`]s, which every server collects and answers
/// nothing — no machine's, and no app's to register.
pub const EVENTS: &str = "ark.events";

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

/// An authority's answer to one intent: judged — sequenced at `seq`, or
/// refused — or held: not judged here, and `last` the last `local` of its
/// origin that is. What its journal keeps is the refusals.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    Ok {
        seq: Seq,
    },
    Refused {
        why: String,
    },
    /// Not judged: past a `local` of its origin the authority has not
    /// judged, or naming a function it does not run. The device keeps it
    /// pending; one that no longer holds the intents between numbers its
    /// pending on from `last` (§1.7).
    Held {
        last: i64,
    },
}

/// A server's answer to one `Call`: what its function gave back, or why
/// it would not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reply {
    Ok { output: Vec<u8> },
    Refused { why: String },
}

impl Frame {
    /// The space whose log the frame is about.
    pub fn space(&self) -> Space {
        match self {
            Frame::Hello { space, .. }
            | Frame::Page { space, .. }
            | Frame::Verdict { space, .. }
            | Frame::Part { space, .. }
            | Frame::More { space, .. } => *space,
            // About no log: the log's space, whose machine passes it by.
            Frame::Call { .. }
            | Frame::Reply { .. }
            | Frame::Offer { .. }
            | Frame::Want { .. }
            | Frame::Chunk { .. }
            | Frame::Have { .. }
            | Frame::Absent { .. } => Space::Durable,
        }
    }

    /// Whether it is a file's (docs/plan-files.md): no machine's.
    pub fn is_file(&self) -> bool {
        matches!(
            self,
            Frame::Offer { .. } | Frame::Want { .. } | Frame::Chunk { .. } | Frame::Have { .. } | Frame::Absent { .. }
        )
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
