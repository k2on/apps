//! Instances, and what a peer keeps beside its log (§1.3, §1.7, §1.9).
//!
//! An [`Instance`] is one machine and what keeps it: a disk or none, the
//! open from it, every record written in order, the log folded by one
//! rule ([`Fold`]). Two concepts make every kind of instance. A table is
//! durable or volatile (§1.3). An instance is wired — one of a
//! [`Peer`](crate::Peer)'s, linked to its server or serving one — or it is
//! not: a machine in the authority role never offered a link. So a
//! schema's tables are durable and synced (the app's log), volatile and
//! synced (its live state), durable and **local** ([`Instance::open`]: the
//! log's own format on a disk, `kv` and `log`), or volatile and
//! **ephemeral** ([`Instance::ephemeral`]: no disk). A local table is a
//! table of another `#[derive(Schema)]` struct, read and written through
//! its `Db<S>`.
//!
//! Only an unwired instance is written by a closure: a typed run, `Db<S,
//! P>` as a function's (§1.16), whose changes are the input of the one
//! entry the machine sequences — a function of the instance's own
//! (`write`) that makes them. A synced entry names a domain's function.
//!
//! A peer's own state is [`Local`]'s rows, under the `local.` names of the
//! peer's disk ([`Shared`]): a server's journal — each origin's last
//! `local` and actor (`origin`), the intents it refused (`verdict`), those
//! waiting behind a held one (`waiting`) — and its volatile space as it
//! last closed (`volatile`); a device's replica (`replica`: its origin,
//! the last `local` drawn, who last signed in, its fork and the chain it
//! hands over) and its pending intents (`pending`).

use std::io;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};

use ark::db::{Db, Durable as Run, Tables, Writes};
use ark::hash::EntryHash;
use ark::log::{Durable, Entry, EntryId, Origin, Seq};
use ark::peer::{Functions, Input, Output, Peer as Machine, Who};
use ark::prelude::{Deserialize, Row, Schema, Serialize};
use ark::row::Bytes;
use ark::schema::Space;
use ark::store::{Change, LOG};
use ark::Refusal;

use ark_auth::remember::Remembered;

use crate::{alone_at, draw_origin, now_ms, Disk, Joining};

/// A server's: an origin's last `local` judged, and the actor it is bound
/// to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[ark(table = "origin")]
pub struct Last {
    #[ark(key)]
    pub origin: Origin,
    pub actor: String,
    pub last: i64,
}

/// A server's: an intent it refused, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[ark(table = "verdict")]
pub struct Refused {
    #[ark(key)]
    pub entry: EntryId,
    pub why: String,
}

/// A server's: an intent waiting behind a held one of its origin, and who
/// pushed it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[ark(table = "waiting")]
pub struct Waits {
    #[ark(key)]
    pub origin: Origin,
    #[ark(key)]
    pub local: i64,
    pub actor: String,
    pub session: String,
    pub roles: Vec<String>,
    pub entry: Entry,
}

/// A server's: a row of its volatile space (its log's row among them) as
/// it stood when the server last closed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[ark(table = "volatile")]
pub struct LiveRow {
    #[ark(key)]
    pub table: String,
    #[ark(key)]
    pub row: Bytes,
}

/// A device's, one row: its origin and the last `local` drawn from it,
/// who last signed in on it, its fork while alone, and the chain it hands
/// over while joined unanswered.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Row)]
#[ark(table = "replica")]
pub struct Replica {
    /// Always 0: a disk is one replica.
    #[ark(key)]
    pub n: i64,
    /// Drawn at the first open; the default until then.
    pub origin: Origin,
    pub local: i64,
    pub actor: Option<String>,
    pub session: String,
    pub fork: Option<(Seq, EntryHash)>,
    pub joining: Option<Joining>,
}

/// A device's: one of its own intents not yet confirmed, by its `local`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[ark(table = "pending")]
pub struct Intent {
    #[ark(key)]
    pub local: i64,
    pub entry: Entry,
}

