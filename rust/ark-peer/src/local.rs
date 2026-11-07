//! What a peer keeps beside its log, each a record under the `local.`
//! names of the peer's disk (`Shared`): a device's replica ([`Replica`],
//! `local.replica`: its origin, the last `local` drawn, who last signed
//! in, how many times it opened) and its pending intents, records appended
//! ([`Kept`], `local.pending`); a server's journal and its volatile space
//! as it last closed, each one record (`local.journal`, `local.volatile`,
//! `crate::authority`). And an app's logins ([`Logins`]), an instance of
//! `ark-auth`'s schema of its own.

use std::io;
use std::sync::{Arc, Mutex};

use ark::db::Durable as Run;
use ark::format::Encode;
use ark::log::Origin;
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

/// One record of a device's pending intents (§1.7), as its machine says
/// it, length-framed in [`PENDING`]: appended, or the blob written afresh
/// when it is whole.
pub use ark::log::Kept;

/// The names a peer keeps its records beside its log under, on its disk.
pub const LOCAL: &str = "local.";

/// One disk's blobs shared, each holder under a prefix of their names: how
/// a peer keeps its records beside its log on the disk its log is on.
struct Shared(Arc<Mutex<Box<dyn Disk>>>, &'static str);

impl Shared {
    fn at<T>(&self, name: &str, f: impl FnOnce(&mut dyn Disk, &str) -> T) -> T {
        let mut d = self.0.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut **d, &format!("{}{name}", self.1))
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

/// A peer's disk apart: the disk, the same disk under [`LOCAL`] — what it
/// keeps beside its log — and the store of its files.
pub(crate) fn beside(disk: Box<dyn Disk>) -> crate::Apart {
    let (files, disk) = (disk.files(), Arc::new(Mutex::new(disk)));
    (Box::new(Shared(disk.clone(), "")), Box::new(Shared(disk, LOCAL)), files)
}

/// A device's replica on `disk` said beforehand, as `f` changes it — what
/// a simulator seeds, or a demo — and written when it changed.
pub fn preset(disk: &mut dyn Disk, f: impl FnOnce(&mut Replica)) -> io::Result<()> {
    let name = format!("{LOCAL}{REPLICA}");
    let was = disk.get(&name)?;
    let mut r = was.as_deref().and_then(|b| Replica::from_bytes(b).ok()).unwrap_or_default();
    f(&mut r);
    match was.is_some_and(|b| b == r.to_bytes()) {
        true => Ok(()),
        false => disk.put(&name, &r.to_bytes()),
    }
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
