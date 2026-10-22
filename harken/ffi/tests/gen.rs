//! The typed Kotlin and Swift the phones call harken with are what the
//! generator writes from the domain now (`ark-dev gen`): regenerating moves
//! nothing. `ARK_WRITE_GEN=1` writes them instead.

use ark_dev::gen::{kotlin, swift, walk};
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

/// The types the domain hands UniFFI (`#[ark(ffi)]`, the experiment of one
/// generator): of these the generator writes only their JSON.
const TYPED: &[&str] = &[
    "CreatePlaylist",
    "JobLooked",
    "LibraryEntry",
    "Look",
    "LookCancel",
    "LookClaim",
    "LookDue",
    "LookFinish",
    "LookOutcome",
    "Looked",
    "Playlist",
    "PlaylistInput",
    "PlaylistsOf",
];

/// Falsified by a field added to `CreatePlaylist` (`note: String`): both
/// files differed from what was generated; and by a name of `TYPED` misspelt
/// (`PlaylistsOfX`): the Kotlin declared `PlaylistsOf` again.
#[test]
fn the_phones_types_are_the_domains() {
    let w = walk("harken", &Catalog::of(&harken_domain::domain())).typed("uniffi.harken_domain", TYPED);
    held(KOTLIN, kotlin::write(&w, "dev.harken.android.gen"));
    held(SWIFT, swift::write(&w));
}
