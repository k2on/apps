//! `gen-unicode UCD-DIR OUT`: the three Unicode tables of §5, from the
//! Unicode Character Database, as `rust/ark/src/unicode_tables.rs`.
//!
//! The file is checked in, so building needs no UCD download; this is how
//! it moves when the pinned Unicode version does, and it is deterministic:
//! the same three files give the same bytes, so a regeneration that changes
//! nothing shows nothing in a diff. Three things are read:
//!
//! - `PropList.txt`, property `White_Space`;
//! - `DerivedCoreProperties.txt`, property `Alphabetic`;
//! - `UnicodeData.txt`, general categories `Nd`, `Nl` and `No` (which,
//!   with `Alphabetic`, is `char::is_alphanumeric`), and field 13, the
//!   simple lowercase mapping.
//!
//! `UnicodeData.txt` writes a large block of identical characters as two
//! lines, `<…, First>` and `<…, Last>`; the category of such a pair covers
//! the whole range. None of those blocks is `Nd`, `Nl` or `No` today, but
//! the pairing is honoured rather than assumed away.
//!
//! The tables are written one pair to a line, which is the layout rustfmt
//! gives them, so the output is formatted as written.

use std::process::exit;

/// The Unicode version the tables are pinned to, and which the header and
/// `UNICODE_VERSION` name. Moving it is regenerating from that version's
/// files.
const VERSION: &str = "16.0.0";

type Range = (u32, u32);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, out] = args.as_slice() else {
        eprintln!("usage: gen-unicode <ucd-directory> <output>");
        exit(1);
    };
    let read = |name: &str| {
        let path = format!("{dir}/{name}");
        std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("gen-unicode: {path}: {e}");
            exit(1)
        })
    };
    let white = merge(property("White_Space", &read("PropList.txt")));
    let alphabetic = property("Alphabetic", &read("DerivedCoreProperties.txt"));
    let unicode_data = read("UnicodeData.txt");
    let rows = unicode_rows(&unicode_data);
    let numeric: Vec<Range> = rows
        .iter()
        .filter(|(_, cat, _)| ["Nd", "Nl", "No"].contains(cat))
        .map(|(r, _, _)| *r)
        .collect();
    let alnum = merge(alphabetic.iter().chain(&numeric).copied().collect());
    let mut lower: Vec<Range> = rows
        .iter()
        .filter_map(|&((lo, hi), _, to)| to.filter(|_| lo == hi).map(|to| (lo, to)))
        .collect();
    lower.sort_by_key(|&(from, _)| from);
    std::fs::write(out, render(&white, &alnum, &lower)).unwrap_or_else(|e| {
        eprintln!("gen-unicode: {out}: {e}");
        exit(1)
    });
    println!("wrote {out}");
    println!("  White_Space ranges:        {}", white.len());
    println!("  Alphabetic ranges (input): {}", alphabetic.len());
    println!("  Nd|Nl|No ranges (input):   {}", numeric.len());
    println!("  alphanumeric ranges:       {}", alnum.len());
    println!("  simple lowercase mappings: {}", lower.len());
}

// Parsing ------------------------------------------------------------------

/// The ranges a property covers in a `PropList`-style file: lines of
/// `XXXX[..YYYY] ; Property # comment`.
fn property(name: &str, file: &str) -> Vec<Range> {
    file.lines()
        .filter_map(|line| {
            let body = line.split('#').next().unwrap_or("");
            let mut parts = body.split(';').map(str::trim);
            let (range, prop) = (parts.next()?, parts.next()?);
            if prop != name {
                return None;
            }
            match range.split_once("..") {
                Some((a, b)) => Some((hex(a)?, hex(b)?)),
                None => hex(range).map(|c| (c, c)),
            }
        })
        .collect()
}

/// Every row of `UnicodeData.txt` as (range, general category, simple
/// lowercase mapping), with `First`/`Last` pairs folded into one range.
fn unicode_rows(file: &str) -> Vec<(Range, &str, Option<u32>)> {
    let lines: Vec<Vec<&str>> = file.lines().filter(|l| !l.is_empty()).map(|l| l.split(';').collect()).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let a = &lines[i];
        if let Some(b) = lines.get(i + 1) {
            if field(a, 1).ends_with(", First>") && field(b, 1).ends_with(", Last>") {
                if let (Some(lo), Some(hi)) = (hex(field(a, 0)), hex(field(b, 0))) {
                    out.push(((lo, hi), field(a, 2), None));
                    i += 2;
                    continue;
                }
            }
        }
        if let Some(c) = hex(field(a, 0)) {
            out.push(((c, c), field(a, 2), hex(field(a, 13))));
        }
        i += 1;
    }
    out
}

