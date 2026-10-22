//! The typed Kotlin and Swift the phones call harken with are what the
//! generator writes from the domain now (`ark-dev gen`): regenerating moves
//! nothing. `ARK_WRITE_GEN=1` writes them instead. And the domain read from
//! its source (`ark_dev::gen::source`) is the compiled domain, so that
//! `ark-dev gen` writes from it, with no compiler, what the compiled domain
//! would.

use std::path::{Path, PathBuf};

use ark_dev::gen::app::{self, HARKEN};
use ark_dev::gen::{kotlin, source, swift, walk};
use ark_explorer::Catalog;

const KOTLIN: &str = "../android/app/src/main/kotlin/dev/harken/android/gen/Harken.kt";
const SWIFT: &str = "../ios/Harken/Gen/Harken.swift";

fn held(path: &str, now: String) {
    let path = format!("{}/{path}", env!("CARGO_MANIFEST_DIR"));
    if std::env::var_os("ARK_WRITE_GEN").is_some() {
        std::fs::create_dir_all(std::path::Path::new(&path).parent().unwrap()).unwrap();
        return std::fs::write(&path, now).unwrap();
    }
    let was = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}; ARK_WRITE_GEN=1 writes it"));
    assert!(was == now, "{path} is not what the domain generates now; ARK_WRITE_GEN=1 writes it");
}

/// The view rows the domain hands UniFFI (`#[ark(ffi_view)]`): of these the
/// generator writes only their JSON, and their views are read typed.
const TYPED: &[&str] = &["LibraryEntry", "Playlist"];

/// Falsified by a field added to `CreatePlaylist` (`note: String`): both
/// files differed from what was generated; and by `Playlist` left out of
/// `TYPED`: the Kotlin declared `Playlist` again and read its views as JSON.
#[test]
fn the_phones_types_are_the_domains() {
    let w = walk("harken", &Catalog::of(&harken_domain::domain())).typed("uniffi.harken_domain", TYPED);
    held(KOTLIN, kotlin::write(&w, "dev.harken.android.gen"));
    held(SWIFT, swift::write(&w));
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn read() -> source::Read {
    source::read(&source::Options {
        dir: root().join(HARKEN.domain),
        crate_name: HARKEN.crate_name.into(),
        features: HARKEN.features.iter().map(|f| f.to_string()).collect(),
    })
    .unwrap_or_else(|e| panic!("{e}"))
}

/// The catalog read from the domain's source is the compiled domain's,
/// whole — every table (columns, key, indexes, references, space) and every
/// function (name, kind, input, answer, refinements) — and its twins are
/// the types UniFFI declares. Falsified by the reader's `snake` keeping a
/// type's name as it is (every table differed), by its `said` dropping a
/// `why` (`create_playlist`'s checks differed), and by the job's `due`
/// answering a list of its rows' ids (`look.due` differed).
#[test]
fn the_source_reads_as_the_compiled_domain() {
    let (read, compiled) = (read(), Catalog::of(&harken_domain::domain()));
    for (a, b) in read.catalog.schema.tables.iter().zip(&compiled.schema.tables) {
        assert_eq!(a, b, "table {}", b.name);
    }
    assert_eq!(read.catalog.schema, compiled.schema);
    let names = |c: &Catalog| c.functions.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&read.catalog), names(&compiled));
    for (a, b) in read.catalog.functions.iter().zip(&compiled.functions) {
        assert_eq!(a, b, "function {}", b.name);
    }
    let mut twins: Vec<&str> = read.twins.iter().map(|t| t.name.as_str()).collect();
    twins.sort();
    assert_eq!(twins, TYPED);
    assert!(read.scaffolding);
}

/// `ark-dev gen` writes, from the source alone, the Kotlin and Swift the
/// compiled domain generates, and every file it writes is the one checked
/// in (UniFFI's for the twins too: what `tests/twins.rs` holds to a built
/// library). Falsified by the generated `Harken.swift` checked in with a
/// line taken out, and by the twins' view object's docstring changed in
/// `gen::uniffi` (both UniFFI files differed).
#[test]
fn ark_dev_gen_writes_from_the_source_what_the_build_would() {
    let r = root();
    let (files, _) = app::generate(&HARKEN, &r, &r.join(HARKEN.domain)).unwrap_or_else(|e| panic!("{e}"));
    let w = walk("harken", &Catalog::of(&harken_domain::domain())).typed("uniffi.harken_domain", TYPED);
    assert!(
        files[HARKEN.kotlin] == kotlin::write(&w, HARKEN.kotlin_package),
        "Harken.kt from the source differs"
    );
    assert!(files[HARKEN.swift] == swift::write(&w), "Harken.swift from the source differs");
    assert_eq!(files.len(), 4);
    for (path, text) in &files {
        let was = std::fs::read_to_string(r.join(path)).unwrap_or_else(|e| panic!("{path}: {e}; `ark-dev gen` writes it"));
        assert!(
            was == *text,
            "{path} is not what the domain's source generates now; `ark-dev gen` writes it"
        );
    }
}

/// What the reader cannot read it refuses, with the file and line: a field
/// of a type outside the crate it does not know. Falsified by the reader
/// taking an unknown type for `()` (it read, and described the field as
/// unit).
#[test]
fn the_reader_refuses_what_it_cannot_read() {
    let dir = tempfile::tempdir().unwrap();
    let from = root().join(HARKEN.domain);
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    for f in std::fs::read_dir(from.join("src")).unwrap() {
        let p = f.unwrap().path();
        std::fs::copy(&p, dir.path().join("src").join(p.file_name().unwrap())).unwrap();
    }
    let types = dir.path().join("src/types.rs");
    let text = std::fs::read_to_string(&types).unwrap();
    let text = text.replacen("pub struct Looked {\n", "pub struct Looked {\n    pub at: std::time::Instant,\n", 1);
    std::fs::write(&types, text).unwrap();
    let e = source::read(&source::Options {
        dir: dir.path().into(),
        crate_name: HARKEN.crate_name.into(),
        features: HARKEN.features.iter().map(|f| f.to_string()).collect(),
    })
    .err()
    .expect("refused");
    assert!(e.contains("types.rs:") && e.contains("std::time::Instant"), "{e}");
}
