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
//! The variants are numbered in that order, `Join` last. The encoders
//! below are written by hand and [`ty`] is each part's type, written once
//! here; the derive (§1.4) replaces both, and `tests/frames.rs` holds every
//! frame's bytes to [`frame_ty`] until it does.

use crate::codec::{put_bytes, put_int, put_list, put_option, put_variant, Reader};
use crate::hash::{EntryHash, FnHash};
use crate::log::{Entry, EntryId, Logged, Seq, Snapshot};
use crate::schema::Space;
use crate::store::Change;
use crate::ty::Ty;
use crate::Refusal;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    Hello { space: Space, cursor: Seq, hash: Option<EntryHash>, device: Option<String> },
    Push { space: Space, entry: Entry, body: Option<Vec<u8>> },
    Fetch { hashes: Vec<FnHash> },
    Ack { id: EntryId, seq: Seq, facts: Option<Vec<Change>> },
    Reject { id: EntryId, why: String },
    Held { id: EntryId, func: FnHash },
    Batch { space: Space, from: Seq, logged: Vec<Logged> },
    Snapshot { space: Space, snapshot: Snapshot },
    Bodies { bodies: Vec<(FnHash, Vec<u8>)> },
    Join { space: Space, from: Seq, hash: EntryHash, logged: Vec<Logged> },
}

/// The types, by name: what the derive will write from the definitions.
pub fn ty(name: &str) -> Ty {
    let b = || Ty::Bytes;
    match name {
        "Space" => Ty::enumeration(2),
        "Entry" => Ty::Tuple(vec![b(), b(), b(), Ty::Int, Ty::list(b()), b(), b(), Ty::Int]),
        "Change" => Ty::Sum(vec![vec![b(), b()], vec![b(), b()], vec![b(), b(), b()]]),
        "Logged" => Ty::Tuple(vec![Ty::Int, ty("Entry"), Ty::list(ty("Change")), b(), b()]),
        "Snapshot" => Ty::Tuple(vec![Ty::Int, b(), b(), Ty::list(Ty::Tuple(vec![b(), Ty::list(b())]))]),
        "Record" => Ty::Sum(vec![vec![ty("Logged")], vec![ty("Snapshot")]]),
        "Frame" => Ty::Sum(vec![
            vec![ty("Space"), Ty::Int, Ty::option(b()), Ty::option(b())],
            vec![ty("Space"), ty("Entry"), Ty::option(b())],
            vec![Ty::list(b())],
            vec![b(), Ty::Int, Ty::option(Ty::list(ty("Change")))],
            vec![b(), b()],
            vec![b(), b()],
            vec![ty("Space"), Ty::Int, Ty::list(ty("Logged"))],
            vec![ty("Space"), ty("Snapshot")],
            vec![Ty::list(Ty::Tuple(vec![b(), b()]))],
            vec![ty("Space"), Ty::Int, b(), Ty::list(ty("Logged"))],
        ]),
        _ => panic!("no frame type {name}"),
    }
}

pub fn frame_ty() -> Ty {
    ty("Frame")
}

fn space(r: &mut Reader) -> Result<Space, Refusal> {
    Ok([Space::Durable, Space::Volatile][r.variant(2)? as usize])
}

fn logged(out: &mut Vec<u8>, ls: &[Logged]) {
    put_list(out, ls, |o, l| l.enc(o));
}

impl Frame {
    pub fn enc(&self) -> Vec<u8> {
        let mut o = vec![];
        let out = &mut o;
        match self {
            Frame::Hello { space, cursor, hash, device } => {
                put_variant(out, 0);
                put_variant(out, *space as u8);
                put_int(out, *cursor);
                put_option(out, hash.as_ref(), |o, h| put_bytes(o, &h.0));
                put_option(out, device.as_ref(), |o, d| put_bytes(o, d.as_bytes()));
            }
            Frame::Push { space, entry, body } => {
                put_variant(out, 1);
                put_variant(out, *space as u8);
                entry.enc(out);
                put_option(out, body.as_ref(), |o, b| put_bytes(o, b));
            }
            Frame::Fetch { hashes } => {
                put_variant(out, 2);
                put_list(out, hashes, |o, h| put_bytes(o, &h.0));
            }
            Frame::Ack { id, seq, facts } => {
                put_variant(out, 3);
                put_bytes(out, &id.0);
                put_int(out, *seq);
                put_option(out, facts.as_ref(), |o, fs| put_list(o, fs, |o, c| c.enc(o)));
            }
            Frame::Reject { id, why } => {
                put_variant(out, 4);
                put_bytes(out, &id.0);
                put_bytes(out, why.as_bytes());
            }
            Frame::Held { id, func } => {
                put_variant(out, 5);
                put_bytes(out, &id.0);
                put_bytes(out, &func.0);
            }
            Frame::Batch { space, from, logged: ls } => {
                put_variant(out, 6);
                put_variant(out, *space as u8);
                put_int(out, *from);
                logged(out, ls);
            }
            Frame::Snapshot { space, snapshot } => {
                put_variant(out, 7);
                put_variant(out, *space as u8);
                snapshot.enc(out);
            }
            Frame::Bodies { bodies } => {
                put_variant(out, 8);
                put_list(out, bodies, |o, (h, b)| {
                    put_bytes(o, &h.0);
                    put_bytes(o, b);
                });
            }
            Frame::Join { space, from, hash, logged: ls } => {
                put_variant(out, 9);
                put_variant(out, *space as u8);
                put_int(out, *from);
                put_bytes(out, &hash.0);
                logged(out, ls);
            }
        }
        o
    }

    /// A frame from canonical bytes, whole, or why not.
    pub fn dec(b: &[u8]) -> Result<Frame, Refusal> {
        let r = &mut Reader::new(b);
        let f = match r.variant(10)? {
            0 => Frame::Hello {
                space: space(r)?,
                cursor: r.int()?,
                hash: r.option(|r| r.fixed().map(EntryHash))?,
                device: r.option(|r| r.text())?,
            },
            1 => Frame::Push {
                space: space(r)?,
                entry: Entry::dec(r)?,
                body: r.option(|r| r.bytes())?,
            },
            2 => Frame::Fetch {
                hashes: r.list(|r| r.fixed().map(FnHash))?,
            },
            3 => Frame::Ack {
                id: EntryId(r.fixed()?),
                seq: r.int()?,
                facts: r.option(|r| r.list(Change::dec))?,
            },
            4 => Frame::Reject {
                id: EntryId(r.fixed()?),
                why: r.text()?,
            },
            5 => Frame::Held {
                id: EntryId(r.fixed()?),
                func: FnHash(r.fixed()?),
            },
            6 => Frame::Batch {
                space: space(r)?,
                from: r.int()?,
                logged: r.list(Logged::dec)?,
            },
            7 => Frame::Snapshot {
                space: space(r)?,
                snapshot: Snapshot::dec(r)?,
            },
            8 => Frame::Bodies {
                bodies: r.list(|r| Ok((FnHash(r.fixed()?), r.bytes()?)))?,
            },
            _ => Frame::Join {
                space: space(r)?,
                from: r.int()?,
                hash: EntryHash(r.fixed()?),
                logged: r.list(Logged::dec)?,
            },
        };
        r.end()?;
        Ok(f)
    }
}
