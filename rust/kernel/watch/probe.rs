//! The probe: a watch program that links what of the kernel a device runs
//! (the codec, the entry, the stores, the node and its frames, plans,
//! cells and the settle; not the authority, serve or relay, which only an
//! upstream links) as an app would, for `checks.kernel-size` to measure at
//! the watch's settings. It runs the watch's slice (`slice.rs`) over the
//! frames its upstream sends, recorded on the build machine by
//! `frames.rs` from a real authority and phone node and embedded here as
//! bytes: so the probe holds only what a watch runs, none of a server's
//! or a phone's. Its inputs are opaque to the optimiser (`black_box`), so
//! nothing is folded away.
//!
//! Under QEMU (`checks.kernel-qemu`) it prints the most heap it held and
//! exits 1 if the slice went wrong or past the budget's 24 KB.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "rt.rs"]
mod rt;
#[path = "slice.rs"]
mod slice;

use alloc::vec::Vec;
use core::hint::black_box;

#[global_allocator]
static HEAP: rt::Heap<{ 1024 * 1024 }> = rt::Heap::new();

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    rt::exit(false)
}

/// The frames the upstream sends at each exchange, recorded by
/// `frames.rs`: each exchange's bytes after their length, in four bytes.
/// A watch is sent them over its link, never holds them in its image: they
/// are a section of their own, which the size the check counts (`.text`
/// and `.rodata`) leaves out.
#[link_section = ".frames"]
static FRAMES: [u8; include_bytes!(env!("ARK_FRAMES")).len()] = *include_bytes!(env!("ARK_FRAMES"));

/// Exchange `n`'s frames.
fn frames(n: u8) -> Vec<u8> {
    let mut rest = black_box(&FRAMES[..]);
    for i in 0.. {
        let Some((len, tail)) = rest.split_first_chunk::<4>() else { break };
        let len = u32::from_le_bytes(*len) as usize;
        let (this, tail) = tail.split_at(len.min(tail.len()));
        if i == n {
            return this.to_vec();
        }
        rest = tail;
    }
    Vec::new()
}

fn main() -> ! {
    let right = slice::run(&mut |n, said| {
        black_box(said);
        frames(n)
    });
    // The heap at most, against the kernel's budget of 24 KB.
    let peak = HEAP.peak();
    rt::print("the kernel's probe held at most ");
    rt::print_number(peak);
    rt::print(" bytes of heap, of the 24576 its budget allows\n");
    rt::exit(peak <= 24 * 1024 && right)
}
