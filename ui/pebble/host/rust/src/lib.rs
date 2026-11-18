//! The Pebble host for the new kernel (step 5 of `docs/north-star.md`):
//! one kernel [`ark_kernel::Node`] running on the watch as a full node.
//!
//! - [`disk`]: its store on persistent storage, rows paged into the
//!   256-byte values PebbleOS keeps, with the pending records, the cursor,
//!   the origin and the count, written in batches.
//! - [`link`]: its link over AppMessage, the kernel's frames cut to the
//!   outbox and put back together from the inbox, up while the phone and
//!   its connection upstream are.
//! - [`host`]: the event loop, PebbleOS's callbacks each becoming entries
//!   or frames into the node, and each space's changes handed to the app.
//! - [`ui`]: arkui's Pebble toolkit: the native layers a screen generated
//!   from a `#[view]` is made of (ui/docs/pebble.md), laid out by
//!   [`tokens`], ui/theme.toml's.
//!
//! The C side (`c/ark_pebble.c`) is a thin shim: the SDK's main loop, the
//! window and its layers, AppMessage and timers, each calling in here.
//! An app says what it is with [`host::App`] and starts with [`app!`].

#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub mod disk;
pub mod host;
pub mod link;
pub mod rows;
pub mod sys;
pub mod text;
pub mod tokens;
pub mod ui;

pub use host::{App, Ctx};
pub use rows::Rows;
pub use ui::{Button, Ui};

/// Names the watch's app: the shim's `ark_start` makes it and starts the
/// host with it.
#[macro_export]
macro_rules! app {
    ($make:expr) => {
        #[no_mangle]
        pub extern "C" fn ark_start() {
            $crate::host::start($make);
        }
    };
}
