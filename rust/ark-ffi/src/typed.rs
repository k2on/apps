//! Typed views through the bridge (`docs/plan-v5.md`, Landed: typed
//! views): a view's rows as a domain's own type, a UniFFI record, so that a
//! phone reads them with no JSON. A view is still made by name with its
//! input's JSON (once); its rows, at every patch, cross as their bytes (the
//! value format) decoded in Rust, never as JSON.
//!
//! UniFFI has no generics, so a row type crosses as its **twin**: a record
//! or enum `ark-macros` writes beside it (`#[ark(ffi_view)]`), each field its
//! own type's twin ([`Twin`]), named in Kotlin and Swift as the type is. A
//! scalar, `String`, `Option` and `Vec` are their own twins; an id is a
//! [`RowId`] (its hex, the apps' `Id`). A view of rows of `T` is an object
//! of `T`'s own (`<T>View`, over [`Typed`]): its rows, and what it did as a
//! [`Taken`] whose inserted and updated rows come beside it, in order.
//! Mutations, queries and the live space stay called by name with JSON.

use std::sync::Arc;

use ark::format::Encode;
use ark::row::Describe;

use crate::{Peer, Refusal, ViewHandle};

/// A type and the record or enum it crosses the bridge as. `UT` is the
/// crate whose UniFFI scaffolding the twin is in (UniFFI's tag).
pub trait Twin<UT>: Sized {
    type Ffi;
    fn ffi(self) -> Self::Ffi;
    fn from_ffi(f: Self::Ffi) -> Self;
}

macro_rules! itself {
    ($($t:ty),*) => {$(
        impl<UT> Twin<UT> for $t {
            type Ffi = $t;
            fn ffi(self) -> $t {
                self
            }
            fn from_ffi(f: $t) -> $t {
                f
            }
        }
    )*};
}
itself!(i64, u64, i32, u32, u8, bool, String);

impl<UT, T: Twin<UT>> Twin<UT> for Option<T> {
    type Ffi = Option<T::Ffi>;
    fn ffi(self) -> Self::Ffi {
        self.map(T::ffi)
    }
    fn from_ffi(f: Self::Ffi) -> Self {
        f.map(T::from_ffi)
    }
}

impl<UT, T: Twin<UT>> Twin<UT> for Vec<T> {
    type Ffi = Vec<T::Ffi>;
    fn ffi(self) -> Self::Ffi {
        self.into_iter().map(T::ffi).collect()
    }
    fn from_ffi(f: Self::Ffi) -> Self {
        f.into_iter().map(T::from_ffi).collect()
    }
}

/// A row id across the bridge: its sixteen bytes, as their hex (the apps'
/// `Id`, by `uniffi.toml`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowId(pub [u8; 16]);

uniffi::custom_type!(RowId, String, {
    lower: |r| ark::hash::hex(&r.0),
    try_lift: |s| ark::hash::unhex(&s)
        .and_then(|b| b.try_into().ok())
        .map(RowId)
        .ok_or_else(|| uniffi::deps::anyhow::anyhow!("an id is 32 hex digits: {s}")),
});

impl<UT, R> Twin<UT> for ark::row::Id<R> {
    type Ffi = RowId;
    fn ffi(self) -> RowId {
        RowId(self.0)
    }
    fn from_ffi(f: RowId) -> Self {
        ark::row::Id::new(f.0)
    }
}

/// One change to a typed view's rows; an insert's and an update's row is
/// the next of the rows taken with it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Op {
    Insert { at: u64 },
    Remove { at: u64 },
    Update { at: u64 },
}

/// What a typed view did since it was last taken: its rows taken with it
/// are, for `Patched`, each insert's and update's in order, and for
/// `Reset`, all of them.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Taken {
    Unchanged,
    Patched { ops: Vec<Op> },
    Reset,
}

/// A view of rows of `T`, read as `T`: what a domain's `<T>View` object is.
pub struct Typed<T> {
    handle: Arc<ViewHandle>,
    _t: std::marker::PhantomData<fn() -> T>,
}

fn decode<T: Encode>(b: &[u8]) -> T {
    Encode::from_bytes(b).expect("a view's row is its type's encoding")
}

impl<T: Encode> Typed<T> {
    pub fn rows(&self) -> Vec<T> {
        self.handle.rows_bytes().iter().map(|b| decode(b)).collect()
    }

    pub fn take(&self) -> (Taken, Vec<T>) {
        let (taken, rows) = self.handle.take_bytes();
        (taken, rows.iter().map(|b| decode(b)).collect())
    }
}

impl Peer {
    /// A maintained view of `name` on its input's JSON, its rows read as
    /// `T`; refused when `name`'s rows are not `T`.
    pub fn view_typed<T: Describe>(&self, name: &str, input: &str) -> Result<Typed<T>, Refusal> {
        let args = {
            let s = self.shared.lock();
            let f = s.peer.binding(name).and_then(|b| s.peer.domain().get(&b).map(|f| f.output.clone()));
            if f.as_ref().is_some_and(|o| *o != Some(T::describe())) {
                return Err(format!("{name}'s rows are not {}", std::any::type_name::<T>()).into());
            }
            s.peer.input_json(name, input)?
        };
        Ok(Typed {
            handle: self.view_bytes(name.into(), args)?,
            _t: std::marker::PhantomData,
        })
    }
}
