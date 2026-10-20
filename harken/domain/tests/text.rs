//! harken's domain is its directory of ArkLang (§1.15): every file, its
//! comments set aside, is what the printer writes of the forms it holds —
//! so `print(build(t)) == t` file by file.

use std::collections::BTreeMap;

use ark::sexp::{forms, layout, read, uncommented, Sx, K};

/// The name a top-level form declares: a table's, a type's, a function's.
fn declared(x: &Sx) -> String {
    match &x.k {
        K::List(xs) => match &xs[1].k {
            K::Sym(n) => n.clone(),
            _ => declared(&xs[1]),
        },
        _ => panic!("a form"),
    }
}

/// Every file is the printer's text of its forms, in its order. Falsified
/// by breaking `playlists.numbered`'s one line in two: the file was not
/// what the printer writes.
#[test]
fn every_file_is_its_forms_printed() {
    let d = harken_domain::text();
    let printed: BTreeMap<String, Sx> = forms(&d.module, &d.functions()).unwrap().into_iter().collect();
    let mut files: Vec<_> = std::fs::read_dir(harken_domain::DIR).unwrap().map(|e| e.unwrap().path()).collect();
    files.retain(|f| f.extension().is_some_and(|x| x == "ark"));
    files.sort();
    assert!(files.len() >= 5, "{files:?}");
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        let mut want = vec![];
        for x in read(&text).unwrap() {
            let K::List(xs) = &x.k else { panic!("a form") };
            let x = match &xs[0].k {
                K::Sym(h) if h == "schema" => Sx {
                    at: (0, 0),
                    k: K::List(
                        [xs[0].clone()]
                            .into_iter()
                            .chain(xs[1..].iter().map(|t| printed[&declared(t)].clone()))
                            .collect(),
                    ),
                },
                _ => printed[&declared(&x)].clone(),
            };
            want.push(layout(&x, 0));
        }
        let (want, got) = (want.join("\n\n") + "\n", uncommented(&text));
        let at = want.lines().zip(got.lines()).position(|(a, b)| a != b);
        assert!(
            want == got,
            "{}, its comments aside, line {at:?}: the printer writes {:?}, the file says {:?}",
            f.display(),
            at.and_then(|i| want.lines().nth(i)),
            at.and_then(|i| got.lines().nth(i))
        );
    }
}
