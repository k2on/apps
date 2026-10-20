//! §1.15 Ark text. `demo.ark` is the demo (`tests/demo.rs`) as text, the
//! printer's output: built, it is the demo's module byte for byte, and
//! printed again, it is itself. harken's module file, printed and built,
//! is itself too.

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

/// Falsified by stepping `pos` by 2 in `demo.ark`: the built
/// `add_to_playlist` was another function and the test failed.
#[test]
fn the_demo_text_is_the_demo() {
    let text = include_str!("demo.ark");
    let d = ark::text::build(text).unwrap_or_else(|e| panic!("{e}"));
    let (m, fns) = demo_text();
    assert_eq!(d.module, m);
    assert_eq!(d.module.hash(), m.hash());
    assert_eq!(ark::text::print(&m, &fns).unwrap(), text);
}

/// Falsified by printing a `Lookup`'s key with its arguments reversed:
/// the rebuilt module differed.
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

/// Falsified by printing every key of a sort on several keys `desc`: the
/// rebuilt functions differed and the test failed. (A name resolved one
/// column off in both directions held — printer and parser agree on it —
/// which `the_demo_text_is_the_demo`, a text written with names, catches.)
#[test]
fn harken_printed_builds_to_itself() {
    let file = include_bytes!("../../../harken/domain/harken.ark");
    let (m, bodies) = <(ark::module::Module, Vec<Vec<u8>>)>::from_bytes(file).unwrap();
    let fns: Vec<Function> = bodies.iter().map(|b| Function::from_bytes(b).unwrap()).collect();
    let text = ark::text::print(&m, &fns).unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(std::env::temp_dir().join("harken.ark.txt"), &text).unwrap();
    let d = ark::text::build(&text).unwrap_or_else(|e| panic!("{e}"));
    let again: Vec<Function> = d.bodies.values().map(|b| Function::from_bytes(b).unwrap()).collect();
    for (n, h) in &m.bindings {
        let g = d.module.get(n).unwrap();
        if g != *h {
            let a = fns.iter().find(|f| f.hash() == *h).unwrap();
            let b = again.iter().find(|f| f.hash() == g).unwrap();
            panic!("{n}\n{:?}\n{:?}\n{:?}\n{:?}", a.types, b.types, a.body, b.body);
        }
    }
    for f in &fns {
        assert!(d.bodies.contains_key(&f.hash()), "{:?}", f);
    }
    assert_eq!(ark::text::print(&d.module, &again).unwrap(), text);
}

/// `live` is a mutator over volatile tables only: one writing a durable
/// table is refused where it is built, and the same body as a `mutator`
/// builds. Falsified by dropping the check: the live one built.
#[test]
fn a_live_function_over_a_durable_table_is_refused() {
    let schema = "schema {\n  table note {\n    id: Text key\n  }\n  volatile table here {\n    id: Text key\n  }\n}\n";
    let body = |kind: &str, t: &str| format!("{schema}\n{kind} room.put(id: Text) = {t}.insert({{ id: id }})\n");
    assert!(ark::text::build(&body("live", "here")).is_ok());
    assert!(ark::text::build(&body("mutator", "note")).is_ok());
    let r = ark::text::build(&body("live", "note")).err().expect("refused");
    assert!(r.0.contains("a live function touches a durable table"), "{r}");
}
