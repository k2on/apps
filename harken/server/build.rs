//! harken's bindings (`ark::gen`), from the domain's directory.

fn main() {
    let dir = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../domain");
    ark::gen::build_script(&dir);
}
