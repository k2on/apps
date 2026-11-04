//! What a peer keeps beside its log, each a record under the `local.`
//! names of the peer's disk (`Shared`): a device's replica ([`Replica`],
//! `local.replica`: its origin, the last `local` drawn, who last signed
//! in, how many times it opened) and its pending intents, records appended
//! ([`Kept`], `local.pending`); a server's journal and its volatile space
//! as it last closed, each one record (`local.journal`, `local.volatile`,
//! `crate::hub`). And an app's logins ([`Logins`]), an instance of
//! `ark-auth`'s schema of its own.

use std::io;
use std::sync::{Arc, Mutex};

use ark::db::Durable as Run;
use ark::format::Encode;
use ark::hash::EntryHash;
use ark::log::{Origin, Seq};
use ark::peer::Pending;
use ark::prelude::{Deserialize, Serialize};
use ark_auth::remember::Remembered;

use crate::{Disk, Instance};

/// A device's, one record: its origin and the last `local` drawn from it
/// (as it was signed in: [`Kept`] has it since), who last signed in on it,
/// and how many times it has opened — what its live machine's origin is
/// drawn from at each.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replica {
    /// Drawn at the first open; the default until then.
    pub origin: Origin,
    pub local: i64,
    pub actor: Option<String>,
    pub session: String,
    pub opens: u64,
}

/// The blob a device's replica is kept in, under [`LOCAL`].
pub const REPLICA: &str = "replica";

impl Replica {
    /// What `disk` (the peer's, under [`LOCAL`]) holds; none, or one that
    /// does not read, the default.
    pub fn read(disk: &mut dyn Disk) -> Replica {
        disk.get(REPLICA)
            .ok()
            .flatten()
            .and_then(|b| Replica::from_bytes(&b).ok())
            .unwrap_or_default()
    }
}

/// The blob a device's pending intents are kept in, under [`LOCAL`].
pub const PENDING: &str = "pending";

/// One record of a device's pending intents (§1.7), length-framed in
/// [`PENDING`]: so many taken off the front (confirmed or refused), these
/// put on the back, each with its preview's changes — or, `whole`, these
/// all of them, the blob written afresh; with the last `local` drawn and
/// the head of the log the changes stand on. A mutate appends one put, so
/// what it writes is the same however many are pending; an open puts the
/// changes back rather than running the intents again.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    pub whole: bool,
    pub gone: u64,
    pub put: Vec<Pending>,
    pub local: i64,
    pub at: Option<(Seq, EntryHash)>,
}

impl Kept {
    /// The pending intents the records in `blob` keep, read up to the first
    /// that does not read; the last of them, and how many there are.
    pub fn read(blob: &[u8]) -> (Vec<Pending>, Kept, usize) {
        let (mut pending, mut last) = (std::collections::VecDeque::new(), Kept::default());
        let records = crate::disk::framed::<Kept>(blob).0;
        let n = records.len();
        for k in records {
            if k.whole {
                pending.clear();
            }
            pending.drain(..(k.gone as usize).min(pending.len()));
            pending.extend(k.put.iter().cloned());
            last = Kept { put: vec![], ..k };
        }
        (pending.into(), last, n)
    }
}

/// The names a peer keeps its records beside its log under, on its disk.
pub const LOCAL: &str = "local.";

/// One disk's blobs shared, each holder under a prefix of their names: how
/// a peer keeps its records beside its log on the disk its log is on.
#[derive(Clone)]
pub(crate) struct Shared {
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

/// A peer's disk, and the same disk under [`LOCAL`]: what it keeps beside
/// its log.
pub(crate) fn beside(disk: Box<dyn Disk>) -> (Box<dyn Disk>, Box<dyn Disk>) {
    let shared = Shared::new(disk);
    let local = Box::new(shared.under(LOCAL));
    (Box::new(shared), local)
}

/// A device's replica on `disk` said beforehand, as `f` changes it — what
/// a simulator seeds, or a demo.
pub fn preset(disk: &mut dyn Disk, f: impl FnOnce(&mut Replica)) -> io::Result<()> {
    let name = format!("{LOCAL}{REPLICA}");
    let mut r = disk.get(&name)?.and_then(|b| Replica::from_bytes(&b).ok()).unwrap_or_default();
    f(&mut r);
    disk.put(&name, &r.to_bytes())
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
