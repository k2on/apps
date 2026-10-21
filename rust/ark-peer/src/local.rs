//! What a peer keeps beside its log, as rows (§1.3, §1.7, §1.9): the
//! schema [`Local`], on a local instance under the `local.` names of the
//! peer's disk ([`Shared`]): a server's journal — each origin's last
//! `local` and actor (`origin`), the intents it refused (`verdict`), those
//! waiting behind a held one (`waiting`) — and its volatile space as it
//! last closed (`volatile`); a device's replica (`replica`: its origin,
//! the last `local` drawn, who last signed in, its fork and the chain it
//! hands over) and its pending intents (`pending`). And an app's logins
//! ([`Logins`]), a local instance of `ark-auth`'s schema.

use std::io;
use std::sync::{Arc, Mutex};

use ark::db::{Db, Durable as Run};
use ark::hash::EntryHash;
use ark::log::{Entry, EntryId, Origin, Seq};
use ark::prelude::{Deserialize, Row, Schema, Serialize};
use ark::row::Bytes;
use ark_auth::remember::Remembered;

use crate::{Disk, Instance, Joining};

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

/// A device's replica row on `disk` said beforehand, as `f` changes it —
/// what a simulator seeds, or a demo: the rest of what the disk's local
/// instance holds is kept.
pub fn preset(disk: &mut dyn Disk, f: impl FnOnce(&mut Replica)) -> io::Result<()> {
    let names = [format!("{LOCAL}kv"), format!("{LOCAL}log")];
    let mut held = crate::Memory::default();
    for n in &names {
        disk.get(n)?.map_or(Ok(()), |b| held.put(n, &b))?;
    }
    let mut local = Instance::open::<Local>(Box::new(Shared::new(Box::new(held)).under(LOCAL)));
    let write = local.write::<Local, Run, _>(|db| {
        let mut r = replica(db).unwrap_or_default();
        f(&mut r);
        db.replica.put(r)
    });
    write.map_err(|r| io::Error::other(r.0))?;
    let mut held = local.into_disk();
    for n in ["kv", "log"] {
        held.get(n)?.map_or(Ok(()), |b| disk.put(&format!("{LOCAL}{n}"), &b))?;
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
