//! §1.11 The frames: one `Sum` type, encoded by §1.1. **The protocol is
//! pages**: a peer pushes pages of its intents; an authority answers with
//! verdicts and pages of its chain.
//!
//! ```text
//! Schema  { space, schema }
//! Hello   { space, cursor, hash, fill: Option<{ table, key }>, most, schema }
//! Push    { space, schema, from, intents: [Ctx] }
//! Page    { space, from, logged: [Logged] }
//! Verdict { space, id, verdict: At { seq, last } | Refused { why } }
//! Update  { space, why }
//! Part    { space, seq, hash, from: Option<{ table, key }>, rows: [(table, [row])], next }
//! Call    { id, name, input }
//! Reply   { id, reply: Ok { output } | Refused { why } }
//! Offer   { hash, size }
//! Want    { hash, at }
//! Chunk   { hash, at, size, bytes }
//! Have    { hash }
//! Absent  { hash, why }
//! ```
//!
//! An authority's `Page` is its log from `from`, sequenced. A peer's `Push`
//! is its intents not yet sequenced — what it did with no server is pushed
//! so too (§1.7) — and its `from` the `local` its pending intents begin
//! after: what its origin had up to there, it no longer holds. A `Verdict`
//! answers one intent: refused, or where its origin stands — the last
//! `local` of it the authority judged, and its head: an intent at or below
//! that last is in the log at or below the head; one past it was not
//! judged (past a gap, or held: see below).
//!
//! **A build says its schema** (§1.2): a replica's `Schema` whole, at each
//! connection before its `Hello`, and its hash in each `Hello` and `Push`.
//! An authority serves a replica whose schema its own grows from (an older
//! build's) or into (a newer one's) — their rows and facts read under each
//! one's own ([`crate::schema::Table::fit`]) — and answers one whose schema
//! is neither once, with `Update`, instead of filling it: it is to update,
//! and asks nothing more until it does; its pushes are held. An intent
//! naming a function the authority does not run is held from a newer build
//! (its server is to update), and refused with [`UPDATE`] from any other
//! (the function is retired), so its device's queue moves on.
//!
//! A `Part` is a part of a fill (§1.7): the authority's answer to a
//! `Hello` its log cannot page — the first, no rows, where they stand (no
//! `from`) — and, after the pages, to one that says where its fill stands
//! (`fill`): the part from there, of one table's rows as they stand when
//! the authority answers, to where the next begins (none: the table done).
//! A replica asks for each part once it holds the one before, the tables
//! its views read first. A
//! `Hello` says how big a replica would have the frames it is sent — a
//! part, `most` bytes of rows at most, or a page, `most` bytes of entries —
//! one that takes them on the thread it draws on asks for small ones, and
//! is sent more of them — up to [`crate::peer::PART`].
//!
//! `Call` and `Reply` are no machine's either: a device's call of a
//! function its server's program registered by name (ark-peer's
//! `Config::calls`) and the server's answer, matched by the device's `id`.
//! A call is never an entry: its input is not logged, journaled, recorded
//! or told to any other connection, so it can carry what must stay off the
//! log (a password for the server alone). It is answered once, while the
//! connection that made it is up, and is not sent again. What a device says
//! of itself is shipped as one ([`EVENTS`]), which every server collects
//! and answers nothing; and what it is, as one at each connection before
//! its `Hello`s ([`DEVICE`]): the bytes its server authors its `on_join`
//! from, answered nothing.
//!
//! `Offer`, `Want`, `Chunk`, `Have` and `Absent` are no machine's either:
//! the bytes of the files rows name (`crate::file`, docs/plan-files.md),
//! which never enter the log. A device offers a file it made; a `Want`
//! asks for a file's bytes from `at`, answered by one `Chunk` of them (or
//! `Absent`: not for this connection, or not on the server yet), and a
//! taker keeps several asked ahead of what it holds; `Have`
//! says the server holds a file whole — an upload done, or one wanted
//! arrived.
//!
//! **No code crosses the wire.** An entry names its function by name; an
//! authority runs exactly the functions its program was built with, and
//! an intent naming one it does not run is held or refused, as above.
//! Nothing a client sends is ever loaded.
//!
//! Every frame names its space, so two machines share a connection (§1.9):
//! a verdict's id is `(origin, local)`'s, and each space counts its own.
//! A frame
//! is its bytes in the value format, serde's derived form (`crate::format`;
//! `tests/frames.rs` holds every frame to
//! reading back as itself).

use serde::{Deserialize, Serialize};

use crate::format::Encode;
use crate::hash::{EntryHash, SchemaHash};
use crate::log::{Ctx, Cursor, EntryId, Logged, Rows, Seq};
use crate::schema::{Schema, Space};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Frame {
    Hello {
        space: Space,
        cursor: Seq,
        hash: EntryHash,
        /// Where its fill stands, filling: the part from there is asked.
        fill: Option<Cursor>,
        most: u64,
        schema: SchemaHash,
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
    /// Rows as they stood at `(seq, hash)`, by table, from `from` (none:
    /// the first part, from nothing) to `next` (none: the last).
    Part {
        space: Space,
        seq: Seq,
        hash: EntryHash,
        from: Option<Cursor>,
        #[serde(with = "crate::format::raw_tables")]
        rows: Rows,
        next: Option<Cursor>,
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
    Push {
        space: Space,
        schema: SchemaHash,
        from: i64,
        intents: Vec<Ctx>,
    },
    Schema {
        space: Space,
        schema: Schema,
    },
    Update {
        space: Space,
        why: String,
    },
}

/// The call a device ships what it said of itself by (docs/plan-observe.md):
/// its input a page of [`Event`]s, which every server collects and answers
/// nothing — no machine's, and no app's to register.
pub const EVENTS: &str = "ark.events";

/// The call a device says what it is by, at each connection before its
/// `Hello`s (§1.9): its input the bytes its server's live space authors
/// `on_join` from — the driver's to read, no machine's — answered nothing.
pub const DEVICE: &str = "ark.device";

/// What an intent refused because its build is older than its server's
/// begins with, and what an authority that cannot serve a build says: the
/// device is to update.
pub const UPDATE: &str = "update needed";

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

/// An authority's answer to one intent: refused, or where its origin
/// stands — `last`, the last `local` of it judged, with the log at `seq`.
/// One at or below `last` is in the log, at or below `seq`; one past it is
/// not judged, and stays pending on the device (§1.7). What its journal
/// keeps is the refusals.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    At { seq: Seq, last: i64 },
    Refused { why: String },
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
            | Frame::Push { space, .. }
            | Frame::Schema { space, .. }
            | Frame::Update { space, .. }
            | Frame::Page { space, .. }
            | Frame::Verdict { space, .. }
            | Frame::Part { space, .. } => *space,
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

    /// A peer's intents, not yet sequenced, pushed by a build of
    /// `schema`: its pending beginning after `from`.
    pub fn push(space: Space, schema: SchemaHash, from: i64, intents: impl IntoIterator<Item = Ctx>) -> Frame {
        let intents = intents.into_iter().collect();
        Frame::Push {
            space,
            schema,
            from,
            intents,
        }
    }

    pub fn enc(&self) -> Vec<u8> {
        self.to_bytes()
    }

    /// A frame from canonical bytes, whole, or why not.
    pub fn dec(b: &[u8]) -> Result<Frame, String> {
        Frame::from_bytes(b)
    }
}
