//! The process fleet (`docs/plan-fleet.md` §2): the real `harken-server`
//! and real `harken-peer`s as child processes on loopback, each peer behind
//! a [`Proxy`] the test holds, and [`Fleet::converged`] — the one invariant
//! every scenario ends on.
//!
//! The server's log is read where it lies (`HARKEN_DATA/log`, the chain on
//! disk, `ark_peer::Files`), from a copy, so a read never cuts a tail the
//! server is writing: its snapshot's rows, each entry's facts applied on
//! top, is the state at its head.
//!
//! Nothing here sleeps for a fixed time to let something happen: every
//! wait is a condition polled against a deadline, so a loaded machine is
//! slower and not wrong. The deadlines are generous; a scenario that needs
//! one is failing.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ark::format::Encode;
use ark::hash::hex;
use ark::log::Logged;
use ark::peer::Who;
use ark::store::{Change, Store};
use ark_peer::{Config, Files, Peer};
use arkdb::sim::{Net, Op};
use harken_server::hosted::Hosted;
use serde_json::{json, Value};

use super::proxy::{Proxy, Upstream};

/// An id, as the log and the peers' lines carry it.
pub type Id = [u8; 16];

/// The fleet's shortened clocks. The server pings every 300ms and closes
/// after three unanswered (`HARKEN_KEEPALIVE_MS`); a dropped link dials
/// again after 20ms, backing off to 250ms. A black hole is noticed by the
/// server in about a second, rather than a minute.
pub const KEEPALIVE_MS: u64 = 300;
pub const PUMP_MS: u64 = 20;
pub const BACKOFF: &str = "20,250";

/// How long a peer's sign-in waits on the server (`--auth-patience-ms`):
/// ark-auth's own ten and twenty seconds are a deployment's, and a fleet
/// that signs in behind a black hole on purpose (scenario 8, the fuzz)
/// wants the failure in two. Loopback answers a sign-in in milliseconds.
pub const AUTH_PATIENCE_MS: u64 = 2_000;

/// How long a condition is waited for before a scenario fails.
pub const PATIENCE: Duration = Duration::from_secs(60);

/// harken's module, assembled once.
pub fn hosted() -> &'static Hosted {
    static H: std::sync::OnceLock<Hosted> = std::sync::OnceLock::new();
    H.get_or_init(Hosted::harken)
}

/// Poll `ok` until it holds or `within` passes; whether it held.
pub fn eventually(within: Duration, ok: impl FnMut() -> bool) -> bool {
    ark_peer::link::until(within, Duration::from_millis(10), ok)
}

