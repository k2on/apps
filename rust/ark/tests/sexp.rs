//! §1.15 ArkLang: the reader, the printer's layout, and the lowering's
//! words. The demo (`tests/demo.rs`, plain IR, no names) printed builds to
//! itself; harken's whole module is `harken/domain/tests/text.rs`'s, and
//! the builders' demo is `rust/ark-dev/tests/vectors.rs`'s.

#[path = "demo.rs"]
mod demo;

use ark::ir::Function;
use ark::module::Lib;
use ark::sexp::{build, layout, print, read, Sx, K};

fn sym(s: &str) -> K {
    K::Sym(s.into())
}

/// Every atom, a list, a comment and the two escapes, at their places.
/// Falsified by reading `\\` as one byte too many: the string differed.
#[test]
fn the_reader_reads_atoms_lists_and_places() {
    let xs = read("; a comment\n(f -5 \"a \\\"b\\\" \\\\\" 0x00ff x.y)  ; more\n()").unwrap();
    let K::List(f) = &xs[0].k else { panic!() };
    let ks: Vec<&K> = f.iter().map(|x| &x.k).collect();
    assert_eq!(
        ks,
        [&sym("f"), &K::Int(-5), &K::Str("a \"b\" \\".into()), &K::Bytes(vec![0, 255]), &sym("x.y")]
    );
    assert_eq!((xs[0].at, f[1].at, xs[1].at), ((2, 1), (2, 4), (3, 1)));
    assert_eq!(xs[1].k, K::List(vec![]));
}

/// Falsified by reporting a string's error at the reader's position rather
/// than its own: the column was the end of the input.
#[test]
fn a_reading_refused_says_where() {
    for (src, why) in [
        ("(a\n  (b)", "1:1: a `(` without its `)`"),
        ("(a))", "1:4: a `)` without its `(`"),
        ("(a \"x\\n\")", "1:6: only"),
        ("(a\n 12x)", "2:2: `12x` is no integer"),
        ("\n\n  \"open", "3:3: a string without its end"),
        ("0xabc", "1:1: `0xabc` is no bytes"),
    ] {
        let e = read(src).unwrap_err().0;
        assert!(e.starts_with(why), "{src:?}: {e}");
    }
}

fn l(xs: Vec<Sx>) -> Sx {
    Sx { at: (0, 0), k: K::List(xs) }
}
fn y(s: &str) -> Sx {
    Sx { at: (0, 0), k: sym(s) }
}

/// A form that ends by column 100 is one line; one more column breaks it,
/// its head first and each argument on its own two spaces in, the same
/// rule inside. Falsified by measuring the width without the column it
/// starts at: the nested form stayed on one line.
#[test]
fn the_layout_breaks_past_100_columns() {
    let word = |n: usize| y(&"w".repeat(n));
    // `(f ` + 94 + `)` is 98 wide.
    let fits = l(vec![y("f"), word(94)]);
    assert_eq!(layout(&fits, 2), format!("(f {})", "w".repeat(94)));
    assert_eq!(layout(&fits, 3), format!("(f\n     {})", "w".repeat(94)));
    let nested = l(vec![y("g"), l(vec![y("h"), word(40), word(40)]), word(10)]);
    let at = |c: usize| layout(&nested, c);
    assert_eq!(at(0).lines().count(), 1);
    assert_eq!(
        at(10),
        format!("(g\n            (h {} {})\n            wwwwwwwwww)", "w".repeat(40), "w".repeat(40))
    );
    assert_eq!(at(18).lines().count(), 5, "{}", at(18));
}

/// The demo's four mutators, built as plain IR, printed and built again,
/// are themselves, and printed again the same text. Falsified by printing
/// a lookup's key columns in reverse: the rebuilt module differed.
#[test]
fn the_demo_printed_builds_to_itself() {
    let m = demo::module(&Lib::default());
    let fns = [demo::CREATE, demo::ADD, demo::RENAME, demo::REMOVE].map(demo::function).to_vec();
    let text = print(&m, &fns).unwrap();
    let d = build(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    for f in &fns {
        assert_eq!(d.lib.get(&f.hash()).map(|x| x.f.clone()).as_ref(), Some(f), "{text}");
    }
    assert_eq!(d.module, m);
    let again: Vec<Function> = d.bodies.keys().map(|h| d.lib.get(h).unwrap().f.clone()).collect();
    assert_eq!(print(&d.module, &again).unwrap(), text);
}

const SCHEMA: &str = "(schema (table t (id Int key) (name Text)))\n(type Pair (record (a Int) (b Text)))\n";

/// What the lowering refuses, in its words and at the form's place: a
/// `none` no place types, a record where none is wanted, a tuple where a
/// record is, a name bound nowhere, a function reaching itself. Falsified
/// by lowering `(tuple …)` without asking what its place wants: the
/// record written as a tuple built.
#[test]
fn a_lowering_refused_says_where_and_why() {
    for (src, why) in [
        ("(helper f ((n Int)) Int (let ((x none)) n))", "3:34: a none no place types"),
        (
            "(helper f ((n Int)) Int\n  (let ((p (record (a n) (b \"x\")))) n))",
            "4:12: a record no place types",
        ),
        ("(helper f ((n Int)) Pair (tuple n \"x\"))", "3:26: a record is written (record"),
        ("(helper f ((n Int)) Int m)", "3:25: `m` is no variable"),
        ("(helper f ((n Int)) Int (g n))\n(helper g ((n Int)) Int (f n))", "reaches itself"),
        (
            "(query f ((n Int)) (list (row t)) (filter t (fn (r) (== r.nmae \"x\"))))",
            "no field nmae",
        ),
    ] {
        let e = build(&format!("{SCHEMA}{src}")).err().unwrap_or_else(|| panic!("{src} built")).0;
        assert!(e.contains(why), "{src}: {e}");
    }
    let ok = build(&format!("{SCHEMA}(helper f ((n Int)) Pair (record (a n) (b \"x\")))")).unwrap();
    assert_eq!(ok.module.bindings.len(), 1);
}
