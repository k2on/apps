//! Reading a system's events (docs/plan-observe.md): `arkdb tail`,
//! `arkdb trace`, `arkdb peer`. A server keeps its system's events — its
//! own and every device's — as lines of JSON (`ark_peer::observe::Files`);
//! these read them, or a test's, or any stream of them on stdin, and print
//! text a person and an agent read alike.
//!
//! ```text
//! arkdb tail  [--from DIR|FILE|-] [-n N] [--follow] [--peer P] [--refused] [--grep S]
//! arkdb trace ID [--from …]     everything a tap or an entry caused, as a tree
//! arkdb peer  P  [--from …]     one peer's events, in order
//! ```
//!
//! `--from` defaults to `$ARKDB_EVENTS`, then `./events`. An id is any prefix
//! of a trace, a span or an entry (hex); ids print as their first 8 digits.

use std::collections::{BTreeMap, BTreeSet};
use std::io::BufRead;
use std::path::{Path, PathBuf};

use ark::frames::Event;

/// Where the events are read from.
fn source(args: &[String]) -> String {
    flag(args, "--from")
        .or_else(|| std::env::var("ARKDB_EVENTS").ok())
        .unwrap_or_else(|| "events".into())
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

/// The words of `args` that are no flag or a flag's value.
fn words(args: &[String]) -> Vec<String> {
    let valued = ["--from", "-n", "--peer", "--grep"];
    let mut out = vec![];
    let mut skip = false;
    for a in args {
        if std::mem::take(&mut skip) {
            continue;
        }
        match a.as_str() {
            a if valued.contains(&a) => skip = true,
            a if a.starts_with("--") => {}
            a => out.push(a.to_string()),
        }
    }
    out
}

/// The event files of `dir` (`events-<n>.jsonl`), oldest first.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut all: Vec<(u64, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let n = name.strip_prefix("events-")?.strip_suffix(".jsonl")?.parse().ok()?;
            Some((n, e.path()))
        })
        .collect();
    all.sort();
    all.into_iter().map(|(_, p)| p).collect()
}

fn parse(line: &str) -> Option<Event> {
    serde_json::from_str(line.trim()).ok()
}

/// Every event `from` holds: a directory of event files, one file, or `-`.
pub fn read(from: &str) -> Result<Vec<Event>, String> {
    let lines: Vec<String> = match from {
        "-" => std::io::stdin().lock().lines().map_while(Result::ok).collect(),
        p if Path::new(p).is_dir() => {
            let mut all = vec![];
            for f in files(Path::new(p)) {
                all.extend(std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?.lines().map(String::from));
            }
            all
        }
        p => std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?.lines().map(String::from).collect(),
    };
    Ok(lines.iter().filter_map(|l| parse(l)).collect())
}

fn field<'a>(e: &'a Event, k: &str) -> Option<&'a str> {
    e.fields.iter().find(|(f, _)| f == k).map(|(_, v)| v.as_str())
}

/// An id as it prints: its first 8 digits.
fn short(v: &str) -> &str {
    let hexy = v.len() >= 16 && v.bytes().all(|b| b.is_ascii_hexdigit());
    if hexy { &v[..8] } else { v }
}

fn clock(ms: i64) -> String {
    let s = ms.rem_euclid(86_400_000);
    format!("{:02}:{:02}:{:02}.{:03}", s / 3_600_000, s / 60_000 % 60, s / 1000 % 60, s % 1000)
}

fn who(e: &Event) -> String {
    match (e.peer.as_str(), field(e, "from")) {
        ("", Some(f)) => f.to_string(),
        (p, _) => p.to_string(),
    }
}

/// One event as one line: when (UTC), how loud, who, what, and its fields.
pub fn line(e: &Event) -> String {
    let fields: Vec<String> = e
        .fields
        .iter()
        .filter(|(k, v)| !v.is_empty() && k != "from")
        .map(|(k, v)| match v.contains(' ') {
            true => format!("{k}=\"{v}\""),
            false => format!("{k}={}", short(v)),
        })
        .collect();
    format!("{} {:5} {} {} {}", clock(e.at), e.level, who(e), e.msg, fields.join(" ")).trim_end().to_string()
}

