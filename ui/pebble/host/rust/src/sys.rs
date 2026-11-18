//! What the host asks of PebbleOS (persistent storage, the heap) and of
//! the C shim (`c/ark_pebble.c`: time, AppMessage, timers, the screen),
//! and the allocator over the app heap. Off the watch (the host's tests)
//! each is a stand-in that does nothing.

#[cfg(target_os = "none")]
mod ffi {
    extern "C" {
        pub fn persist_read_data(key: u32, buf: *mut u8, size: usize) -> i32;
        pub fn persist_write_data(key: u32, data: *const u8, size: usize) -> i32;
        pub fn malloc(n: usize) -> *mut u8;
        pub fn free(p: *mut u8);
        pub fn heap_bytes_used() -> usize;
        pub fn heap_bytes_free() -> usize;

        pub fn ark_now_ms() -> u64;
        pub fn ark_random() -> u32;
        pub fn ark_log(msg: *const u8, len: usize);
        pub fn ark_send_ready(inbox: u32, origin: *const u8, len: usize) -> bool;
        pub fn ark_send_chunk(epoch: u32, data: *const u8, len: usize) -> bool;
        pub fn ark_timer(kind: u8, ms: u32);
        pub fn ark_inbox_size() -> u32;
        pub fn ark_outbox_size() -> u32;
        pub fn ark_phone_connected() -> bool;
        pub fn ark_ui_text(slot: u8, s: *const u8, len: usize);
        pub fn ark_ui_row(i: u8, t: *const u8, tl: usize, s: *const u8, sl: usize);
        pub fn ark_ui_rows(n: u8);
        pub fn ark_ui_action(button: u8, glyph: u8);
        pub fn ark_ui_progress(permille: u16);
        pub fn ark_rust_panicked();
    }
}

#[cfg(target_os = "none")]
pub use watch::*;

#[cfg(target_os = "none")]
mod watch {
    use super::ffi;
    use crate::disk::{Persist, VALUE};
    use core::alloc::{GlobalAlloc, Layout};
    use core::cell::Cell;

    pub fn now() -> u64 {
        unsafe { ffi::ark_now_ms() }
    }

    pub fn random() -> u32 {
        unsafe { ffi::ark_random() }
    }

    pub fn log(line: &[u8]) {
        unsafe { ffi::ark_log(line.as_ptr(), line.len()) }
    }

    pub fn send_ready(inbox: u32, origin: &[u8]) -> bool {
        unsafe { ffi::ark_send_ready(inbox, origin.as_ptr(), origin.len()) }
    }

    pub fn send_chunk(epoch: u32, data: &[u8]) -> bool {
        unsafe { ffi::ark_send_chunk(epoch, data.as_ptr(), data.len()) }
    }

    /// Sets timer `kind` to fire in `ms`, or cancels it (`None`).
    pub fn timer(kind: u8, ms: Option<u32>) {
        unsafe { ffi::ark_timer(kind, ms.unwrap_or(u32::MAX)) }
    }

    pub fn inbox_size() -> u32 {
        unsafe { ffi::ark_inbox_size() }
    }

    pub fn outbox_size() -> u32 {
        unsafe { ffi::ark_outbox_size() }
    }

    pub fn phone_connected() -> bool {
        unsafe { ffi::ark_phone_connected() }
    }

    pub fn ui_text(slot: u8, s: &[u8]) {
        unsafe { ffi::ark_ui_text(slot, s.as_ptr(), s.len()) }
    }

    pub fn ui_row(i: u8, title: &[u8], sub: &[u8]) {
        unsafe { ffi::ark_ui_row(i, title.as_ptr(), title.len(), sub.as_ptr(), sub.len()) }
    }

    pub fn ui_rows(n: u8) {
        unsafe { ffi::ark_ui_rows(n) }
    }

    pub fn ui_action(button: u8, glyph: u8) {
        unsafe { ffi::ark_ui_action(button, glyph) }
    }

    pub fn ui_progress(permille: u16) {
        unsafe { ffi::ark_ui_progress(permille) }
    }

    /// The app heap: what all of it holds now (the SDK's count, C's
    /// allocations and AppMessage's buffers too), and what is free.
    pub fn heap() -> (usize, usize) {
        unsafe { (ffi::heap_bytes_used(), ffi::heap_bytes_free()) }
    }

