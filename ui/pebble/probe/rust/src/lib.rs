//! The probe's Rust: a `no_std` static library linked into the watchapp
//! through the Pebble SDK's C API. Its global allocator is the app heap
//! (the SDK's `malloc`/`free`), and `probe_rust_run` exercises it with a
//! `BTreeMap` and a `Vec`, reporting what the heap did.

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

extern "C" {
    fn malloc(size: usize) -> *mut u8;
    fn free(ptr: *mut u8);
    fn heap_bytes_free() -> usize;
    fn heap_bytes_used() -> usize;
    /// The C side's panic report: it logs, then the app cannot go on.
    fn probe_rust_panicked();
}

/// What the SDK's `malloc` is relied on to align to. Anything stricter is
/// over-allocated and aligned here, with the original pointer kept in the
/// word before the one handed out.
const MALLOC_ALIGN: usize = 4;

struct PebbleHeap;

unsafe impl GlobalAlloc for PebbleHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() <= MALLOC_ALIGN {
            return malloc(layout.size());
        }
        let extra = layout.align() + core::mem::size_of::<usize>();
        let raw = malloc(layout.size() + extra);
        if raw.is_null() {
            return raw;
        }
        let start = raw as usize + core::mem::size_of::<usize>();
        let aligned = (start + layout.align() - 1) & !(layout.align() - 1);
        *((aligned - core::mem::size_of::<usize>()) as *mut usize) = raw as usize;
        aligned as *mut u8
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if layout.align() <= MALLOC_ALIGN {
            free(ptr);
        } else {
            let raw = *((ptr as usize - core::mem::size_of::<usize>()) as *const usize);
            free(raw as *mut u8);
        }
    }
}

#[global_allocator]
static HEAP: PebbleHeap = PebbleHeap;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    // No formatting: core::fmt is kilobytes the watch can keep.
    unsafe { probe_rust_panicked() };
    loop {}
}

/// What `probe_rust_run` found, read by the C side.
#[repr(C)]
pub struct RustReport {
    /// 1 when every check passed.
    pub ok: u32,
    pub map_len: u32,
    pub vec_sum: u32,
    pub aligned_ok: u32,
    pub free_before: u32,
    pub free_peak: u32,
    pub free_after: u32,
    pub used_before: u32,
    pub used_peak: u32,
    pub used_after: u32,
}

#[repr(C, align(16))]
struct Aligned16([u8; 48]);

/// Inserts into a `BTreeMap`, iterates it into a `Vec`, checks both against
/// a bitset kept on the stack, and checks an over-aligned box.
#[no_mangle]
pub extern "C" fn probe_rust_run(out: *mut RustReport) {
    if out.is_null() {
        return;
    }
    let (free_before, used_before) = unsafe { (heap_bytes_free(), heap_bytes_used()) };

    const N: u32 = 300;
    const RANGE: u32 = 1000;
    let mut seen = [0u32; (RANGE as usize + 31) / 32];
    let mut distinct = 0u32;
    let mut map = BTreeMap::new();
    for i in 0..N {
        let k = i.wrapping_mul(2_654_435_761) % RANGE;
        map.insert(k, i);
        let (w, b) = ((k / 32) as usize, k % 32);
        if seen[w] & (1 << b) == 0 {
            seen[w] |= 1 << b;
            distinct += 1;
        }
    }
    let mut v: Vec<u32> = Vec::new();
    let mut sorted = true;
    let mut last: Option<u32> = None;
    for (&k, &val) in map.iter() {
        if let Some(prev) = last {
            sorted &= prev < k;
        }
        last = Some(k);
        v.push(k ^ val);
    }
    let aligned = Box::new(Aligned16([7; 48]));
    let aligned_ok = (&*aligned as *const Aligned16 as usize) % 16 == 0 && aligned.0[47] == 7;

    let (free_peak, used_peak) = unsafe { (heap_bytes_free(), heap_bytes_used()) };
    let vec_sum = v.iter().fold(0u32, |a, x| a.wrapping_add(*x));
    let map_len = map.len() as u32;
    let ok = sorted && map_len == distinct && v.len() as u32 == distinct && aligned_ok;
    drop(aligned);
    drop(v);
    drop(map);
    let (free_after, used_after) = unsafe { (heap_bytes_free(), heap_bytes_used()) };

    let report = RustReport {
        ok: ok as u32,
        map_len,
        vec_sum,
        aligned_ok: aligned_ok as u32,
        free_before: free_before as u32,
        free_peak: free_peak as u32,
        free_after: free_after as u32,
        used_before: used_before as u32,
        used_peak: used_peak as u32,
        used_after: used_after as u32,
    };
    unsafe { ptr::write(out, report) };
}
