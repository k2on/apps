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
/// Falsified by keeping an escape's backslash: the string differed.
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

/// A form that ends by column 100 is one line. Past it, its head stays on
/// the first line with its first argument when that is an atom or fits
/// there, and each other argument is on its own two spaces in; a list
/// with no head (a `let`'s bindings) is one element a line, under the
/// first; a line's width counts the `)`s that close on it. Falsified by
/// measuring the width without the column it starts at: the nested form
/// stayed on one line; by breaking every argument onto its own line: the
/// `if` lost its condition from the first line; and by not counting the
/// closing `)`s: `(f (g …))` was one line of 101 columns.
#[test]
fn the_layout_breaks_past_100_columns() {
    let w = |n: usize| "w".repeat(n);
    let word = |n: usize| y(&w(n));
    // `(f ` + 94 + `)` is 98 wide.
    let fits = l(vec![y("f"), word(94)]);
    assert_eq!(layout(&fits, 2), format!("(f {})", w(94)));
    let two = l(vec![y("f"), word(50), word(50)]);
    assert_eq!(layout(&two, 0), format!("(f {}\n  {})", w(50), w(50)));
    let cond = l(vec![y("if"), l(vec![y("=="), y("x"), y("1")]), word(60), word(40)]);
    assert_eq!(layout(&cond, 0), format!("(if (== x 1)\n  {}\n  {})", w(60), w(40)));
    let nested = l(vec![y("g"), l(vec![y("h"), word(40), word(40)]), word(10)]);
    assert_eq!(layout(&nested, 0).lines().count(), 1);
    assert_eq!(
        layout(&nested, 18),
        format!(
            "(g\n                    (h {}\n                      {})\n                    {})",
            w(40),
            w(40),
            w(10)
        )
    );
    let closed = l(vec![y("f"), l(vec![y("g"), word(93)])]);
    assert_eq!(layout(&closed, 0), format!("(f\n  (g {}))", w(93)), "its `)` is on its line");
    let binds = l(vec![l(vec![y("a"), word(50)]), l(vec![y("b"), word(50)])]);
    assert_eq!(layout(&binds, 4), format!("((a {})\n     (b {}))", w(50), w(50)));
}

