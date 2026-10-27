//! The documentation site, generated.
//!
//! `docs <dir>` writes it: an overview and a page per component
//! ([`components`]), each saying how to use it on the desktop — arkui's
//! modules ([`catalogue`]), with pictures drawn by arkui itself
//! ([`examples`], [`render`]) — and on a phone, Android or iOS, with the
//! latest capture from a real device, fetched by the page from what the
//! `screens` workflow last published. [`site`] writes the HTML.
//!
//! `docs --json` prints the catalogue for a site that wants it as data.
mod catalogue;
mod components;
mod examples;
mod render;
mod site;

use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--json") {
        print!("{}", catalogue::json());
        return Ok(());
    }
    let out = PathBuf::from(args.first().map(String::as_str).unwrap_or("out"));
    site::write(&out, true)?;
    println!("wrote {}", out.display());
    Ok(())
}

#[cfg(test)]
mod tests;