/// What a peer keeps beside its log.
#[derive(Schema)]
pub struct Local {
    pub origin: Last,
    pub verdict: Refused,
    pub waiting: Waits,
    pub volatile: LiveRow,
    pub replica: Replica,
    pub pending: Intent,
}

/// The names a peer's local instance journals under on its disk.
pub const LOCAL: &str = "local.";

/// When an instance folds its log into its rows, the one rule: once the
/// entries above its snapshot outnumber `keep` and `slack` (and the rows it
/// holds, when `rows`), all but the last `keep` are folded. A server keeps
/// `keep` entries and folds at twice that; a local instance keeps none and
/// folds once its entries outnumber its rows (and 64), so a fold, which
/// writes every row, is paid for by as many writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fold {
    pub keep: usize,
    pub slack: usize,
    pub rows: bool,
}

impl Fold {
    /// A server's: `keep` entries above the horizon.
    pub fn keep(keep: usize) -> Fold {
        Fold {
            keep,
            slack: keep,
            rows: false,
        }
    }
}

/// One machine and what keeps it (§1.7): its disk, when it has one —
/// opened from what the disk holds, every `Persist` written in order (and,
/// while a write fails, kept to write again, the failure said), its log
/// folded as its [`Fold`] says. A [`Peer`](crate::Peer) is three of these
/// wired together; one alone is a schema's tables on a machine never
/// linked, read and written through a `Db<S>` of its schema's struct.
pub struct Instance {
    pub machine: Machine,
    pub disk: Option<Box<dyn Disk>>,
    pub fold: Option<Fold>,
    /// Records not yet written.
    unwritten: Vec<Vec<u8>>,
    /// The rows it holds, which a fold by rows is measured against.
    rows: usize,
    /// The disk's last refusal, while it lasts.
    pub failure: Option<String>,
}

impl Deref for Instance {
    type Target = Machine;
    fn deref(&self) -> &Machine {
        &self.machine
    }
}

impl DerefMut for Instance {
    fn deref_mut(&mut self) -> &mut Machine {
        &mut self.machine
    }
}

impl Instance {
    pub fn new(machine: Machine, disk: Option<Box<dyn Disk>>) -> Instance {
        Instance {
            machine,
            disk,
            fold: None,
            unwritten: vec![],
            rows: 0,
            failure: None,
        }
    }

    /// `S`'s tables as `disk` last kept them, on an unwired machine; empty,
    /// saying why in [`Instance::failure`], when it does not read.
    pub fn open<S: Tables>(disk: Box<dyn Disk>) -> Instance {
        let mut i = Instance::unwired::<S>(Space::Durable, Some(disk));
        i.fold = Some(Fold {
            keep: 0,
            slack: 64,
            rows: true,
        });
        let stored = i.stored();
        i.step(Input::Open(stored));
        i
    }

    /// `S`'s volatile tables, empty, kept nowhere.
    pub fn ephemeral<S: Tables>() -> Instance {
        Instance::unwired::<S>(Space::Volatile, None)
    }

    fn unwired<S: Tables>(space: Space, disk: Option<Box<dyn Disk>>) -> Instance {
        let schema = Arc::new(S::schema());
        let mut machine = Machine::new(schema, Functions::default(), alone_at(None), Who::default()).in_space(space);
        // Its own ids, drawn afresh at each open: the log it folds keeps none.
        machine.origin = draw_origin();
        Instance::new(machine, disk)
    }

    /// What its disk holds, to open from: empty, the failure said, when it
    /// does not read.
    pub fn stored(&mut self) -> Durable {
        let stored = self.disk.as_mut().map_or(Ok(Default::default()), |d| d.load());
        let s = stored.unwrap_or_else(|e| {
            self.failure = Some(e.to_string());
            Default::default()
        });
        let rows = s.rows.unwrap_or_else(|| self.machine.confirmed_rows());
        Durable {
            rows,
            logged: s.logged,
            ..Durable::default()
        }
    }

