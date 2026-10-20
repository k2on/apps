//! The domain's bindings for its tests (`ark::gen`), from the directory.

fn main() {
    ark::gen::build_script(std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()));
}
