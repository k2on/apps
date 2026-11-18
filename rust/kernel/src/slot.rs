//! The slot arena, the one module of the kernel with `unsafe` in it: each
//! slot holds a value of any type behind a pointer, with a vtable to
//! compare and drop it and a version, so the engine over it is one
//! monomorphic walk and each type costs only its two small functions.
//! [`crate::cell`] puts typed handles over it; nothing else touches it.
//!
//! - A value is boxed once, when it is made ([`Boxed::new`]); a write
//!   ([`Slots::swap`]) compares the newcomer with what the slot holds
//!   through the vtable and either drops it (equal: nothing changed) or
//!   swaps the pointers and bumps the version.
//! - What a swap replaced is **retired**, not dropped: it stays readable,
//!   as the value before the settle ([`Slots::was`]), and so that no
//!   reference handed out under `&self` is left dangling. [`Slots::sweep`]
//!   drops them, under `&mut self`, once the settle has ended.
//! - A read names its type ([`Slots::get`]); a slot of another type reads
//!   as nothing, so a handle can never read a value as what it is not.
//!
//! Its tests run under Miri on the host as well as on the watch: with a
//! nightly that has Miri (none is in the flake or CI; a one-off from the
//! flake's rust-overlay, `rust-bin.selectLatestNightlyWith (t:
//! t.minimal.override { extensions = [ "miri" "rust-src" ]; })`), `cargo
//! miri test -p ark-kernel slot`, and with `MIRIFLAGS=-Zmiri-tree-borrows`.
//! The whole suite runs clean too, its property tests at a few seeds
//! (`ARK_CELLS=3 ARK_PLANS=3 ARK_SEEDS=1 ARK_STEPS=30`, about 16 minutes),
//! but for the convergence test's closing assertions that each of its
//! paths was taken, which so few steps cannot meet.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::any::TypeId;
use core::cell::{Cell, RefCell};

/// What the engine knows of a value's type: how to compare two of them,
/// how to drop one, and which type it is.
pub(crate) struct VTable {
    eq: unsafe fn(*const (), *const ()) -> bool,
    drop: unsafe fn(*mut ()),
    ty: fn() -> TypeId,
}

/// # Safety
/// Both pointers are live `Box<T>`s' (`Boxed::new::<T>`'s).
unsafe fn eq<T: PartialEq>(a: *const (), b: *const ()) -> bool {
    unsafe { *(a as *const T) == *(b as *const T) }
}

/// # Safety
/// The pointer is a live `Box<T>`'s, dropped once.
unsafe fn drop_box<T>(p: *mut ()) {
    drop(unsafe { Box::from_raw(p as *mut T) });
}

/// The vtable of `T`.
trait Typed {
    const VT: VTable;
}

impl<T: PartialEq + 'static> Typed for T {
    const VT: VTable = VTable {
        eq: eq::<T>,
        drop: drop_box::<T>,
        ty: TypeId::of::<T>,
    };
}

/// A value of some type, boxed, with its vtable: owned, and dropped through
/// it.
pub(crate) struct Boxed {
    ptr: *mut (),
    vt: &'static VTable,
}

impl Boxed {
    pub(crate) fn new<T: PartialEq + 'static>(v: T) -> Boxed {
        Boxed {
            ptr: Box::into_raw(Box::new(v)) as *mut (),
            vt: &<T as Typed>::VT,
        }
    }

    /// The value, if it is a `T`.
    pub(crate) fn get<T: 'static>(&self) -> Option<&T> {
        // SAFETY: the pointer is a live box of the type `vt` names.
        ((self.vt.ty)() == TypeId::of::<T>()).then(|| unsafe { &*(self.ptr as *const T) })
    }
}

impl Drop for Boxed {
    fn drop(&mut self) {
        // SAFETY: owned, and dropped once, through its own type's vtable.
        unsafe { (self.vt.drop)(self.ptr) }
    }
}

/// One slot: a value (none for a watch's number, whose answer the node
/// keeps apart) and its version, bumped by every swap that changed it.
struct Slot {
    value: Cell<Option<Boxed>>,
    version: Cell<u32>,
}

/// The arena: slots by number, and the values swaps replaced since the
/// last sweep.
#[derive(Default)]
pub(crate) struct Slots {
    slots: Vec<Slot>,
    retired: RefCell<Vec<(u32, Option<Boxed>)>>,
}

impl Slots {
    /// A new slot holding `v`: its number.
    pub(crate) fn add(&mut self, v: Option<Boxed>) -> u32 {
        self.slots.push(Slot {
            value: Cell::new(v),
            version: Cell::new(0),
        });
        self.slots.len() as u32 - 1
    }

    /// The value slot `id` holds, or (`was`) held before the first swap
    /// since the last sweep: what it was before the settle.
    fn held(&self, id: u32, was: bool) -> Option<(*mut (), &'static VTable)> {
        let raw = |b: &Option<Boxed>| b.as_ref().map(|b| (b.ptr, b.vt));
        if was {
            if let Some((_, b)) = self.retired.borrow().iter().find(|r| r.0 == id) {
                return raw(b);
            }
        }
        // SAFETY: read in place, the box not moved; see `get`.
        raw(unsafe { &*self.slots.get(id as usize)?.value.as_ptr() })
    }

