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
        Frame::Part { part, .. } => {
            let rows: usize = part.ranges.iter().map(|r| r.rows.len()).sum();
            let first = part.first;
            debug!(target: "ark", peer, space = space(sp), dir, conn, seq = part.seq, rows, first, last = part.next.is_none(), "part")
        }
        Frame::More { from, .. } => debug!(target: "ark", peer, space = space(sp), dir, conn, table = %from.table, "more"),
        Frame::Parts { most, .. } => debug!(target: "ark", peer, space = space(sp), dir, conn, most, "parts"),
        Frame::Verdict { id: e, verdict, .. } => verdict_of(peer, sp, dir, conn, &e, &verdict),
        Frame::Telemetry { events } => debug!(target: "ark", peer, dir, conn, events = events.len(), "telemetry"),
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
pub fn step(peer: &str, sp: Space, from: Option<ConnId>, input: &Input, out: &[Output]) {
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

// * Collecting
//
// One `tracing` layer turns what is said — ark's events and any other
// crate's at `info` or louder, a job's own code among them — into
// [`Event`]s, each with the fields of the spans around it (a `job` span's
// peer and span), and hands them to the process's one [`Sink`]: memory in
// a test, files on a server, the outbox on a device, whose peers send
// their own to their server ([`Peer::ship`](crate::Peer)).

pub use ark::frames::Event;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id};
use tracing::{Metadata, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;

/// Where a process's events go.
pub enum Sink {
    /// Kept, for a test to read.
    Memory(Arc<Mutex<Vec<Event>>>),
    /// A server's: lines of JSON in a directory, [`FILES`] files of
    /// [`FILE_BYTES`] at most, the oldest dropped.
    #[cfg(not(target_arch = "wasm32"))]
    Files(Mutex<Files>),
    /// A device's: kept for its peer to send, [`OUTBOX`] at most.
    Outbox,
}

pub const FILES: usize = 8;
pub const FILE_BYTES: u64 = 8 << 20;
pub const OUTBOX: usize = 4096;

static SINK: OnceLock<Sink> = OnceLock::new();
static OUT: Mutex<VecDeque<Event>> = Mutex::new(VecDeque::new());

/// `sink` made the process's, its layer the subscriber: whether it was (a
/// process has one; the first wins).
pub fn install(sink: Sink) -> bool {
    if SINK.set(sink).is_err() {
        return false;
    }
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(Layer)).is_ok()
}

/// A test's: the events of every peer in this process, kept.
pub fn memory() -> Arc<Mutex<Vec<Event>>> {
    let kept: Arc<Mutex<Vec<Event>>> = Arc::default();
    install(Sink::Memory(kept.clone()));
    match SINK.get() {
        Some(Sink::Memory(k)) => k.clone(),
        _ => kept,
    }
}

/// Events said elsewhere — a device's, sent — kept with this process's.
pub fn collect(events: Vec<Event>) {
    let Some(sink) = SINK.get() else { return };
    for e in events {
        put(sink, e);
    }
}

/// Every event in the outbox, taken.
pub fn take_all() -> Vec<Event> {
    OUT.lock().unwrap_or_else(|p| p.into_inner()).drain(..).collect()
}

/// The outbox's events of `peer`, up to `n`, taken.
pub(crate) fn take(peer: &str, n: usize) -> Vec<Event> {
    let mut out = OUT.lock().unwrap_or_else(|p| p.into_inner());
    let (mine, rest): (Vec<Event>, Vec<Event>) = out.drain(..).partition(|e| e.peer == peer);
    out.extend(rest);
    let mut mine: VecDeque<Event> = mine.into();
    let taken: Vec<Event> = mine.drain(..n.min(mine.len())).collect();
    for e in mine.into_iter().rev() {
        out.push_front(e);
    }
    taken
}

fn put(sink: &Sink, e: Event) {
    match sink {
        Sink::Memory(k) => k.lock().unwrap_or_else(|p| p.into_inner()).push(e),
        #[cfg(not(target_arch = "wasm32"))]
        Sink::Files(f) => f.lock().unwrap_or_else(|p| p.into_inner()).write(&e),
        Sink::Outbox => {
            let mut out = OUT.lock().unwrap_or_else(|p| p.into_inner());
            if out.len() == OUTBOX {
                out.pop_front();
            }
            out.push_back(e);
        }
    }
}

