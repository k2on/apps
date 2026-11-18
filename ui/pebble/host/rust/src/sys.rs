//! What the host asks of PebbleOS (persistent storage, the heap) and of
//! the C shim (`c/ark_pebble.c`: time, AppMessage, timers, the screen's
//! layers, which `ui` makes and sets),
//! and the allocator over the app heap. Off the watch (the host's tests)
//! each is a stand-in that does nothing.

/// A frame as the shim takes it: PebbleOS's `GRect`.
#[repr(C)]
pub struct Frame {
    pub x: i16,
    pub y: i16,
    pub w: i16,
    pub h: i16,
}

/// A list's look as the shim takes it.
#[repr(C)]
pub struct CList {
    pub title_size: u8,
    pub title_bold: bool,
    pub sub_size: u8,
    pub sub_bold: bool,
    pub header_size: u8,
    pub header_bold: bool,
    pub ink: u8,
    pub quiet: u8,
    pub ground: u8,
    pub inset: i16,
    pub title_line: i16,
    pub row_height: i16,
    pub header_height: i16,
    pub lift: i16,
}

impl From<crate::ui::Frame> for Frame {
    fn from(f: crate::ui::Frame) -> Frame {
        Frame {
            x: f.x,
            y: f.y,
            w: f.w,
            h: f.h,
        }
    }
}

/// A list's look, its rows `row` tall under a heading `header` tall,
/// their words lifted by `lift`.
pub fn list_style(s: &crate::ui::ListStyle, row: i16, header: i16, lift: i16) -> CList {
    CList {
        title_size: s.title.size,
        title_bold: s.title.bold,
        sub_size: s.subtitle.size,
        sub_bold: s.subtitle.bold,
        header_size: s.header.size,
        header_bold: s.header.bold,
        ink: s.ink,
        quiet: s.quiet,
        ground: s.ground,
        inset: s.inset,
        title_line: s.title_line,
        row_height: row,
        header_height: header,
        lift,
    }
}

#[cfg(target_os = "none")]
mod ffi {
    use super::{CList, Frame};

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
        pub fn ark_ui_window(ground: u8);
        pub fn ark_ui_bounds(w: *mut i16, h: *mut i16);
        pub fn ark_ui_action_bar(ground: u8) -> i16;
        pub fn ark_ui_text(id: u8, f: *const Frame, size: u8, bold: bool, ink: u8, align: u8);
        pub fn ark_ui_meter(id: u8, f: *const Frame, fill: u8, track: u8, track_h: i16, corner: i16);
        pub fn ark_ui_rect(id: u8, f: *const Frame, color: u8, corner: i16);
        pub fn ark_ui_image(id: u8, f: *const Frame);
        pub fn ark_ui_list(id: u8, f: *const Frame, style: *const CList);
        pub fn ark_ui_bitmap(side: i16, bits: *const u8, len: usize, ink: u8) -> u8;
        pub fn ark_ui_hide(id: u8, hidden: bool);
        pub fn ark_ui_set_text(id: u8, s: *const u8, len: usize);
        pub fn ark_ui_set_meter(id: u8, permille: u16);
        pub fn ark_ui_set_image(id: u8, bitmap: u8);
        pub fn ark_ui_set_header(id: u8, s: *const u8, len: usize);
        pub fn ark_ui_set_row(id: u8, i: u8, t: *const u8, tl: usize, s: *const u8, sl: usize);
        pub fn ark_ui_set_rows(id: u8, n: u8);
        pub fn ark_ui_set_action(button: u8, bitmap: u8);
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

    pub fn ui_window(ground: u8) {
        unsafe { ffi::ark_ui_window(ground) }
    }

    /// The window's width and height.
    pub fn ui_bounds() -> (i16, i16) {
        let (mut w, mut h) = (0, 0);
        unsafe { ffi::ark_ui_bounds(&mut w, &mut h) };
        (w, h)
    }

    /// The action bar, made: its width.
    pub fn ui_action_bar(ground: u8) -> i16 {
        unsafe { ffi::ark_ui_action_bar(ground) }
    }