/// The demo's four mutators, built as plain IR, printed and built again,
/// are themselves with the names the printer gave their binders, and
/// printed again the same text. Falsified by printing a lookup's key
/// columns in reverse: the rebuilt `add_to_playlist` was not the demo's.
#[test]
fn the_demo_printed_builds_to_itself() {
    let m = demo::module(&Lib::default());
    let fns = [demo::CREATE, demo::ADD, demo::RENAME, demo::REMOVE].map(demo::function).to_vec();
    let text = print(&m, &fns).unwrap();
    let d = build(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    let built: Vec<Function> = d.bodies.keys().map(|h| d.lib.get(h).unwrap().f.clone()).collect();
    for f in &fns {
        assert!(built.iter().any(|b| b.unnamed() == *f && !b.names.is_empty()), "{text}");
    }
    let names = |m: &ark::module::Module| m.bindings.iter().map(|b| b.0.clone()).collect::<Vec<_>>();
    assert_eq!((names(&d.module), &d.module.schema, &d.module.types), (names(&m), &m.schema, &m.types));
    assert_eq!(print(&d.module, &built).unwrap(), text);
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

const ROWS: &str = "(schema (table t (id Int key) (name Text) (next (option Int))))\n";

/// The functions of `src` (after [`ROWS`]), unnamed, by their names.
fn unnamed(src: &str) -> Vec<(String, Function)> {
    let d = build(&format!("{ROWS}{src}")).unwrap_or_else(|e| panic!("{e}\n{src}"));
    let f = |h| d.lib.get(h).unwrap().f.unnamed();
    d.module.bindings.iter().map(|(n, h)| (n.clone(), f(h))).collect()
}

/// `(?? P D)` through an option is the match it stands for, `(?. E f)`
/// the match that keeps the option, a `??` crossing none the standard
/// function; each prints as it was written. Falsified by lowering `??`
/// through an option to the standard function: the first pair differed.
#[test]
fn optional_paths_are_the_matches_they_stand_for() {
    let pairs = [
        (
            "(query a ((k Int)) Text (?? (. (get t k) name) \"\"))",
            "(query a ((k Int)) Text (match (get t k) (none () \"\") (some (r) r.name)))",
        ),
        (
            "(query a ((k Int)) (option Text) (?. (get t k) name))",
            "(query a ((k Int)) (option Text) (match (get t k) (none () none) (some (r) (some r.name))))",
        ),
        (
            "(helper a ((row (row t))) Int (?? row.next 0))",
            "(helper a ((row (row t))) Int (match row.next (none () 0) (some (n) n)))",
        ),
    ];
    for (sugar, spelt) in pairs {
        let (a, b) = (unnamed(sugar), unnamed(spelt));
        if sugar.contains("next") {
            assert_ne!(a, b, "a `??` of a path crossing none is the standard function");
        } else {
            assert_eq!(a, b, "{sugar}");
        }
    }
    for src in [
        "(query a ((k Int)) Text (?? (. (get t k) name) \"\"))",
        "(query a ((k Int)) (option Text) (?. (get t k) name))",
        "(helper b ((row (row t))) Int (?? row.next 0))",
        "(query c ((k Int)) (option Int) (?? (. (get t k) next) none))",
    ] {
        let d = build(&format!("{ROWS}{src}")).unwrap_or_else(|e| panic!("{e}\n{src}"));
        let fns: Vec<Function> = d.bodies.keys().map(|h| d.lib.get(h).unwrap().f.clone()).collect();
        let text = print(&d.module, &fns).unwrap();
        assert!(text.ends_with(&format!("{src}\n")), "{text}");
    }
}

/// The names an author gives binders are the function's and print back;
/// a function with none is named by the printer. Falsified by printing
/// from types alone: `item` came back `t2`.
#[test]
fn binders_keep_their_names() {
    let src = "(query a ((w Int)) (list Text) (map (filter t (fn (item) (== item.id w))) (fn (row) row.name)))";
    let d = build(&format!("{ROWS}{src}")).unwrap();
    let f = d.lib.get(&d.module.bindings[0].1).unwrap().f.clone();
    assert_eq!(f.names, ["w", "item", "row"]);
    let text = print(&d.module, std::slice::from_ref(&f)).unwrap();
    assert!(text.ends_with(&format!("{src}\n")), "{text}");
    let bare = print(&d.module, &[f.unnamed()]).unwrap();
    assert!(bare.contains("(fn (t2) (== t2.id n))"), "{bare}");
}

/// A directory is its files read together in sorted path order, below it
/// too: one module however the forms are spread, and a name declared
/// twice refused with both places. Falsified by reading only the top of
/// the directory: `b/c.ark`'s `twice` was no function.
#[test]
fn a_domain_is_a_directory_of_files() {
    let dir = std::env::temp_dir().join(format!("ark-sexp-dir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("b")).unwrap();
    let files = [
        ("schema.ark", ROWS),
        ("a.ark", "; the first\n(helper once ((n Int)) Int (+ n 1))\n"),
        ("b/c.ark", "(helper twice ((n Int)) Int (once (once n)))\n"),
    ];
    for (f, t) in files {
        std::fs::write(dir.join(f), t).unwrap();
    }
    let whole = build(&files.map(|f| f.1).concat()).unwrap();
    let d = ark::sexp::build_dir(&dir).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d.module, whole.module);
    std::fs::write(dir.join("b/d.ark"), "\n(helper once ((n Int)) Int n)\n").unwrap();
    let e = ark::sexp::build_dir(&dir).err().unwrap().0;
    assert_eq!(e, "once is declared twice: at a.ark:2:1 and at b/d.ark:2:1");
    std::fs::write(dir.join("b/d.ark"), "(helper other ((n Int)) Int m)\n").unwrap();
    let e = ark::sexp::build_dir(&dir).err().unwrap().0;
    assert!(e.starts_with("b/d.ark:1:29: `m` is no variable"), "{e}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A `let`'s tuple that is a row of one table is written as that row's
/// record, and a `map`'s lambda gives the record its result's element
/// wants: each the bytes the tuple is, printed as written. Falsified by
/// printing a `map`'s lambda with no type wanted: the record came back a
/// tuple, which the lowering then refused; and by printing a `let`'s row
/// as a tuple: `(as (row t) …)` was not printed.
#[test]
fn records_take_the_type_their_place_wants() {
    let row = "(mutator a ((k Int)) (let ((row (as (row t) (record (id k) (name \"x\") (next none))))) (insert t row)))";
    let tuple = "(mutator a ((k Int)) (let ((row (tuple k \"x\" (as (option Int) none)))) (insert t row)))";
    assert_eq!(unnamed(row), unnamed(tuple));
    let pairs = "(type Pair (record (a Int) (b Text)))\n(query p () (list Pair) (map t (fn (r) (record (a r.id) (b r.name)))))";
    for src in [row, pairs] {
        let d = build(&format!("{ROWS}{src}")).unwrap_or_else(|e| panic!("{e}\n{src}"));
        let fns: Vec<Function> = d.bodies.keys().map(|h| d.lib.get(h).unwrap().f.clone()).collect();
        let text = print(&d.module, &fns).unwrap();
        let last = src.lines().last().unwrap();
        assert!(text.contains(&ark::sexp::layout(&ark::sexp::read(last).unwrap()[0], 0)), "{text}");
    }
    let e = build(&format!(
        "{ROWS}(type Pair (record (a Int) (b Text)))\n(query p () (list Pair) (map t (fn (r) (tuple r.id r.name))))"
    ))
    .err()
    .unwrap()
    .0;
    assert!(e.contains("a record is written (record"), "{e}");
}
