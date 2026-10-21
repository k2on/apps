//! What a peer keeps beside its log, as rows (§1.3, §1.7, §1.9): an
//! **instance** of a schema that is not wired, and the schema a peer keeps
//! its own state in ([`Local`]).
//!
//! Two concepts make every kind of instance. A table is durable or
//! volatile (§1.3). An instance is wired — a [`Peer`](crate::Peer), linked
//! to its server or serving one — or it is not: an [`Instance`], a machine
//! in the authority role that is never offered a link. So a schema's
//! tables are durable and synced (the app's log), volatile and synced (its
//! live state), durable and **local** ([`Instance::open`]: the log's own
//! format on a disk, `kv` and `log`, one entry appended per write, folded
//! into the rows whole once the entries outnumber the rows), or volatile
//! and **ephemeral** ([`Instance::ephemeral`]: no journal at all). Nothing
//! is added to the kernel for it: a local table is a table of another
//! `#[derive(Schema)]` struct, read and written through its `Db<S>`.
//!
//! A write is a typed run, `Db<S, P>` as a function's (§1.16); its changes
//! are the one entry the machine sequences, logs and makes durable, by a
//! function of the instance's own (`write`) that applies them — the run is
//! not done again, since a local instance is never rebased.
//!
//! A peer's own state is [`Local`]'s rows, its journal under the `local.`
//! names of the peer's disk ([`Shared`]): a server's journal — each
//! origin's last `local` and actor (`origin`), the intents it refused
//! (`verdict`), those waiting behind a held one (`waiting`) — and its
//! volatile space as it last closed (`volatile`); a device's replica
//! (`replica`: its origin, the last `local` drawn, who last signed in,
//! its fork and the chain it hands over) and its pending intents
//! (`pending`).

use std::io;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use ark::db::{Db, Durable as DurableRun, Tables, Volatile, Writes};
use ark::format::Encode;
use ark::log::{Durable, Origin};
use ark::peer::{Ctx, Functions, Input, Output, Peer as Machine, Procedure, Who};
use ark::prelude::{Deserialize, Row, Schema, Serialize};
use ark::row::{Describe, Type};
use ark::store::{Change, Store};
use ark::Refusal;

use crate::{alone_at, draw_origin, now_ms, Disk};

/// A value of the kernel's kept in a column: its bytes in the value
/// format.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Bytes(pub Vec<u8>);

impl Describe for Bytes {
    fn describe() -> Type {
        Type::Bytes
    }
}

impl Bytes {
    pub fn of(v: &impl Encode) -> Bytes {
        Bytes(v.to_bytes())
    }

    pub fn read<T: Encode>(&self) -> Option<T> {
        T::from_bytes(&self.0).ok()
    }
}

/// A server's: an origin's last `local` judged, and the actor it is bound
/// to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[ark(table = "origin")]
pub struct Last {
    #[ark(key)]
    pub origin: Bytes,
    pub actor: String,
    pub last: i64,
}

/// A server's: an intent it refused, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[ark(table = "verdict")]
pub struct Refused {
    #[ark(key)]
    pub entry: Bytes,
    pub why: String,
}

/// A server's: an intent waiting behind a held one of its origin, and who
/// pushed it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[ark(table = "waiting")]
pub struct Waits {
    #[ark(key)]
    pub origin: Bytes,
    #[ark(key)]
    pub local: i64,
    pub actor: String,
    pub session: String,
    pub roles: Vec<String>,
    pub entry: Bytes,
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
    pub origin: Bytes,
    pub local: i64,
    pub actor: Option<String>,
    pub session: String,
    pub fork: Option<Bytes>,
    pub joining: Option<Bytes>,
}

/// A device's: one of its own intents not yet confirmed, by its `local`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row)]
#[ark(table = "pending")]
pub struct Intent {
    #[ark(key)]
    pub local: i64,
    pub entry: Bytes,
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

/// The one function an instance runs: the changes its run made.
struct Apply(Arc<Mutex<Vec<Change>>>);

impl Procedure for Apply {
    fn apply(&self, store: &mut Store, _: &Ctx, _: &[u8]) -> Result<Vec<Change>, Refusal> {
        let cs = std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()));
        for (n, c) in cs.iter().enumerate() {
            if let Err(e) = store.apply(c) {
                for c in cs[..n].iter().rev() {
                    store.apply(&c.inverse()).expect("an inverse applies");
                }
                return Err(e);
            }
        }
        Ok(cs)
    }
}

/// A schema's tables on a machine that is never linked: local over a disk
/// (`P` durable), or ephemeral (volatile, no disk).
pub struct Instance<S: Tables, P: Writes = DurableRun> {
    machine: Machine,
    disk: Option<Box<dyn Disk>>,
    run: Arc<Mutex<Vec<Change>>>,
    /// The rows it holds, which its journal is folded into once it holds
    /// more entries than that.
    rows: usize,
    /// The disk's last refusal, while it lasts.
    pub failure: Option<String>,
    _s: PhantomData<fn() -> (S, P)>,
}

