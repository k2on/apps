//! UniFFI's binding generator: `uniffi-bindgen generate LIBRARY --config
//! ROOTS ...`, told each crate's directory by `[crate-roots]`, so it runs no
//! cargo.

fn main() {
    uniffi::uniffi_bindgen_main()
}
