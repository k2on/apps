//! The two things the vocabulary needs that safe Rust cannot say, and
//! nothing else.
//!
//! **A struct of the vocabulary's values, built and taken apart by
//! position.** A domain declares its rows, scopes and inputs as ordinary
//! structs with no derive (`spec/AUTHORING.md` §2.5), and the builder has to
//! hand a body a `Playlist` it never saw constructed and read back the one
//! a body wrote. Swift and Kotlin reflect; Rust cannot, so every value type
//! of the vocabulary is one [`H`] (`#[repr(transparent)]` over a `u32`) and
//! a row, a scope or an input is read as the array of its fields' handles
//! in declaration order. What makes that sound, and what is checked:
//!
//! - the size of the struct is exactly `n` handles (checked on every call,
//!   with the column or field count the declaration gives), and its
//!   alignment is at most a handle's;
//! - every bit pattern of a handle is a valid handle, so whatever the
//!   field order, no invalid value is ever made;
//! - the struct has no field that is not a value of the vocabulary — the
//!   one requirement not checked, and the one §2.5 states.
//!
//! The order is rustc's field order for a struct whose fields all have one
//! size and one alignment, which is declaration order; if it ever were not,
//! a field would read a sibling's handle — a wrong program, never an unsafe
//! one — and the demo's byte vector and the agreement tests would say so.
//!
//! **The store a `Native` body reads and writes**, borrowed for the extent
//! of one call and reached from the thread's context: a scoped pointer, set
//! and cleared around the call and never reachable from outside this file.

#![allow(unsafe_code)]

use std::cell::Cell;
use std::mem::{align_of, size_of};
use std::ptr::NonNull;

use crate::store::Store;

use super::cx::H;

/// A value of `T` whose fields are these handles, in declaration order.
///
/// # Panics
///
/// If `T` is not exactly `hs.len()` handles.
pub(crate) fn assemble<T>(hs: &[H], what: &str) -> T {
    assert!(
        size_of::<T>() == hs.len() * size_of::<H>() && align_of::<T>() <= align_of::<H>(),
        "{what}: {} is {} bytes, not {} values of the vocabulary; a row, a scope or an input is a struct of the vocabulary's values and nothing else, one per declared column or field",
        std::any::type_name::<T>(),
        size_of::<T>(),
        hs.len()
    );
    if hs.is_empty() {
        // SAFETY: `T` is zero-sized, so reading it reads nothing, and a
        // dangling non-null pointer is aligned for it.
        return unsafe { std::ptr::read(NonNull::<T>::dangling().as_ptr()) };
    }
    // SAFETY: the source is `size_of::<T>()` initialised bytes (checked
    // above), read unaligned; every field of `T` is a handle and every bit
    // pattern of a handle is valid.
    unsafe { std::ptr::read_unaligned(hs.as_ptr() as *const T) }
}

/// The handles of a value's fields, in declaration order.
///
/// # Panics
///
/// If `T` is not exactly `n` handles.
pub(crate) fn disassemble<T>(t: &T, n: usize, what: &str) -> Vec<H> {
    assert!(
        size_of::<T>() == n * size_of::<H>(),
        "{what}: {} is {} bytes, not {n} values of the vocabulary",
        std::any::type_name::<T>(),
        size_of::<T>()
    );
    let p = t as *const T as *const H;
    // SAFETY: `t` is `n` handles long (checked above) and borrowed for the
    // duration; each read is of initialised memory, unaligned-safe.
    (0..n).map(|i| unsafe { p.add(i).read_unaligned() }).collect()
}

thread_local! {
    static STORE: Cell<Option<*mut (dyn Store + 'static)>> = const { Cell::new(None) };
}

struct Restore(Option<*mut (dyn Store + 'static)>);

impl Drop for Restore {
    fn drop(&mut self) {
        STORE.with(|s| s.set(self.0));
    }
}

/// Run `f` with `st` as the store [`store`] reaches, and restore whatever
/// was there before on the way out, unwinding included.
pub(crate) fn with_store<R>(st: &mut dyn Store, f: impl FnOnce() -> R) -> R {
    let p: *mut (dyn Store + '_) = st;
    // SAFETY: only the lifetime is erased. The pointer is reachable only
    // through `store`, only on this thread, and only until `Restore` runs
    // at the end of this call, while `st` is mutably borrowed by it.
    let p: *mut (dyn Store + 'static) = unsafe { std::mem::transmute(p) };
    let prev = STORE.with(|s| s.replace(Some(p)));
    let _restore = Restore(prev);
    f()
}

/// The store of the call in progress.
///
/// # Panics
///
/// Outside [`with_store`].
pub(crate) fn store<R>(f: impl FnOnce(&mut dyn Store) -> R) -> R {
    let p = STORE.with(|s| s.get()).expect("a Native read or write outside a procedure run");
    // SAFETY: `p` is live (see `with_store`), and no other reference to it
    // exists while `f` runs: `f` is this crate's and never re-enters.
    unsafe { f(&mut *p) }
}