    /// One input to its machine, what it made durable written, its log
    /// folded when its rule says; what the machine said.
    pub fn step(&mut self, input: Input) -> Vec<Output> {
        let out = self.machine.step(input);
        self.took(&out);
        let (head, base) = (self.machine.log().head().0, self.machine.log().base_seq());
        if let Some(f) = self
            .fold
            .filter(|f| head - base > (f.keep + f.slack + f.rows as usize * self.rows) as i64)
        {
            let folded = self.machine.step(Input::Compact(head - f.keep as i64));
            self.took(&folded);
        }
        self.flush();
        out
    }

    fn took(&mut self, out: &[Output]) {
        for o in out {
            match o {
                Output::Persist(b) => self.unwritten.push(b.clone()),
                Output::Changes(cs) => {
                    let n = |c: &Change| match c {
                        Change::Add { .. } => 1,
                        Change::Remove { .. } => -1,
                        Change::Edit { .. } => 0,
                    };
                    self.rows = self.rows.saturating_add_signed(cs.iter().filter(|c| c.table() != LOG).map(n).sum());
                }
                _ => {}
            }
        }
    }

    /// What is unwritten, written in order up to the first write that
    /// fails: whether all of it is.
    pub fn flush(&mut self) -> bool {
        let Some(d) = &mut self.disk else {
            self.unwritten.clear();
            return true;
        };
        while let Some(r) = self.unwritten.first() {
            if let Err(e) = d.record(r) {
                self.failure = Some(format!("the log cannot be written: {e}"));
                return false;
            }
            self.unwritten.remove(0);
        }
        self.failure = None;
        true
    }

    /// Its disk, to open it again from.
    pub fn into_disk(self) -> Box<dyn Disk> {
        self.disk.expect("an instance with a disk")
    }

    /// `f` over its tables, as `S`'s.
    pub fn read<S: Tables, T>(&self, f: impl for<'t> FnOnce(&Db<'t, S>) -> T) -> T {
        ark::db::read::<S, T>(&self.machine.store, f)
    }

    /// `f` over its tables as `S`'s, writing `P`'s: what it made is the
    /// facts of one entry (`Input::Write`), all or none, and its answer.
    pub fn write<S: Tables, P: Writes, T>(&mut self, f: impl for<'t> FnOnce(&mut Db<'t, S, P>) -> Result<T, Refusal>) -> Result<T, Refusal> {
        let (t, facts) = ark::db::write_in::<S, P, T>(&mut self.machine.store, f)?;
        if facts.is_empty() {
            return Ok(t);
        }
        self.machine
            .store
            .apply_all(&facts.iter().rev().map(Change::inverse).collect::<Vec<_>>())?;
        let said = self.step(Input::Write { facts, now: now_ms() });
        match said.into_iter().find_map(|o| match o {
            Output::Refused { why, .. } => Some(why),
            _ => None,
        }) {
            Some(why) => Err(Refusal::new(why)),
            None => Ok(t),
        }
    }
}

/// One disk's blobs shared, each holder under a prefix of their names: how
/// a peer's local instance keeps its journal on the disk its log is on.
#[derive(Clone)]
pub struct Shared {
    disk: Arc<Mutex<Box<dyn Disk>>>,
    prefix: String,
}

impl Shared {
    pub fn new(disk: Box<dyn Disk>) -> Shared {
        Shared {
            disk: Arc::new(Mutex::new(disk)),
            prefix: String::new(),
        }
    }

    /// The same disk, under `prefix` of this one's names.
    pub fn under(&self, prefix: &str) -> Shared {
        Shared {
            disk: self.disk.clone(),
            prefix: format!("{}{prefix}", self.prefix),
        }
    }

    fn at<T>(&self, name: &str, f: impl FnOnce(&mut dyn Disk, &str) -> T) -> T {
        let mut d = self.disk.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut **d, &format!("{}{name}", self.prefix))
    }
}

