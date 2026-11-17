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

/// Ends the program: QEMU exits 0 if `ok`, else 1.
pub fn exit(ok: bool) -> ! {
    // SYS_EXIT, with ADP_Stopped_ApplicationExit or RunTimeErrorUnknown.
    call(0x18, if ok { 0x20026 } else { 0x20023 });
    loop {
        unsafe { core::arch::asm!("wfi") };
    }
}

/// An allocator over `N` bytes, from the bottom up: freeing the last block
/// gives it back, and growing the last block grows it in place. The
/// selftest gives the whole arena back after each test.
pub struct Bump<const N: usize> {
    arena: UnsafeCell<[u8; N]>,
    top: Cell<usize>,
    peak: Cell<usize>,
}

// One thread, no interrupts that allocate.
unsafe impl<const N: usize> Sync for Bump<N> {}

impl<const N: usize> Bump<N> {
    pub const fn new() -> Self {
        Bump {
            arena: UnsafeCell::new([0; N]),
            top: Cell::new(0),
            peak: Cell::new(0),
        }
    }

    fn base(&self) -> usize {
        self.arena.get() as usize
    }

    /// Gives back everything, and starts counting the peak again.
    #[allow(dead_code)]
    pub fn reset(&self) {
        self.top.set(0);
        self.peak.set(0);
    }

    /// The most bytes held at once since the last reset.
    #[allow(dead_code)]
    pub fn peak(&self) -> usize {
        self.peak.get()
    }

    fn set_top(&self, top: usize) {
        self.top.set(top);
        if top > self.peak.get() {
            self.peak.set(top);
        }
    }
}

unsafe impl<const N: usize> GlobalAlloc for Bump<N> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let base = self.base();
        let start = (base + self.top.get()).next_multiple_of(layout.align()) - base;
        match start.checked_add(layout.size()) {
            Some(end) if end <= N => {
                self.set_top(end);
                (base + start) as *mut u8
            }
            _ => core::ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr as usize + layout.size() == self.base() + self.top.get() {
            self.top.set(ptr as usize - self.base());
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let start = ptr as usize - self.base();
        if start + layout.size() == self.top.get() && start + new_size <= N {
            self.set_top(start + new_size);
            return ptr;
        }
        let new = self.alloc(Layout::from_size_align_unchecked(new_size, layout.align()));
        if !new.is_null() {
            core::ptr::copy_nonoverlapping(ptr, new, layout.size().min(new_size));
            self.dealloc(ptr, layout);
        }
        new
    }
}
