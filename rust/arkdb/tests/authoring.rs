//! `spec/AUTHORING.md` is held to the sources: every Rust block in it is
//! cut from `rust/arkdb/src/demo.rs` or a file of `harken/domain/src/`,
//! its lines that file's lines in order, indentation aside, a line `// …`
//! standing for any run of them. Falsified by changing one word of the
//! `Item` row in the document (`pub pos: i64` to `pub pos: u64`): "no
//! source holds block 1".

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The Rust blocks of a Markdown text, each its lines.
fn blocks(md: &str) -> Vec<Vec<&str>> {
    let mut out = vec![];
    let mut open: Option<Vec<&str>> = None;
    for l in md.lines() {
        match (&mut open, l.trim()) {
            (None, "```rust") => open = Some(vec![]),
            (Some(_), "```") => out.extend(open.take()),
            (Some(b), t) if !t.is_empty() => b.push(t),
            _ => {}
        }
    }
    out
}

/// Whether `src`'s lines hold `block` in order from some line on, `// …`
/// skipping any run of them.
fn holds(src: &str, block: &[&str]) -> bool {
    let lines: Vec<&str> = src.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let from = |mut i: usize| {
        let mut skip = false;
        for &b in block {
            if b == "// …" {
                skip = true;
                continue;
            }
            loop {
                match lines.get(i) {
                    Some(&l) if l == b => break,
                    Some(_) if skip => i += 1,
                    _ => return false,
                }
            }
            (i, skip) = (i + 1, false);
        }
        true
    };
    (0..lines.len()).any(|i| lines[i] == block[0] && from(i))
}

#[test]
fn every_example_is_cut_from_a_source() {
    let md = std::fs::read_to_string(root().join("spec/AUTHORING.md")).unwrap();
    let mut sources = vec![root().join("rust/arkdb/src/demo.rs")];
    let domain = root().join("harken/domain/src");
    sources.extend(std::fs::read_dir(&domain).unwrap().map(|e| e.unwrap().path()));
    let texts: Vec<String> = sources.iter().map(|p| std::fs::read_to_string(p).unwrap()).collect();
    let bs = blocks(&md);
    assert!(bs.len() >= 5, "the document's examples: {}", bs.len());
    for (n, b) in bs.iter().enumerate() {
        assert!(texts.iter().any(|t| holds(t, b)), "no source holds block {}:\n{}", n + 1, b.join("\n"));
    }
    assert!(md.lines().count() <= 150, "AUTHORING.md is at most 150 lines");
}

/// `spec/README.md`'s index of the kernel names every module of
/// `rust/ark/src` and nothing else. Falsified by adding `view.rs` to the
/// index, a module the kernel does not have: "view.rs is not a module".
#[test]
fn the_index_names_every_module() {
    let md = std::fs::read_to_string(root().join("spec/README.md")).unwrap();
    let table = md.split("## The modules").nth(1).unwrap().split("\n## ").next().unwrap();
    let named: std::collections::BTreeSet<&str> = table
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|w| w.ends_with(".rs") && !w.contains('/'))
        .collect();
    let have: std::collections::BTreeSet<String> = std::fs::read_dir(root().join("rust/ark/src"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".rs"))
        .collect();
    for n in &named {
        assert!(have.contains(*n), "{n} is not a module");
    }
    for n in &have {
        assert!(named.contains(n.as_str()), "{n} is not in the index");
    }
}
