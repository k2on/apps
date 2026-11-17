//! The kernel's tests on the watch's processor: each test of the crate's
//! list (`ark_kernel::selftest`, built with `--cfg ark_selftest`) run in
//! turn under QEMU, printed with the most heap it held, the heap given
//! back after each. A failure prints its panic and exits 1.

#![no_std]
#![no_main]

extern crate alloc;

#[path = "rt.rs"]
mod rt;

use core::fmt::Write;

#[global_allocator]
static HEAP: rt::Bump<{ 2 * 1024 * 1024 }> = rt::Bump::new();

struct Out;

impl Write for Out {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        rt::print(s);
        Ok(())
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(Out, "FAILED\n{info}");
    rt::exit(false)
}

fn main() -> ! {
    let mut ran = 0;
    for tests in ark_kernel::selftest::TESTS {
        for (name, test) in *tests {
            let _ = write!(Out, "test {name} ... ");
            HEAP.reset();
            test();
            let _ = writeln!(Out, "ok, {} bytes of heap at most", HEAP.peak());
            HEAP.reset();
            ran += 1;
        }
    }
    let _ = writeln!(Out, "{ran} tests passed on a Cortex-M4");
    rt::exit(true)
}