/// The last `n` lines of a file, for a failure message.
pub fn tail(path: &Path, n: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// A timing, said once where `--nocapture` shows it.
pub fn measured(what: &str, took: Duration) {
    println!("fleet: {what}: {:.0} ms", took.as_secs_f64() * 1000.0);
}

/// An id as the peers' lines carry it: its hex.
pub fn id(i: Id) -> Value {
    json!(hex(&i))
}

/// An id from a peer's line.
pub fn id_of(v: &Value) -> Id {
    let t = v.as_str().unwrap_or_else(|| panic!("not an id: {v}"));
    ark::hash::unhex(t)
        .and_then(|b| b.try_into().ok())
        .unwrap_or_else(|| panic!("not an id: {t}"))
}

// -- the log on disk ----------------------------------------------------------------

/// The server's log as it lies on disk: the snapshot it stands on, the
/// chain above it, and the state at its head.
pub struct Log {
    pub base: i64,
    pub entries: Vec<Logged>,
    pub state: Store,
}

impl Log {
    /// The log in `dir` (the server's `HARKEN_DATA/log`), read from a copy;
    /// `None` before anything was written.
    pub fn read(dir: &Path) -> Option<Log> {
        if !dir.join("kv").exists() && !dir.join("log").exists() {
            return None;
        }
        let copy = tempfile::tempdir().unwrap();
        for e in std::fs::read_dir(dir).ok()?.flatten() {
            if e.file_type().is_ok_and(|t| t.is_file()) {
                let _ = std::fs::copy(e.path(), copy.path().join(e.file_name()));
            }
        }
        let disk = Box::new(Files::new(copy.path()).unwrap());
        let mut p = Peer::serve(harken_server::hub_config(hosted(), 10_000), disk, 0);
        let state = std::mem::replace(
            &mut p.log.store,
            Store::new(hosted().module.schema.clone(), ark::schema::Space::Durable),
        );
        Some(Log {
            base: state.log().base_seq(),
            entries: state.log().entries(),
            state,
        })
    }

    pub fn head_seq(&self) -> i64 {
        self.entries.last().map_or(self.base, |l| l.seq)
    }

    /// The snapshot it stands on: entries below it are gone.
    pub fn horizon(&self) -> i64 {
        self.base
    }

    pub fn hash(&self) -> String {
        self.state.state_hash().hex()
    }

    pub fn seq_of(&self, i: &Id) -> Option<i64> {
        self.entries
            .iter()
            .find(|l| l.entry.id.0 == *i)
            .map(|l| l.seq)
    }

    /// A table's rows at the head.
    pub fn rows<T: Encode>(&self, table: &str) -> Vec<T> {
        self.state
            .rows(table)
            .map(|r| T::from_bytes(r).unwrap())
            .collect()
    }
}

/// The function an entry names, by its binding's last part.
pub fn function(e: &Logged) -> String {
    let n = &e.entry.func;
    match hosted().domain.fns.contains_key(n) {
        true => n.rsplit('.').next().unwrap_or(n).to_string(),
        false => format!("#{n}"),
    }
}

/// Whether an entry is a sign-in: the hub's own raw write of a `sign_in`
/// row (`docs/plan-v5.md` S1), which every login now is, or the welcome a
/// person's first one runs (`playlists.welcome`).
pub fn is_sign_in(e: &Logged) -> bool {
    let welcome = e.entry.func == "playlists.welcome";
    welcome || e.entry.func.is_empty() && e.facts.iter().all(|c| c.table() == "sign_in")
}

/// Whether an entry is the scanner's own bookkeeping of a look: asking for
/// one, claiming it, finishing it (`docs/plan-v5.md` S3a).
pub fn is_look(e: &Logged) -> bool {
    e.entry.func == "library.look" || e.entry.func.starts_with("look.")
}

impl Log {
    /// How many of its entries are neither sign-ins nor looks: the work in
    /// it.
    pub fn work(&self) -> i64 {
        self.entries
            .iter()
            .filter(|e| !is_sign_in(e) && !is_look(e))
            .count() as i64
    }
}

impl Converged {
    /// How many entries above the horizon are not sign-ins.
    pub fn work(&self) -> i64 {
        self.log.work()
    }
}

/// The rows an entry added to `table`.
pub fn added<T: Encode>(e: &Logged, table: &str) -> Vec<T> {
    e.facts
        .iter()
        .filter_map(|c| match c {
            Change::Add { table: t, row } if t == table => Some(T::from_bytes(row).unwrap()),
            _ => None,
        })
        .collect()
}

// -- the server ---------------------------------------------------------------------

/// `harken-server` as a child process, in dev auth, with a data directory
/// and a media root of its own, on an ephemeral port it reports.
pub struct Server {
    child: Option<Child>,
    pub data: PathBuf,
    pub media: PathBuf,
    /// stdout and stderr, both, one line each, across every start.
    pub log: PathBuf,
    pub port: u16,
    upstream: Upstream,
    pub env: Vec<(String, String)>,
    pub starts: usize,
    /// A pinned revision's binary (`tests/versions.rs`); this build's when none.
    pub bin: Option<PathBuf>,
}

impl Server {
    fn spawn(
        root: &Path,
        upstream: Upstream,
        env: Vec<(String, String)>,
        bin: Option<PathBuf>,
    ) -> Server {
        let mut s = Server {
            bin,
            child: None,
            data: root.join("server-data"),
            media: root.join("media"),
            log: root.join("server.log"),
            port: 0,
            upstream,
            env,
            starts: 0,
        };
        std::fs::create_dir_all(s.media.join("music")).unwrap();
        s.start();
        s
    }

    /// Start (again) over the same data directory, and wait for `/healthz`.
    pub fn start(&mut self) {
        assert!(self.child.is_none(), "the server is already running");
        let mut cmd = Command::new(
            self.bin
                .clone()
                .unwrap_or_else(|| env!("CARGO_BIN_EXE_harken-server").into()),
        );
        cmd.arg("127.0.0.1:0")
            .env("HARKEN_DEV_AUTH", "1")
            .env("HARKEN_DATA", &self.data)
            .env("HARKEN_MEDIA", &self.media)
            .env("HARKEN_KEEPALIVE_MS", KEEPALIVE_MS.to_string())
            // The fleet's people add songs, which the library's guard gives
            // the `library` role alone: it holds to convergence, so the two
            // people it signs in hold it, unless a scenario's own `env`
            // says otherwise.
            .env("HARKEN_ROLES", FLEET_ROLES)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().expect("harken-server starts");
        let (port_tx, port_rx) = channel::<u16>();
        for (tag, out) in [
            (
                "out",
                Box::new(child.stdout.take().unwrap()) as Box<dyn std::io::Read + Send>,
            ),
            ("err", Box::new(child.stderr.take().unwrap())),
        ] {
            let (log, port_tx) = (self.log.clone(), port_tx.clone());
            std::thread::spawn(move || {
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log)
                    .unwrap();
                for line in BufReader::new(out).lines() {
                    let Ok(line) = line else { break };
                    let _ = writeln!(file, "{tag}: {line}");
                    if let Some(rest) = line.split("listening on http://127.0.0.1:").nth(1) {
                        if let Some(port) = rest
                            .split(|c: char| !c.is_ascii_digit())
                            .next()
                            .and_then(|p| p.parse::<u16>().ok())
                        {
                            let _ = port_tx.send(port);
                        }
                    }
                }
            });
        }
        self.child = Some(child);
        self.port = match port_rx.recv_timeout(PATIENCE) {
            Ok(p) => p,
            _ => panic!("the server said no port:\n{}", tail(&self.log, 30)),
        };
        *self.upstream.lock().unwrap() = format!("127.0.0.1:{}", self.port).parse().unwrap();
        self.starts += 1;
        assert!(
            eventually(PATIENCE, || self.health().is_some()),
            "the server is not healthy:\n{}",
            tail(&self.log, 30)
        );
    }

    pub fn running(&self) -> bool {
        self.child.is_some()
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// `/healthz`, or `None` while it does not answer.
    pub fn health(&self) -> Option<String> {
        let r = ureq::get(&format!("{}/healthz", self.url()))
            .timeout(Duration::from_secs(2))
            .call()
            .ok()?;
        r.into_string().ok()
    }

    /// How many connections the account `user` has open, from `/healthz`.
    pub fn room(&self, user: &str) -> usize {
        let h = self.health().unwrap_or_default();
        h.lines()
            .find_map(|l| {
                l.strip_prefix(&format!("room {user} peers "))
                    .and_then(|n| n.trim().parse().ok())
            })
            .unwrap_or(0)
    }

    /// `kill -9`: nothing flushed, nothing closed.
    pub fn kill9(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// `SIGTERM`, which it answers by stopping the scanner and the
    /// listener; waited for.
    pub fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            // SAFETY: a signal to a child this process spawned and has not
            // reaped, so the pid is still its.
            #[allow(unsafe_code)]
            unsafe {
                libc::kill(c.id() as i32, libc::SIGTERM);
            }
            let stopped = eventually(Duration::from_secs(20), || {
                matches!(c.try_wait(), Ok(Some(_)))
            });
            if !stopped {
                let _ = c.kill();
                panic!(
                    "the server did not stop on SIGTERM:\n{}",
                    tail(&self.log, 20)
                );
            }
        }
    }

    pub fn restart(&mut self) {
        self.kill9();
        self.start();
    }

    /// Stop cleanly and start again over the same data directory, with
    /// `env` set or replaced: a deploy, in place.
    pub fn upgrade(&mut self, env: Vec<(&str, &str)>) {
        self.stop();
        self.bin = None;
        for (k, v) in env {
            self.env.retain(|(ek, _)| ek != k);
            self.env.push((k.into(), v.into()));
        }
        self.start();
    }

    /// Where the hub keeps its log.
    pub fn log_dir(&self) -> PathBuf {
        self.data.join("log")
    }

    /// The log as it is on disk. `None` before the first write.
    pub fn log_on_disk(&self) -> Option<Log> {
        Log::read(&self.log_dir())
    }

    /// The log on disk, in bytes: every file of it.
    pub fn log_bytes(&self) -> u64 {
        std::fs::read_dir(self.log_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.kill9();
    }
}

// -- a peer -----------------------------------------------------------------------------

/// What `status` answers.
#[derive(Clone, Debug)]
pub struct Status {
    pub cursor: i64,
    pub pending: i64,
    pub linked: bool,
    pub denied: Option<String>,
    pub user: String,
    pub session: String,
    pub epoch: i64,
    pub opens: i64,
    pub link: String,
}

/// A peer's view of the listening session (`{"cmd":"live"}`): the value,
/// null before anything is known of it; this device's id; the volatile
/// log's head, its own calls pending, and whether the hub has paged it
/// since the connection opened.
#[derive(Clone, Debug)]
pub struct Live {
    pub value: Option<Value>,
    pub device: String,
    pub seq: i64,
    pub pending: i64,
    pub synced: bool,
}

/// `harken-peer` as a child process, dialling its own [`Proxy`].
pub struct PeerProc {
    pub name: String,
    pub dir: PathBuf,
    pub user: Option<String>,
    /// Started `--alone`: its own authority, dialling nothing. Cleared, the
    /// next start is `--server` over the same directory — the join.
    pub alone: bool,
    pub proxy: Proxy,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    lines: Option<Receiver<String>>,
    pub stderr: PathBuf,
    /// Every id a `mutate` answered with, fleet-wide: what the log owes.
    accepted: Arc<Mutex<BTreeMap<Id, String>>>,
    pub starts: usize,
    /// Said on every start after the rest.
    pub args: Vec<String>,
    /// `--roles`: what the device's entries carry, beside its login's — its
    /// preview's, since the hub stamps every entry with the roles it
    /// grants. `library` by default, as [`FLEET_ROLES`] grants it.
    pub roles: Vec<String>,
    /// A pinned revision's binary (`tests/versions.rs`); this build's when none.
    pub bin: Option<PathBuf>,
}

/// What the fleet's server grants (`HARKEN_ROLES`): its people hold the
/// library's role, because its scenarios add songs from every peer.
pub const FLEET_ROLES: &str = "library=alice,bob";

fn field<'a>(v: &'a Value, k: &str) -> &'a Value {
    v.get(k).unwrap_or_else(|| panic!("no `{k}` in {v}"))
}

