//! What a host hands the explorer, and what it is asked to do: the
//! [`Writer`] that turns an edit into an entry, the [`LogView`] that says
//! what the log holds, and the [`Source`] that is everything read. No
//! window is needed for any of it, so a server can speak these without
//! linking iced.

use std::collections::BTreeMap;

use ark::eval::{Args, Ctx};
use ark::hash::FnHash;
use ark::ir::{FnKind, Module};
use ark::log::{Entry, Facts, Seq};
use ark::schema::Schema;
use ark::store::{Change, Store};
use ark::value::TableName;

/// `docs/plan-guards.md` D4 Which of a table's four CRUD mutations a module
/// exposes — `insert_<t>`, `update_<t>`, `delete_<t>`, `put_<t>`, generated
/// by `r.crud::<T>()` or written by hand under those names with the same
/// input — and whether this host may author them at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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

    /// The names exposed, as the module has them.
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
    fn exposed(&self) -> Vec<(TableName, CrudVerbs)>;

    /// Author a domain mutation as this host's identity: the signed-in
    /// person on a client, the authority on a server. `Ok` once it is
    /// authored — what becomes of it is the log's to say.
    fn author(&mut self, function: &str, args: Args) -> Result<(), String>;

    /// The authority's raw write ([`ark::raw`]): a client host answers
    /// `Err("not the authority")`.
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

/// One line of the log, as a host shows it: where it is (none while it is
/// pending), the entry — who pushed it is its actor and its login — the
/// function's name, its facts where they are known, and where it stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub seq: Option<Seq>,
    pub entry: Entry,
    pub function: String,
    pub facts: Option<Facts>,
    pub standing: String,
}

/// One connection, or this device: who, where in the log, and how many of
/// its intents are pending where that is known (a server cannot know a
/// device's pending queue; it knows how far behind its head the connection
/// has been sent, which is `note`'s to say).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    pub who: String,
    pub cursor: Seq,
    pub pending: Option<usize>,
    pub note: String,
}

/// A `Verify` asked and answered: by whom, at what sequence, and whether the
/// authority agreed — `None` where it said it cannot say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verified {
    pub who: String,
    pub seq: Seq,
    pub answer: Option<bool>,
}

/// What a host says of its log. A server has the log; a client keeps none —
/// its pending intents, what was refused, its cursor and its `Verify`
/// answers are what it can show.
pub trait LogView {
    /// The last sequence this host knows of.
    fn head(&self) -> Seq;
    /// The lines, oldest first, as many as the host keeps.
    fn lines(&self) -> Vec<Line>;
    fn connections(&self) -> Vec<Connection>;
    fn verifies(&self) -> Vec<Verified>;
}

/// Everything the explorer reads, handed to it on every call: it keeps no
/// copy of any of it. `store` is what the host holds — the authority's
/// whole store, a partial replica's union — and the tables and columns
/// drawn are the store's own schema's, so a column the store has is drawn
/// and one it lacks is not. `schema` is the module's, `module` names the
/// queries the console runs, and `ctx` is who the console reads as.
pub struct Source<'a> {
    pub store: &'a dyn Store,
    pub schema: &'a Schema,
    pub module: &'a Module,
    pub log: &'a dyn LogView,
    pub ctx: Ctx,
}

/// The CRUD a module exposes, per table, as its functions say it: a mutator
/// named `insert_<t>`, `update_<t>` or `put_<t>` whose input is `t`'s
/// columns, or `delete_<t>` whose input is its key's — the shape
/// `r.crud::<T>()` generates, and a hand-written replacement keeps, since
/// that is what its callers pass. `may_author` is false: the host decides
/// that.
pub fn crud_of(module: &Module) -> Vec<(TableName, CrudVerbs)> {
    let mut out = Vec::new();
    for t in module.schema.tables() {
        let columns: Vec<&str> = t.columns.iter().map(|c| c.name.as_str()).collect();
        let key: Vec<&str> = t.key.iter().map(String::as_str).collect();
        let takes = |verb: &str, want: &[&str]| {
            module
                .lookup_function(&format!("{verb}_{}", t.name))
                .is_some_and(|f| f.kind == FnKind::Mutator && f.input.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>() == want)
        };
        let verbs = CrudVerbs {
            insert: takes("insert", &columns),
            update: takes("update", &columns),
            delete: takes("delete", &key),
            put: takes("put", &columns),
            may_author: false,
        };
        if verbs.any() {
            out.push((t.name.clone(), verbs));
        }
    }
    out
}

/// Every function a log may name, by hash: the module's closures, and the
/// two raw writes every module has by construction ([`ark::raw`]). What a
/// host names a log line's function with.
pub fn function_names(module: &Module) -> BTreeMap<FnHash, String> {
    let mut names: BTreeMap<FnHash, String> = ark::hash::closures(module).into_iter().map(|(h, c)| (h, c.function.name)).collect();
    for r in [ark::raw::Raw::PutRow, ark::raw::Raw::DeleteRow] {
        names.insert(r.hash().clone(), r.name().to_string());
    }
    names
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
    fn verifies(&self) -> Vec<Verified> {
        vec![]
    }
}