fn field<'a>(fs: &[&'a str], i: usize) -> &'a str {
    fs.get(i).copied().unwrap_or("")
}

/// Sort and coalesce ranges; adjacent and overlapping ranges become one.
fn merge(mut rs: Vec<Range>) -> Vec<Range> {
    rs.sort_by_key(|&(lo, _)| lo);
    let mut out: Vec<Range> = Vec::new();
    for (lo, hi) in rs {
        match out.last_mut() {
            Some(last) if lo <= last.1 + 1 => last.1 = last.1.max(hi),
            _ => out.push((lo, hi)),
        }
    }
    out
}

fn hex(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    u32::from_str_radix(s, 16).ok()
}

// Rendering -----------------------------------------------------------------

fn render(white: &[Range], alnum: &[Range], lower: &[Range]) -> String {
    let table = |doc: String, name: &str, rows: &[Range]| {
        let mut s = format!("{doc}\nstatic {name}: [(u32, u32); {}] = [\n", rows.len());
        for (a, b) in rows {
            s.push_str(&format!("    (0x{a:04X}, 0x{b:04X}),\n"));
        }
        s.push_str("];\n");
        s
    };
    [
        HEADER.replace("{VERSION}", VERSION),
        table(format!("/// {} ranges.", white.len()), "WHITE_SPACE", white),
        table(format!("/// {} ranges.", alnum.len()), "ALPHANUMERIC", alnum),
        table(format!("/// {} mappings, sorted by source code point.", lower.len()), "LOWER", lower),
    ]
    .join("\n")
}

/// Everything above the tables. It names the Haskell program that wrote
/// the file first, so that this one regenerates it byte for byte.
const HEADER: &str = "//! Unicode character properties, pinned to UCD {VERSION}.
//!
//! GENERATED by tools/GenUnicode.hs from the Unicode {VERSION}
//! Character Database. Do not edit: regenerate.
//!
//! A runtime is conformant when its predicates agree with the spec's on
//! every code point, which is why the Unicode version is part of the spec
//! rather than whatever a platform's library happens to carry: two
//! runtimes on two operating systems must sort, match and lowercase the
//! same text the same way. These tables are the spec's, so they cannot
//! disagree with it.
//!
//! The tables are inclusive code-point ranges, sorted and coalesced, and
//! the simple lowercase mapping as (from, to) pairs sorted by source, each
//! looked up by binary search.
//!
//! Core only: nothing here needs `std`.

use core::cmp::Ordering;

/// The Unicode version every table here was generated from.
pub const UNICODE_VERSION: &str = \"{VERSION}\";

/// Property `White_Space` (PropList.txt).
pub fn is_white_space(c: char) -> bool {
    in_ranges(&WHITE_SPACE, c as u32)
}

/// `Alphabetic` (DerivedCoreProperties.txt) or a general category of
/// `Nd`, `Nl` or `No` (UnicodeData.txt) - the definition of
/// `char::is_alphanumeric`, pinned to this Unicode version rather than
/// to the one the compiler's `core` was built with.
pub fn is_alphanumeric(c: char) -> bool {
    in_ranges(&ALPHANUMERIC, c as u32)
}

/// The simple lowercase mapping (UnicodeData.txt field 13); identity
/// where the database gives none.
///
/// Simple, not full: SpecialCasing.txt is not consulted, so U+0130
/// becomes U+0069, one code point, as the spec has it.
pub fn to_lower_simple(c: char) -> char {
    let n = c as u32;
    match LOWER.binary_search_by(|&(from, _)| from.cmp(&n)) {
        Ok(i) => char::from_u32(LOWER[i].1).unwrap_or(c),
        Err(_) => c,
    }
}

/// Is the code point inside one of the sorted, disjoint ranges?
fn in_ranges(table: &[(u32, u32)], n: u32) -> bool {
    table
        .binary_search_by(|&(lo, hi)| {
            if hi < n {
                Ordering::Less
            } else if lo > n {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        })
        .is_ok()
}
";
