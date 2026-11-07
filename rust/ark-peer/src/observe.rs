//! What a peer says of what it does (docs/plan-observe.md): `tracing`, as
//! any code says it. The machine says what it does where it does it (an
//! entry authored, judged, held, confirmed; a fill done; a divergence —
//! `ark::peer`), inside the span its driver opens around every step, which
//! names the peer ([`crate::Core::label`]), the space and the connection;
//! the driver says what is its own (the link, a slow pump, a job's run).
//! Every entry's event carries its id, `trace` and `span` (an entry's span
//! is its id) and the span it was a child of (`parent`), so that one tap's
//! trace is read across every peer that saw it.

use tracing::{info, Level};

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
// peer and span), and hands them to the process's one sink: memory in a
// test ([`memory`]), files on a server, the outbox on a device, whose peers
// send their own to their server as they send their frames (`Peer::ship`).

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
    /// A server's: lines of JSON in this directory, eight files of 8 MiB
    /// at most, the oldest dropped.
    #[cfg(not(target_arch = "wasm32"))]
    Files(std::path::PathBuf),
    /// A device's: kept for its peer to send, the newest 4096.
    Outbox,
}

/// The sink a process has: a [`Sink`], or a test's memory.
enum Kept {
    Memory(Arc<Mutex<Vec<Event>>>),
    #[cfg(not(target_arch = "wasm32"))]
    Files(Mutex<Files>),
    Outbox,
}

const OUTBOX: usize = 4096;

static SINK: OnceLock<Kept> = OnceLock::new();
static OUT: Mutex<VecDeque<Event>> = Mutex::new(VecDeque::new());

/// `sink` made the process's, its layer the subscriber; refused when its
/// directory cannot be made, or the process has a sink already (the first
/// wins).
pub fn install(sink: Sink) -> std::io::Result<()> {
    let kept = match sink {
        #[cfg(not(target_arch = "wasm32"))]
        Sink::Files(dir) => Kept::Files(Mutex::new(Files::new(dir)?)),
        Sink::Outbox => Kept::Outbox,
    };
    keep(kept)
}

fn keep(kept: Kept) -> std::io::Result<()> {
    SINK.set(kept)
        .map_err(|_| std::io::Error::other("this process keeps its events already"))?;
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(Layer)).map_err(std::io::Error::other)
}

/// A test's: the events of every peer in this process, kept.
pub fn memory() -> Arc<Mutex<Vec<Event>>> {
    let kept: Arc<Mutex<Vec<Event>>> = Arc::default();
    let _ = keep(Kept::Memory(kept.clone()));
    match SINK.get() {
        Some(Kept::Memory(k)) => k.clone(),
        _ => kept,
    }
}

/// Events said elsewhere — a device's, sent — kept with this process's.
pub(crate) fn collect(events: Vec<Event>) {
    let Some(sink) = SINK.get() else { return };
    for e in events {
        put(sink, e);
    }
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

fn put(sink: &Kept, e: Event) {
    match sink {
        Kept::Memory(k) => k.lock().unwrap_or_else(|p| p.into_inner()).push(e),
        #[cfg(not(target_arch = "wasm32"))]
        Kept::Files(f) => f.lock().unwrap_or_else(|p| p.into_inner()).write(&e),
        Kept::Outbox => {
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
struct Files {
    dir: std::path::PathBuf,
    n: u64,
    file: Option<std::fs::File>,
    bytes: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl Files {
    const KEPT: usize = 8;
    const BYTES: u64 = 8 << 20;

    fn new(dir: std::path::PathBuf) -> std::io::Result<Files> {
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
        if self.file.is_none() || self.bytes + line.len() as u64 > Self::BYTES {
            let path = self.dir.join(format!("events-{:06}.jsonl", self.n));
            self.n += 1;
            self.bytes = 0;
            self.file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok();
            let all = numbered(&self.dir);
            for (_, old) in all.iter().take(all.len().saturating_sub(Self::KEPT)) {
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
fn numbered(dir: &std::path::Path) -> Vec<(u64, std::path::PathBuf)> {
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
        let m = event.metadata();
        // A server's own say, on its standard error too, for its service's
        // journal: its app's at `info` or louder, ark's at `warn`.
        #[cfg(not(target_arch = "wasm32"))]
        if matches!(sink, Kept::Files(_)) && (m.target() != "ark" || *m.level() <= Level::WARN) {
            let said: String = seen.fields.iter().map(|(k, v)| format!(" {k}={v}")).collect();
            eprintln!("{}: {}{said}", m.target(), seen.msg);
        }
        fields.extend(seen.fields);
        let peer = fields.iter().rev().find(|(k, _)| k == "peer").map(|(_, v)| v.clone()).unwrap_or_default();
        fields.retain(|(k, _)| k != "peer");
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