fn matches(e: &Event, args: &[String]) -> bool {
    let peer = flag(args, "--peer").is_none_or(|p| who(e).contains(&p) || e.peer.contains(&p));
    let refused = !has(args, "--refused") || e.msg == "refused" || e.level == "warn" || e.level == "error";
    let grep = flag(args, "--grep").is_none_or(|g| line(e).contains(&g));
    peer && refused && grep
}

/// `arkdb tail`: the last events (`-n`, 50 by default), as they come with
/// `--follow`.
fn tail(args: &[String]) -> Result<(), String> {
    let from = source(args);
    let n: usize = flag(args, "-n").and_then(|n| n.parse().ok()).unwrap_or(50);
    let all: Vec<Event> = read(&from)?.into_iter().filter(|e| matches(e, args)).collect();
    for e in &all[all.len().saturating_sub(n)..] {
        println!("{}", line(e));
    }
    if !has(args, "--follow") {
        return Ok(());
    }
    // Followed: the newest file read on from where it was, a newer one when
    // the server starts it.
    let dir = Path::new(&from);
    let (mut path, mut at) = match files(dir).last() {
        Some(p) => (p.clone(), std::fs::metadata(p).map_or(0, |m| m.len())),
        None => (dir.join("events-000000.jsonl"), 0),
    };
    loop {
        std::thread::sleep(std::time::Duration::from_millis(300));
        if let Some(newest) = files(dir).last().filter(|p| **p != path) {
            (path, at) = (newest.clone(), 0);
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        if bytes.len() as u64 <= at {
            continue;
        }
        let new = String::from_utf8_lossy(&bytes[at as usize..]).to_string();
        let Some(end) = new.rfind('\n') else { continue };
        at += end as u64 + 1;
        for e in new[..end].lines().filter_map(parse).filter(|e| matches(e, args)) {
            println!("{}", line(&e));
        }
    }
}

/// `arkdb peer P`: one peer's events, in order.
fn peer(args: &[String]) -> Result<(), String> {
    let w = words(args);
    let p = w.first().ok_or("arkdb peer P: which peer (a prefix of its label or its login)")?;
    for e in read(&source(args))?.iter().filter(|e| who(e).contains(p.as_str()) || e.peer.contains(p.as_str())) {
        println!("{}", line(e));
    }
    Ok(())
}

/// One step of a trace: an entry, or a span opened (a tap).
#[derive(Default)]
struct Node {
    at: i64,
    parent: Option<String>,
    what: String,
    by: String,
    /// What became of it, where: `peer: ok 381`, `peer: refused: why`.
    became: Vec<String>,
    /// Who saw it go by, besides its author.
    seen: BTreeSet<String>,
    /// What was said under it: a job's lines, an app's events.
    said: Vec<String>,
}

/// The trace an id names: a trace's, or that of the span or entry it is.
fn trace_of(all: &[Event], id: &str) -> Option<String> {
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for e in all {
        let named = ["trace", "span", "parent", "entry"].iter().any(|k| field(e, k).is_some_and(|v| v.starts_with(id)));
        if let (true, Some(t)) = (named, field(e, "trace").filter(|t| !t.is_empty())) {
            *count.entry(t.to_string()).or_default() += 1;
        }
    }
    count.into_iter().max_by_key(|(_, n)| *n).map(|(t, _)| t)
}

/// `arkdb trace ID`: everything the trace ID names caused, on every peer,
/// as a tree — each entry with who authored it, what every peer that
/// judged it made of it, and what was said under it.
fn trace(args: &[String]) -> Result<(), String> {
    let w = words(args);
    let id = w.first().ok_or("arkdb trace ID: a prefix of a trace, a span or an entry")?;
    print!("{}", render_trace(&read(&source(args))?, id)?);
    Ok(())
}

/// What `arkdb trace ID` prints, over `all`.
pub fn render_trace(all: &[Event], id: &str) -> Result<String, String> {
    let t = trace_of(all, id).ok_or(format!("no trace names {id}"))?;
    let mut nodes: BTreeMap<String, Node> = BTreeMap::new();
    let mine: Vec<&Event> = all.iter().filter(|e| field(e, "trace") == Some(t.as_str())).collect();
    for e in &mine {
        match e.msg.as_str() {
            "begin" => {
                let span = field(e, "span").unwrap_or_default().to_string();
                let n = nodes.entry(span).or_default();
                (n.at, n.what, n.by) = (e.at, field(e, "name").unwrap_or("begin").to_string(), who(e));
            }
            "entry" => {
                let span = field(e, "span").unwrap_or_default().to_string();
                let n = nodes.entry(span).or_default();
                let kind = field(e, "kind").unwrap_or_default();
                if n.what.is_empty() || (kind == "push" && field(e, "dir") == Some("out")) {
                    (n.at, n.what) = (e.at, field(e, "func").unwrap_or_default().to_string());
                    n.by = format!("{} \u{b7} {}", field(e, "actor").unwrap_or_default(), field(e, "session").unwrap_or_default());
                    n.parent = field(e, "parent").filter(|p| !p.is_empty()).map(String::from);
                }
                n.seen.insert(who(e));
            }
            _ => {}
        }
    }
    // The verdicts name an entry, not its trace; and what was said inside a
    // span (a job's run) names the span.
    for e in all {
        let entry = field(e, "entry").unwrap_or_default();
        if let Some(n) = nodes.get_mut(entry) {
            match e.msg.as_str() {
                "ok" => n.became.push(format!("{}: ok {}", who(e), field(e, "seq").unwrap_or_default())),
                "refused" => n.became.push(format!("{}: refused: {}", who(e), field(e, "why").unwrap_or_default())),
                "held" => n.became.push(format!("{}: held", who(e))),
                _ => {}
            }
        }
    }
    for e in &mine {
        let quiet = ["begin", "entry", "ok", "refused", "held", "mutate", "changed", "confirmed", "to", "page"];
        if quiet.contains(&e.msg.as_str()) {
            continue;
        }
        if let Some(n) = field(e, "span").and_then(|s| nodes.get_mut(s)) {
            n.said.push(line(e));
        }
    }
    let mut kids: BTreeMap<String, Vec<(i64, String)>> = BTreeMap::new();
    let mut roots = vec![];
    for (id, n) in &nodes {
        match n.parent.as_ref().filter(|p| nodes.contains_key(*p)) {
            Some(p) => kids.entry(p.clone()).or_default().push((n.at, id.clone())),
            None => roots.push((n.at, id.clone())),
        }
    }
    roots.sort();
    kids.values_mut().for_each(|k| k.sort());
    let mut out = format!("trace {}\n", short(&t));
    for (_, r) in roots {
        print_node(&mut out, &nodes, &kids, &r, 0);
    }
    Ok(out)
}

fn print_node(out: &mut String, nodes: &BTreeMap<String, Node>, kids: &BTreeMap<String, Vec<(i64, String)>>, id: &str, depth: usize) {
    use std::fmt::Write;
    let n = &nodes[id];
    let pad = "  ".repeat(depth);
    let _ = writeln!(out, "{pad}{} {}  {}  by {}", clock(n.at), n.what, short(id), n.by);
    for b in &n.became {
        let _ = writeln!(out, "{pad}    {b}");
    }
    if n.seen.len() > 1 {
        let _ = writeln!(out, "{pad}    seen by {} peers", n.seen.len());
    }
    for s in &n.said {
        let _ = writeln!(out, "{pad}    \u{b7} {s}");
    }
    for (_, k) in kids.get(id).into_iter().flatten() {
        print_node(out, nodes, kids, k, depth + 1);
    }
}

/// `arkdb tail|trace|peer …`.
pub fn main(cmd: &str, args: &[String]) -> Result<(), String> {
    match cmd {
        "tail" => tail(args),
        "trace" => trace(args),
        "peer" => peer(args),
        _ => Err(format!("no command {cmd}")),
    }
}
