//! A type's shape for an adapter to write its twin from (ui/docs/app.md,
//! Layers): what crosses to the phones is written in the adapter, never in
//! the domain's or the app's crate.
//!
//! `#[ark(ffi)]` on a row, an input or any `#[derive(Describe)]` type, or
//! `#[derive(Twin)]` on a plain one, writes beside the type each field's
//! type by position ([`Field`]) and a hidden macro that writes the twin.
//! The adapter writes them all with [`twins!`](crate::twins):
//!
//! ```ignore
//! ark::twins!(money_core: AccountLine, BudgetLine, Tone);
//! ```
//!
//! Neither the type's crate nor this one depends on UniFFI or the bridge.

pub use ark_macros::{twins, Twin};

/// The type of field `F` of variant `V` (a struct's fields are variant 0),
/// so a twin written in another crate names it without its path.
pub trait Field<const V: usize, const F: usize> {
    type Ty;
}

/// What crosses to the phones, found rather than listed: a type with a twin
/// says its own path and each of its fields' types ([`Reach`], written
/// beside its shape), and an app's manifest (`ark_app::manifest`) walks
/// from its screens' pages and messages and its route to every type their
/// twins hold, which its generated library twins (`twins!`).
pub trait Reach {
    /// This type's path and what its fields reach, into `r`.
    fn reach(r: &mut Reached);
}

/// The types reached so far, by their paths (`money_core::types::AccountLine`),
/// in the order they were found.
#[derive(Debug, Default)]
pub struct Reached {
    seen: std::collections::BTreeSet<&'static str>,
    paths: Vec<&'static str>,
}

impl Reached {
    /// `path` reached; whether it is new (so its fields are to be walked).
    pub fn add(&mut self, path: &'static str) -> bool {
        let new = self.seen.insert(path);
        if new {
            self.paths.push(path);
        }
        new
    }

    /// Every path reached, in the order found.
    pub fn paths(&self) -> &[&'static str] {
        &self.paths
    }
}

/// `$t` and what it reaches, into the [`Reached`] `$r`, where `$t` has a
/// twin; nothing for a type that has none (an `Id`, a `Cents`). A type
/// written out, never a parameter: whether it reaches is asked of the type
/// itself.
#[macro_export]
macro_rules! reach {
    ($r:expr, $t:ty) => {{
        #[allow(unused_imports)]
        use $crate::twin::{Reaches as _, Unreached as _};
        (&$crate::twin::Probe::<$t>::NEW).reach_into($r)
    }};
}

/// A field's type, asked whether it reaches anything: [`Reaches`] where it
/// does, [`Unreached`] (by autoref, so only where it does not) otherwise.
#[doc(hidden)]
pub struct Probe<T: ?Sized>(std::marker::PhantomData<fn() -> Box<T>>);

impl<T: ?Sized> Probe<T> {
    pub const NEW: Probe<T> = Probe(std::marker::PhantomData);
}

#[doc(hidden)]
pub trait Reaches {
    fn reach_into(&self, r: &mut Reached);
}

impl<T: Reach + ?Sized> Reaches for Probe<T> {
    fn reach_into(&self, r: &mut Reached) {
        T::reach(r)
    }
}

#[doc(hidden)]
pub trait Unreached {
    fn reach_into(&self, r: &mut Reached);
}

impl<T: ?Sized> Unreached for &Probe<T> {
    fn reach_into(&self, _: &mut Reached) {}
}

/// What holds a type reaches what it holds.
macro_rules! through {
    ($([$($t:ident),*] $ty:ty;)*) => {$(
        impl<$($t: Reach),*> Reach for $ty {
            fn reach(r: &mut Reached) {
                $($t::reach(r);)*
            }
        }
    )*};
}

through! {
    [T] Vec<T>;
    [T] Option<T>;
    [T] Box<T>;
    [T] std::sync::Arc<T>;
    [T] std::rc::Rc<T>;
    [T] std::collections::VecDeque<T>;
    [T] std::collections::BTreeSet<T>;
    [K, V] std::collections::BTreeMap<K, V>;
    [K, V] std::collections::HashMap<K, V>;
    [A, B] (A, B);
    [A, B, C] (A, B, C);
}

/// Types with no twin of their own reach nothing, so a tuple or a map of
/// them still reaches what is beside them.
macro_rules! leaves {
    ($($ty:ty),*) => {$(
        impl Reach for $ty {
            fn reach(_: &mut Reached) {}
        }
    )*};
}

leaves!(String, bool, char, u8, u16, u32, u64, i8, i16, i32, i64, f32, f64, usize, isize, ());

impl<T> Reach for crate::row::Id<T> {
    fn reach(_: &mut Reached) {}
}
