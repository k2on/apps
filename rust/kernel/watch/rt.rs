//! What a bare Cortex-M3 needs to run the kernel, and nothing else: the
//! vector table and reset (link.x lays them out), an allocator over a
//! fixed arena, and semihosting, by which a program under QEMU prints and
//! exits. Shared by the size probe and the selftest, each of which says
//! `main`, its panic handler and its heap.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::{Cell, UnsafeCell};
use core::ptr::addr_of_mut;

extern "C" {
    static mut __bss_start: u32;
    static mut __bss_end: u32;
}

/// After the stack's top (link.x): reset, then the thirteen exceptions
/// and the reserved slots between, each a fault.
#[link_section = ".vectors"]
#[used]
static VECTORS: [unsafe extern "C" fn() -> !; 15] = [
    reset, fault, fault, fault, fault, fault, fault, fault, fault, fault, fault, fault, fault, fault, fault,
];

#[no_mangle]
unsafe extern "C" fn reset() -> ! {
    // The zeroed statics are zeroed here: QEMU loads no bytes for them.
    let mut p = addr_of_mut!(__bss_start);
    let end = addr_of_mut!(__bss_end);
    while p < end {
        p.write_volatile(0);
        p = p.add(1);
    }
    crate::main()
}

unsafe extern "C" fn fault() -> ! {
    print("fault\n");
    exit(false)
}

/// A semihosting call: QEMU, given `-semihosting`, answers `bkpt 0xab`.
fn call(op: usize, arg: usize) -> usize {
    let r;
    unsafe { core::arch::asm!("bkpt #0xab", inout("r0") op => r, in("r1") arg, options(nostack)) };
    r
}

/// Writes `s` to QEMU's standard output.
pub fn print(s: &str) {
    // SYS_WRITE0 takes a string ended by a zero, written in pieces.
    let mut buf = [0u8; 65];
    for piece in s.as_bytes().chunks(64) {
        buf[..piece.len()].copy_from_slice(piece);
        buf[piece.len()] = 0;
        call(0x04, buf.as_ptr() as usize);
    }
}

/// Writes `n` in decimal, with no formatting machinery.
#[allow(dead_code)]
pub fn print_number(mut n: usize) {
    let mut digits = [0u8; 20];
    let mut at = digits.len();
    loop {
        at -= 1;
        digits[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    print(core::str::from_utf8(&digits[at..]).unwrap_or("?"));
}

/// Ends the program: QEMU exits 0 if `ok`, else 1.
pub fn exit(ok: bool) -> ! {
    // SYS_EXIT, with ADP_Stopped_ApplicationExit or RunTimeErrorUnknown.
    call(0x18, if ok { 0x20026 } else { 0x20023 });
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}

/// An allocator over `N` bytes: blocks in sizes of powers of two, each
/// size's freed blocks kept in a list and given out again, new ones cut
/// from the arena's bottom up. It counts the heap held as a watch's
/// allocator would hold it: the bytes of the blocks live at once, each
/// with the eight bytes of header kept beside it. The selftest gives the
/// whole arena back after each test.
pub struct Heap<const N: usize> {
    arena: UnsafeCell<[u8; N]>,
    top: Cell<usize>,
    /// Each size's first freed block; each freed block holds the next.
    free: [Cell<usize>; 32],
    live: Cell<usize>,
    peak: Cell<usize>,
}

/// What a watch's allocator keeps beside each block.
const HEADER: usize = 8;

// One thread, no interrupts that allocate.
unsafe impl<const N: usize> Sync for Heap<N> {}

/// The size class of a layout: its block is `1 << class` bytes, at least
/// eight, and aligned to its size.
fn class(layout: Layout) -> usize {
    layout.size().max(layout.align()).max(8).next_power_of_two().trailing_zeros() as usize
}

impl<const N: usize> Heap<N> {
    pub const fn new() -> Self {
        Heap {
            arena: UnsafeCell::new([0; N]),
            top: Cell::new(0),
            free: [const { Cell::new(0) }; 32],
            live: Cell::new(0),
            peak: Cell::new(0),
        }
    }

    /// Gives back everything, and starts counting the peak again.
    #[allow(dead_code)]
    pub fn reset(&self) {
        self.top.set(0);
        for f in &self.free {
            f.set(0);
        }
        self.live.set(0);
        self.peak.set(0);
    }

    /// The most heap held at once since the last reset: the live blocks'
    /// bytes and their headers.
    #[allow(dead_code)]
    pub fn peak(&self) -> usize {
        self.peak.get()
    }

    fn held(&self, more: usize, less: usize) {
        let live = self.live.get() + more - less;
        self.live.set(live);
        if live > self.peak.get() {
            self.peak.set(live);
        }
    }
}

unsafe impl<const N: usize> GlobalAlloc for Heap<N> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let c = class(layout);
        let head = self.free[c].get();
        let block = if head != 0 {
            self.free[c].set(*(head as *const usize));
            head
        } else {
            let base = self.arena.get() as usize;
            let start = (base + self.top.get()).next_multiple_of(1 << c);
            if start + (1 << c) > base + N {
                return core::ptr::null_mut();
            }
            self.top.set(start + (1 << c) - base);
            start
        };
        self.held(layout.size() + HEADER, 0);
        block as *mut u8
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.held(0, layout.size() + HEADER);
        let c = class(layout);
        *(ptr as *mut usize) = self.free[c].get();
        self.free[c].set(ptr as usize);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = Layout::from_size_align_unchecked(new_size, layout.align());
        if class(new) == class(layout) {
            self.held(new_size, layout.size());
            return ptr;
        }
        let moved = self.alloc(new);
        if !moved.is_null() {
            core::ptr::copy_nonoverlapping(ptr, moved, layout.size().min(new_size));
            self.dealloc(ptr, layout);
        }
        moved
    }
}
