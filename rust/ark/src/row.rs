//! §1.16 A domain's values in Rust: rows, inputs, ids, indexes, and the
//! type description every one of them carries.
//!
//! A [`Value`] is what a domain stores or takes: it has one canonical
//! encoding ([`Value::encode`]) and reads back from it alone. A [`Row`] is
//! a value that is a table's row (`#[derive(Row)]`): its table, its key,
//! its indexes as typed handles ([`Index`]) and the schema's [`Table`] it
//! is. An [`Input`] is a value a function takes (`#[derive(Input)]`), whose
//! refinements — trims and checks, each with the words a form shows — are
//! [`Input::check`]. A [`Describe`] type says what it is as a [`Type`]: what
//! the schema's columns, a form's fields and the explorer read.
//!
//! **Keys are bytes that compare as the values do** ([`Key`]): a row's key,
//! an index entry's columns and a prefix of them encode to bytes whose
//! `memcmp` order is the values' order, and no encoding is a proper prefix
//! of another's, so a prefix of the columns is a prefix of the bytes.
//!
//! A prefix is typed by the index it reads ([`Lead`]): an index over
//! `(Id<Playlist>, i64)` reads from an `Id<Playlist>`, or from both, and
//! anything else is a compile error.

use std::borrow::Borrow;
use std::fmt;
use std::marker::PhantomData;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::format::Encode;
use crate::hash::{EntryHash, StateHash};
use crate::log::{Entry, EntryId, Logged, Origin};
use crate::schema::{Column, Table};
use crate::store::Change;
use crate::Refusal;

/// A value a domain stores, takes or answers: its bytes in the value
/// format ([`crate::format`]), read back from them alone.
pub trait Value: Serialize + DeserializeOwned + Clone + fmt::Debug + PartialEq + Send + Sync + 'static {
    fn encode(&self) -> Vec<u8> {
        self.to_bytes()
    }
    fn decode(b: &[u8]) -> Result<Self, Refusal> {
        Self::from_bytes(b)
    }
}

impl<T: Serialize + DeserializeOwned + Clone + fmt::Debug + PartialEq + Send + Sync + 'static> Value for T {}

/// A part of a key: bytes in its values' order, prefix-free.
pub trait Key {
    fn put_key(&self, out: &mut Vec<u8>);
    fn key(&self) -> Vec<u8> {
        let mut out = vec![];
        self.put_key(&mut out);
        out
    }
}

impl<T: Serialize + ?Sized> Key for T {
    fn put_key(&self, out: &mut Vec<u8>) {
        crate::format::put_key(self, out)
    }
}

// * What a type is

/// What a type is, as a schema's column, a form's field and the explorer
/// read it. A record and a sum carry their names and their fields'.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Type {
    Int,
    Text,
    Bool,
    Bytes,
    Unit,
    /// A row id of the named table.
    Id(String),
    Option(Box<Type>),
    List(Box<Type>),
    Tuple(Vec<Type>),
    Record(String, Vec<(String, Type)>),
    Sum(String, Vec<(String, Vec<(String, Type)>)>),
}

impl Type {
    /// The type's name as a column declares it: `Text`, `Id<playlist>`,
    /// `Option<Text>`, a record's or a sum's own name.
    pub fn decl(&self) -> String {
        match self {
            Type::Int => "Int".into(),
            Type::Text => "Text".into(),
            Type::Bool => "Bool".into(),
            Type::Bytes => "Bytes".into(),
            Type::Unit => "()".into(),
            Type::Id(t) => format!("Id<{t}>"),
            Type::Option(t) => format!("Option<{}>", t.decl()),
            Type::List(t) => format!("List<{}>", t.decl()),
            Type::Tuple(ts) => format!("({})", ts.iter().map(Type::decl).collect::<Vec<_>>().join(", ")),
            Type::Record(n, _) | Type::Sum(n, _) => n.clone(),
        }
    }

    /// Whether this is `Option<T>`, which is what a column appended to a
    /// table may be without a default (§1.2).
    pub fn is_option(&self) -> bool {
        matches!(self, Type::Option(_))
    }
}

/// A type that says what it is.
pub trait Describe {
    fn describe() -> Type;
}

macro_rules! leaf {
    ($($t:ty => $v:expr),* $(,)?) => {$(
        impl Describe for $t {
            fn describe() -> Type {
                $v
            }
        }
    )*};
}
leaf!(i64 => Type::Int, String => Type::Text, bool => Type::Bool, () => Type::Unit);

impl<T: Describe> Describe for Option<T> {
    fn describe() -> Type {
        Type::Option(Box::new(T::describe()))
    }
}

impl<T: Describe> Describe for Vec<T> {
    fn describe() -> Type {
        Type::List(Box::new(T::describe()))
    }
}

impl<T: Describe> Describe for Box<T> {
    fn describe() -> Type {
        T::describe()
    }
}

