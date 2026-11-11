//! The views an app names, and what it reads of them (§1.10): the port
//! between a domain's keys and whatever holds the views.
//!
//! An app names every view it opens as one value of its own key type
//! ([`ViewKey`]): money's `MoneyView`, harken's `HarkenView`. A key is one
//! view of the domain ([`ViewKey::open`]: its function, its input and the
//! type its rows decode as), or a page made of several ([`ViewKey::parts`]):
//! money's budget for a month is its lines, its summary, its spending and
//! its formulas, wherever it is drawn. The core says what a page needs, so
//! every shell opens the same views for it.
//!
//! What holds them is an adapter's (`ark_peer::store::Store`, over a
//! peer); what reads them — an app's update and its screens — reads
//! through [`Views`], and never sees the peer.

use std::any::Any;
use std::fmt::Debug;

use crate::file::FileRef;
use crate::format::Encode;
use crate::plan::Patch;

/// A key of an app's views: one view of the domain, or a page made of
/// several.
pub trait ViewKey: Clone + Ord + Debug + Send + 'static {
    /// The views this key is made of: itself when it is one view, or every
    /// view a page needs — each of them a key whose [`ViewKey::open`] says
    /// how it is opened.
    fn parts(&self) -> Vec<Self> {
        vec![self.clone()]
    }

    /// How the view this key names is opened; none for a page, which is
    /// only its parts.
    fn open(&self) -> Option<Opening>;
}

/// A view's rows as the app's own type, kept as they move: read whole, or
/// patched.
pub trait Rows: Send {
    /// The rows read whole.
    fn fill(&mut self, rows: &[Vec<u8>]);
    /// The patches, in order, onto the rows.
    fn patch(&mut self, patches: &[Patch<Vec<u8>>]);
    /// The rows, a `Vec` of the type they were opened as.
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

struct Typed<T> {
    rows: Vec<T>,
    read: fn(&[u8]) -> T,
}

impl<T: Send + 'static> Rows for Typed<T> {
    fn fill(&mut self, rows: &[Vec<u8>]) {
        self.rows = rows.iter().map(|b| (self.read)(b)).collect();
    }

    fn patch(&mut self, patches: &[Patch<Vec<u8>>]) {
        splice(&mut self.rows, patches, self.read)
    }

    fn as_any(&self) -> &dyn Any {
        &self.rows
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        &mut self.rows
    }
}

/// The patches, in order, onto the decoded list: `crate::plan::splice`
/// with the decoding said, because a row here is the UI's type.
pub fn splice<T>(rows: &mut Vec<T>, patches: &[Patch<Vec<u8>>], read: fn(&[u8]) -> T) {
    for p in patches {
        match p {
            Patch::Insert { at, node } => rows.insert((*at).min(rows.len()), read(node)),
            Patch::Remove { at } if *at < rows.len() => drop(rows.remove(*at)),
            Patch::Update { at, node } if *at < rows.len() => rows[*at] = read(node),
            _ => {}
        }
    }
}

fn decode<T: Encode>(b: &[u8]) -> T {
    T::from_bytes(b).unwrap_or_else(|e| panic!("a view's row: {e}"))
}

/// How one view is opened: its function by name, its input's bytes, and
/// the rows it is read into.
pub struct Opening {
    name: String,
    input: Vec<u8>,
    rows: Box<dyn Rows>,
}

impl Opening {
    /// The view of `name` on `input`, its rows read as `T` (a row that does
    /// not decode is a domain this build does not match: not a state to
    /// draw).
    pub fn of<T: Encode + Send + 'static>(name: &str, input: Vec<u8>) -> Opening {
        Opening::read(name, input, decode::<T>)
    }

    /// The view of `name` on `input`, its rows read by `read`: an app's own
    /// type, or the bytes themselves (`<[u8]>::to_vec`) for a host that
    /// reads them another way.
    pub fn read<T: Send + 'static>(name: &str, input: Vec<u8>, read: fn(&[u8]) -> T) -> Opening {
        Opening {
            name: name.into(),
            input,
            rows: Box::new(Typed { rows: vec![], read }),
        }
    }

    /// The function the view is of.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its input's bytes.
    pub fn input(&self) -> &[u8] {
        &self.input
    }

    /// Its name, its input and its (empty) rows, for whatever holds it.
    pub fn into_parts(self) -> (String, Vec<u8>, Box<dyn Rows>) {
        (self.name, self.input, self.rows)
    }
}

/// What an app reads of the views held for it, by key: the port its update
/// and its screens read through.
pub trait Views<K: ViewKey> {
    /// The rows of the view `key` (a `Vec` of the type it was opened as),
    /// while it is held.
    fn rows_of(&self, key: &K) -> Option<&dyn Any>;
    /// Whether anything holds `key`.
    fn holds(&self, key: &K) -> bool;
    /// A number that grows whenever `key`'s rows moved; zero while nothing
    /// holds it.
    fn version(&self, key: &K) -> u64;
    /// Why the last change sent was refused, while it is the last.
    fn refusal(&self) -> Option<&str>;
    /// Where a file a row names can be read while this device holds it (a
    /// `file:` URL on a device with a disk, a `data:` one without); none
    /// while it is not here, and from a store that keeps no files.
    fn file_url(&self, _file: &FileRef) -> Option<String> {
        None
    }
}

/// Views read beside the files the device holds: [`Views::file_url`]
/// answered by `url` (a peer's `ark_peer::Peer::file_url`), everything
/// else by `views`. What a host hands an app's screen.
pub struct WithFiles<'a, K: ViewKey, F: Fn(&FileRef) -> Option<String>> {
    pub views: &'a dyn Views<K>,
    pub url: F,
}

impl<K: ViewKey, F: Fn(&FileRef) -> Option<String>> Views<K> for WithFiles<'_, K, F> {
    fn rows_of(&self, key: &K) -> Option<&dyn Any> {
        self.views.rows_of(key)
    }
    fn holds(&self, key: &K) -> bool {
        self.views.holds(key)
    }
    fn version(&self, key: &K) -> u64 {
        self.views.version(key)
    }
    fn refusal(&self) -> Option<&str> {
        self.views.refusal()
    }
    fn file_url(&self, file: &FileRef) -> Option<String> {
        (self.url)(file)
    }
}

impl<K: ViewKey> dyn Views<K> + '_ {
    /// The rows of the view `key`, as the type it was opened as; none while
    /// it is not held, or when they are not `T`.
    pub fn rows<T: 'static>(&self, key: &K) -> &[T] {
        self.rows_of(key).and_then(|r| r.downcast_ref::<Vec<T>>()).map_or(&[], |r| &r[..])
    }

    /// The first of a view's rows: a view of one row (a month's summary).
    pub fn first<T: 'static>(&self, key: &K) -> Option<&T> {
        self.rows(key).first()
    }
}
