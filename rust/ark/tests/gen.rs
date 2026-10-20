//! §1.15 Bindings generated from a module (`ark::gen`): its types and a
//! stub per function, no body; and the check an app's bindings are held
//! to when a module is loaded.

use ark::gen::{fits, rust, Shape};
use ark::hash::hex;
use ark::ir::Node;
use ark::sexp::build;

const TABLE: &str = "(schema (table t (id Int key) (name Text)))\n";
const REST: &str = "(type Pair (record (a Int) (b Text)))
(type Mode (sum (Off) (On (level Int))))
(query q ((pair Pair)) (list (row t)) (filter t (fn (row) (== row.id pair.a))))
(mutator m ((pair Pair)) (insert t (record (id pair.a) (name pair.b))))
(helper h ((mode Mode) (words Text)) Int (match mode (Off () 0) (On (level) level)))
";

fn shape(src: &str) -> String {
    hex(&Shape::of(&build(src).unwrap()).to_bytes())
}

/// A struct per row and record, an enum per sum, a stub per function
/// that encodes its parameters, calls by name and decodes the answer —
/// and no body: no branch of the function's is in the bindings. Falsified
/// by generating a mutator's stub as a query's: `m` asked rather than
/// authored.
#[test]
fn bindings_are_types_and_stubs() {
    let g = rust(&build(&format!("{TABLE}{REST}")).unwrap());
    for want in [
        "pub struct T {\n    pub id: i64,\n    pub name: String,\n}",
        "pub struct Pair {\n    pub a: i64,\n    pub b: String,\n}",
        "pub enum Mode {\n    Off,\n    On { level: i64 },\n}",
        "pub fn q<C: Call>(peer: &C, pair: &Pair) -> Result<Vec<T>, String> {",
        "pub fn m<C: Call>(peer: &mut C, pair: &Pair) -> C::Authored {",
        "pub fn h<C: Call>(peer: &C, mode: &Mode, words: &str) -> Result<i64, String> {",
        "peer.author(\"m\",",
    ] {
        assert!(g.contains(want), "{want}\n---\n{g}");
    }
    assert!(!g.lines().any(|l| l.contains(" if ") || l.contains("match ")), "{g}");
}

/// Bindings fit the module they were made from and one whose schema grew
/// by appending; not one missing a column, nor one whose type or
/// function changed its structure. Falsified by leaving the schema out of
/// the check: the module missing `name` fitted.
#[test]
fn bindings_fit_a_module_that_grew() {
    let made = shape(&format!("{TABLE}{REST}"));
    let load = |src: String| build(&src).unwrap();
    assert_eq!(fits(&made, &load(format!("{TABLE}{REST}"))), Ok(()));
    let grown = "(schema (table t (id Int key) (name Text) (note (option Text))) (table u (id Int key)))\n";
    assert_eq!(
        fits(
            &made,
            &load(format!("{grown}{}", REST.replace("(name pair.b)", "(name pair.b) (note none)")))
        ),
        Ok(())
    );
    let missing = "(schema (table t (id Int key)))\n";
    let rest = REST.replace("(name pair.b)", "");
    let e = fits(&made, &load(format!("{missing}{rest}"))).unwrap_err().0;
    assert!(e.contains("the bindings' schema"), "{e}");
    let e = fits(
        &made,
        &load(format!(
            "{TABLE}{}",
            REST.replace("(b Text)", "(b Int)").replace("(name pair.b)", "(name \"x\")")
        )),
    )
    .unwrap_err()
    .0;
    assert!(e.contains("Pair is not the type"), "{e}");
    let e = fits(&made, &load(format!("{TABLE}{}", REST.replace("(words Text)", "(words Int)"))))
        .unwrap_err()
        .0;
    assert!(e.contains("h is not the function"), "{e}");
}

/// A grown row is read by the bindings as the prefix they know, inside a
/// list and an option too. Falsified by copying a tuple whole: the grown
/// rows were not the old ones.
#[test]
fn a_grown_row_is_read_by_its_prefix() {
    use ark::codec::{put_bytes, put_int};
    use ark::ty::Ty;
    let (old, new) = (
        Ty::Tuple(vec![Ty::Int, Ty::Bytes]),
        Ty::Tuple(vec![Ty::Int, Ty::Bytes, Ty::option(Ty::Bytes)]),
    );
    let row = |n: i64, grown: bool| {
        let mut b = vec![];
        put_int(&mut b, n);
        put_bytes(&mut b, b"x");
        if grown {
            b.extend([1]);
            put_bytes(&mut b, b"note");
        }
        b
    };
    let list = |grown: bool| [vec![1], row(1, grown), vec![1], row(2, grown), vec![0]].concat();
    assert_eq!(
        ark::gen::project(&Ty::list(old.clone()), &Ty::list(new.clone()), &list(true)),
        Some(list(false))
    );
    let some = |grown: bool| [vec![1], row(3, grown)].concat();
    assert_eq!(
        ark::gen::project(&Ty::option(old.clone()), &Ty::option(new), &some(true)),
        Some(some(false))
    );
    assert_eq!(ark::gen::project(&Ty::Tuple(vec![Ty::Int, Ty::Int]), &old, &row(1, false)), None);
}