macro_rules! tuples {
    ($(($($t:ident),+))*) => {$(
        impl<$($t: Describe),+> Describe for ($($t,)+) {
            fn describe() -> Type {
                Type::Tuple(vec![$($t::describe()),+])
            }
        }
    )*};
}
tuples!((A)(A, B)(A, B, C)(A, B, C, D)(A, B, C, D, E));

/// Bytes as a column, `Type::Bytes`: a row's encoding kept as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Bytes(pub Vec<u8>);

// The log's own values as columns (what a peer keeps beside its log): an
// origin and an entry's id are sixteen bytes as an `Id` is (hex in JSON and
// in a key); a hash is bytes, hex in JSON, which `json`'s `Bytes` does not
// read: no schema holding one is read through JSON.
leaf!(Bytes => Type::Bytes, Origin => Type::Id("origin".into()), EntryId => Type::Id("entry".into()), EntryHash => Type::Bytes, StateHash => Type::Bytes);

macro_rules! fields {
    ($($f:ident: $t:expr),*) => { vec![$((stringify!($f).to_string(), $t)),*] };
}

impl Describe for Entry {
    fn describe() -> Type {
        use Type::{Bytes as B, Int, Text};
        let (id, origin, roles) = (EntryId::describe(), Origin::describe(), Vec::<String>::describe());
        let fs = fields! { id: id, actor: Text, session: Text, origin: origin, local: Int, roles: roles, func: Text, input: B, now: Int };
        Type::Record("Entry".into(), fs)
    }
}

impl Describe for Change {
    fn describe() -> Type {
        use Type::{Bytes as B, Text};
        let (one, two) = (fields! { table: Text, row: B }, fields! { table: Text, old: B, new: B });
        Type::Sum(
            "Change".into(),
            vec![("add".into(), one.clone()), ("remove".into(), one), ("edit".into(), two)],
        )
    }
}

impl Describe for Logged {
    fn describe() -> Type {
        let (entry, facts, b) = (Entry::describe(), Vec::<Change>::describe(), Type::Bytes);
        Type::Record(
            "Logged".into(),
            fields! { seq: Type::Int, entry: entry, facts: facts, state: b.clone(), hash: b },
        )
    }
}

// * Ids

/// An id of a row of `R`'s table: sixteen bytes, the `n`-th an entry drew
/// (`Ctx::new_id`, §1.6).
pub struct Id<R>(pub [u8; 16], PhantomData<fn() -> R>);

impl<R> Id<R> {
    pub const fn new(b: [u8; 16]) -> Id<R> {
        Id(b, PhantomData)
    }

    /// The same bytes, as another table's id.
    pub fn cast<T>(self) -> Id<T> {
        Id::new(self.0)
    }

    /// The id as text: its bytes in hex.
    pub fn hex(&self) -> String {
        crate::hash::hex(&self.0)
    }
}
impl<R> Clone for Id<R> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<R> Copy for Id<R> {}
impl<R> Default for Id<R> {
    fn default() -> Self {
        Id::new([0; 16])
    }
}
impl<R> PartialEq for Id<R> {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0
    }
}
impl<R> Eq for Id<R> {}
impl<R> PartialOrd for Id<R> {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl<R> Ord for Id<R> {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.0.cmp(&o.0)
    }
}
impl<R> std::hash::Hash for Id<R> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.0.hash(h)
    }
}
impl<R> fmt::Debug for Id<R> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Id({})", self.hex())
    }
}
impl<R> Serialize for Id<R> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        crate::format::hex_bytes::serialize(&self.0, s)
    }
}
impl<'de, R> Deserialize<'de> for Id<R> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Id<R>, D::Error> {
        crate::format::hex_bytes::deserialize(d).map(Id::new)
    }
}
impl<R: Row> Describe for Id<R> {
    fn describe() -> Type {
        Type::Id(R::TABLE.into())
    }
}

// * Rows and their indexes

/// A table's row (`#[derive(Row)]`): its table's name, its key — the
/// leading fields marked `#[ark(key)]` — and the schema's table it is.
pub trait Row: Value + Describe {
    const TABLE: &'static str;
    /// The key's fields, as a tuple (one field: a one-tuple).
    type Key: Value;
    /// Its space as a type (`#[ark(volatile)]`): which runs write it
    /// ([`crate::db`]).
    type Space: crate::db::Writes;
    fn key(&self) -> Self::Key;
    /// The table: its columns (the fields, in order), key, indexes,
    /// references and space.
    fn table() -> Table;
    /// The key, as the store holds it.
    fn key_bytes(&self) -> Vec<u8> {
        let mut out = vec![];
        crate::format::put_key(&self.key(), &mut out);
        out
    }
}

/// A typed read path over `R`'s rows: its key (`None`) or its `n`-th
/// declared index, whose columns are the tuple `C`.
pub struct Index<R, C> {
    pub at: Option<usize>,
    _p: PhantomData<fn() -> (R, C)>,
}

impl<R, C> Index<R, C> {
    pub const fn key() -> Index<R, C> {
        Index { at: None, _p: PhantomData }
    }
    pub const fn nth(n: usize) -> Index<R, C> {
        Index {
            at: Some(n),
            _p: PhantomData,
        }
    }
}
impl<R, C> Clone for Index<R, C> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<R, C> Copy for Index<R, C> {}

