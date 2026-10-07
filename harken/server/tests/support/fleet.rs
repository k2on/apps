//! The process fleet (`docs/plan-fleet.md` §2): the real `harken-server`
//! and real `harken-peer`s as child processes on loopback, each peer behind
//! a [`Proxy`] the test holds, and [`Fleet::converged`] — the one invariant
//! every scenario ends on.
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

use ark::hash::state_hash;
use ark::json;
use ark::log::Log;
use ark::value::{hex, Id, Value};
use ark_client::Domain;

use super::proxy::{Proxy, Upstream};

/// The fleet's shortened clocks. The server pings every 300ms and closes
/// after three unanswered (`HARKEN_KEEPALIVE_MS`, the hook this harness
/// added); a peer pings as often and gives up after three silent intervals;
/// a dropped link dials again after 20ms, backing off to 250ms. A black
/// hole is noticed in about a second by both ends, rather than a minute.
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

pub fn domain() -> Domain {
    Domain::new(&harken_domain::module())
}

/// Poll `ok` until it holds or `within` passes; whether it held.
pub fn eventually(within: Duration, mut ok: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + within;
    loop {
        if ok() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The last `n` lines of a file, for a failure message.
pub fn tail(path: &Path, n: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// A timing, said once where `--nocapture` shows it and gathered into
/// `docs/plan-fleet.md`'s "Measured".
pub fn measured(what: &str, took: Duration) {
    println!("fleet: {what}: {:.0} ms", took.as_secs_f64() * 1000.0);
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
    /// The binary the next start runs: this build's, or a pinned
    /// revision's ([`Server::old`]).
    pub bin: PathBuf,
}

impl Server {
    fn spawn(
        root: &Path,
        upstream: Upstream,
        env: Vec<(String, String)>,
        bin: Option<&Path>,
    ) -> Server {
        let mut s = Server {
            child: None,
            data: root.join("server-data"),
            media: root.join("media"),
            log: root.join("server.log"),
            port: 0,
            upstream,
            env,
            starts: 0,
            bin: bin.map_or_else(
                || PathBuf::from(env!("CARGO_BIN_EXE_harken-server")),
                Path::to_path_buf,
            ),
        };
        std::fs::create_dir_all(s.media.join("music")).unwrap();
        s.start();
        s
    }

    /// Start (again) over the same data directory, and wait for `/healthz`.
    pub fn start(&mut self) {
        assert!(self.child.is_none(), "the server is already running");
        let mut cmd = Command::new(&self.bin);
        cmd.arg("127.0.0.1:0")
            .env("HARKEN_DEV_AUTH", "1")
            .env("HARKEN_DATA", &self.data)
            .env("HARKEN_MEDIA", &self.media)
            .env("HARKEN_KEEPALIVE_MS", KEEPALIVE_MS.to_string())
            .env("HARKEN_KEEPALIVE_MISSED", "3")
            // `docs/plan-auth.md` The fleet's people add songs, which the
            // library's guard is to give the `library` role alone
            // (`docs/plan-guards.md` G2): it holds to convergence, so the two
            // people it signs in hold it, unless a scenario's own `env` says
            // otherwise.
            .env("HARKEN_ROLES", FLEET_ROLES)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().expect("harken-server starts");
        let (port_tx, port_rx) = channel();
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
            Err(_) => panic!("the server said no port:\n{}", tail(&self.log, 30)),
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

    /// How many peers the room for `user` holds, from `/healthz`; 0 for a
    /// room that is not open.
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

    /// Run `bin` — a pinned revision's `harken-server`
    /// (`HARKEN_OLD_<n>_SERVER`, [`olds`]) — from the next start on.
    pub fn old(&mut self, bin: &Path) {
        self.bin = bin.to_path_buf();
    }

    /// Stop cleanly and start this build's binary over the same data
    /// directory, with `env` set or replaced: a deploy, in place.
    pub fn upgrade(&mut self, env: Vec<(&str, &str)>) {
        self.stop();
        self.bin = PathBuf::from(env!("CARGO_BIN_EXE_harken-server"));
        for (k, v) in env {
            self.env.retain(|(ek, _)| ek != k);
            self.env.push((k.into(), v.into()));
        }
        self.start();
    }

    /// The module hashes `/healthz` lists, each with whether it is the
    /// current one (`ark-server`'s `modules`); none from a server older
    /// than that.
    pub fn modules(&self) -> Vec<(String, bool)> {
        let h = self.health().unwrap_or_default();
        h.lines()
            .filter_map(|l| l.strip_prefix("module "))
            .map(|rest| {
                let mut w = rest.split_whitespace();
                (
                    w.next().unwrap_or("").to_string(),
                    w.next() == Some("current"),
                )
            })
            .collect()
    }

    /// The log as it is on disk: `log.ark-log`, read the way the server
    /// reads it at start. `None` before the first write.
    pub fn log_on_disk(&self) -> Option<Log> {
        let schema = domain().module().schema.clone();
        // The file is replaced by a rename, so a read is of one version or
        // the next; an error is a file that is not whole, which is a finding.
        // A server upgraded to the grown domain (`docs/plan-db.md` D1)
        // hashed its snapshot again under that schema, so it is read so.
        ark_server::persist::load(&self.data, &schema)
            .or_else(|e| {
                let grown = harken_server::grown::domain().module().schema.clone();
                ark_server::persist::load(&self.data, &grown).map_err(|_| e)
            })
            .unwrap_or_else(|e| panic!("the log on disk is not a log: {e:#}"))
    }

    /// The size of `log.ark-log`, in bytes.
    /// The log on disk, in bytes: the snapshot `log.ark-log` and the
    /// journal `log.ark-journal` appended after it.
    pub fn log_bytes(&self) -> u64 {
        let size = |p: std::path::PathBuf| std::fs::metadata(p).map_or(0, |m| m.len());
        size(ark_server::persist::path_of(&self.data))
            + size(ark_server::persist::journal_path_of(&self.data))
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
    /// `docs/plan-db.md` D1: `None` from a peer older than holding.
    pub held: Option<i64>,
    pub behind: Option<bool>,
}

/// `harken-peer` as a child process, dialling its own [`Proxy`].
pub struct PeerProc {
    pub name: String,
    pub dir: PathBuf,
    pub user: Option<String>,
    /// Started `--alone`: its own authority, dialling nothing
    /// (`docs/plan-alone.md` §4). Cleared, the next start is `--server`
    /// over the same directory — the join.
    pub alone: bool,
    pub proxy: Proxy,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    lines: Option<Receiver<String>>,
    pub stderr: PathBuf,
    /// Every id a `mutate` answered with, fleet-wide: what the log owes.
    accepted: Arc<Mutex<BTreeMap<Id, String>>>,
    pub starts: usize,
    /// The binary the next start runs ([`PeerProc::old`]).
    pub bin: PathBuf,
    /// Said on every start after the rest (`--module FILE`, say).
    pub args: Vec<String>,
    /// `--roles` for this build's peer (`docs/plan-auth.md`): what the
    /// device's `Ctx` carries, beside its login's. `library` by
    /// default, as [`FLEET_ROLES`] grants it at the server; an older build
    /// knows no such flag and is told nothing.
    pub roles: Vec<String>,
}

/// What the fleet's server grants (`HARKEN_ROLES`): its people hold the
/// library's role, because its scenarios add songs from every peer.
pub const FLEET_ROLES: &str = "library=alice,bob";

fn field<'a>(v: &'a Value, k: &str) -> &'a Value {
    match v {
        Value::Struct(m) => m
            .get(k)
            .unwrap_or_else(|| panic!("no `{k}` in {}", json::json(v))),
        other => panic!("not an answer: {other:?}"),
    }
}

/// A field an older peer may not say.
fn has<'a>(v: &'a Value, k: &str) -> Option<&'a Value> {
    match v {
        Value::Struct(m) => m.get(k),
        _ => None,
    }
}

fn int(v: &Value) -> i64 {
    match v {
        Value::Int(n) => *n,
        other => panic!("not an int: {other:?}"),
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::Text(t) => t.to_string(),
        other => panic!("not text: {other:?}"),
    }
}

/// A command line: `cmd` and its fields.
pub fn command(cmd: &str, fields: Vec<(&str, Value)>) -> String {
    let mut all = vec![("cmd", Value::text(cmd))];
    all.extend(fields);
    json::json(&Value::record(all))
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
        let mut cmd = Command::new(&self.bin);
        cmd.args(["--dir", self.dir.to_str().unwrap()]);
        cmd.args(&self.args);
        if !self.roles.is_empty() && self.bin == Path::new(env!("CARGO_BIN_EXE_harken-peer")) {
            cmd.args(["--roles", &self.roles.join(",")]);
        }
        if self.alone {
            cmd.arg("--alone");
        } else {
            cmd.args(["--server", &self.proxy.url()]);
        }
        cmd.args([
            "--pump-ms",
            &PUMP_MS.to_string(),
            "--ping-ms",
            &KEEPALIVE_MS.to_string(),
        ])
        .args(["--backoff-ms", BACKOFF])
        .args(["--auth-patience-ms", &AUTH_PATIENCE_MS.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(err));
        if let (Some(u), false) = (&self.user, self.alone) {
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
            Ok(answer) => {
                json::decode(&answer).unwrap_or_else(|e| panic!("{}: {answer}: {e}", self.name))
            }
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
            .map(|l| json::decode(&l).unwrap_or_else(|e| panic!("{l}: {e}")))
            .collect()
    }

    /// Record an intent as owed by the log: one this peer made whose
    /// answer a kill took, and that the directory kept.
    pub fn owe(&self, id: Id, what: &str) {
        self.accepted
            .lock()
            .unwrap()
            .insert(id, format!("{} {what}", self.name));
    }

    pub fn ask(&mut self, cmd: &str, fields: Vec<(&str, Value)>) -> Value {
        self.ask_line(&command(cmd, fields), PATIENCE)
    }

    /// Author an intent; its id, recorded as owed by the log, or the
    /// refusal at authoring.
    pub fn mutate(&mut self, name: &str, args: Vec<(&str, Value)>) -> Result<Id, String> {
        let a = self.ask(
            "mutate",
            vec![("name", Value::text(name)), ("args", Value::record(args))],
        );
        if field(&a, "ok") == &Value::Bool(true) {
            let Value::Id(id) = field(&a, "id") else {
                panic!("{a:?}")
            };
            self.accepted
                .lock()
                .unwrap()
                .insert(*id, format!("{} {name}", self.name));
            Ok(*id)
        } else {
            Err(text(field(&a, "why")))
        }
    }

    /// [`PeerProc::mutate`], which must be accepted here.
    pub fn author(&mut self, name: &str, args: Vec<(&str, Value)>) -> Id {
        self.mutate(name, args)
            .unwrap_or_else(|why| panic!("{} refused {name}: {why}", self.name))
    }

    pub fn status(&mut self) -> Status {
        let a = self.ask("status", vec![]);
        Status {
            cursor: int(field(&a, "cursor")),
            pending: int(field(&a, "pending")),
            linked: field(&a, "linked") == &Value::Bool(true),
            denied: match field(&a, "denied") {
                Value::Text(t) => Some(t.to_string()),
                _ => None,
            },
            user: text(field(&a, "user")),
            session: text(field(&a, "session")),
            epoch: int(field(&a, "epoch")),
            opens: int(field(&a, "opens")),
            link: text(field(&a, "link")),
            held: has(&a, "held").map(int),
            behind: has(&a, "behind").map(|b| b == &Value::Bool(true)),
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
                vec![("timeout_ms", Value::int(within.as_millis() as i64))],
            ),
            within + PATIENCE,
        );
        field(&a, "ok") == &Value::Bool(true)
    }

    pub fn wait(&mut self, cursor: i64, within: Duration) -> bool {
        let a = self.ask_line(
            &command(
                "wait",
                vec![
                    ("cursor", Value::int(cursor)),
                    ("timeout_ms", Value::int(within.as_millis() as i64)),
                ],
            ),
            within + PATIENCE,
        );
        field(&a, "ok") == &Value::Bool(true)
    }

    /// A query's rows.
    pub fn query(&mut self, name: &str, args: Vec<(&str, Value)>) -> Vec<BTreeMap<String, Value>> {
        let a = self.ask(
            "query",
            vec![("name", Value::text(name)), ("args", Value::record(args))],
        );
        match field(&a, "rows") {
            Value::List(rows) => rows
                .iter()
                .map(|r| match r {
                    Value::Struct(m) => (**m).clone(),
                    other => panic!("a row: {other:?}"),
                })
                .collect(),
            other => panic!("rows: {other:?}"),
        }
    }

    /// Every verdict this directory has had, `(id, reason)`.
    pub fn rejections(&mut self) -> Vec<(Id, String)> {
        let a = self.ask("rejections", vec![("all", Value::Bool(true))]);
        let Value::List(items) = field(&a, "rejections") else {
            panic!("{a:?}")
        };
        items
            .iter()
            .map(|r| {
                let Value::Id(id) = field(r, "id") else {
                    panic!("{r:?}")
                };
                (*id, text(field(r, "reason")))
            })
            .collect()
    }

    /// `sign_in`, where it may be refused: the peer's answer, `Err` with
    /// its reason. A sign-in the server does not answer fails within
    /// [`AUTH_PATIENCE_MS`].
    pub fn try_sign_in(&mut self, user: &str) -> Result<(String, String), String> {
        let a = self.ask("sign_in", vec![("user", Value::text(user))]);
        if field(&a, "ok") != &Value::Bool(true) {
            return Err(text(field(&a, "why")));
        }
        self.user = Some(user.into());
        Ok((text(field(&a, "user")), text(field(&a, "session"))))
    }

    pub fn sign_in(&mut self, user: &str) -> (String, String) {
        let a = self.ask("sign_in", vec![("user", Value::text(user))]);
        assert_eq!(
            field(&a, "ok"),
            &Value::Bool(true),
            "{}: {}",
            self.name,
            json::json(&a)
        );
        self.user = Some(user.into());
        (text(field(&a, "user")), text(field(&a, "session")))
    }

    /// The token the remembered login proves, from `DIR/login.json`.
    pub fn token(&self) -> String {
        let body = std::fs::read_to_string(self.dir.join("login.json")).unwrap();
        let v = json::decode(&body).unwrap();
        text(field(field(&v, "login"), "token"))
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

    /// `quit`: a persist, then exit.
    pub fn quit(&mut self) {
        if self.child.is_some() {
            let _ = self.ask("quit", vec![]);
            if let Some(mut c) = self.child.take() {
                let _ = c.wait();
            }
            self.stdin = None;
            self.lines = None;
        }
    }

    pub fn restart(&mut self) {
        self.kill9();
        self.start();
    }

    /// Run `bin` — a pinned revision's `harken-peer`
    /// (`HARKEN_OLD_<n>_PEER`, [`olds`]) — from the next start on.
    pub fn old(&mut self, bin: &Path) {
        self.bin = bin.to_path_buf();
    }

    /// Quit, and start this build's binary over the same directory: the
    /// client upgraded in place.
    pub fn upgrade(&mut self) {
        self.quit();
        self.bin = PathBuf::from(env!("CARGO_BIN_EXE_harken-peer"));
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
/// push one through a peer (every push rewrites the whole file; see
/// "Measured" in `docs/plan-fleet.md`). The entries are sequenced by the
/// same native procedures the server holds, so the log is the one a peer
/// would have made; they are authored under a session no server issued,
/// which only the log's reader ever sees.
pub struct Seeder {
    authority: ark::peer::Authority,
    domain: Domain,
    autos: ark_client::Autos,
    pub ids: BTreeSet<Id>,
}

impl Seeder {
    fn new() -> Seeder {
        let d = domain();
        let mut authority =
            ark::peer::Authority::new(d.module().schema.clone(), d.closures().clone());
        authority.hold(d.native_list());
        Seeder {
            authority,
            domain: d,
            autos: ark_client::Autos::seeded(0x5eed),
            ids: BTreeSet::new(),
        }
    }

    /// Sequence one intent as `user`; the autos it drew, so a scenario can
    /// name the row it made.
    pub fn author(
        &mut self,
        user: &str,
        name: &str,
        args: Vec<(&str, Value)>,
    ) -> BTreeMap<String, Value> {
        let (fh, f) = self.domain.mutator(name).unwrap();
        let autos = self.autos.draw(f);
        let e = ark::log::Entry {
            id: self.autos.new_id(),
            actor: user.into(),
            session: "seeded".into(),
            roles: Default::default(),
            fn_hash: fh.clone(),
            args: args.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            autos: autos.clone(),
        };
        match self.authority.sequence_entry(&e) {
            ark::peer::Sequenced::Appended(..) => {}
            other => panic!("seeding {name}: {other:?}"),
        }
        self.ids.insert(e.id);
        autos
    }

    pub fn head(&self) -> i64 {
        self.authority.log.head_seq()
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
#[derive(Clone, Debug)]
pub struct Converged {
    pub head: i64,
    pub hash: String,
    pub took: Duration,
    /// Every entry, by seq: `(actor, session, function name, id, args)`.
    pub entries: Vec<(i64, ark::log::Entry)>,
    pub log: Log,
}

impl Fleet {
    pub fn new(name: &str) -> Fleet {
        Fleet::with_env(name, vec![])
    }

    pub fn with_env(name: &str, env: Vec<(&str, &str)>) -> Fleet {
        Fleet::build(name, env, |_| {})
    }

    /// A fleet whose server starts on a log `seed` wrote.
    pub fn seeded(name: &str, seed: impl FnOnce(&mut Seeder)) -> Fleet {
        Fleet::build(name, vec![], seed)
    }

    /// A fleet whose server is `bin` — a pinned revision's — from its
    /// first start ([`olds`]).
    pub fn old(name: &str, bin: &Path) -> Fleet {
        Fleet::built(name, vec![], |_| {}, Some(bin))
    }

    fn build(name: &str, env: Vec<(&str, &str)>, seed: impl FnOnce(&mut Seeder)) -> Fleet {
        Fleet::built(name, env, seed, None)
    }

    fn built(
        name: &str,
        env: Vec<(&str, &str)>,
        seed: impl FnOnce(&mut Seeder),
        bin: Option<&Path>,
    ) -> Fleet {
        let root = tempfile::Builder::new()
            .prefix(&format!("fleet-{name}-"))
            .tempdir()
            .unwrap();
        let mut seeder = Seeder::new();
        seed(&mut seeder);
        if seeder.head() > 0 {
            ark_server::persist::save(&root.path().join("server-data"), &seeder.authority.log)
                .unwrap();
        }
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
            seeded: seeder.ids,
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
            bin: PathBuf::from(env!("CARGO_BIN_EXE_harken-peer")),
            args: vec![],
            roles: vec!["library".into()],
        }
    }

    /// Forget that these intents were accepted: a log the server lost
    /// took them with it, and a scenario that loses one on purpose (3c)
    /// says so rather than `converged` finding them missing.
    pub fn lost(&self, ids: impl IntoIterator<Item = Id>) {
        let mut accepted = self.accepted.lock().unwrap();
        for id in ids {
            accepted.remove(&id);
        }
    }

    /// Every id a `mutate` was answered with, so far.
    pub fn accepted(&self) -> BTreeMap<Id, String> {
        self.accepted.lock().unwrap().clone()
    }

    /// The name of the function an entry names.
    pub fn function(&self, e: &ark::log::Entry) -> String {
        domain().closures().get(&e.fn_hash).map_or_else(
            || format!("#{}", hex(&e.fn_hash)),
            |c| c.function.name.clone(),
        )
    }

    /// **The invariant**: once the network is back, every peer's confirmed
    /// store hashes as the log's replay does, every cursor is the log's
    /// head, no intent id is in the log twice, and every intent a peer
    /// accepted is in the log — or was refused, and said so — while every
    /// entry in the log that the scanner did not author is one a peer
    /// accepted.
    ///
    /// The network is not given back here: a scenario releases its proxies
    /// first. Peers that are not running are left out of the comparison and
    /// still owe their intents; restart them before asking.
    ///
    /// **A refusal fails it.** Nothing a scenario authors should be refused
    /// by the authority, and a check that forgave one passed once for the
    /// wrong reason — an intent turned away as `not yours` looked exactly
    /// like one accounted for. Only the fuzz, whose schedule can race a
    /// removal against another peer's, asks
    /// [`Fleet::converged_allowing_refusals`].
    pub fn converged(&self, peers: &mut [&mut PeerProc]) -> Converged {
        self.converge(peers, false)
    }

    /// [`Fleet::converged`], where an accepted intent the authority refused
    /// — and said so — is accounted for.
    pub fn converged_allowing_refusals(&self, peers: &mut [&mut PeerProc]) -> Converged {
        self.converge(peers, true)
    }

    fn converge(&self, peers: &mut [&mut PeerProc], refusals: bool) -> Converged {
        let started = Instant::now();
        let deadline = started + PATIENCE;
        let schema = domain().module().schema.clone();
        let mut last = String::new();
        let (log, head, hash) = loop {
            for p in peers.iter_mut() {
                p.settle(Duration::from_secs(2));
            }
            let log = self
                .server
                .log_on_disk()
                .unwrap_or_else(|| Log::empty(schema.clone()));
            let head = log.head_seq();
            let hash = hex(&state_hash(
                &log.state_at(head).expect("a state at the head"),
            ));
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
                    "{}: not converged after {PATIENCE:?}:\n{last}\n--- server ---\n{}\n{}",
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
        for (n, (e, _)) in &log.entries {
            if let Some(first) = ids.insert(e.id, *n) {
                panic!(
                    "{}: intent {} is in the log twice, at {first} and {n}",
                    self.name,
                    hex(&e.id)
                );
            }
        }
        // What was accepted is there, or was refused and said so; and what
        // is there was accepted, or is the scanner's.
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
        // An entry the server compacted below its horizon is gone from
        // `entries` and its id is not: the log keeps every id it ever
        // sequenced (§10.3, `ark::retention`).
        let accepted = self.accepted();
        for (id, who) in &accepted {
            let sequenced = ids.contains_key(id) || log.seq_of(id).is_some();
            match (sequenced, refused.get(id)) {
                (true, None) => {}
                (false, Some(_)) => {}
                (true, Some(why)) => panic!(
                    "{}: {who} {} is in the log and was refused: {why}",
                    self.name,
                    hex(id)
                ),
                (false, None) => panic!(
                    "{}: {who} {} was accepted and is not in the log (head {head}):\n{last}",
                    self.name,
                    hex(id)
                ),
            }
        }
        for (n, (e, _)) in &log.entries {
            if e.actor != harken_server::library::ACCOUNT
                && !accepted.contains_key(&e.id)
                && !self.seeded.contains(&e.id)
            {
                panic!(
                    "{}: entry {n} ({} by {}) was never accepted by any peer",
                    self.name,
                    self.function(e),
                    e.actor
                );
            }
        }
        let entries = log
            .entries
            .iter()
            .map(|(n, (e, _))| (*n, e.clone()))
            .collect();
        Converged {
            head,
            hash,
            took,
            entries,
            log,
        }
    }

    /// The rows of `table` in the log's state at its head.
    pub fn rows(&self, table: &str) -> Vec<BTreeMap<String, Value>> {
        use ark::store::Store;
        let log = self.server.log_on_disk().expect("a log");
        log.state_at(log.head_seq())
            .unwrap()
            .scan(table)
            .iter()
            .map(ark::store::Row::to_struct)
            .collect()
    }
}

/// `add_song`'s arguments for a song typed in by hand, with a file of its
/// own so no two collide (`add_song` does nothing for a file it has).
pub fn song(title: &str) -> Vec<(&'static str, Value)> {
    vec![
        ("title", Value::text(title)),
        ("artist", Value::text("The Fleet")),
        ("album", Value::text("Loopback")),
        ("duration_ms", Value::int(1000)),
        ("file", Value::text(format!("typed/{title}.wav"))),
        ("track", Value::int(0)),
        ("part", Value::text("")),
        ("catalogue", Value::text("")),
        ("performer", Value::text("")),
        ("bpm", Value::int(0)),
        ("album_art", Value::text("")),
        ("artist_art", Value::text("")),
        ("disc", Value::int(0)),
        ("work_title", Value::text("")),
        ("movement_no", Value::int(0)),
    ]
}

/// The ids of what a query's rows name, in order.
pub fn ids_of(rows: &[BTreeMap<String, Value>], key: &str) -> Vec<Id> {
    rows.iter()
        .map(|r| match &r[key] {
            Value::Id(i) => *i,
            other => panic!("{key}: {other:?}"),
        })
        .collect()
}

/// Every playlist item in `rows` (the `playlist_item` table), as each
/// playlist's media in `pos` order.
pub fn playlists_in_pos_order(rows: &[BTreeMap<String, Value>]) -> BTreeMap<Id, Vec<Id>> {
    let mut by: BTreeMap<Id, Vec<(i64, Id)>> = BTreeMap::new();
    for r in rows {
        let (Value::Id(p), Value::Id(m), Value::Int(pos)) =
            (&r["playlist_id"], &r["media_id"], &r["pos"])
        else {
            panic!("{r:?}")
        };
        by.entry(*p).or_default().push((*pos, *m));
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

// -- versions (`docs/plan-db.md` D1) -------------------------------------------------

/// A pinned previous revision's binaries, as `checks.versions` hands them
/// to the fleet: `HARKEN_OLD_<n>_SERVER` and `HARKEN_OLD_<n>_PEER` for
/// `n` from 1, in `nix/versions.nix`'s order, and `HARKEN_OLD_<n>_NAME`
/// for what a scenario prints. One environment variable per binary rather
/// than a directory per revision, because that is what a shell sets by
/// hand: `HARKEN_OLD_1_SERVER=$(nix build --print-out-paths
/// .#harken-server-v4-journal)/bin/harken-server`.
#[derive(Clone, Debug)]
pub struct Old {
    pub n: usize,
    pub name: String,
    pub server: PathBuf,
    pub peer: PathBuf,
}

/// Every pinned revision the environment names, in order; a gap ends the
/// list. Empty on a laptop, where the version scenarios say so and pass.
pub fn olds() -> Vec<Old> {
    let mut out = vec![];
    for n in 1.. {
        let var = |what: &str| std::env::var(format!("HARKEN_OLD_{n}_{what}")).ok();
        let (Some(server), Some(peer)) = (var("SERVER"), var("PEER")) else {
            break;
        };
        out.push(Old {
            n,
            name: var("NAME").unwrap_or_else(|| format!("old-{n}")),
            server: server.into(),
            peer: peer.into(),
        });
    }
    out
}

/// The pinned revisions, or a line saying the scenario is skipped and why.
pub fn olds_or_skip(scenario: &str) -> Vec<Old> {
    let olds = olds();
    if olds.is_empty() {
        println!(
            "fleet: {scenario}: skipped — no pinned revision named (HARKEN_OLD_1_SERVER and \
             HARKEN_OLD_1_PEER unset; `nix flake check` sets them in checks.versions)"
        );
    }
    olds
}

/// The grown domain (`harken_server::grown`) written as an `.ark` file
/// under `dir`: what `HARKEN_MODULE` and `harken-peer --module` take.
pub fn grown_module(dir: &Path) -> PathBuf {
    let path = dir.join("grown.ark");
    harken_server::grown::write(&path).unwrap();
    path
}

/// The fields of a playlist every revision has.
fn listed(rows: &[BTreeMap<String, Value>]) -> Vec<(Value, Value)> {
    rows.iter()
        .map(|r| (r["id"].clone(), r["name"].clone()))
        .collect()
}

impl Fleet {
    /// **What a version scenario ends on** (`docs/plan-db.md` D1): every
    /// running peer at the log's head with nothing pending, and what each
    /// peer's user sees — their playlists in order, and what is on each in
    /// order — equal to the log's state at the head; every intent once;
    /// nothing refused. Rows rather than a state hash, because a peer of
    /// another revision computes its hash over its own schema, or by its
    /// own definition (`docs/plan-db.md` D3 moved it), so equal rows can
    /// hash apart. `peers` are `(peer, user)`.
    pub fn converged_across(&self, peers: &mut [(&mut PeerProc, &str)]) -> Duration {
        let started = Instant::now();
        let deadline = started + PATIENCE;
        loop {
            for (p, _) in peers.iter_mut() {
                p.settle(Duration::from_secs(2));
            }
            let log = self.server.log_on_disk().expect("a log");
            let head = log.head_seq();
            let st = log.state_at(head).expect("a state at the head");
            let rows = |t: &str| -> Vec<BTreeMap<String, Value>> {
                use ark::store::Store;
                st.scan(t).iter().map(ark::store::Row::to_struct).collect()
            };
            let (playlists, items, media) =
                (rows("playlist"), rows("playlist_item"), rows("media"));
            let mut off = vec![];
            for (p, user) in peers.iter_mut() {
                let s = p.status();
                if s.cursor != head || s.pending != 0 {
                    off.push(format!(
                        "{}: cursor {} pending {} (head {head}) held {:?} behind {:?}",
                        p.name, s.cursor, s.pending, s.held, s.behind
                    ));
                    continue;
                }
                let mut mine: Vec<&BTreeMap<String, Value>> = playlists
                    .iter()
                    .filter(|r| r["user_id"] == Value::text(*user))
                    .collect();
                mine.sort_by_key(|r| match r["pos"] {
                    Value::Int(n) => n,
                    _ => 0,
                });
                let want: Vec<(Value, Value)> = mine
                    .iter()
                    .map(|r| (r["id"].clone(), r["name"].clone()))
                    .collect();
                let got = listed(&p.query("playlists", vec![]));
                if got != want {
                    off.push(format!("{}: playlists {got:?}, the log {want:?}", p.name));
                    continue;
                }
                for (id, _) in &want {
                    let mut on: Vec<(i64, Value)> = items
                        .iter()
                        .filter(|r| &r["playlist_id"] == id)
                        .filter(|r| media.iter().any(|m| m["id"] == r["media_id"]))
                        .map(|r| {
                            let Value::Int(pos) = r["pos"] else {
                                panic!("{r:?}")
                            };
                            (pos, r["media_id"].clone())
                        })
                        .collect();
                    on.sort_by_key(|(pos, _)| *pos);
                    let want: Vec<Value> = on.into_iter().map(|(_, m)| m).collect();
                    let got: Vec<Value> = p
                        .query("playlist", vec![("playlist_id", id.clone())])
                        .iter()
                        .map(|r| r["id"].clone())
                        .collect();
                    if got != want {
                        off.push(format!(
                            "{}: playlist {id:?} holds {got:?}, the log {want:?}",
                            p.name
                        ));
                    }
                }
            }
            if off.is_empty() {
                let mut seen = BTreeSet::new();
                for (n, (e, _)) in &log.entries {
                    assert!(
                        seen.insert(e.id),
                        "{}: intent {} twice, the second at {n}",
                        self.name,
                        hex(&e.id)
                    );
                }
                for (p, _) in peers.iter_mut() {
                    let refused = p.rejections();
                    assert!(
                        refused.is_empty(),
                        "{}: {} was refused {refused:?}",
                        self.name,
                        p.name
                    );
                }
                for (id, who) in self.accepted() {
                    assert!(
                        seen.contains(&id) || log.seq_of(&id).is_some(),
                        "{}: {who} {} was accepted and is not in the log",
                        self.name,
                        hex(&id)
                    );
                }
                return started.elapsed();
            }
            if Instant::now() >= deadline {
                let tails: Vec<String> = peers
                    .iter()
                    .map(|(p, _)| format!("--- {} ---\n{}", p.name, tail(&p.stderr, 15)))
                    .collect();
                panic!(
                    "{}: not converged after {PATIENCE:?}:\n{}\n--- server ---\n{}\n{}",
                    self.name,
                    off.join("\n"),
                    tail(&self.server.log, 20),
                    tails.join("\n")
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
