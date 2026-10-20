//! What a host hands the explorer, and what it is asked to do: the
//! [`Writer`] that turns an edit into an entry, the [`LogView`] that says
//! what the log holds, and the [`Source`] that is everything read. No
//! window is needed for any of it, so a server can speak these without
//! linking iced.

use std::collections::BTreeMap;

use ark5::author::Node;
use ark5::hash::FnHash;
use ark5::ir::{Effect, Expr};
use ark5::log::{Entry, Seq};
use ark5::module::{Lib, Module};
use ark5::peer::Ctx;
use ark5::schema::{Schema, Space};
use ark5::store::{Change, Store};

/// `docs/plan-guards.md` D4 Which of a table's four CRUD mutations a module
/// exposes — `insert_<t>`, `update_<t>`, `delete_<t>`, `put_<t>`, bound
/// under any router — and whether this host may author them at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Node)]
pub struct CrudVerbs {
    pub insert: bool,
    pub update: bool,
    pub delete: bool,
    pub put: bool,
    /// This host authors them: a client as the signed-in person, a server
    /// as its own identity. `false` and they are shown and never called.
    pub may_author: bool,
}

impl CrudVerbs {
    /// Whether any of the four is exposed.
    pub fn any(&self) -> bool {
        self.insert || self.update || self.delete || self.put
    }

    /// The verbs exposed, as `<verb>_<table>`.
    pub fn names(&self, table: &str) -> Vec<String> {
        [
            ("insert", self.insert),
            ("update", self.update),
            ("delete", self.delete),
            ("put", self.put),
        ]
        .into_iter()
        .filter(|(_, on)| *on)
        .map(|(v, _)| format!("{v}_{table}"))
        .collect()
    }
}

/// What the explorer writes through, supplied by its host
/// (`docs/plan-guards.md` D4). A cell edit or a row delete goes through an
/// exposed CRUD mutation when the table has one and the host may author it
/// ([`Writer::author`]) — so the domain's logic applies to the dashboard
/// as it does to any button — and through [`Writer::raw`] otherwise, or
/// when the person flips the switch to write raw. A client's host has no
/// raw writer at all: it says so from [`Writer::raw`] and from
/// [`Writer::can_raw`], and the switch is not drawn.
pub trait Writer {
    /// Which CRUD mutations the module exposes, per table, and whether this
    /// host may author them.
    fn exposed(&self) -> Vec<(String, CrudVerbs)>;

    /// Author the function bound as `function` with `input` (its input's
    /// bytes) as this host's identity: the signed-in person on a client,
    /// the authority on a server. `Ok` once it is authored, with a
    /// [`Ticket`] to ask [`Writer::verdict`] about; `Err` with the
    /// refusal's text when it is refused here and now.
    fn author(&mut self, function: &str, input: Vec<u8>) -> Result<Ticket, String>;

    /// What has become of an authored call, as this host knows it now.
    fn verdict(&self, _ticket: &Ticket) -> Verdict {
        Verdict::Unknown
    }

    /// The authority's raw write: a client host answers `Err("not the
    /// authority")`.
    fn raw(&mut self, change: Change) -> Result<(), String>;

    /// Whether [`Writer::raw`] can succeed here: the switch to write raw is
    /// drawn only where it can.
    fn can_raw(&self) -> bool {
        false
    }

    /// Who this host writes as, as a line for the header.
    fn who(&self) -> String {
        String::new()
    }
}

/// What a host hands back for a call it authored, to be asked about later
/// ([`Writer::verdict`]): an intent's id on a client, a request's place on
/// the admin page. Opaque to the explorer, which only keeps it.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ticket(pub String);

/// What has become of a mutation the explorer authored: written here and
/// waiting for the authority, in the log (at a sequence, where the host
/// knows it), refused with the refusal's text, or nothing the host can say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pending,
    Logged(Option<Seq>),
    Refused(String),
    Unknown,
}

impl Verdict {
    /// As a person would say it.
    pub fn describe(&self) -> String {
        match self {
            Verdict::Pending => "accepted, pending".into(),
            Verdict::Logged(Some(n)) => format!("in the log, at {n}"),
            Verdict::Logged(None) => "in the log".into(),
            Verdict::Refused(why) => format!("refused: {why}"),
            Verdict::Unknown => "authored".into(),
        }
    }
}

