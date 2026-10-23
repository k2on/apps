//! What a peer says of what it does (docs/plan-observe.md): `tracing`
//! events, target `ark`, at the one door every machine input and output
//! passes ([`Peer::route`](crate::Peer)) — each intent, each frame and
//! the entries in it, each verdict, each change told and whose it was.
//! Nothing here decides anything; with no subscriber listening, nothing is
//! decoded.
//!
//! Every event names the peer (`peer`, [`crate::Peer::label`]) and its
//! space, and an entry's events its id, `trace` and `span` (an entry's span
//! is its id) and the span it was a child of (`parent`), so that one tap's
//! trace is read across every peer that saw it.

use ark::frames::{Frame, Verdict};
use ark::hash::hex;
use ark::log::{Entry, EntryId, Span};
use ark::peer::{Input, Output};
use ark::schema::Space;
use tracing::{debug, event_enabled, info, warn, Level};

use crate::ConnId;

fn space(s: Space) -> &'static str {
    ["durable", "volatile"][s as usize]
}

fn id(e: &EntryId) -> String {
    hex(&e.0)
}

fn span(s: &Option<Span>) -> (String, String) {
    s.map_or((String::new(), String::new()), |s| (hex(&s.trace.0), hex(&s.id)))
}

/// An entry, as one event: where it was going and what it is.
fn entry(peer: &str, sp: Space, dir: &str, conn: Option<ConnId>, kind: &str, seq: Option<i64>, e: &Entry) {
    let own = e.span();
    let (_, parent) = span(&e.cause);
    debug!(
        target: "ark",
        peer,
        space = space(sp),
        dir,
        conn,
        kind,
        seq,
        entry = %id(&e.id),
        func = %e.func,
        actor = %e.actor,
        session = %e.session,
        local = e.local,
        trace = %hex(&own.trace.0),
        span = %hex(&own.id),
        parent = %parent,
        "entry"
    );
}

fn frame(peer: &str, sp: Space, dir: &str, conn: Option<ConnId>, bytes: &[u8]) {
    let Ok(f) = Frame::dec(bytes) else {
        return warn!(target: "ark", peer, space = space(sp), dir, conn, "a frame that does not read");
    };
    match f {
        Frame::Hello { cursor, device, version, .. } => {
            let device = device.map(|d| hex(&d)).unwrap_or_default();
            debug!(target: "ark", peer, space = space(sp), dir, conn, cursor, device = %device, version = %version, "hello")
        }
        Frame::Page { from, logged, .. } => {
            for l in &logged {
                // A push is a page of intents, sequenced nowhere yet.
                let (kind, seq) = if l.seq > 0 { ("logged", Some(l.seq)) } else { ("push", None) };
                entry(peer, sp, dir, conn, kind, seq, &l.entry);
            }
            if logged.is_empty() {
                debug!(target: "ark", peer, space = space(sp), dir, conn, from, "page");
            }
        }
        Frame::Snapshot { snapshot, .. } => {
            debug!(target: "ark", peer, space = space(sp), dir, conn, seq = snapshot.seq, "snapshot")
        }
        Frame::Verdict { id: e, verdict, .. } => verdict_of(peer, sp, dir, conn, &e, &verdict),
    }
}

fn verdict_of(peer: &str, sp: Space, dir: &str, conn: Option<ConnId>, e: &EntryId, v: &Verdict) {
    let e = id(e);
    match v {
        Verdict::Ok { seq } => debug!(target: "ark", peer, space = space(sp), dir, conn, entry = %e, seq, "ok"),
        Verdict::Held { func } => info!(target: "ark", peer, space = space(sp), dir, conn, entry = %e, func = %func, "held"),
        Verdict::Refused { why } => warn!(target: "ark", peer, space = space(sp), dir, conn, entry = %e, why = %why, "refused"),
    }
}

/// One step of a machine: its input, and what it said.
pub(crate) fn step(peer: &str, sp: Space, from: Option<ConnId>, input: &Input, out: &[Output]) {
    if !event_enabled!(target: "ark", Level::WARN) {
        return;
    }
    match input {
        Input::Frame { bytes, .. } => frame(peer, sp, "in", from, bytes),
        Input::Mutate { func, cause, .. } => {
            let (trace, parent) = span(cause);
            debug!(target: "ark", peer, space = space(sp), func = %func, trace = %trace, parent = %parent, "mutate")
        }
        Input::Connected => debug!(target: "ark", peer, space = space(sp), "connected"),
        Input::Alone => info!(target: "ark", peer, space = space(sp), "alone"),
        Input::Join => info!(target: "ark", peer, space = space(sp), "join"),
        Input::Write { facts, .. } => info!(target: "ark", peer, space = space(sp), facts = facts.len(), "write"),
        Input::Open(_) | Input::Tick(_) | Input::Persisted(_) | Input::Compact(_) | Input::Feed { .. } => {}
    }
    for o in out {
        match o {
            Output::Send(b) => frame(peer, sp, "out", from, b),
            Output::Fanout(b) => frame(peer, sp, "fanout", None, b),
            Output::To { session, bytes } => {
                debug!(target: "ark", peer, space = space(sp), to = %session, "to");
                frame(peer, sp, "to", None, bytes)
            }
            Output::Refused { id: e, why } => warn!(target: "ark", peer, space = space(sp), entry = %id(e), why = %why, "refused"),
            Output::Confirmed(e) => debug!(target: "ark", peer, space = space(sp), entry = %id(e), "confirmed"),
            Output::Changes(told) => {
                let mut by: Vec<Span> = told.iter().filter_map(|t| t.by).collect();
                by.dedup();
                for s in by {
                    let n = told.iter().filter(|t| t.by == Some(s)).count();
                    debug!(target: "ark", peer, space = space(sp), changes = n, trace = %hex(&s.trace.0), span = %hex(&s.id), "changed");
                }
                let whole = told.iter().filter(|t| t.by.is_none()).count();
                if whole > 0 {
                    debug!(target: "ark", peer, space = space(sp), changes = whole, "changed");
                }
            }
            Output::Persist(_) => {}
        }
    }
}

/// The link, as it comes and goes.
pub(crate) fn link(peer: &str, what: &str, why: Option<&str>) {
    match why {
        Some(why) => info!(target: "ark", peer, why = %why, "{what}"),
        None => info!(target: "ark", peer, "{what}"),
    }
}
