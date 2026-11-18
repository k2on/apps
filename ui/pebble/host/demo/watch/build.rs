//! The screen, written at build time: the demo's `#[view]`
//! (`now-playing-view`) recorded on the dev machine and written out as
//! Pebble's layers by ark-ui-gen (ui/docs/pebble.md), into this build's
//! `OUT_DIR` for `src/lib.rs` to include. Nothing of it is committed; the
//! view's own tests hold what it writes.

fn main() {
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("screen.rs");
    std::fs::write(&out, now_playing_view::watch()).expect("the screen is written");
    println!("cargo:rerun-if-changed=build.rs");
}