impl Disk for Shared {
    fn get(&mut self, name: &str) -> io::Result<Option<Vec<u8>>> {
        self.at(name, |d, n| d.get(n))
    }

    fn put(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        self.at(name, |d, n| d.put(n, bytes))
    }

    fn append(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        self.at(name, |d, n| d.append(n, bytes))
    }
}

/// A peer's disk, shared with its local instance.
pub(crate) fn beside(disk: Box<dyn Disk>) -> (Box<dyn Disk>, Instance) {
    let shared = Shared::new(disk);
    let local = Instance::open::<Local>(Box::new(shared.under(LOCAL)));
    (Box::new(shared), local)
}

/// A device's replica row, as its disk holds it.
pub fn replica(db: &Db<Local>) -> Option<Replica> {
    db.replica.get(0)
}

/// A device's replica row on `disk` said beforehand, as `f` changes it
/// (its origin drawn when none is said) — what a simulator seeds, or a
/// demo: the rest of what the disk's local instance holds is kept.
pub fn preset(disk: &mut dyn Disk, f: impl FnOnce(&mut Replica)) -> io::Result<()> {
    let mut held = crate::Memory::default();
    for n in ["kv", "log"] {
        if let Some(b) = disk.get(&format!("{LOCAL}{n}"))? {
            held.put(n, &b)?;
        }
    }
    let mut local = Instance::open::<Local>(Box::new(held));
    local
        .write::<Local, Run, _>(|db| {
            let mut r = replica(db).unwrap_or_default();
            f(&mut r);
            if r.origin == Origin::default() {
                r.origin = draw_origin();
            }
            db.replica.put(r)
        })
        .map_err(|r| io::Error::other(r.0))?;
    let mut held = local.into_disk();
    for n in ["kv", "log"] {
        if let Some(b) = held.get(n)? {
            disk.put(&format!("{LOCAL}{n}"), &b)?;
        }
    }
    Ok(())
}

/// Where one app's logins are remembered, one per server: `ark-auth`'s
/// [`ark_auth::remember::Remembered`], a local instance of the app's own —
/// natively under its configuration directory (or a directory of the
/// caller's, [`Logins::at`]), in a browser its page's `localStorage`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Logins {
    /// The directory natively, the key prefix in a browser.
    at: Option<String>,
}

impl Logins {
    /// `app` names the directory, or the key prefix: `"harken"`.
    pub fn new(app: &str) -> Logins {
        #[cfg(not(target_arch = "wasm32"))]
        let at = ark_auth::remember::dir(app).map(|d| d.to_string_lossy().into_owned());
        #[cfg(target_arch = "wasm32")]
        let at = Some(format!("{app}.logins"));
        Logins { at }
    }

    /// Kept in `dir`: one device's of several on a machine.
    pub fn at(dir: &std::path::Path) -> Logins {
        Logins {
            at: Some(dir.to_string_lossy().into_owned()),
        }
    }

    fn open(&self) -> Option<Instance> {
        let at = self.at.clone()?;
        #[cfg(not(target_arch = "wasm32"))]
        let disk: Box<dyn Disk> = Box::new(crate::Files::new(at).ok()?);
        #[cfg(target_arch = "wasm32")]
        let disk: Box<dyn Disk> = Box::new(crate::disk::Local(at));
        Some(Instance::open::<Remembered>(disk))
    }

    /// The login remembered for `server`.
    pub fn recall(&self, server: &str) -> Option<ark_auth::Login> {
        self.open()?.read::<Remembered, _>(|db| ark_auth::remember::recall(db, server))
    }

    pub fn remember(&self, server: &str, login: &ark_auth::Login) {
        if let Some(mut i) = self.open() {
            let _ = i.write::<Remembered, Run, _>(|db| ark_auth::remember::remember(db, server, login));
        }
    }

    pub fn forget(&self, server: &str) {
        if let Some(mut i) = self.open() {
            let _ = i.write::<Remembered, Run, _>(|db| ark_auth::remember::forget(db, server));
        }
    }
}