    pub fn ui_text(id: u8, f: crate::ui::Frame, font: crate::ui::Font, ink: u8, align: u8) {
        unsafe { ffi::ark_ui_text(id, &f.into(), font.size, font.bold, ink, align) }
    }

    pub fn ui_meter(id: u8, f: crate::ui::Frame, fill: u8, track: u8) {
        use crate::tokens::{CORNER, TRACK_HEIGHT};
        unsafe { ffi::ark_ui_meter(id, &f.into(), fill, track, TRACK_HEIGHT, CORNER) }
    }

    pub fn ui_rect(id: u8, f: crate::ui::Frame, color: u8, corner: i16) {
        unsafe { ffi::ark_ui_rect(id, &f.into(), color, corner) }
    }

    pub fn ui_image(id: u8, f: crate::ui::Frame) {
        unsafe { ffi::ark_ui_image(id, &f.into()) }
    }

    pub fn ui_list(id: u8, f: crate::ui::Frame, style: &crate::ui::ListStyle, row: i16, header: i16, lift: i16) {
        unsafe { ffi::ark_ui_list(id, &f.into(), &super::list_style(style, row, header, lift)) }
    }

    /// A glyph's bits made a bitmap in `ink`: its number, or `u8::MAX`.
    pub fn ui_bitmap(side: i16, bits: &[u8], ink: u8) -> u8 {
        unsafe { ffi::ark_ui_bitmap(side, bits.as_ptr(), bits.len(), ink) }
    }

    pub fn ui_hide(id: u8, hidden: bool) {
        unsafe { ffi::ark_ui_hide(id, hidden) }
    }

    pub fn ui_set_text(id: u8, s: &[u8]) {
        unsafe { ffi::ark_ui_set_text(id, s.as_ptr(), s.len()) }
    }

    pub fn ui_set_meter(id: u8, permille: u16) {
        unsafe { ffi::ark_ui_set_meter(id, permille) }
    }

    pub fn ui_set_image(id: u8, bitmap: u8) {
        unsafe { ffi::ark_ui_set_image(id, bitmap) }
    }

    pub fn ui_set_header(id: u8, s: &[u8]) {
        unsafe { ffi::ark_ui_set_header(id, s.as_ptr(), s.len()) }
    }

    pub fn ui_set_row(id: u8, i: u8, title: &[u8], sub: &[u8]) {
        unsafe { ffi::ark_ui_set_row(id, i, title.as_ptr(), title.len(), sub.as_ptr(), sub.len()) }
    }

    pub fn ui_set_rows(id: u8, n: u8) {
        unsafe { ffi::ark_ui_set_rows(id, n) }
    }

    pub fn ui_set_action(button: u8, bitmap: u8) {
        unsafe { ffi::ark_ui_set_action(button, bitmap) }
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
    pub fn ui_window(_: u8) {}
    /// A Time 2's window.
    pub fn ui_bounds() -> (i16, i16) {
        (200, 228)
    }
    pub fn ui_action_bar(_: u8) -> i16 {
        34
    }
    pub fn ui_text(_: u8, _: crate::ui::Frame, _: crate::ui::Font, _: u8, _: u8) {}
    pub fn ui_meter(_: u8, _: crate::ui::Frame, _: u8, _: u8) {}
    pub fn ui_rect(_: u8, _: crate::ui::Frame, _: u8, _: i16) {}
    pub fn ui_image(_: u8, _: crate::ui::Frame) {}
    pub fn ui_list(_: u8, _: crate::ui::Frame, _: &crate::ui::ListStyle, _: i16, _: i16, _: i16) {}
    pub fn ui_bitmap(_: i16, _: &[u8], _: u8) -> u8 {
        0
    }
    pub fn ui_hide(_: u8, _: bool) {}
    pub fn ui_set_text(_: u8, _: &[u8]) {}
    pub fn ui_set_meter(_: u8, _: u16) {}
    pub fn ui_set_image(_: u8, _: u8) {}
    pub fn ui_set_header(_: u8, _: &[u8]) {}
    pub fn ui_set_row(_: u8, _: u8, _: &[u8], _: &[u8]) {}
    pub fn ui_set_rows(_: u8, _: u8) {}
    pub fn ui_set_action(_: u8, _: u8) {}
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
