//! UniFFI's Kotlin and Swift for harken's twins, written from the source
//! (`ark_dev::gen::uniffi`), are what UniFFI writes from the library built:
//! the metadata made of the source is the library's, checksums and all,
//! and UniFFI's generators, reading the library as `uniffi-bindgen
//! generate --library` does, write the files checked in. It needs the
//! library for the host (`cargo build -p harken-ffi --lib`), so it is
//! ignored by default: `cargo test -p harken-ffi --test twins -- --ignored`.
//! The nix check `ffi` holds the same files to the release library's
//! bindings, by UniFFI's own `uniffi-bindgen`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ark_dev::gen::app::HARKEN;
use ark_dev::gen::{source, uniffi};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn library() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or(root().join("rust/target"), PathBuf::from);
    let lib = target.join("debug/libharken_ffi.so");
    assert!(lib.exists(), "{}: `cargo build -p harken-ffi --lib` first", lib.display());
    lib
}

/// Falsified by the view's constructor's docstring changed in
/// `gen::uniffi` (its checksum and the constructor's metadata differed),
/// and by `Playlist`'s twin read as of `harken_domain` rather than
/// `harken_domain::schema` (every Playlist item differed).
#[test]
#[ignore]
fn the_twins_metadata_is_the_librarys() {
    let read = source::read(&source::Options {
        dir: root().join(HARKEN.domain),
        crate_name: HARKEN.crate_name.into(),
        features: HARKEN.features.iter().map(|f| f.to_string()).collect(),
    })
    .unwrap_or_else(|e| panic!("{e}"));
    let ours = |m: &uniffi::meta::Metadata| m.module_path().split("::").next() == Some(HARKEN.crate_name);
    let mut made: Vec<_> = uniffi::metadata(HARKEN.crate_name, &read.twins).into_iter().filter(ours).collect();
    let mut built: Vec<_> = uniffi::read_library(&library()).unwrap().into_iter().filter(ours).collect();
    made.sort();
    built.sort();
    for (a, b) in made.iter().zip(&built) {
        assert_eq!(a, b);
    }
    assert_eq!(made.len(), built.len());
}

/// Every metadata item of the library, encoded here, reads back as itself:
/// the encoding is UniFFI's, byte for byte, as each checksum is of it. And
/// UniFFI's generators over the built library write the files `ark-dev
/// gen` checked in. Falsified by a docstring's length written as one byte
/// (the items read back wrong), and by the checked-in Swift with a line
/// taken out.
#[test]
#[ignore]
fn the_built_library_binds_as_the_source_does() {
    let lib = library();
    for m in uniffi::read_library(&lib).unwrap() {
        let b = uniffi::encode(&m).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(uniffi::meta::Metadata::read(&b).unwrap(), m);
    }
    let r = root();
    let roots: BTreeMap<String, PathBuf> = HARKEN.roots.iter().map(|(c, d)| (c.to_string(), r.join(d))).collect();
    let scratch = tempfile::tempdir().unwrap();
    for (lang, file, to) in [
        ("kotlin", "uniffi/harken_domain/harken_domain.kt", HARKEN.uniffi_kotlin),
        ("swift", "harken_domain.swift", HARKEN.uniffi_swift),
    ] {
        let files = uniffi::of_library(&lib, HARKEN.crate_name, lang, &roots, scratch.path()).unwrap();
        let was = std::fs::read_to_string(r.join(to)).unwrap();
        assert!(files[file] == was, "{to} is not what UniFFI writes from the built library");
    }
}