    /// The value slot `id` holds (or, `was`, held before the settle), if it
    /// is a `T`.
    pub(crate) fn get<T: 'static>(&self, id: u32, was: bool) -> Option<&T> {
        // SAFETY: a box is only taken out of its slot by `swap`, which
        // retires it rather than dropping it, so it lives until `sweep`,
        // which takes `&mut self` and so outlives every `&T` given out; the
        // retired list may move its entries, never the boxes they own. The
        // pointer is to a `T`, as its vtable says.
        self.typed(id, was, TypeId::of::<T>()).map(|p| unsafe { &*(p as *const T) })
    }

    /// The pointer to the value `held` finds, if it is of type `ty`: one
    /// copy, whatever the type.
    fn typed(&self, id: u32, was: bool, ty: TypeId) -> Option<*mut ()> {
        let (ptr, vt) = self.held(id, was)?;
        ((vt.ty)() == ty).then_some(ptr)
    }

    /// Whether two values are of one type and equal.
    fn same(a: Option<(*mut (), &'static VTable)>, b: Option<(*mut (), &'static VTable)>) -> bool {
        match (a, b) {
            // SAFETY: both live boxes of the type their vtables name,
            // checked to be the same.
            (Some(a), Some(b)) => (a.1.ty)() == (b.1.ty)() && unsafe { (a.1.eq)(a.0, b.0) },
            (a, b) => a.is_none() && b.is_none(),
        }
    }

    /// Puts `new` in slot `id` unless it equals what is there: whether it
    /// changed, the version bumped and the old value retired. An equal
    /// newcomer, or one of another type, is dropped.
    pub(crate) fn swap(&self, id: u32, new: Boxed) -> bool {
        let Some(slot) = self.slots.get(id as usize) else { return false };
        // An empty slot is a watch's, which holds no value.
        let Some(held) = self.held(id, false) else { return false };
        if (held.1.ty)() != (new.vt.ty)() || Self::same(Some(held), Some((new.ptr, new.vt))) {
            return false;
        }
        let old = slot.value.replace(Some(new));
        self.retired.borrow_mut().push((id, old));
        slot.version.set(slot.version.get().wrapping_add(1));
        true
    }

    /// Whether slot `id` holds other than it did at the last sweep: some
    /// writes since may have put it back.
    pub(crate) fn changed(&self, id: u32) -> bool {
        !Self::same(self.held(id, true), self.held(id, false))
    }

    /// Slot `id`'s version: how many swaps changed it.
    pub(crate) fn version(&self, id: u32) -> u32 {
        self.slots.get(id as usize).map_or(0, |s| s.version.get())
    }

    /// Drops what the swaps since the last one replaced.
    pub(crate) fn sweep(&mut self) {
        self.retired.get_mut().clear();
    }
}

#[cfg(any(test, ark_selftest))]
pub(crate) mod tests {
    use super::*;
    use crate::Rng;
    use alloc::rc::Rc;
    use alloc::string::String;

    /// A value that counts its drops.
    #[derive(Clone)]
    struct Counted(u8, Rc<Cell<u32>>);

    impl PartialEq for Counted {
        fn eq(&self, other: &Counted) -> bool {
            self.0 == other.0
        }
    }

    impl Drop for Counted {
        fn drop(&mut self) {
            self.1.set(self.1.get() + 1);
        }
    }

    /// Random swaps of values of several types into several slots, reads
    /// of them as the right and the wrong type, and sweeps: every value is
    /// what was last put, `was` what was there at the last sweep, and every
    /// box made is dropped exactly once. Run under Miri, this is what the
    /// unsafe core is held to.
    #[cfg_attr(test, test)]
    fn slots_swap_retire_and_drop_once() {
        let drops = Rc::new(Cell::new(0));
        let mut made = 0u32;
        {
            let mut slots = Slots::default();
            let mut rng = Rng::new(7);
            let mut now: Vec<Option<u8>> = Vec::new();
            let mut swept: Vec<Option<u8>> = Vec::new();
            for i in 0..6 {
                if i % 3 == 2 {
                    slots.add(Some(Boxed::new(String::from("words"))));
                    now.push(None);
                } else {
                    let v = (i % 2 == 0).then(|| {
                        made += 1;
                        Boxed::new(Counted(i, drops.clone()))
                    });
                    slots.add(v);
                    now.push((i % 2 == 0).then_some(i));
                }
                swept.push(now[i as usize]);
            }
            for _ in 0..if cfg!(miri) { 60 } else { 2000 } {
                let id = rng.below(6) as u32;
                match rng.below(5) {
                    0 => {
                        slots.sweep();
                        swept.clone_from(&now);
                    }
                    // Another type, or into an empty slot (a watch's):
                    // refused, dropped.
                    1 => {
                        assert!(!slots.swap(id, Boxed::new(String::from("not a count"))) || id % 3 == 2);
                    }
                    _ if id % 3 != 2 => {
                        let v = rng.below(3) as u8;
                        made += 1;
                        let version = slots.version(id);
                        let changed = slots.swap(id, Boxed::new(Counted(v, drops.clone())));
                        let held = now[id as usize];
                        assert_eq!(changed, held.is_some_and(|h| h != v));
                        assert_eq!(slots.version(id), version.wrapping_add(u32::from(changed)));
                        if held.is_some() {
                            now[id as usize] = Some(v);
                        }
                    }
                    _ => {}
                }
                let held = slots.get::<Counted>(id, false).map(|c| c.0);
                if id % 3 == 2 {
                    assert_eq!(held, None);
                    assert!(slots.get::<String>(id, false).is_some());
                } else {
                    assert_eq!(held, now[id as usize]);
                    assert_eq!(slots.get::<Counted>(id, true).map(|c| c.0), swept[id as usize]);
                    assert!(slots.get::<String>(id, false).is_none());
                }
            }
        }
        assert_eq!(drops.get(), made);
    }

    crate::listed!(slots_swap_retire_and_drop_once);
}
