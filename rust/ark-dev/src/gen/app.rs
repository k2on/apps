//! `ark-dev gen`: every file an app's IDEs read of its domain, written from
//! the domain's source in one run, with no compiler (`docs/plan-v5.md`,
//! Landed G1) — the typed Kotlin and Swift ([`super::kotlin`],
//! [`super::swift`]) of the catalog read from the source
//! ([`super::source`]), and UniFFI's own Kotlin and Swift of its twins
//! ([`super::uniffi`]). A file is written only when it changed; `--check`
//! writes none and fails on any that differs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{kotlin, source, swift, uniffi, walk};

/// An app: its domain's crate and where each generated file goes, every
/// path under the repository's root.
pub struct App {
    /// The domain's name, as the generated stubs are named (`Harken`).
    pub name: &'static str,
    pub domain: &'static str,
    pub crate_name: &'static str,
    /// The features the app's library builds the domain with.
    pub features: &'static [&'static str],
    /// The library the bindings load, and each UniFFI crate's root (its
    /// `uniffi.toml`).
    pub library: &'static str,
    pub roots: &'static [(&'static str, &'static str)],
    pub kotlin_package: &'static str,
    pub kotlin: &'static str,
    pub swift: &'static str,
    pub uniffi_kotlin: &'static str,
    pub uniffi_swift: &'static str,
}

/// harken: its domain, and the phones' generated files.
pub const HARKEN: App = App {
    name: "harken",
    domain: "harken/domain",
    crate_name: "harken_domain",
    features: &["ffi"],
    library: "harken_ffi",
    roots: &[
        ("harken_domain", "harken/domain"),
        ("ark_ffi", "rust/ark-ffi"),
        ("harken_ffi", "harken/ffi"),
    ],
    kotlin_package: "dev.harken.android.gen",
    kotlin: "harken/android/app/src/main/kotlin/dev/harken/android/gen/Harken.kt",
    swift: "harken/ios/Harken/Gen/Harken.swift",
    uniffi_kotlin: "harken/android/app/src/main/kotlin/dev/harken/android/gen/uniffi/harken_domain.kt",
    uniffi_swift: "harken/ios/Harken/Gen/harken_domain.swift",
};

/// What a run took, step by step.
#[derive(Default, Debug)]
pub struct Took {
    pub parse: Duration,
    pub emit: Duration,
    pub uniffi: Duration,
    pub write: Duration,
}

/// Every file `app`'s domain at `source` (its crate's directory) generates,
/// by its path under `root`, and what each step took.
pub fn generate(app: &App, root: &Path, source: &Path) -> Result<(BTreeMap<String, String>, Took), String> {
    let mut took = Took::default();
    let t = Instant::now();
    let read = source::read(&source::Options {
        dir: source.into(),
        crate_name: app.crate_name.into(),
        features: app.features.iter().map(|f| f.to_string()).collect(),
    })?;
    took.parse = t.elapsed();
    let t = Instant::now();
    let package = format!("uniffi.{}", app.crate_name);
    let typed: Vec<&str> = read.twins.iter().map(|t| t.name.as_str()).collect();
    let w = walk(app.name, &read.catalog).typed(&package, &typed);
    let mut out = BTreeMap::new();
    out.insert(app.kotlin.to_string(), kotlin::write(&w, app.kotlin_package));
    out.insert(app.swift.to_string(), swift::write(&w));
    took.emit = t.elapsed();
    let t = Instant::now();
    if read.scaffolding {
        let items = uniffi::metadata(app.crate_name, &read.twins);
        let roots: BTreeMap<String, PathBuf> = app.roots.iter().map(|(c, d)| (c.to_string(), root.join(d))).collect();
        let scratch = std::env::temp_dir().join(format!("ark-dev-gen-{}", std::process::id()));
        let made = (|| -> Result<(), String> {
            for (lang, file, to) in [
                ("kotlin", format!("uniffi/{0}/{0}.kt", app.crate_name), app.uniffi_kotlin),
                ("swift", format!("{}.swift", app.crate_name), app.uniffi_swift),
            ] {
                let mut files = uniffi::bindings(&items, app.library, app.crate_name, lang, &roots, &scratch)?;
                let text = files.remove(&file).ok_or_else(|| format!("UniFFI wrote no {file}"))?;
                out.insert(to.to_string(), text);
            }
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&scratch);
        made?;
    }
    took.uniffi = t.elapsed();
    Ok((out, took))
}

/// The repository's root above `from`: where `app`'s domain is.
fn root_of(app: &App, from: &Path) -> Option<PathBuf> {
    from.ancestors()
        .find(|d| d.join(app.domain).join("src/lib.rs").exists())
        .map(Path::to_path_buf)
}

/// `ark-dev gen [--source DIR] [--root DIR] [--check]`: one run, as an
/// editor's on-save hook calls it.
pub fn main(args: &[String]) -> Result<String, String> {
    let started = Instant::now();
    let app = &HARKEN;
    let (mut source, mut root, mut check) = (None, None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--check" => check = true,
            "--source" => source = Some(PathBuf::from(it.next().ok_or("--source DIR")?)),
            "--root" => root = Some(PathBuf::from(it.next().ok_or("--root DIR")?)),
            other => return Err(format!("ark-dev gen [--source DIR] [--root DIR] [--check]: not {other}")),
        }
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = match root {
        Some(r) => r,
        None => root_of(app, source.as_deref().unwrap_or(&cwd))
            .or_else(|| root_of(app, &cwd))
            .ok_or_else(|| format!("no {} above {}: --root DIR", app.domain, cwd.display()))?,
    };
    let source = source.unwrap_or_else(|| root.join(app.domain));
    let (files, mut took) = generate(app, &root, &source)?;
    let t = Instant::now();
    let mut differ = BTreeSet::new();
    for (path, text) in &files {
        let p = root.join(path);
        if std::fs::read_to_string(&p).ok().as_deref() == Some(text.as_str()) {
            continue;
        }
        differ.insert(path.clone());
        if !check {
            std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| format!("{}: {e}", p.display()))?;
            std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))?;
        }
    }
    took.write = t.elapsed();
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    let said = format!(
        "{} files, {} {} in {:.1} ms (parse {:.1}, emit {:.1}, uniffi {:.1}, write {:.1})",
        files.len(),
        differ.len(),
        if check { "differ" } else { "written" },
        ms(started.elapsed()),
        ms(took.parse),
        ms(took.emit),
        ms(took.uniffi),
        ms(took.write)
    );
    match check && !differ.is_empty() {
        true => Err(format!(
            "{said}: not what the domain generates now ({}); `ark-dev gen` writes them",
            differ.into_iter().collect::<Vec<_>>().join(", ")
        )),
        false => Ok(format!("ark-dev gen: {said}\n")),
    }
}
