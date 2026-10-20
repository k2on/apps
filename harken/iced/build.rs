//! harken's bindings (`ark::gen`), from the domain's directory: the types and stubs
//! the client calls the domain by, generated where rust-analyzer sees them.

fn main() {
    let dir = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../domain");
    ark::gen::build_script(&dir);
}
