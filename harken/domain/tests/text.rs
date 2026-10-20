//! harken's domain as ArkLang (§1.15): `domain.ark` is the module file
//! `harken.ark` printed, and built it is that file byte for byte — which is
//! what the builders (`harken_domain::domain`) write. Falsified by changing
//! `"a song needs a title"` in `domain.ark`: the built `library.add_song`
//! was another function, and the printed text was not the file.

use ark::ir::{Function, Node};

#[test]
fn domain_ark_is_harken_ark() {
    let file = include_bytes!("../harken.ark");
    let text = include_str!("../domain.ark");
    let d = harken_domain::domain();
    let built = (d.module.clone(), d.bodies.values().cloned().collect::<Vec<_>>()).to_bytes();
    assert!(
        built == file,
        "harken.ark is not what the builders write: `harken-domain harken.ark` writes it again"
    );
    let b = ark::sexp::build(text).unwrap_or_else(|r| panic!("domain.ark: {r}"));
    let from_text = (b.module.clone(), b.bodies.values().cloned().collect::<Vec<_>>()).to_bytes();
    assert!(from_text == file, "domain.ark does not build to harken.ark");
    let fns: Vec<Function> = d.bodies.values().map(|b| Function::from_bytes(b).unwrap()).collect();
    assert_eq!(ark::sexp::print(&d.module, &fns).unwrap(), text, "domain.ark is not harken.ark printed");
}
