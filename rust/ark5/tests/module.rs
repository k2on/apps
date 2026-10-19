//! §1.5 and the derive: the demo as a module — its bytes and its hash,
//! which `spec5/vectors/module/` pins — De Bruijn normalisation (two
//! spellings, one hash), and `#[derive(Node)]`: every derived type's
//! bytes read back as the value, and a recursive type recurring by name.

#[path = "demo.rs"]
mod demo;
mod support {
    pub mod json;
    pub mod vectors;
}

use ark5::check::Types;
use ark5::hash::{hex, sha256};
use ark5::interp::{decode, parse};
use ark5::ir::{Decl, Expr, Function, Node};
use ark5::module::{verify, Lib, Module};
use ark5::ty::Ty;
use demo::*;
use support::json::{array, hexed, object, quoted};
use support::vectors::{falsify, vector};

/// `module/demo`: the module's bytes and hash, its bindings, and each
/// function's bytes. Falsified by `SPEC` moving to 6: the module's bytes
/// moved.
#[test]
fn the_demo_module() {
    let lib = Lib::default();
    let m = module(&lib);
    verify(&m, &lib).unwrap();
    assert_eq!(Module::from_bytes(&m.to_bytes()), Ok(m.clone()), "decode of encode is the identity");
    let fns = m.bindings.iter().map(|(n, h)| {
        let f = lib.get(h).unwrap();
        object(&[("bytes", hexed(&f.f.to_bytes())), ("hash", quoted(&h.hex())), ("name", quoted(n))])
    });
    vector(
        "module",
        "demo.json",
        &[
            ("bytes", hexed(&m.to_bytes())),
            ("hash", quoted(&m.hash().hex())),
            ("functions", array(fns)),
        ],
    );
}

fn spellings(add: impl Fn([&str; 4]) -> Function) -> Vec<(&'static str, String)> {
    let names = [["playlist_id", "track_id", "last", "row"], ["p", "t", "l", "r"]];
    let fs: Vec<Function> = names.iter().map(|n| add(*n)).collect();
    vec![
        ("spellings", array(names.iter().map(|n| array(n.iter().map(|x| quoted(x)))))),
        ("hashes", array(fs.iter().map(|f| quoted(&f.hash().hex())))),
        ("bytes", array(fs.iter().map(|f| hexed(&f.to_bytes())))),
    ]
}

/// `module/de-bruijn`: `add_to_playlist` spelled with two sets of names,
/// one function and one hash; the falsify is what a hash that read the
/// names would claim. Falsified by `B::v` counting from the outermost
/// binder rather than the innermost: still one hash for both spellings,
/// which is why the vector pins the bytes too, and they moved.
#[test]
fn two_spellings_one_hash() {
    let right = spellings(|n| add_to_playlist_as(1, n));
    assert_eq!(add_to_playlist_as(1, ["a", "b", "c", "d"]), add_to_playlist(), "the names are not in the function");
    vector("module", "de-bruijn.json", &right);
    let by_name = spellings(|n| {
        let mut f = add_to_playlist_as(1, n);
        let salted = sha256(&[&f.to_bytes(), n.concat().as_bytes()]);
        f.body = Expr::Seq(vec![Expr::Lit(Ty::Bytes, salted.to_vec()), f.body]);
        f
    });
    falsify("module", "de-bruijn-by-name.json", &[right[0].clone(), by_name[1].clone(), right[2].clone()], &right);
}

/// `#[derive(Node)]`: the types of the IR and `Ty` itself as the codec
/// reads them, `Ty` recurring by name; a value of a recursive type parsed
/// under its declaration. Falsified by the derive numbering an enum's
/// variants from 1: a `Ty`'s bytes did not read back.
#[test]
fn the_derive() {
    let t = Ty::ty();
    let ty = || Ty::Named("Ty".into());
    assert_eq!(
        t,
        Ty::Sum(vec![vec![], vec![], vec![ty()], vec![Ty::list(ty())], vec![Ty::list(Ty::list(ty()))], vec![Ty::Bytes]])
    );
    let f = add_to_playlist();
    for v in [Ty::Sum(vec![vec![Ty::Int], vec![Ty::Named("x".into())]]), Function::ty(), Expr::ty()] {
        assert_eq!(Ty::from_bytes(&v.to_bytes()), Ok(v.clone()));
    }
    assert_eq!(Function::from_bytes(&f.to_bytes()), Ok(f.clone()));
    // A Ty's bytes under the declaration `Ty = …`, Named("Ty") recurring.
    let cx = Types::new(&[("Ty".into(), Decl { ty: t, refine: vec![] })]);
    let value = Ty::list(Ty::Tuple(vec![Ty::Int, Ty::option(Ty::Bytes)]));
    assert!(decode(&cx, &ty(), &value.to_bytes()).is_ok());
    assert_eq!(parse(&cx, &ty(), &value.to_bytes()).unwrap().bytes(), &value.to_bytes()[..]);
    let mut longer = value.to_bytes();
    longer.push(0);
    assert!(parse(&cx, &ty(), &longer).is_err(), "nothing may follow");
    assert_eq!(hex(&Ty::Int.to_bytes()), "00");
}