    /// PebbleOS's persistent storage.
    pub struct Sdk;

    impl Persist for Sdk {
        fn read(&mut self, key: u32, buf: &mut [u8; VALUE]) -> Option<usize> {
            let n = unsafe { ffi::persist_read_data(key, buf.as_mut_ptr(), VALUE) };
            usize::try_from(n).ok()
        }

        fn write(&mut self, key: u32, data: &[u8]) -> bool {
            let n = unsafe { ffi::persist_write_data(key, data.as_ptr(), data.len()) };
            usize::try_from(n).is_ok_and(|n| n == data.len())
        }
    }

    /// The app heap (the SDK's `malloc`), counting what Rust holds of it.
    /// `malloc` aligns to 4: a stricter alignment is over-allocated and
    /// aligned here, the original pointer in the word before.
    pub struct Heap {
        live: Cell<usize>,
        peak: Cell<usize>,
    }

    // One thread, no interrupts that allocate.
    unsafe impl Sync for Heap {}

    impl Default for Heap {
        fn default() -> Heap {
            Heap::new()
        }
    }

    const WORD: usize = core::mem::size_of::<usize>();

    impl Heap {
        pub const fn new() -> Heap {
            Heap {
                live: Cell::new(0),
                peak: Cell::new(0),
            }
        }

        /// What Rust holds of the heap now, and the most it has held.
        pub fn held(&self) -> (usize, usize) {
            (self.live.get(), self.peak.get())
        }

        fn count(&self, more: usize, less: usize) {
            let live = self.live.get() + more - less;
            self.live.set(live);
            if live > self.peak.get() {
                self.peak.set(live);
            }
        }
    }

    unsafe impl GlobalAlloc for Heap {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let p = if layout.align() <= 4 {
                ffi::malloc(layout.size())
            } else {
                let raw = ffi::malloc(layout.size() + layout.align() + WORD);
                if raw.is_null() {
                    return raw;
                }
                let aligned = (raw as usize + WORD).next_multiple_of(layout.align());
                *((aligned - WORD) as *mut usize) = raw as usize;
                aligned as *mut u8
            };
            if !p.is_null() {
                self.count(layout.size(), 0);
            }
            p
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            self.count(0, layout.size());
            if layout.align() <= 4 {
                ffi::free(ptr);
            } else {
                ffi::free(*((ptr as usize - WORD) as *const usize) as *mut u8);
            }
        }
    }

    #[global_allocator]
    pub static HEAP: Heap = Heap::new();

    #[panic_handler]
    fn panic(_: &core::panic::PanicInfo) -> ! {
        // No formatting: core::fmt is kilobytes the watch can keep.
        unsafe { ffi::ark_rust_panicked() };
        loop {}
    }
}

/// Off the watch: stand-ins, for the host's tests.
#[cfg(not(target_os = "none"))]
pub use stand_in::*;

#[cfg(not(target_os = "none"))]
#[allow(dead_code)]
mod stand_in {
    pub fn now() -> u64 {
        0
    }
    pub fn random() -> u32 {
        4
    }
    pub fn log(_: &[u8]) {}
    pub fn send_ready(_: u32, _: &[u8]) -> bool {
        false
    }
    pub fn send_chunk(_: u32, _: &[u8]) -> bool {
        false
    }
    pub fn timer(_: u8, _: Option<u32>) {}
    pub fn inbox_size() -> u32 {
        1024
    }
    pub fn outbox_size() -> u32 {
        512
    }
    pub fn phone_connected() -> bool {
        false
    }
    pub fn ui_text(_: u8, _: &[u8]) {}
    pub fn ui_row(_: u8, _: &[u8], _: &[u8]) {}
    pub fn ui_rows(_: u8) {}
    pub fn ui_action(_: u8, _: u8) {}
    pub fn ui_progress(_: u16) {}
    pub fn heap() -> (usize, usize) {
        (0, 0)
    }

    /// Storage that holds nothing.
    pub struct Sdk;

    impl crate::disk::Persist for Sdk {
        fn read(&mut self, _: u32, _: &mut [u8; crate::disk::VALUE]) -> Option<usize> {
            None
        }

        fn write(&mut self, _: u32, _: &[u8]) -> bool {
            false
        }
    }
}
