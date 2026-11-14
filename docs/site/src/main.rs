//! The documentation site, written: `docs <dir>` writes every page of the
//! docs app as static HTML (ui/docs/app.md, "The Browser's Modes"), the
//! desktop pictures drawn by arkui itself, sitemap.xml and catalogue.json
//! (`docs::write`). The module that takes the pages over (`docs-web`) is
//! built for wasm32 apart and laid beside them as `pkg/`.
//!
//!   docs <dir> [--base /apps/docs/] [--site https://k2on.github.io] [--version dev] [--no-pictures]
//!
//! `docs --json` prints the catalogue for a site that wants it as data.
use std::path::PathBuf;

use docs::write::{write, Options};

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut out = None;
    let mut o = Options {
        base: "/".into(),
        site: "https://k2on.github.io".into(),
        pictures: true,
        version: "dev".into(),
    };
    while let Some(a) = args.next() {
        match a.as_str() {
            "--json" => {
                print!("{}", docs::catalogue::json());
                return Ok(());
            }
            "--base" => o.base = args.next().expect("--base /path/"),
            "--site" => o.site = args.next().expect("--site https://host"),
            "--version" => o.version = args.next().expect("--version v"),
            "--no-pictures" => o.pictures = false,
            dir => out = Some(PathBuf::from(dir)),
        }
    }
    if !o.base.ends_with('/') {
        o.base.push('/');
    }
    let out = out.unwrap_or_else(|| PathBuf::from("out"));
    let pages = write(&out, &o)?;
    println!("wrote {} pages in {}", pages.len(), out.display());
    Ok(())
}