/// The JSON-lines files a server keeps its system's events in:
/// `events-<n>.jsonl`, numbered up, the newest written.
#[cfg(not(target_arch = "wasm32"))]
pub struct Files {
    dir: std::path::PathBuf,
    n: u64,
    file: Option<std::fs::File>,
    bytes: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl Files {
    pub fn new(dir: impl Into<std::path::PathBuf>) -> std::io::Result<Files> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        let n = numbered(&dir).last().map_or(0, |(n, _)| n + 1);
        Ok(Files {
            dir,
            n,
            file: None,
            bytes: 0,
        })
    }

    fn write(&mut self, e: &Event) {
        use std::io::Write;
        let mut line = serde_json::to_vec(e).unwrap_or_default();
        line.push(b'\n');
        if self.file.is_none() || self.bytes + line.len() as u64 > FILE_BYTES {
            let path = self.dir.join(format!("events-{:06}.jsonl", self.n));
            self.n += 1;
            self.bytes = 0;
            self.file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok();
            let all = numbered(&self.dir);
            for (_, old) in all.iter().take(all.len().saturating_sub(FILES)) {
                let _ = std::fs::remove_file(old);
            }
        }
        if let Some(f) = &mut self.file {
            if f.write_all(&line).is_ok() {
                self.bytes += line.len() as u64;
            }
        }
    }
}

/// The event files of `dir`, oldest first, each its number and path.
#[cfg(not(target_arch = "wasm32"))]
pub fn numbered(dir: &std::path::Path) -> Vec<(u64, std::path::PathBuf)> {
    let mut all: Vec<(u64, std::path::PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let n = name.strip_prefix("events-")?.strip_suffix(".jsonl")?.parse().ok()?;
            Some((n, e.path()))
        })
        .collect();
    all.sort();
    all
}

struct Layer;

/// A span's fields, kept on it for the events inside.
struct Fields(Vec<(String, String)>);

#[derive(Default)]
struct Seen {
    msg: String,
    fields: Vec<(String, String)>,
}

impl Visit for Seen {
    fn record_debug(&mut self, f: &Field, v: &dyn std::fmt::Debug) {
        let v = format!("{v:?}");
        self.record(f, v);
    }
    fn record_str(&mut self, f: &Field, v: &str) {
        self.record(f, v.to_string());
    }
}

impl Seen {
    fn record(&mut self, f: &Field, v: String) {
        match f.name() {
            "message" => self.msg = v,
            name => self.fields.push((name.to_string(), v)),
        }
    }
}

fn wanted(m: &Metadata) -> bool {
    match m.target() == "ark" || m.target().starts_with("ark::") {
        true => *m.level() <= Level::DEBUG,
        false => *m.level() <= Level::INFO,
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> tracing_subscriber::Layer<S> for Layer {
    fn enabled(&self, m: &Metadata<'_>, _: Context<'_, S>) -> bool {
        wanted(m)
    }

    fn on_new_span(&self, a: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut seen = Seen::default();
        a.record(&mut seen);
        seen.fields.insert(0, ("in".into(), a.metadata().name().to_string()));
        if let Some(s) = ctx.span(id) {
            s.extensions_mut().insert(Fields(seen.fields));
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        let Some(sink) = SINK.get() else { return };
        let mut seen = Seen::default();
        event.record(&mut seen);
        let mut fields = vec![];
        if let Some(scope) = ctx.event_scope(event) {
            for s in scope.from_root() {
                if let Some(Fields(f)) = s.extensions().get::<Fields>() {
                    fields.extend(f.iter().cloned());
                }
            }
        }
        fields.extend(seen.fields);
        let peer = fields.iter().rev().find(|(k, _)| k == "peer").map(|(_, v)| v.clone()).unwrap_or_default();
        fields.retain(|(k, _)| k != "peer");
        let m = event.metadata();
        if m.target() != "ark" {
            fields.insert(0, ("target".into(), m.target().to_string()));
        }
        put(
            sink,
            Event {
                at: crate::now_ms(),
                peer,
                level: m.level().as_str().to_lowercase(),
                msg: seen.msg,
                fields,
            },
        );
    }
}
