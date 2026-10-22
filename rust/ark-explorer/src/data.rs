//! What a host hands the explorer, and what it is asked to do: the
//! [`Writer`] that turns an edit into an entry, the [`LogView`] that says
//! what the log holds, and the [`Source`] that is everything read. No
//! window is needed for any of it, so a server can speak these without
//! linking iced.

use ark::domain::{Domain, Kind};
use ark::log::{Entry, Seq};
use ark::peer::Ctx;
use ark::row::Type;
use ark::schema::Schema;
use ark::store::{Change, Store};
use serde::{Deserialize, Serialize};

/// `docs/plan-guards.md` D4 Which of a table's four CRUD mutations a module
/// exposes — `insert_<t>`, `update_<t>`, `delete_<t>`, `put_<t>`, bound
/// under any router — and whether this host may author them at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub seq: Option<Seq>,
    pub entry: Entry,
    pub function: String,
    pub facts: Option<Vec<Change>>,
    pub standing: String,
}

/// One connection, or this device: who, where in the log, and how many of
/// its intents are pending where that is known.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

pub use ark::domain::{Catalog, Described};

/// What answers a query and checks an input for the explorer: the domain,
/// where the host holds it.
pub trait Ask {
    /// The function's answer over `store` as `ctx` reads it: its encoding.
    fn ask(&self, store: &Store, ctx: &Ctx, name: &str, input: &[u8]) -> Result<Vec<u8>, String>;
    /// The input's refinements: every field that fails, and why.
    fn check(&self, name: &str, input: &[u8]) -> Result<(), Vec<(String, String)>>;
}

impl Ask for Domain {
    fn ask(&self, store: &Store, ctx: &Ctx, name: &str, input: &[u8]) -> Result<Vec<u8>, String> {
        Domain::ask(self, store, ctx, name, input).map_err(|r| r.0)
    }
    fn check(&self, name: &str, input: &[u8]) -> Result<(), Vec<(String, String)>> {
        match self.get(name) {
            Some(f) => (f.check)(input),
            None => Err(vec![(String::new(), format!("no function {name}"))]),
        }
    }
}

/// Everything the explorer reads, handed to it on every call: it keeps no
/// copy of any of it. `store` is what the host holds — the authority's
/// whole store, a partial replica's union — `catalog` names and describes
/// the functions, `ask` answers and checks them where the host holds the
/// domain, and `ctx` is who a query reads as.
pub struct Source<'a> {
    pub store: &'a Store,
    pub catalog: &'a Catalog,
    pub ask: Option<&'a dyn Ask>,
    pub log: &'a dyn LogView,
    pub ctx: Ctx,
}

impl Source<'_> {
    pub fn schema(&self) -> &Schema {
        &self.catalog.schema
    }
}

/// The fields of a record the input of a function is: their names and
/// declared types; none for an input that is not one.
pub fn record_fields(t: &Type) -> Option<&[(String, Type)]> {
    match t {
        Type::Record(_, fs) => Some(fs),
        _ => None,
    }
}

/// The CRUD a domain exposes, per table, as its names say it: a mutation
/// named `insert_<t>`, `update_<t>` or `put_<t>` whose input's fields are
/// `t`'s columns, or `delete_<t>` whose input's are its key's. `may_author`
/// is false: the host decides that.
pub fn crud_of(catalog: &Catalog) -> Vec<(String, CrudVerbs)> {
    let mut out = Vec::new();
    for t in &catalog.schema.tables {
        let said = |cs: &[ark::schema::Column]| cs.iter().map(|c| c.ty.decl()).collect::<Vec<_>>();
        let takes = |verb: &str, want: Vec<String>| {
            catalog
                .functions
                .iter()
                .filter(|f| f.kind == Kind::Mutator && f.name.rsplit('.').next() == Some(&format!("{verb}_{}", t.name)))
                .any(|f| record_fields(&f.input).is_some_and(|fs| fs.iter().map(|(_, t)| t.decl()).collect::<Vec<_>>() == want))
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

/// Whether a function is a live transition (§1.9), which the log's
/// machine does not run.
pub fn is_live(catalog: &Catalog, name: &str) -> bool {
    catalog.get(name).is_some_and(|f| f.kind == Kind::Live)
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