/// What a prefix of an index's columns `C` may be read from (`M` tells the
/// impls apart): its first column, by value or by reference (`&str` for a
/// `String`), or its first two.
pub trait Lead<Q, M> {
    fn put(q: &Q, out: &mut Vec<u8>);
}

/// The markers [`Lead`]'s impls are told apart by.
pub mod lead {
    pub struct First;
    pub struct FirstRef;
    pub struct Two;
    pub struct Three;
}

macro_rules! leads {
    ($(($a:ident $(, $t:ident)*))*) => {$(
        impl<$a: Key $(, $t)*> Lead<$a, lead::First> for ($a, $($t,)*) {
            fn put(q: &$a, out: &mut Vec<u8>) {
                q.put_key(out)
            }
        }
        impl<'q, Q: Key + ?Sized, $a: Borrow<Q> $(, $t)*> Lead<&'q Q, lead::FirstRef> for ($a, $($t,)*) {
            fn put(q: &&'q Q, out: &mut Vec<u8>) {
                q.put_key(out)
            }
        }
    )*};
}
leads!((A)(A, B)(A, B, C)(A, B, C, D));

impl<A: Key, B: Key> Lead<(A, B), lead::Two> for (A, B) {
    fn put(q: &(A, B), out: &mut Vec<u8>) {
        q.0.put_key(out);
        q.1.put_key(out);
    }
}
impl<A: Key, B: Key, C> Lead<(A, B), lead::Two> for (A, B, C) {
    fn put(q: &(A, B), out: &mut Vec<u8>) {
        q.0.put_key(out);
        q.1.put_key(out);
    }
}
impl<A: Key, B: Key, C: Key> Lead<(A, B, C), lead::Three> for (A, B, C) {
    fn put(q: &(A, B, C), out: &mut Vec<u8>) {
        q.0.put_key(out);
        q.1.put_key(out);
        q.2.put_key(out);
    }
}

/// A column of a row being derived: what `#[derive(Row)]` builds a
/// table's columns from.
pub fn column<T: Describe>(name: &str) -> Column {
    Column {
        name: name.into(),
        ty: T::describe(),
        default: None,
    }
}

/// A column appended with `#[serde(default)]`: a row written before it
/// existed takes its type's default.
pub fn column_or_default<T: Describe + Default + Serialize>(name: &str) -> Column {
    Column {
        default: Some(serde_json::to_value(T::default()).expect("a default serializes")),
        ..column::<T>(name)
    }
}

// * Inputs

/// What a function takes (`#[derive(Input)]`): its refinements applied —
/// a trim normalises, a check refuses with its words — field by field, in
/// order. Every failing field is said, by name, with its words; the first
/// is what a refused intent says.
pub trait Input: Value + Describe {
    fn check(self) -> Result<Self, Vec<(String, String)>>;

    /// Each refined field's refinements, as a form says them beside it.
    fn checks() -> Vec<(String, Vec<String>)> {
        vec![]
    }

    /// [`Input::check`], the first failure its refusal.
    fn refine(self) -> Result<Self, Refusal> {
        self.check().map_err(|mut fs| Refusal(fs.remove(0).1))
    }
}

impl Input for () {
    fn check(self) -> Result<(), Vec<(String, String)>> {
        Ok(())
    }
}

/// How long a refined value is: a text's characters, a list's elements.
pub trait Length {
    fn length(&self) -> usize;
    /// What one of them is called, for a refusal's words.
    fn unit(&self) -> &'static str;
}

impl Length for String {
    fn length(&self) -> usize {
        self.chars().count()
    }
    fn unit(&self) -> &'static str {
        "character"
    }
}

impl<T> Length for Vec<T> {
    fn length(&self) -> usize {
        self.len()
    }
    fn unit(&self) -> &'static str {
        "element"
    }
}

/// What a trim does: the text without the whitespace at either end.
pub trait Trim {
    fn trimmed(self) -> Self;
}

impl Trim for String {
    fn trimmed(self) -> String {
        let t = self.trim();
        if t.len() == self.len() {
            self
        } else {
            t.to_string()
        }
    }
}

impl<T: Trim> Trim for Option<T> {
    fn trimmed(self) -> Self {
        self.map(Trim::trimmed)
    }
}

fn many(n: usize, unit: &str) -> String {
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

/// A length within `lo..=hi`, or the words for why not.
pub fn within<T: Length + ?Sized>(v: &T, lo: usize, hi: usize) -> Option<String> {
    let n = v.length();
    if n < lo {
        Some(format!("at least {}", many(lo, v.unit())))
    } else if n > hi {
        Some(format!("at most {}", many(hi, v.unit())))
    } else {
        None
    }
}

/// Something, or the words for why not.
pub fn non_empty<T: Length + ?Sized>(v: &T) -> Option<String> {
    (v.length() == 0).then(|| "must not be empty".into())
}
