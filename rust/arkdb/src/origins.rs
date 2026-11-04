//! `arkdb origins [--from DIR]`: where a server has every device's intents —
//! each origin's last `local` judged, the actor it is bound to, and its
//! newest entry — a JSON object a line, the origin last heard from last.
//!
//! What the server judges a push by (`Journal::last_of`) is its log's
//! entries as it opens and, for those a horizon or a migration took from
//! the log, the `local.` journal's `origin` rows: `last`. Beside it, what
//! every log on the disk says — the log and those a migration kept aside
//! (`log.<hash>`) — as `logged`. Where `logged` is past `last` the server
//! has forgotten what it judged: a device pushing on from there is past a
//! gap (`Verdict::Gap`, a warning `arkdb tail --refused` shows), and one pushing again what it
//! was never told was judged would have it judged twice.
//!
//! The logs are read as their records, never opened (an instance folds a
//! log long past its rows as it opens). Nothing is written, unless
//! `--repair`: then every origin forgotten so is put in the journal at what
//! its logs say (`ark_peer::local::passed`, as a migration does), and the
//! server, opened next, judges by it. Its rows are not touched. Stop the
//! server first: it holds its journal in memory and writes it as it moves.

use std::collections::BTreeMap;
use std::path::Path;

use ark::hash::hex;
use ark::log::Logged;
use ark_peer::{Disk, Instance, Local, Memory};
use serde_json::json;

const USAGE: &str = "arkdb origins [--from DIR] [--repair]";

/// One origin as the server has it.
#[derive(Default)]
struct Seen {
    actor: String,
    /// What the server judges by: the journal's, or its log's where higher.
    last: i64,
    journal: Option<i64>,
    /// What every log on the disk says, the ones kept aside among them.
    logged: i64,
    /// Its entries in the server's log, and the newest anywhere.
    entries: usize,
    newest: Option<(String, i64, i64, String, i64)>,
}

pub fn main(args: &[String]) -> Result<String, String> {
    let repair = args.iter().any(|a| a == "--repair");
    let args: Vec<&String> = args.iter().filter(|a| *a != "--repair").collect();
    let dir = match &args[..] {
        [] => ".".to_string(),
        [f, d] if *f == "--from" => d.to_string(),
        _ => return Err(USAGE.into()),
    };
    let log = Path::new(&dir).join("log");
    let mut all: BTreeMap<[u8; 16], Seen> = BTreeMap::new();
    for (origin, actor, last) in journal(&log)? {
        let s = all.entry(origin).or_default();
        (s.actor, s.last, s.journal) = (actor, last, Some(last));
    }
    let mut seen = |name: &str, l: &Logged, live: bool| {
        let e = &l.entry;
        let s = all.entry(e.origin.0).or_default();
        if s.actor.is_empty() {
            s.actor = e.actor.clone();
        }
        s.logged = s.logged.max(e.local);
        if live {
            s.last = s.last.max(e.local);
            s.entries += 1;
        }
        s.newest = Some((name.into(), l.seq, e.local, e.func.clone(), e.now));
    };
    // The aside logs first, oldest written first: the server's own after.
    let mut aside: Vec<_> = std::fs::read_dir(&log)
        .map_err(|e| format!("{}: {e}", log.display()))?
        .flatten()
        .filter_map(|f| {
            let name = f.file_name().into_string().ok()?;
            let at = f.metadata().ok()?.modified().ok()?;
            (name.starts_with("log.") && name != "log.tmp").then_some((at, name))
        })
        .collect();
    aside.sort();
    for (_, name) in aside {
        for l in records(&log, &name)? {
            seen(&name, &l, false);
        }
    }
    for l in records(&log, "log")? {
        seen("log", &l, true);
    }
    let mut rows: Vec<_> = all.into_iter().collect();
    rows.sort_by_key(|(_, s)| s.newest.as_ref().map(|n| n.4));
    let mut out = String::new();
    if repair {
        let forgotten: Vec<_> = rows
            .iter()
            .filter(|(_, s)| s.logged > s.last)
            .map(|(o, s)| (ark::log::Origin(*o), s.actor.clone(), s.logged))
            .collect();
        let n = forgotten.len();
        let mut disk = ark_peer::Files::new(&log).map_err(|e| format!("{}: {e}", log.display()))?;
        ark_peer::local::passed(&mut disk, forgotten).map_err(|e| format!("the journal: {e}"))?;
        for (_, s) in rows.iter_mut().filter(|(_, s)| s.logged > s.last) {
            (s.last, s.journal) = (s.logged, Some(s.logged));
        }
        out += &format!("{n} origins put in the journal\n");
    }
    for (origin, s) in rows {
        let newest = s
            .newest
            .map(|(log, seq, local, func, now)| json!({"log": log, "seq": seq, "local": local, "func": func, "now": now}));
        let row = json!({
            "origin": hex(&origin),
            "actor": s.actor,
            "last": s.last,
            "logged": s.logged,
            "forgotten": s.logged > s.last,
            "journal": s.journal,
            "entries": s.entries,
            "newest": newest,
        });
        out += &row.to_string();
        out.push('\n');
    }
    Ok(out)
}

/// The entries a log file on the server's disk holds, as its records: what
/// a server's or a migration's opening reads, without its fold.
fn records(log: &Path, name: &str) -> Result<Vec<Logged>, String> {
    let path = log.join(name);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut disk = Memory::default();
    disk.0.insert("log".into(), bytes);
    Ok(disk.load().map_err(|e| format!("{}: {e}", path.display()))?.logged)
}

/// The `local.` journal's `origin` rows on the server's disk: what it keeps
/// of origins whose entries left the log.
fn journal(log: &Path) -> Result<Vec<([u8; 16], String, i64)>, String> {
    let mut disk = Memory::default();
    for n in ["kv", "log"] {
        let path = log.join(format!("{}{n}", ark_peer::local::LOCAL));
        match std::fs::read(&path) {
            Ok(b) => {
                disk.0.insert(n.to_string(), b);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    let local = Instance::open::<Local>(Box::new(disk));
    if let Some(why) = &local.failure {
        return Err(format!("{}: the journal: {why}", log.display()));
    }
    Ok(local.read::<Local, _>(|db| db.origin.scan().into_iter().map(|l| (l.origin.0, l.actor, l.last)).collect()))
}