fn int(v: &Value) -> i64 {
    v.as_i64().unwrap_or_else(|| panic!("not an int: {v}"))
}

fn text(v: &Value) -> String {
    v.as_str()
        .unwrap_or_else(|| panic!("not text: {v}"))
        .to_string()
}

/// A command line: `cmd` and its fields.
pub fn command(cmd: &str, fields: Vec<(&str, Value)>) -> String {
    let mut m = serde_json::Map::new();
    m.insert("cmd".into(), json!(cmd));
    for (k, v) in fields {
        m.insert(k.into(), v);
    }
    Value::Object(m).to_string()
}

/// Arguments, by name.
pub fn args(fields: Vec<(&str, Value)>) -> Value {
    Value::Object(
        fields
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}

impl PeerProc {
    pub fn running(&self) -> bool {
        self.child.is_some()
    }

    /// Start (again) over the same directory.
    pub fn start(&mut self) {
        if let Err(e) = self.try_start() {
            panic!("{} did not start: {e}", self.name);
        }
    }

    /// [`PeerProc::start`], where the peer may not come up: a sign-in it
    /// has to make at start that fails — into a black hole, or a server
    /// that is down — ends the process, and that is an answer here rather
    /// than a panic. `Err` with the tail of its stderr, and not running.
    pub fn try_start(&mut self) -> Result<(), String> {
        assert!(self.child.is_none(), "{} is already running", self.name);
        let err = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.stderr)
            .unwrap();
        let mut cmd = Command::new(
            self.bin
                .clone()
                .unwrap_or_else(|| env!("CARGO_BIN_EXE_harken-peer").into()),
        );
        cmd.args(["--dir", self.dir.to_str().unwrap()]);
        cmd.args(&self.args);
        if !self.roles.is_empty() {
            cmd.args(["--roles", &self.roles.join(",")]);
        }
        if self.alone {
            cmd.arg("--alone");
        } else {
            cmd.args(["--server", &self.proxy.url()]);
        }
        cmd.args(["--pump-ms", &PUMP_MS.to_string()])
            .args(["--backoff-ms", BACKOFF])
            .args(["--auth-patience-ms", &AUTH_PATIENCE_MS.to_string()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(err));
        if let Some(u) = &self.user {
            cmd.args(["--user", u]);
        }
        let mut child = cmd.spawn().expect("harken-peer starts");
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        self.stdin = child.stdin.take();
        self.lines = Some(rx);
        self.child = Some(child);
        self.starts += 1;
        // Answered once it is open and, with a login to make, signed in —
        // so a scenario that kills it next kills a peer, not a start-up.
        let line = command("status", vec![]);
        let asked = self
            .stdin
            .as_mut()
            .map(|i| writeln!(i, "{line}").and_then(|_| i.flush()));
        let answered = asked.is_some_and(|w| w.is_ok())
            && self.lines.as_ref().unwrap().recv_timeout(PATIENCE).is_ok();
        if answered {
            return Ok(());
        }
        self.kill9();
        Err(tail(&self.stderr, 5))
    }

    /// Send one line, read its one answer.
    pub fn ask_line(&mut self, line: &str, within: Duration) -> Value {
        let stdin = self
            .stdin
            .as_mut()
            .unwrap_or_else(|| panic!("{} is not running", self.name));
        if writeln!(stdin, "{line}")
            .and_then(|_| stdin.flush())
            .is_err()
        {
            panic!("{} has gone:\n{}", self.name, tail(&self.stderr, 30));
        }
        match self.lines.as_ref().unwrap().recv_timeout(within) {
            Ok(answer) => serde_json::from_str(&answer)
                .unwrap_or_else(|e| panic!("{}: {answer}: {e}", self.name)),
            Err(RecvTimeoutError::Timeout) => panic!(
                "{} did not answer {line} in {within:?}:\n{}",
                self.name,
                tail(&self.stderr, 30)
            ),
            Err(RecvTimeoutError::Disconnected) => panic!(
                "{} ended answering {line}:\n{}",
                self.name,
                tail(&self.stderr, 30)
            ),
        }
    }

    /// Send one line and do not wait for its answer: what a scenario that
    /// kills a peer in the middle of a command does. [`PeerProc::answers`]
    /// reads what came back.
    pub fn say(&mut self, cmd: &str, fields: Vec<(&str, Value)>) {
        let stdin = self
            .stdin
            .as_mut()
            .unwrap_or_else(|| panic!("{} is not running", self.name));
        writeln!(stdin, "{}", command(cmd, fields))
            .and_then(|_| stdin.flush())
            .unwrap_or_else(|e| panic!("{} has gone: {e}", self.name));
    }

    /// Every answer that has arrived and not been read, without waiting.
    pub fn answers(&mut self) -> Vec<Value> {
        let Some(lines) = &self.lines else {
            return vec![];
        };
        lines
            .try_iter()
            .map(|l| serde_json::from_str(&l).unwrap_or_else(|e| panic!("{l}: {e}")))
            .collect()
    }

    /// Record an intent as owed by the log: one this peer made whose
    /// answer a kill took, and that the directory kept.
    pub fn owe(&self, i: Id, what: &str) {
        self.accepted
            .lock()
            .unwrap()
            .insert(i, format!("{} {what}", self.name));
    }

    pub fn ask(&mut self, cmd: &str, fields: Vec<(&str, Value)>) -> Value {
        self.ask_line(&command(cmd, fields), PATIENCE)
    }

    /// Author an intent; its id, recorded as owed by the log, or the
    /// refusal at authoring.
    pub fn mutate(&mut self, name: &str, a: Vec<(&str, Value)>) -> Result<Id, String> {
        let r = self.ask("mutate", vec![("name", json!(name)), ("args", args(a))]);
        if field(&r, "ok") == &json!(true) {
            let i = id_of(field(&r, "id"));
            self.accepted
                .lock()
                .unwrap()
                .insert(i, format!("{} {name}", self.name));
            Ok(i)
        } else {
            Err(text(field(&r, "why")))
        }
    }

    /// [`PeerProc::mutate`], which must be accepted here.
    pub fn author(&mut self, name: &str, a: Vec<(&str, Value)>) -> Id {
        self.mutate(name, a)
            .unwrap_or_else(|why| panic!("{} refused {name}: {why}", self.name))
    }

    pub fn status(&mut self) -> Status {
        let a = self.ask("status", vec![]);
        Status {
            cursor: int(field(&a, "cursor")),
            pending: int(field(&a, "pending")),
            linked: field(&a, "linked") == &json!(true),
            denied: field(&a, "denied").as_str().map(str::to_string),
            user: text(field(&a, "user")),
            session: text(field(&a, "session")),
            epoch: int(field(&a, "epoch")),
            opens: int(field(&a, "opens")),
            link: text(field(&a, "link")),
        }
    }

    /// `(cursor, confirmed hash, view hash)`.
    pub fn hash(&mut self) -> (i64, String, String) {
        let a = self.ask("hash", vec![]);
        (
            int(field(&a, "cursor")),
            text(field(&a, "hash")),
            text(field(&a, "view")),
        )
    }

    pub fn settle(&mut self, within: Duration) -> bool {
        let a = self.ask_line(
            &command(
                "settle",
                vec![("timeout_ms", json!(within.as_millis() as i64))],
            ),
            within + PATIENCE,
        );
        field(&a, "ok") == &json!(true)
    }

    pub fn wait(&mut self, cursor: i64, within: Duration) -> bool {
        let a = self.ask_line(
            &command(
                "wait",
                vec![
                    ("cursor", json!(cursor)),
                    ("timeout_ms", json!(within.as_millis() as i64)),
                ],
            ),
            within + PATIENCE,
        );
        field(&a, "ok") == &json!(true)
    }

    /// A query's rows.
    pub fn query(&mut self, name: &str, a: Vec<(&str, Value)>) -> Vec<Value> {
        let r = self.ask("query", vec![("name", json!(name)), ("args", args(a))]);
        match r.get("rows") {
            Some(Value::Array(rows)) => rows.clone(),
            _ => panic!("{}: {name}: {r}", self.name),
        }
    }

    /// A transition of the listening session, called as this peer's device:
    /// previewed at once, sent if linked. `Err` with the preview's refusal.
    pub fn call(&mut self, transition: &str, input: Vec<(&str, Value)>) -> Result<(), String> {
        let a = self.ask(
            "call",
            vec![
                ("live", json!("listening")),
                ("transition", json!(transition)),
                ("input", args(input)),
            ],
        );
        if field(&a, "ok") == &json!(true) {
            Ok(())
        } else {
            Err(text(field(&a, "why")))
        }
    }

    /// The listening session as this peer's device sees it.
    pub fn live(&mut self) -> Live {
        let a = self.ask("live", vec![("name", json!("listening"))]);
        Live {
            value: Some(field(&a, "value").clone()).filter(|v| !v.is_null()),
            device: text(field(&a, "device")),
            seq: int(field(&a, "seq")),
            pending: int(field(&a, "pending")),
            synced: field(&a, "synced") == &json!(true),
        }
    }

    /// What the server refused of this peer's live calls since last asked.
    pub fn live_refusals(&mut self) -> Vec<String> {
        let a = self.ask("live_refusals", vec![]);
        field(&a, "refusals")
            .as_array()
            .unwrap()
            .iter()
            .map(text)
            .collect()
    }

    /// Every verdict this directory has had, `(id, reason)`.
    pub fn rejections(&mut self) -> Vec<(Id, String)> {
        let a = self.ask("rejections", vec![("all", json!(true))]);
        field(&a, "rejections")
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (id_of(field(r, "id")), text(field(r, "reason"))))
            .collect()
    }

    /// `sign_in`, where it may be refused: the peer's answer, `Err` with
    /// its reason. A sign-in the server does not answer fails within
    /// [`AUTH_PATIENCE_MS`].
    pub fn try_sign_in(&mut self, user: &str) -> Result<(String, String), String> {
        let a = self.ask("sign_in", vec![("user", json!(user))]);
        if field(&a, "ok") != &json!(true) {
            return Err(text(field(&a, "why")));
        }
        self.user = Some(user.into());
        Ok((text(field(&a, "user")), text(field(&a, "session"))))
    }

    pub fn sign_in(&mut self, user: &str) -> (String, String) {
        let name = self.name.clone();
        self.try_sign_in(user)
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    /// The token the remembered login proves, from `DIR/logins`.
    pub fn token(&self) -> String {
        let logins = ark_peer::Instance::open::<ark_auth::remember::Remembered>(Box::new(
            ark_peer::Files::new(self.dir.join("logins")).unwrap(),
        ));
        logins
            .read::<ark_auth::remember::Remembered, _>(|db| db.login.scan())
            .pop()
            .expect("a login remembered")
            .token
    }

    /// `kill -9`: whatever was not written is gone; the directory stays.
    pub fn kill9(&mut self) {
        self.stdin = None;
        self.lines = None;
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// `quit`, then exit.
    pub fn quit(&mut self) {
        if self.child.is_some() {
            let _ = self.ask("quit", vec![]);
            if let Some(mut c) = self.child.take() {
                // A peer that answers `quit` and then does not exit is a bug
                // in that peer; five seconds, then the test fails naming it.
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    match c.try_wait() {
                        Ok(Some(_)) => break,
                        Ok(None) if Instant::now() < deadline => {
                            std::thread::sleep(Duration::from_millis(20))
                        }
                        _ => {
                            let _ = c.kill();
                            let _ = c.wait();
                            panic!(
                                "{}: answered quit and did not exit within five seconds",
                                self.name
                            );
                        }
                    }
                }
            }
            self.stdin = None;
            self.lines = None;
        }
    }

    pub fn restart(&mut self) {
        self.kill9();
        self.start();
    }
}

impl Drop for PeerProc {
    fn drop(&mut self) {
        self.kill9();
    }
}

// -- a log written before the server starts ------------------------------------------

/// A log made in this process and written where the server will read it,
/// for a scenario that needs a long log and not the minute it takes to
/// push one through a peer: `arkdb`'s simulator, its server over the
/// server's own directory, with a device of it authoring as each user asked
/// for, holding the library's role.
pub struct Seeder {
    net: Net,
    users: BTreeMap<String, i64>,
    pub ids: BTreeSet<Id>,
}

impl Seeder {
    fn new(dir: &Path) -> Seeder {
        let config = harken_server::hub_config(hosted(), 10_000);
        let mut net = Net::of(config.clone(), vec![], 0);
        net.hub = Peer::serve(config, Box::new(Files::new(dir.join("log")).unwrap()), 0);
        Seeder {
            net,
            users: BTreeMap::new(),
            ids: BTreeSet::new(),
        }
    }

    /// Sequence one intent as `user`; the facts it was sequenced with, so a
    /// scenario can name the rows it made.
    pub fn author(&mut self, user: &str, name: &str, input: &impl Encode) -> Vec<Change> {
        let i = match self.users.get(user) {
            Some(i) => *i,
            None => {
                let who = Who {
                    actor: user.into(),
                    session: format!("seeded-{user}"),
                    roles: vec!["library".into()],
                };
                let i = self.net.add(who, Config::new(hosted().module.clone()));
                *self.users.entry(user.into()).or_insert(i)
            }
        };
        self.net
            .run(&Op::Mutate(i, hosted().func(name), input.to_bytes()));
        self.net.deliver();
        let c = &self.net.clients[&i];
        assert!(c.log.pending.is_empty(), "seeding {name}: still pending");
        let last = c.log.log().entries().pop().expect("sequenced");
        self.ids.insert(last.entry.id.0);
        last.facts
    }

    pub fn head(&self) -> i64 {
        self.net.hub.log.log().head().0
    }
}

// -- the fleet --------------------------------------------------------------------------

/// A server, the peers of it, and what they are owed.
pub struct Fleet {
    pub root: tempfile::TempDir,
    pub server: Server,
    upstream: Upstream,
    accepted: Arc<Mutex<BTreeMap<Id, String>>>,
    /// Entries written into the log before the server started.
    seeded: BTreeSet<Id>,
    pub name: String,
}

/// A fleet that failed keeps its directory — the server's log, every
/// peer's replica and stderr — and says where; `FLEET_KEEP=1` keeps it
/// always.
impl Drop for Fleet {
    fn drop(&mut self) {
        if std::thread::panicking() || std::env::var("FLEET_KEEP").is_ok_and(|v| v == "1") {
            self.root.disable_cleanup(true);
            eprintln!("fleet {}: kept {}", self.name, self.root.path().display());
        }
    }
}

/// What [`Fleet::converged`] found.
pub struct Converged {
    pub head: i64,
    pub hash: String,
    pub took: Duration,
    /// Every entry above the horizon, by seq.
    pub entries: Vec<(i64, Logged)>,
    pub log: Log,
}

impl Fleet {
    pub fn new(name: &str) -> Fleet {
        Fleet::with_env(name, vec![])
    }

    pub fn with_env(name: &str, env: Vec<(&str, &str)>) -> Fleet {
        Fleet::build(name, env, |_| {}, None)
    }

    /// A fleet whose server starts on a log `seed` wrote.
    pub fn seeded(name: &str, seed: impl FnOnce(&mut Seeder)) -> Fleet {
        Fleet::build(name, vec![], seed, None)
    }

    /// A fleet whose server is a pinned revision's binary.
    pub fn old(name: &str, server: &Path) -> Fleet {
        Fleet::build(name, vec![], |_| {}, Some(server.into()))
    }

    fn build(
        name: &str,
        env: Vec<(&str, &str)>,
        seed: impl FnOnce(&mut Seeder),
        bin: Option<PathBuf>,
    ) -> Fleet {
        let root = tempfile::Builder::new()
            .prefix(&format!("fleet-{name}-"))
            .tempdir()
            .unwrap();
        let ids = {
            let mut seeder = Seeder::new(&root.path().join("server-data"));
            seed(&mut seeder);
            seeder.net.hub.close();
            seeder.ids
        };
        let upstream: Upstream = Arc::new(Mutex::new("127.0.0.1:1".parse().unwrap()));
        let env = env
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let server = Server::spawn(root.path(), upstream.clone(), env, bin);
        Fleet {
            root,
            server,
            upstream,
            accepted: Arc::default(),
            seeded: ids,
            name: name.into(),
        }
    }

    /// A peer named `name` (its directory), signed in as `user` — or
    /// signed out, authoring as nobody and dialling nothing — started.
    pub fn peer(&self, name: &str, user: Option<&str>) -> PeerProc {
        let mut p = self.peer_stopped(name, user);
        p.start();
        p
    }

    /// [`Fleet::peer`], not started yet: its proxy exists, so a test can
    /// arrange the network before the first byte.
    pub fn peer_stopped(&self, name: &str, user: Option<&str>) -> PeerProc {
        PeerProc {
            name: name.into(),
            dir: self.root.path().join(format!("peer-{name}")),
            user: user.map(str::to_string),
            alone: false,
            proxy: Proxy::start(self.upstream.clone()),
            child: None,
            stdin: None,
            lines: None,
            stderr: self.root.path().join(format!("peer-{name}.stderr")),
            accepted: self.accepted.clone(),
            starts: 0,
            args: vec![],
            roles: vec!["library".into()],
            bin: None,
        }
    }

    /// Forget that these intents were accepted: a log the server lost
    /// took them with it, and a scenario that loses one on purpose says so
    /// rather than `converged` finding them missing.
    pub fn lost(&self, ids: impl IntoIterator<Item = Id>) {
        let mut accepted = self.accepted.lock().unwrap();
        for i in ids {
            accepted.remove(&i);
        }
    }

    /// Every id a `mutate` was answered with, so far.
    pub fn accepted(&self) -> BTreeMap<Id, String> {
        self.accepted.lock().unwrap().clone()
    }

    /// **The invariant**: once the network is back, every peer's confirmed
    /// store hashes as the log's state at its head does, every cursor is
    /// the log's head, no intent id is in the log twice, and every intent a
    /// peer accepted is in the log — or was refused, and said so — while
    /// every entry in the log that neither the scanner nor the authority
    /// authored is one a peer accepted.
    ///
    /// The network is not given back here: a scenario releases its proxies
    /// first. Peers that are not running are left out of the comparison and
    /// still owe their intents; restart them before asking.
    ///
    /// **A refusal fails it.** Only the fuzz, whose schedule can race a
    /// removal against another peer's, asks
    /// [`Fleet::converged_allowing_refusals`].
    pub fn converged(&self, peers: &mut [&mut PeerProc]) -> Converged {
        self.converge(peers, false, PATIENCE)
    }

    /// [`Fleet::converged`], waiting `patience` rather than [`PATIENCE`].
    pub fn converged_within(&self, peers: &mut [&mut PeerProc], patience: Duration) -> Converged {
        self.converge(peers, false, patience)
    }

    /// [`Fleet::converged`], where an accepted intent the authority refused
    /// — and said so — is accounted for.
    pub fn converged_allowing_refusals(&self, peers: &mut [&mut PeerProc]) -> Converged {
        self.converge(peers, true, PATIENCE)
    }

    fn converge(
        &self,
        peers: &mut [&mut PeerProc],
        refusals: bool,
        patience: Duration,
    ) -> Converged {
        let started = Instant::now();
        let deadline = started + patience;
        let mut last = String::new();
        let (log, head, hash) = loop {
            for p in peers.iter_mut() {
                p.settle(Duration::from_secs(2));
            }
            let log = self.server.log_on_disk().unwrap_or_else(|| Log {
                base: 0,
                entries: vec![],
                state: Store::new(hosted().module.schema.clone(), ark::schema::Space::Durable),
            });
            let head = log.head_seq();
            let hash = log.hash();
            let mut all = true;
            let mut seen = vec![];
            for p in peers.iter_mut() {
                let (cursor, h, view) = p.hash();
                let st = p.status();
                seen.push(format!(
                    "{}: cursor {cursor} pending {} linked {} denied {:?} link {} hash {} view {}",
                    p.name,
                    st.pending,
                    st.linked,
                    st.denied,
                    st.link,
                    &h[..12],
                    &view[..12]
                ));
                all &= cursor == head && h == hash && view == hash && st.pending == 0;
            }
            if all {
                break (log, head, hash);
            }
            last = format!("log head {head} hash {}\n{}", &hash[..12], seen.join("\n"));
            if Instant::now() >= deadline {
                let tails: Vec<String> = peers
                    .iter()
                    .map(|p| format!("--- {} ---\n{}", p.name, tail(&p.stderr, 15)))
                    .collect();
                panic!(
                    "{}: not converged after {patience:?}:\n{last}\n--- server ---\n{}\n{}",
                    self.name,
                    tail(&self.server.log, 20),
                    tails.join("\n")
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let took = started.elapsed();

        // Every intent once.
        let mut ids: BTreeMap<Id, i64> = BTreeMap::new();
        for l in &log.entries {
            if let Some(first) = ids.insert(l.entry.id.0, l.seq) {
                panic!(
                    "{}: intent {} is in the log twice, at {first} and {}",
                    self.name,
                    hex(&l.entry.id.0),
                    l.seq
                );
            }
        }
        // What was accepted is there, or was refused and said so; and what
        // is there was accepted, or is the scanner's. An entry the server
        // compacted below its horizon is gone with its id: one accepted and
        // not above the horizon is counted against how many went below it.
        let mut refused: BTreeMap<Id, String> = BTreeMap::new();
        for p in peers.iter_mut() {
            refused.extend(p.rejections());
        }
        if !refusals {
            assert!(
                refused.is_empty(),
                "{}: the authority refused {refused:?}",
                self.name
            );
        }
        let accepted = self.accepted();
        let mut below = 0;
        for (i, who) in &accepted {
            match (ids.contains_key(i), refused.get(i)) {
                (true, None) => {}
                (false, Some(_)) => {}
                (true, Some(why)) => panic!(
                    "{}: {who} {} is in the log and was refused: {why}",
                    self.name,
                    hex(i)
                ),
                (false, None) if log.base > 0 => below += 1,
                (false, None) => panic!(
                    "{}: {who} {} was accepted and is not in the log (head {head}):\n{last}",
                    self.name,
                    hex(i)
                ),
            }
        }
        assert!(
            below <= log.base,
            "{}: {below} accepted intents are not in the log, and only {} went below its horizon",
            self.name,
            log.base
        );
        for l in &log.entries {
            let e = &l.entry;
            // The scanner's, and the authority's own, are no peer's.
            let unowned = e.actor == harken_server::library::ACCOUNT || e.actor == "authority";
            if !unowned && !accepted.contains_key(&e.id.0) && !self.seeded.contains(&e.id.0) {
                panic!(
                    "{}: entry {} ({} by {}) was never accepted by any peer",
                    self.name,
                    l.seq,
                    function(l),
                    e.actor
                );
            }
        }
        let entries = log.entries.iter().map(|l| (l.seq, l.clone())).collect();
        Converged {
            head,
            hash,
            took,
            entries,
            log,
        }
    }

    /// The rows of `table` in the log's state at its head.
    pub fn rows<T: Encode>(&self, table: &str) -> Vec<T> {
        self.server.log_on_disk().expect("a log").rows(table)
    }
}

/// `add_song`'s arguments for a song typed in by hand, with a file of its
/// own so no two collide (`add_song` does nothing for a file it has).
pub fn song(title: &str) -> Vec<(&'static str, Value)> {
    vec![
        ("title", json!(title)),
        ("artist", json!("The Fleet")),
        ("album", json!("Loopback")),
        ("duration_ms", json!(1000)),
        ("file", json!(format!("typed/{title}.wav"))),
        ("track", json!(0)),
        ("part", json!("")),
        ("catalogue", json!("")),
        ("performer", json!("")),
        ("bpm", json!(0)),
        ("album_art", json!("")),
        ("artist_art", json!("")),
        ("disc", json!(0)),
        ("work_title", json!("")),
        ("movement_no", json!(0)),
    ]
}

/// [`song`], as the domain's input.
pub fn song_input(title: &str) -> harken_server::domain::AddSong {
    harken_server::domain::AddSong {
        title: title.into(),
        artist: "The Fleet".into(),
        album: "Loopback".into(),
        duration_ms: 1000,
        file: format!("typed/{title}.wav"),
        ..Default::default()
    }
}

/// The ids of what a query's rows name, in order.
pub fn ids_of(rows: &[Value], key: &str) -> Vec<Id> {
    rows.iter().map(|r| id_of(&r[key])).collect()
}

/// Every playlist item, as each playlist's media in `pos` order.
pub fn playlists_in_pos_order(
    rows: &[harken_server::domain::PlaylistItem],
) -> BTreeMap<Id, Vec<Id>> {
    let mut by: BTreeMap<Id, Vec<(i64, Id)>> = BTreeMap::new();
    for r in rows {
        by.entry(r.playlist_id.0)
            .or_default()
            .push((r.pos, r.media_id.0));
    }
    by.into_iter()
        .map(|(p, mut xs)| {
            xs.sort();
            (p, xs.into_iter().map(|(_, m)| m).collect())
        })
        .collect()
}

/// A set of ids, for comparing.
pub fn set(ids: &[Id]) -> BTreeSet<Id> {
    ids.iter().copied().collect()
}