/// One line of the log, as a host shows it: where it is (none while it is
/// pending), the entry — who pushed it is its actor and its login — the
/// function's name, its facts where they are known, and where it stands.
#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub struct Line {
    pub seq: Option<Seq>,
    pub entry: Entry,
    pub function: String,
    pub facts: Option<Vec<Change>>,
    pub standing: String,
}

/// One connection, or this device: who, where in the log, and how many of
/// its intents are pending where that is known.
#[derive(Clone, Debug, PartialEq, Eq, Node)]
pub struct Connection {
    pub who: String,
    pub cursor: Seq,
    pub pending: Option<usize>,
    pub note: String,
}

/// What a host says of its log. A server has the log; a client has the
/// chain it has confirmed and its pending intents.
pub trait LogView {
    /// The last sequence this host knows of.
    fn head(&self) -> Seq;
    /// The lines, oldest first, as many as the host keeps.
    fn lines(&self) -> Vec<Line>;
    fn connections(&self) -> Vec<Connection>;
}

/// Everything the explorer reads, handed to it on every call: it keeps no
/// copy of any of it. `store` is what the host holds — the authority's
/// whole store, a partial replica's union — `module` and `lib` name and
/// run the functions, and `ctx` is who a query reads as.
pub struct Source<'a> {
    pub store: &'a Store,
    pub module: &'a Module,
    pub lib: &'a Lib,
    pub log: &'a dyn LogView,
    pub ctx: Ctx,
}

impl Source<'_> {
    pub fn schema(&self) -> &Schema {
        &self.module.schema
    }
}

/// The CRUD a module exposes, per table, as its bindings say it: a
/// mutation bound as `insert_<t>`, `update_<t>` or `put_<t>` whose input is
/// `t`'s row, or `delete_<t>` whose input is its key's columns. `may_author`
/// is false: the host decides that.
pub fn crud_of(module: &Module, lib: &Lib) -> Vec<(String, CrudVerbs)> {
    let cx = crate::value::types(module);
    let mut out = Vec::new();
    for t in &module.schema.tables {
        let said = |cs: &[ark5::schema::Column]| cs.iter().map(|c| c.decl.clone()).collect::<Vec<_>>();
        let takes = |verb: &str, want: Vec<String>| {
            module
                .bindings
                .iter()
                .filter(|(n, _)| n.rsplit('.').next() == Some(&format!("{verb}_{}", t.name)))
                .filter_map(|(_, h)| lib.get(h))
                .any(|f| {
                    let input: Vec<String> = crate::value::fields_of(&cx, &f.f.params).0.iter().map(crate::value::decl_name).collect();
                    f.f.effect == Effect::Write && input == want
                })
        };
        let verbs = CrudVerbs {
            insert: takes("insert", said(&t.columns)),
            update: takes("update", said(&t.columns)),
            delete: takes("delete", said(&t.columns[..t.key])),
            put: takes("put", said(&t.columns)),
            may_author: false,
        };
        if verbs.any() {
            out.push((t.name.clone(), verbs));
        }
    }
    out
}

/// Every function a log may name, by hash: the module's bindings.
pub fn function_names(module: &Module) -> BTreeMap<FnHash, String> {
    module.bindings.iter().map(|(n, h)| (*h, n.clone())).collect()
}

/// Whether a function reads or writes a volatile table: a live transition
/// or a read of the live state (§1.9), which the log's machine does not run.
pub fn is_live(module: &Module, lib: &Lib, h: &FnHash) -> bool {
    let volatile = |t: &str| module.schema.table(t).is_some_and(|(_, t)| t.space == Space::Volatile);
    let mut todo: Vec<FnHash> = vec![*h];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(h) = todo.pop() {
        if !seen.insert(h) {
            continue;
        }
        let Some(f) = lib.get(&h) else { continue };
        let mut hit = false;
        ark5::interp::walk(&f.f.body, None, &mut |e, _| match e {
            Expr::Scan(t) | Expr::Lookup(t, _) | Expr::Insert(t, _) | Expr::Update(t, ..) | Expr::Delete(t, _) => hit |= volatile(t),
            Expr::Call(c, _) => todo.push(*c),
            _ => {}
        });
        if hit {
            return true;
        }
    }
    false
}

/// A log view of nothing: a host with no log to show.
pub struct NoLog;

impl LogView for NoLog {
    fn head(&self) -> Seq {
        0
    }
    fn lines(&self) -> Vec<Line> {
        vec![]
    }
    fn connections(&self) -> Vec<Connection> {
        vec![]
    }
}
