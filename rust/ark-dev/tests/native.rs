//! The demo in plain Rust (§1.16) as a domain: its functions by name, an
//! input's refinements as the form's validation and the intent's refusal,
//! and a scope by table.

use ark::domain::Kind;
use ark::log::EntryId;
use ark::peer::Ctx;
use ark::row::Input;
use ark::store::Store;
use ark_dev::native::{demo, CreatePlaylist};

fn ctx() -> Ctx {
    Ctx {
        draws: Default::default(),
        id: EntryId::of("s", 1),
        actor: "alice".into(),
        session: "s".into(),
        roles: vec![],
        now: 5,
    }
}

/// Every function is registered by its name with its kind, a short name
/// finds its binding, and a name trims and must be something — with the
/// words a form shows, and the refusal the intent gets. Falsified by
/// `non_empty` checking before `trim`: "  " passed.
#[test]
fn the_demo_registers_its_functions_and_refines_its_inputs() {
    let d = demo();
    let kinds: Vec<(&str, Kind)> = d.fns.values().map(|f| (f.name.as_str(), f.kind)).collect();
    assert_eq!(kinds.len(), 6);
    assert!(kinds.contains(&("demo.items", Kind::View)) && kinds.contains(&("demo.top_two", Kind::Scope)));
    assert_eq!(d.binding("create_playlist"), Some("demo.create_playlist"));
    let blank = CreatePlaylist { name: "  ".into() };
    assert_eq!(blank.clone().check(), Err(vec![("name".into(), "a playlist needs a name".into())]));
    let named = CreatePlaylist { name: " Mix ".into() }.refine().unwrap();
    assert_eq!(named.name, "Mix");
    let mut store = Store::new(d.schema.clone());
    let f = d.get("demo.create_playlist").unwrap().proc.clone().unwrap();
    let refused = f.apply(&mut store, &ctx(), &ark::ir::Node::to_bytes(&blank));
    assert_eq!(refused.unwrap_err().0, "a playlist needs a name");
    let made = f.apply(&mut store, &ctx(), &ark::ir::Node::to_bytes(&named)).unwrap();
    assert_eq!(made.len(), 1);
}
