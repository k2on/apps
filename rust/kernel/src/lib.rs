//! The new kernel, as `docs/north-star.md` describes it: one message, the
//! [`Entry`], in one of four [`Space`]s; its own compact [`codec`], where
//! every id and name is an integer; the [`Store`] a host gives it, with an
//! in-memory one ([`Memory`]) and an [`Overlay`] that a step's writes go
//! into; and one loop, [`Node::step`], which runs an entry's handler on its
//! space's store and returns the entries it made, each routed by its space
//! ([`Route`]). For the shared space, a node keeps its own entries pending
//! over the confirmed rows and rebases them on the facts the [`Authority`]
//! logs, speaking to its upstream in [`sync`]'s frames; the session space's
//! live state, a playback position an [`Anchor`] each view extrapolates,
//! is stepped on every node and never logged: each row has an owner, its
//! controls are delivered only over a link that is up, and every hello
//! heals what diverged. A node estimates the tree's clock from its hellos,
//! and anchors are written and read in it. Queries are
//! [`plan`]s of incremental operators, pulled from the store in order and
//! pushed the changes a node's view takes ([`Node::take_changes`]). An
//! upstream serves its downstream nodes with a [`Serve`], keeping per
//! client the record of what it holds: the server's authority does, and a
//! phone's [`Relay`] serves its watch from the phone's own view. Neither is
//! linked on a watch: they, and the authority, are the `upstream` feature's,
//! which a watch's build never enables.
//!
//! It is `no_std` with `alloc`: no I/O, no threads, no clock and no random
//! numbers. Time comes in on the entries and ids are counted, so the same
//! entries give the same rows on a server, a phone and a watch.
//! `checks.kernel-size` holds its code to the watch's budget of 48 KB, and
//! `checks.kernel-qemu` runs these tests on a Cortex-M3, as a Pebble's.
//!
//! The old kernel (`rust/ark`) runs the apps until each moves here.

#![no_std]

extern crate alloc;

pub mod anchor;
#[cfg(feature = "upstream")]
pub mod authority;
pub mod codec;
pub mod entry;
pub mod node;
pub mod plan;
#[cfg(feature = "upstream")]
pub mod relay;
#[cfg(feature = "upstream")]
pub mod serve;
pub mod store;
pub mod sync;

pub use anchor::Anchor;
#[cfg(feature = "upstream")]
pub use authority::Authority;
pub use codec::{Decode, Encode, Frames};
pub use entry::{Actor, Entry, EntryId, Hlc, Origin, Seq, Space, Version};
pub use node::{Body, Cx, Handler, HandlerId, Link, Node, Refusal, Route, Spaces};
pub use plan::{prefix, scan, Changes, Diff, Live, Patch, Plan};
#[cfg(feature = "upstream")]
pub use relay::{Relay, Served};
#[cfg(feature = "upstream")]
pub use serve::{Conn, Serve};
pub use store::{Layer, Memory, Overlay, Read, Store, TableId, View, Write, Writes};
#[cfg(feature = "upstream")]
pub use sync::Feed;
pub use sync::{Down, Logged, Pending, Query, QueryId, Up, Verdict};

/// The tests, as a list a runner with no test harness calls one by one:
/// the watch's under QEMU (`rust/kernel/watch/selftest.rs`). Each module's
/// tests are `#[test]` on the host and listed here under `ark_selftest`.
#[cfg(ark_selftest)]
pub mod selftest {
    /// A test, by its module and name.
    pub type Test = (&'static str, fn());

    /// Every module's tests.
    pub const TESTS: &[&[Test]] = &[
        crate::codec::tests::ALL,
        crate::anchor::tests::ALL,
        crate::entry::tests::ALL,
        crate::store::tests::ALL,
        crate::node::tests::ALL,
        crate::plan::tests::ALL,
        crate::sync::tests::ALL,
        #[cfg(feature = "upstream")]
        crate::authority::tests::ALL,
        #[cfg(feature = "upstream")]
        crate::serve::tests::ALL,
        #[cfg(feature = "upstream")]
        crate::relay::tests::ALL,
    ];
}

/// The list of a module's tests for `selftest`: a test left out of it is
/// dead code in the selftest build, which denies warnings.
#[cfg(any(test, ark_selftest))]
macro_rules! listed {
    ($($name:ident),* $(,)?) => {
        #[cfg(ark_selftest)]
        pub(crate) const ALL: &[crate::selftest::Test] = &[$((concat!(module_path!(), "::", stringify!($name)), $name)),*];
    };
}
#[cfg(any(test, ark_selftest))]
pub(crate) use listed;

/// A small seeded generator for the tests' random cases (xorshift64*), so
/// a property test needs no crate and runs on the watch as well.
#[cfg(any(test, ark_selftest))]
pub(crate) struct Rng(u64);

#[cfg(any(test, ark_selftest))]
impl Rng {
    pub(crate) fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    pub(crate) fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A number below `n`.
    pub(crate) fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// A number of any size, small ones as often as large: what a varint
    /// is worth testing at.
    pub(crate) fn any(&mut self) -> u64 {
        let bits = self.below(65) as u32;
        if bits == 0 {
            0
        } else {
            self.next() >> (64 - bits)
        }
    }

    pub(crate) fn bytes(&mut self, most: u64) -> alloc::vec::Vec<u8> {
        (0..self.below(most + 1)).map(|_| self.next() as u8).collect()
    }
}
