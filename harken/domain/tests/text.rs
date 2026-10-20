//! harken's domain as ArkLang (§1.15): `domain.ark` is the module file
//! `harken.ark` printed, and built it is that module with the names the
//! printer gave its binders — the builders (`harken_domain::domain`) write
//! none. Falsified by changing `"a song needs a title"` in `domain.ark`:
//! the built `library.add_song` was another function, and the printed text
//! was not the file.

use ark::ir::{Function, Node};

#[test]
fn domain_ark_is_harken_ark() {
    let file = include_bytes!("../harken.ark");
    let text = include_str!("../domain.ark");
    let d = harken_domain::domain();
    assert!(
        d.to_file() == file,
        "harken.ark is not what the builders write: `harken-domain harken.ark` writes it again"
    );
    let b = ark::sexp::build(text).unwrap_or_else(|r| panic!("domain.ark: {r}"));
    // Each function, its names set aside and its callees named by the
    // builders' hashes for them, is the builders'.
    let to: Vec<(Vec<u8>, Vec<u8>)> = b
        .module
        .bindings
        .iter()
        .map(|(n, h)| (ark::codec::bytes_enc(&h.0), ark::codec::bytes_enc(&d.get(n).0)))
        .collect();
    for (n, h) in &b.module.bindings {
        let mut bytes = b.lib.get(h).unwrap().f.unnamed().to_bytes();
        for (from, to) in to.iter().filter(|(f, t)| f != t) {
            while let Some(i) = bytes.windows(from.len()).position(|w| w == from) {
                bytes.splice(i..i + from.len(), to.iter().copied());
            }
        }
        assert!(
            bytes == d.lib.get(&d.get(n)).unwrap().f.to_bytes(),
            "domain.ark's {n} is not harken.ark's"
        );
    }
    let fns: Vec<Function> = d.bodies.values().map(|b| Function::from_bytes(b).unwrap()).collect();
    assert_eq!(ark::sexp::print(&d.module, &fns).unwrap(), text, "domain.ark is not harken.ark printed");
}