impl<S: Tables> Instance<S> {
    /// `S`'s tables as `disk` last kept them; empty, saying why in
    /// [`Instance::failure`], when it does not read.
    pub fn open(mut disk: Box<dyn Disk>) -> Instance<S> {
        let stored = disk.load();
        let mut i = Instance::made(Some(disk));
        match stored {
            Ok(s) => {
                let rows = s.rows.unwrap_or_else(|| i.machine.confirmed_rows());
                let out = i.machine.step(Input::Open(Durable {
                    rows,
                    logged: s.logged,
                    ..Durable::default()
                }));
                i.persist(out);
            }
            Err(e) => i.failure = Some(e.to_string()),
        }
        i.rows = i.machine.store.snapshot_rows().iter().map(|(_, r)| r.len()).sum();
        i
    }

    /// Its disk, to open it again from.
    pub fn into_disk(self) -> Box<dyn Disk> {
        self.disk.expect("a local instance has a disk")
    }
}

impl<S: Tables> Instance<S, Volatile> {
    /// `S`'s volatile tables, empty, kept nowhere.
    pub fn ephemeral() -> Instance<S, Volatile> {
        Instance::made(None)
    }
}

impl<S: Tables, P: Writes> Instance<S, P> {
    fn made(disk: Option<Box<dyn Disk>>) -> Instance<S, P> {
        let run = Arc::new(Mutex::new(vec![]));
        let mut functions = Functions::default();
        functions.insert("write", Arc::new(Apply(run.clone())));
        let me = Who {
            actor: "local".into(),
            session: "local".into(),
            roles: vec![],
        };
        let mut machine = Machine::new(Arc::new(S::schema()), functions, alone_at(None), me).in_space(P::SPACE);
        // Its own ids, drawn afresh at each open: the log it folds keeps none.
        machine.origin = draw_origin();
        Instance {
            machine,
            disk,
            run,
            rows: 0,
            failure: None,
            _s: PhantomData,
        }
    }

    /// `f` over its tables.
    pub fn read<T>(&self, f: impl for<'t> FnOnce(&Db<'t, S>) -> T) -> T {
        ark::db::read::<S, T>(&self.machine.store, f)
    }

    /// `f` over its tables, writing: what it made is kept, all or none,
    /// and its answer.
    pub fn write<T>(&mut self, f: impl for<'t> FnOnce(&mut Db<'t, S, P>) -> Result<T, Refusal>) -> Result<T, Refusal> {
        let (t, changes) = ark::db::write_in::<S, P, T>(&mut self.machine.store, f)?;
        if changes.is_empty() {
            return Ok(t);
        }
        for c in changes.iter().rev() {
            self.machine.store.apply(&c.inverse()).expect("an inverse applies");
        }
        let moved: isize = changes
            .iter()
            .map(|c| match c {
                Change::Add { .. } => 1,
                Change::Remove { .. } => -1,
                Change::Edit { .. } => 0,
            })
            .sum();
        *self.run.lock().unwrap_or_else(|e| e.into_inner()) = changes;
        let out = self.machine.step(Input::Mutate {
            func: "write".into(),
            input: vec![],
            now: now_ms(),
        });
        let refused = out.iter().find_map(|o| match o {
            Output::Refused { why, .. } => Some(Refusal(why.clone())),
            _ => None,
        });
        self.persist(out);
        if let Some(r) = refused {
            return Err(r);
        }
        self.rows = self.rows.saturating_add_signed(moved);
        let (head, base) = (self.machine.log().head().0, self.machine.log().base_seq());
        if head - base > 64 + self.rows as i64 {
            let out = self.machine.step(Input::Compact(head));
            self.persist(out);
        }
        Ok(t)
    }

    fn persist(&mut self, out: Vec<Output>) {
        let Some(d) = &mut self.disk else { return };
        for o in out {
            if let Output::Persist(b) = o {
                self.failure = d.record(&b).err().map(|e| e.to_string());
            }
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
pub(crate) fn beside(disk: Box<dyn Disk>) -> (Box<dyn Disk>, Instance<Local>) {
    let shared = Shared::new(disk);
    let local = Instance::open(Box::new(shared.under(LOCAL)));
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
    let mut local = Instance::<Local>::open(Box::new(held));
    local
        .write(|db| {
            let mut r = replica(db).unwrap_or_default();
            f(&mut r);
            if r.origin.read::<Origin>().is_none() {
                r.origin = Bytes::of(&draw_origin());
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

    fn open(&self) -> Option<Instance<ark_auth::remember::Remembered>> {
        let at = self.at.clone()?;
        #[cfg(not(target_arch = "wasm32"))]
        let disk: Box<dyn Disk> = Box::new(crate::Files::new(at).ok()?);
        #[cfg(target_arch = "wasm32")]
        let disk: Box<dyn Disk> = Box::new(crate::disk::Local(at));
        Some(Instance::open(disk))
    }

    /// The login remembered for `server`.
    pub fn recall(&self, server: &str) -> Option<ark_auth::Login> {
        self.open()?.read(|db| ark_auth::remember::recall(db, server))
    }

    pub fn remember(&self, server: &str, login: &ark_auth::Login) {
        if let Some(mut i) = self.open() {
            let _ = i.write(|db| ark_auth::remember::remember(db, server, login));
        }
    }

    pub fn forget(&self, server: &str) {
        if let Some(mut i) = self.open() {
            let _ = i.write(|db| ark_auth::remember::forget(db, server));
        }
    }
}
