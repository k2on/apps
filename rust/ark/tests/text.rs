//! §1.15 Ark text: the demo as text.

#[path = "demo.rs"]
mod demo;

use ark::ir::{Function, Node};
use ark::module::Lib;

fn demo_text() -> (ark::module::Module, Vec<Function>) {
    let lib = Lib::default();
    let m = demo::module(&lib);
    let fns = [demo::CREATE, demo::ADD, demo::RENAME, demo::REMOVE].map(demo::function).to_vec();
    (m, fns)
}

#[test]
fn the_demo_printed_builds_to_itself() {
    let (m, fns) = demo_text();
    let text = ark::text::print(&m, &fns).unwrap();
    std::fs::write(std::env::temp_dir().join("demo.ark.txt"), &text).unwrap();
    println!("{text}");
    let d = ark::text::build(&text).unwrap_or_else(|e| panic!("{e}"));
    for f in &fns {
        let b = d.bodies.get(&f.hash()).map(|b| Function::from_bytes(b).unwrap());
        assert_eq!(b.as_ref(), Some(f), "{text}");
    }
    assert_eq!(d.module, m);
    assert_eq!(
        ark::text::print(
            &d.module,
            &d.bodies.values().map(|b| Function::from_bytes(b).unwrap()).collect::<Vec<_>>()
        )
        .unwrap(),
        text
    );
}
