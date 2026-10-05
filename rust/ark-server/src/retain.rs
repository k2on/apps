//! What the hub remembers of each session's place in the log, so that
//! `ark::retention` can decide how much of the log to keep
//! (`docs/plan-alone.md` §3, `docs/plan-perf.md` R10).
//!
//! ```text
//! DATA/cursors.cbor   canonical CBOR of
//!                     { t: "cursors",
//!                       sessions: [ { user, session, cursor: Int, heard: Int } … ] }
//!                     — beside `live.cbor`, not in it: that file is a map of
//!                     rooms to what each kept, and a session's cursor is not
//!                     a room's
//! ```
//!
//! **A session is `(user, session)`**, not the session id alone: dev auth
//! gives every login the session `dev`, and a key on that alone would fold
//! every user's cursor into one. Under `ark-auth` a session id is unique
//! and the user beside it changes nothing.
//!
//! **What is recorded, and when.** At every `Hello` the cursor the peer
//! names (unless its `Hello` names another log, whose cursor is a place in
//! that log and not this one); at every page or snapshot delivered to it,
//! where it stood before that page — the start of the batch, or the
//! snapshot's sequence. There is no frame by which a peer acknowledges a
//! page, and the server's `Ack` answers its pushes, not its cursor; so
//! "the ack" of `docs/plan-alone.md` §3 is, here, the delivery. Recording
//! the page's start rather than its end keeps a page in flight — and any
//! `NeedFacts` about it — above the horizon. A page sent is not a page
//! received: a connection black-holed has pages written to it that never
//! arrive, so its recorded place can run ahead of it, and it is then served
//! the snapshot where a page would have done — the answer that is right
//! wherever a peer is. The time heard is updated at
//! every frame from the session and when its connection closes, and a
//! session with a connection open counts as heard now.
//!
//! **Why the file is written lazily and without a sync.** A cursor moves
//! with every page delivered; a synced write per page would double what a
//! push costs the disk. And nothing about correctness rests on this file:
//! a stale one holds an older cursor (so more of the log is kept) or an
//! older time (so a session near the window's edge may drop out, and its
//! peer is served the snapshot, which is the designed answer below the
//! horizon). So it is written by a rename whenever a session arrives or
//! leaves, whenever the horizon moves, at most every [`WRITE_EVERY`]
//! otherwise, and when the hub stops; and a file that does not decode is
//! said and started over rather than refused.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use ark::canon;
use ark::log::Seq;
use ark::value::Value;

/// The file, beside `live.cbor`.
pub const FILE: &str = "cursors.cbor";

/// How often a cursor that only moved is written: often enough that a
/// kill loses a second of positions, which the module docs say is safe.
pub const WRITE_EVERY: Duration = Duration::from_secs(1);

/// Where a session was, and when it was last heard from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heard {
    pub cursor: Seq,
    /// Milliseconds since the Unix epoch.
    pub at_ms: i64,
}

/// Every session the hub has heard from, by `(user, session)`.
pub type Cursors = BTreeMap<(String, String), Heard>;

/// The file's bytes.
pub fn encode(cursors: &Cursors) -> Vec<u8> {
    let sessions = cursors
        .iter()
        .map(|((user, session), h)| {
            Value::record(vec![
                ("user", Value::text(user.as_str())),
                ("session", Value::text(session.as_str())),
                ("cursor", Value::int(h.cursor)),
                ("heard", Value::int(h.at_ms)),
            ])
        })
        .collect();
    canon::encode(&Value::record(vec![("t", Value::text("cursors")), ("sessions", Value::list(sessions))]))
}

/// The file back.
pub fn decode(bytes: &[u8]) -> Result<Cursors> {
    let v = canon::decode(bytes)?;
    let Value::Struct(m) = &v else { bail!("not a record") };
    if m.get("t") != Some(&Value::text("cursors")) {
        bail!("not a cursors file: t = {:?}", m.get("t"));
    }
    let Some(Value::List(items)) = m.get("sessions") else {
        bail!("no sessions")
    };
    let mut out = Cursors::new();
    for item in items {
        let Value::Struct(s) = item else {
            bail!("a session that is not a record")
        };
        let text = |k: &str| match s.get(k) {
            Some(Value::Text(t)) => Ok(t.to_string()),
            other => bail!("{k}: {other:?}"),
        };
        let int = |k: &str| match s.get(k) {
            Some(Value::Int(n)) => Ok(*n),
            other => bail!("{k}: {other:?}"),
        };
        out.insert(
            (text("user")?, text("session")?),
            Heard {
                cursor: int("cursor")?,
                at_ms: int("heard")?,
            },
        );
    }
    Ok(out)
}

/// What `dir` holds; nothing where there is no file, and nothing — said —
/// where it does not decode (the module docs).
pub fn load(dir: &Path) -> Cursors {
    let path = dir.join(FILE);
    match std::fs::read(&path) {
        Ok(b) => decode(&b).unwrap_or_else(|e| {
            eprintln!("ark-server: {} is not a cursors file ({e:#}); starting with none", path.display());
            Cursors::new()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Cursors::new(),
        Err(e) => {
            eprintln!("ark-server: could not read {}: {e}; starting with none", path.display());
            Cursors::new()
        }
    }
}

/// Write `cursors` as `dir`'s file, by a rename (the module docs).
pub fn save(dir: &Path, cursors: &Cursors) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let tmp = dir.join(format!(".{FILE}.tmp"));
    std::fs::write(&tmp, encode(cursors)).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, dir.join(FILE)).with_context(|| format!("moving {} into place", tmp.display()))
}

/// Milliseconds since the Unix epoch, as a last-heard time is kept.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
