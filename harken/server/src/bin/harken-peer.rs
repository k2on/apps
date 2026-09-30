//! `harken-peer`: a harken peer with no screen (`docs/plan-fleet.md` §1).
//!
//! ```text
//! harken-peer --dir DIR --server URL [--user NAME]     a peer of that server
//! harken-peer --dir DIR --alone [--user NAME]          its own authority
//!   [--pump-ms 50] [--ping-ms 20000] [--backoff-ms 500,30000]
//!   [--auth-patience-ms N]
//! ```
//!
//! `--auth-patience-ms` bounds each sign-in request — its connection and
//! each read — where `ark_auth::client::Patience::DEFAULT` is ten seconds
//! and twenty; a sign-in the server does not answer in time fails, and at
//! start that ends the process (exit 1), as any other failed sign-in does
//! (`docs/plan-perf.md` R6).
//!
//! It is what the desktop is, minus the window: the replica in `DIR/replica`
//! (`Peer::open_path`), signed in the way the desktop signs in under dev
//! auth (`ark_auth::client::login` with a name, which a dev server answers
//! with a code and no browser), the login remembered in `DIR/login.json` so
//! a restart reuses it, the sync socket dialled and pumped on the desktop's
//! fifty milliseconds. Without `--user` it opens signed out, authoring as
//! nobody and dialling nothing, until `sign_in`.
//!
//! **It reads commands as JSON lines on stdin and answers each with one JSON
//! line on stdout**, so a test — or a shell — drives it with no socket of
//! its own. Ids, arguments and rows cross in the vectors' dialect
//! (`ark::json`: `$id`, `$int`, `$bytes`); the peer's own counters (a
//! cursor, a count) are plain JSON numbers, and the reader takes either.
//!
//! ```text
//! {"cmd":"mutate","name":"create_playlist","args":{"name":"Road trip"}}
//!                              {"ok":true,"id":{"$id":"…"}} | {"ok":false,"why":"…"}
//! {"cmd":"status"}             {"cursor":N,"pending":K,"linked":b,"denied":null|"…","user":"…",…}
//! {"cmd":"hash"}               {"cursor":N,"hash":"hex","view":"hex"}
//! {"cmd":"wait","cursor":N,"timeout_ms":T}      {"ok":b,"cursor":N}
//! {"cmd":"settle","timeout_ms":T}               {"ok":b,"cursor":N,"pending":K}
//! {"cmd":"query","name":"playlists","args":{}}  {"rows":…} | {"ok":false,"why":"…"}
//! {"cmd":"rejections"}         {"rejections":[{"id":…,"reason":"…"}]} — since last asked;
//!                              `"all":true` for every one this directory has seen
//! {"cmd":"standing","id":…}    {"standing":"pending|confirmed|rejected|unknown","why":…}
//! {"cmd":"disconnect"} {"cmd":"reconnect"}      {"ok":true}
//! {"cmd":"sign_in","user":"…"} {"cmd":"sign_out"}  {"ok":b,…}
//! {"cmd":"persist"}            {"ok":true}  — a pump and a write, now
//! {"cmd":"quit"}               {"ok":true}, then exit 0 after a persist
//! ```
//!
//! A line that is not a command is answered `{"ok":false,"why":"…"}` and
//! the peer carries on; the end of stdin is a `quit`. What happens on the
//! link — opened, dropped, denied — goes to stderr, one line each, prefixed
//! `harken-peer:`, so a test can attach it to a failure.
//!
//! `wait` and `settle` pump while they wait and read no more commands until
//! they answer: a driver sends one command and reads one answer.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use ark::hash::state_hash;
use ark::json;
use ark::value::{hex, Value};
use ark_auth::client::Patience;
use ark_auth::Login;
use ark_client::{Args, Domain, Options, Peer, Standing, Timing};

const USAGE: &str =
    "usage: harken-peer --dir DIR (--server URL [--user NAME] | --alone [--user NAME])
                   [--pump-ms 50] [--ping-ms 20000] [--backoff-ms 500,30000]
                   [--auth-patience-ms N]
Commands are JSON lines on stdin; see the source's first page.";

/// What the command line says.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Flags {
    dir: PathBuf,
    server: Option<String>,
    user: Option<String>,
    alone: bool,
    pump: Duration,
    timing: Timing,
    patience: Patience,
}

fn flags(args: &[String]) -> Result<Flags, String> {
    let mut f = Flags {
        dir: PathBuf::new(),
        server: None,
        user: None,
        alone: false,
        pump: Duration::from_millis(50),
        timing: Timing::default(),
        patience: Patience::DEFAULT,
    };
    let mut dir = None;
    let mut it = args.iter();
    let ms = |v: Option<&String>, name: &str| -> Result<u64, String> {
        v.ok_or(format!("{name} needs a value"))?
            .parse()
            .map_err(|_| format!("{name} takes milliseconds"))
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dir" => dir = Some(PathBuf::from(it.next().ok_or("--dir needs a value")?)),
            "--server" => {
                f.server = Some(
                    it.next()
                        .ok_or("--server needs a value")?
                        .trim_end_matches('/')
                        .to_string(),
                )
            }
            "--user" => f.user = Some(it.next().ok_or("--user needs a value")?.clone()),
            "--alone" => f.alone = true,
            "--pump-ms" => f.pump = Duration::from_millis(ms(it.next(), "--pump-ms")?.max(1)),
            "--ping-ms" => f.timing.ping_every_ms = ms(it.next(), "--ping-ms")?,
            "--backoff-ms" => {
                let v = it.next().ok_or("--backoff-ms needs FIRST,MAX")?;
                let (a, b) = v.split_once(',').ok_or("--backoff-ms takes FIRST,MAX")?;
                f.timing.first_backoff_ms = a
                    .trim()
                    .parse()
                    .map_err(|_| "--backoff-ms takes milliseconds")?;
                f.timing.max_backoff_ms = b
                    .trim()
                    .parse()
                    .map_err(|_| "--backoff-ms takes milliseconds")?;
            }
            "--auth-patience-ms" => {
                f.patience = Patience::of(Duration::from_millis(
                    ms(it.next(), "--auth-patience-ms")?.max(1),
                ))
            }
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unexpected argument {other}\n{USAGE}")),
        }
    }
    f.dir = dir.ok_or_else(|| format!("--dir is required\n{USAGE}"))?;
    match (f.alone, &f.server) {
        (true, Some(_)) => return Err("--alone has no server".into()),
        (false, None) => return Err(format!("--server or --alone\n{USAGE}")),
        _ => {}
    }
    Ok(f)
}

// -- the login, remembered under DIR ------------------------------------------

/// `DIR/login.json`: the login for one server. Not `ark_auth::remember`,
/// which keeps one file per user account in `$XDG_CONFIG_HOME` — a fleet
/// of peers on one machine is several devices, and each keeps its own.
fn login_file(dir: &Path) -> PathBuf {
    dir.join("login.json")
}

fn recall(dir: &Path, server: &str) -> Option<Login> {
    let text = std::fs::read_to_string(login_file(dir)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    if v.get("server")?.as_str()? != server {
        return None;
    }
    serde_json::from_value(v.get("login")?.clone()).ok()
}

fn remember(dir: &Path, server: &str, login: &Login) -> Result<(), String> {
    let body = serde_json::json!({ "server": server, "login": login });
    let tmp = dir.join(".login.json.tmp");
    std::fs::write(&tmp, body.to_string()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, login_file(dir))
        .map_err(|e| format!("{}: {e}", login_file(dir).display()))
}

fn forget(dir: &Path) {
    let _ = std::fs::remove_file(login_file(dir));
}

/// Sign `user` in at `server` the desktop's way. A dev server answers with
/// a code and no page; anything else needs a person, so the URL is said on
/// stderr for one to open.
fn log_in(server: &str, user: &str, patience: Patience) -> Result<Login, String> {
    ark_auth::client::login_with(
        server,
        Some(user),
        |url| {
            eprintln!("harken-peer: open {url} to sign in");
        },
        patience,
    )
}

// -- a command, and its answer ----------------------------------------------------

/// One line from stdin, read.
#[derive(Clone, Debug, PartialEq)]
enum Cmd {
    Mutate { name: String, args: Args },
    Status,
    Hash,
    Wait { cursor: i64, timeout: Duration },
    Settle { timeout: Duration },
    Query { name: String, args: Args },
    Rejections { all: bool },
    Standing { id: ark::value::Id },
    Disconnect,
    Reconnect,
    SignIn { user: String },
    SignOut,
    Persist,
    Quit,
}

/// How long a `wait` or `settle` waits when the line does not say.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

fn parse(line: &str) -> Result<Cmd, String> {
    let v = json::decode(line).map_err(|e| format!("not JSON in the dialect: {e}"))?;
    let Value::Struct(m) = v else {
        return Err("a command is an object".into());
    };
    let text = |k: &str| match m.get(k) {
        Some(Value::Text(t)) => Ok(t.clone()),
        Some(_) => Err(format!("`{k}` is text")),
        None => Err(format!("`{k}` is missing")),
    };
    let int = |k: &str| match m.get(k) {
        Some(Value::Int(n)) => Ok(Some(*n)),
        Some(_) => Err(format!("`{k}` is an integer")),
        None => Ok(None),
    };
    let args = || match m.get("args") {
        Some(Value::Struct(a)) => Ok(a.clone()),
        Some(_) => Err("`args` is an object".to_string()),
        None => Ok(Args::new()),
    };
    let timeout = || -> Result<Duration, String> {
        Ok(int("timeout_ms")?.map_or(DEFAULT_TIMEOUT, |n| Duration::from_millis(n.max(0) as u64)))
    };
    Ok(match text("cmd")?.as_str() {
        "mutate" => Cmd::Mutate {
            name: text("name")?,
            args: args()?,
        },
        "status" => Cmd::Status,
        "hash" => Cmd::Hash,
        "wait" => Cmd::Wait {
            cursor: int("cursor")?.ok_or("`cursor` is missing")?,
            timeout: timeout()?,
        },
        "settle" => Cmd::Settle {
            timeout: timeout()?,
        },
        "query" => Cmd::Query {
            name: text("name")?,
            args: args()?,
        },
        "rejections" => Cmd::Rejections {
            all: matches!(m.get("all"), Some(Value::Bool(true))),
        },
        "standing" => match m.get("id") {
            Some(Value::Id(id)) => Cmd::Standing { id: *id },
            _ => return Err("`id` is an id: {\"$id\":\"8-4-4-4-12\"}".into()),
        },
        "disconnect" => Cmd::Disconnect,
        "reconnect" => Cmd::Reconnect,
        "sign_in" => Cmd::SignIn {
            user: text("user")?,
        },
        "sign_out" => Cmd::SignOut,
        "persist" => Cmd::Persist,
        "quit" => Cmd::Quit,
        other => return Err(format!("no command {other:?}")),
    })
}

/// An answer, a field at a time, each already printed: `{"k":v,…}` in the
/// order written.
#[derive(Default)]
struct Answer(Vec<(&'static str, String)>);

impl Answer {
    fn ok(ok: bool) -> Answer {
        Answer::default().raw("ok", ok.to_string())
    }
    fn refused(why: impl std::fmt::Display) -> Answer {
        Answer::ok(false).text("why", &why.to_string())
    }
    fn raw(mut self, k: &'static str, v: String) -> Answer {
        self.0.push((k, v));
        self
    }
    fn num(self, k: &'static str, n: i64) -> Answer {
        self.raw(k, n.to_string())
    }
    fn text(self, k: &'static str, t: &str) -> Answer {
        self.raw(k, json::quoted(t))
    }
    fn value(self, k: &'static str, v: &Value) -> Answer {
        self.raw(k, json::json(v))
    }
    fn line(&self) -> String {
        let fields: Vec<String> = self
            .0
            .iter()
            .map(|(k, v)| format!("{}:{v}", json::quoted(k)))
            .collect();
        format!("{{{}}}", fields.join(","))
    }
}

// -- the peer --------------------------------------------------------------------------

struct Headless {
    peer: Peer,
    dir: PathBuf,
    server: Option<String>,
    /// How long a sign-in waits on the server.
    patience: Patience,
    pump: Duration,
    /// Verdicts collected and not yet asked for. Every one is also appended
    /// to `DIR/rejections.jsonl` the pump it arrives in, because the engine
    /// keeps a verdict in memory only: a peer killed before anybody asked
    /// would take it with it, and a fleet counting what became of every
    /// intent would count one intent as lost that was refused.
    unasked: Vec<String>,
}

impl Headless {
    fn open(f: &Flags, domain: Domain) -> Result<Headless, String> {
        std::fs::create_dir_all(&f.dir).map_err(|e| format!("{}: {e}", f.dir.display()))?;
        let opts = match (&f.server, f.alone) {
            (_, true) => Options::alone(f.user.clone().unwrap_or_else(|| "me".into())),
            (Some(server), false) => {
                let remembered = recall(&f.dir, server)
                    .filter(|l| f.user.as_ref().is_none_or(|u| *u == l.user.id));
                let login = match (remembered, &f.user) {
                    (Some(l), _) => Some(l),
                    (None, Some(user)) => {
                        let l = log_in(server, user, f.patience)
                            .map_err(|e| format!("signing {user} in at {server}: {e}"))?;
                        remember(&f.dir, server, &l)?;
                        Some(l)
                    }
                    (None, None) => None,
                };
                match login {
                    Some(l) => Options::server(l.user.id, l.session, Some(l.token)),
                    None => Options::signed_out(),
                }
            }
            (None, false) => unreachable!("flags() requires one"),
        }
        .with_timing(f.timing.clone());
        let mut peer = Peer::open_path(domain, f.dir.join("replica"), opts)
            .map_err(|e| format!("opening {}: {e}", f.dir.display()))?;
        if let Some(server) = &f.server {
            peer.connect(&ark_auth::socket_url(server));
        }
        Ok(Headless {
            peer,
            dir: f.dir.clone(),
            server: f.server.clone(),
            patience: f.patience,
            pump: f.pump,
            unasked: vec![],
        })
    }

    /// One turn, with whatever it came to said on stderr.
    fn pump(&mut self) {
        let p = self.peer.pump();
        if p.opened {
            eprintln!(
                "harken-peer: linked (epoch {}, cursor {})",
                self.peer.epoch(),
                self.peer.cursor()
            );
        }
        if let Some(why) = p.dropped {
            eprintln!("harken-peer: dropped: {why}");
        }
        if let Some(why) = p.denied {
            eprintln!("harken-peer: denied: {why}");
        }
        if let Some(note) = p.note {
            eprintln!("harken-peer: {note}");
        }
        // Nothing here listens to the room; what it said is not kept.
        drop(self.peer.heard());
        self.collect();
    }

    /// Take the verdicts that arrived, and write each down.
    fn collect(&mut self) {
        let fresh: Vec<String> = self
            .peer
            .take_rejections()
            .into_iter()
            .map(|r| {
                Answer::default()
                    .value("id", &Value::Id(r.id))
                    .text("reason", &r.reason)
                    .line()
            })
            .collect();
        if fresh.is_empty() {
            return;
        }
        let file = self.dir.join("rejections.jsonl");
        let wrote = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file)
            .and_then(|mut f| f.write_all(format!("{}\n", fresh.join("\n")).as_bytes()));
        if let Err(e) = wrote {
            eprintln!("harken-peer: {}: {e}", file.display());
        }
        self.unasked.extend(fresh);
    }

    /// Pump until `done`, or the timeout. Whether it was done.
    fn pump_until(&mut self, timeout: Duration, mut done: impl FnMut(&Peer) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            self.pump();
            if done(&self.peer) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(
                self.pump
                    .min(deadline.saturating_duration_since(Instant::now())),
            );
        }
    }

    fn status(&self) -> Answer {
        let s = self.peer.status();
        Answer::default()
            .num("cursor", s.cursor)
            .num("pending", s.pending as i64)
            .raw("linked", s.linked.to_string())
            .raw(
                "denied",
                s.denied.as_deref().map_or("null".into(), json::quoted),
            )
            .text("user", &s.user)
            .text("session", &s.session)
            .raw("signed_out", s.signed_out.to_string())
            .num("epoch", s.epoch)
            .text("link", &s.link)
            .num("opens", s.opens as i64)
            .num("diverged", s.diverged as i64)
    }

    /// Answer one command. `quit` is answered like `persist`; the loop is
    /// what stops.
    fn handle(&mut self, cmd: Cmd) -> Answer {
        match cmd {
            Cmd::Mutate { name, args } => match self.peer.mutate(&name, args) {
                Ok(id) => Answer::ok(true).value("id", &Value::Id(id)),
                Err(e) => Answer::refused(e),
            },
            Cmd::Status => self.status(),
            Cmd::Hash => {
                let r = self.peer.replica();
                Answer::default()
                    .num("cursor", r.cursor)
                    .text("hash", &hex(&state_hash(&r.confirmed)))
                    .text("view", &hex(&state_hash(&r.view)))
            }
            Cmd::Wait { cursor, timeout } => {
                let ok = self.pump_until(timeout, |p| p.cursor() >= cursor);
                Answer::ok(ok).num("cursor", self.peer.cursor())
            }
            Cmd::Settle { timeout } => {
                // Linked, nothing pending, and the cursor still for two
                // pumps: what "caught up" can mean from this end alone. The
                // fleet compares against the server's head as well.
                let (mut last, mut still) = (-1, 0);
                let ok = self.pump_until(timeout, |p| {
                    if p.cursor() == last {
                        still += 1;
                    } else {
                        (last, still) = (p.cursor(), 0);
                    }
                    p.linked() && p.pending_len() == 0 && still >= 2
                });
                Answer::ok(ok)
                    .num("cursor", self.peer.cursor())
                    .num("pending", self.peer.pending_len() as i64)
            }
            Cmd::Query { name, args } => match self.peer.query(&name, &args) {
                Ok(v) => Answer::default().value("rows", &v),
                Err(e) => Answer::refused(e),
            },
            Cmd::Rejections { all } => {
                self.collect();
                let asked = std::mem::take(&mut self.unasked);
                let items = if all {
                    std::fs::read_to_string(self.dir.join("rejections.jsonl"))
                        .unwrap_or_default()
                        .lines()
                        .map(str::to_string)
                        .collect()
                } else {
                    asked
                };
                Answer::default().raw("rejections", json::array(items))
            }
            Cmd::Standing { id } => match self.peer.standing(&id) {
                Standing::Pending => Answer::default().text("standing", "pending"),
                Standing::Confirmed => Answer::default().text("standing", "confirmed"),
                Standing::Rejected(why) => Answer::default()
                    .text("standing", "rejected")
                    .text("why", &why),
                Standing::Unknown => Answer::default().text("standing", "unknown"),
            },
            Cmd::Disconnect => {
                self.peer.disconnect();
                Answer::ok(true)
            }
            Cmd::Reconnect => {
                self.peer.reconnect();
                Answer::ok(true)
            }
            Cmd::SignIn { user } => {
                let Some(server) = self.server.clone() else {
                    return Answer::refused("a peer alone has no server to sign in to");
                };
                let login = match log_in(&server, &user, self.patience) {
                    Ok(l) => l,
                    Err(e) => {
                        return Answer::refused(format!("signing {user} in at {server}: {e}"))
                    }
                };
                if let Err(e) = remember(&self.dir, &server, &login) {
                    eprintln!("harken-peer: the login is not remembered: {e}");
                }
                match self.peer.sign_in(
                    login.user.id.clone(),
                    login.session.clone(),
                    Some(login.token),
                ) {
                    Ok(()) => Answer::ok(true)
                        .text("user", &login.user.id)
                        .text("session", &login.session),
                    Err(e) => Answer::refused(e),
                }
            }
            Cmd::SignOut => {
                self.peer.sign_out();
                forget(&self.dir);
                Answer::ok(true)
            }
            Cmd::Persist | Cmd::Quit => {
                self.pump();
                match self.peer.persist() {
                    Ok(()) => Answer::ok(true),
                    Err(e) => Answer::refused(e),
                }
            }
        }
    }
}

/// Every line of stdin, on a thread of its own, so the peer pumps while
/// nothing is said. The channel closing is the end of stdin.
fn lines() -> Receiver<String> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}

fn run(f: Flags) -> Result<(), String> {
    let mut h = Headless::open(&f, Domain::new(&harken_domain::module()))?;
    let input = lines();
    let mut out = std::io::stdout().lock();
    // The pump keeps its own clock rather than waiting for a quiet stdin:
    // a driver asking `status` every ten milliseconds would otherwise be a
    // peer that never pumps, and a test polling for progress would be what
    // stopped it.
    let mut due = Instant::now();
    loop {
        if Instant::now() >= due {
            h.pump();
            due = Instant::now() + h.pump;
        }
        let line = match input.recv_timeout(due.saturating_duration_since(Instant::now())) {
            Ok(l) => l,
            Err(RecvTimeoutError::Timeout) => continue,
            // The end of stdin is a quit.
            Err(RecvTimeoutError::Disconnected) => {
                h.handle(Cmd::Quit);
                return Ok(());
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let (answer, quit) = match parse(&line) {
            Ok(cmd) => {
                let quit = cmd == Cmd::Quit;
                (h.handle(cmd), quit)
            }
            Err(why) => (Answer::refused(why), false),
        };
        // A reader that has gone is a driver that has gone: stop, with what
        // is pending written down (Drop persists).
        if writeln!(out, "{}", answer.line())
            .and_then(|_| out.flush())
            .is_err()
            || quit
        {
            return Ok(());
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let f = match flags(&args) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("harken-peer: {e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(f) {
        eprintln!("harken-peer: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// The command line: a server or alone, never both, never neither; the
    /// timings a test shortens. Falsified by letting `--alone` take a
    /// server: the third case parses.
    #[test]
    fn the_flags() {
        let f = flags(&a(&[
            "--dir",
            "/d",
            "--server",
            "http://h:1/",
            "--user",
            "alice",
            "--ping-ms",
            "300",
            "--backoff-ms",
            "20,200",
        ]))
        .unwrap();
        assert_eq!(
            f.server.as_deref(),
            Some("http://h:1"),
            "a trailing slash is not a second server"
        );
        assert_eq!(f.user.as_deref(), Some("alice"));
        assert_eq!(
            (
                f.timing.ping_every_ms,
                f.timing.first_backoff_ms,
                f.timing.max_backoff_ms
            ),
            (300, 20, 200)
        );
        assert!(
            flags(&a(&["--server", "http://h:1"])).is_err(),
            "a directory is required"
        );
        assert!(flags(&a(&["--dir", "/d", "--alone", "--server", "http://h:1"])).is_err());
        assert!(flags(&a(&["--dir", "/d"])).is_err());
        assert!(flags(&a(&["--dir", "/d", "--alone", "--what"])).is_err());
        assert!(flags(&a(&["--dir", "/d", "--alone"])).unwrap().alone);
    }

    /// Every command reads, in the dialect and plainly; what is not a
    /// command says why. Falsified by reading `cursor` as text: `wait`
    /// fails to parse.
    #[test]
    fn the_commands_read() {
        let id = "00000000-0000-0000-0000-00000000002a";
        assert_eq!(
            parse(r#"{"cmd":"mutate","name":"create_playlist","args":{"name":"Road trip"}}"#)
                .unwrap(),
            Cmd::Mutate {
                name: "create_playlist".into(),
                args: [("name".to_string(), Value::text("Road trip"))].into(),
            }
        );
        assert_eq!(
            parse(r#"{"cmd":"wait","cursor":7,"timeout_ms":{"$int":"250"}}"#).unwrap(),
            Cmd::Wait {
                cursor: 7,
                timeout: Duration::from_millis(250)
            }
        );
        assert_eq!(
            parse(r#"{"cmd":"settle"}"#).unwrap(),
            Cmd::Settle {
                timeout: DEFAULT_TIMEOUT
            }
        );
        assert_eq!(
            parse(r#"{"cmd":"query","name":"playlists"}"#).unwrap(),
            Cmd::Query {
                name: "playlists".into(),
                args: Args::new()
            }
        );
        let mut want = [0u8; 16];
        want[15] = 42;
        assert_eq!(
            parse(&format!(r#"{{"cmd":"standing","id":{{"$id":"{id}"}}}}"#)).unwrap(),
            Cmd::Standing { id: want }
        );
        assert_eq!(
            parse(r#"{"cmd":"rejections","all":true}"#).unwrap(),
            Cmd::Rejections { all: true }
        );
        assert_eq!(
            parse(r#"{"cmd":"sign_in","user":"alice"}"#).unwrap(),
            Cmd::SignIn {
                user: "alice".into()
            }
        );
        for (line, simple) in [
            ("status", Cmd::Status),
            ("hash", Cmd::Hash),
            ("rejections", Cmd::Rejections { all: false }),
            ("disconnect", Cmd::Disconnect),
            ("reconnect", Cmd::Reconnect),
            ("sign_out", Cmd::SignOut),
            ("persist", Cmd::Persist),
            ("quit", Cmd::Quit),
        ] {
            assert_eq!(parse(&format!(r#"{{"cmd":"{line}"}}"#)).unwrap(), simple);
        }
        for (bad, says) in [
            ("nope", "not JSON"),
            ("[1]", "an object"),
            (r#"{"cmd":"fly"}"#, "no command"),
            (r#"{"cmd":"wait"}"#, "`cursor` is missing"),
            (r#"{"cmd":"wait","cursor":"7"}"#, "integer"),
            (
                r#"{"cmd":"mutate","name":"x","args":[]}"#,
                "`args` is an object",
            ),
            (r#"{"cmd":"standing","id":"x"}"#, "`id` is an id"),
        ] {
            let e = parse(bad).unwrap_err();
            assert!(e.contains(says), "{bad}: {e}");
        }
    }

    /// A peer alone, driven through `handle` exactly as stdin would: an
    /// intent and its id, a refusal, the rows, the hashes of a store that
    /// moved, the standing, and an answer that reads back as the dialect.
    /// Falsified by `Answer::refused` leaving out `why`: the refusal's
    /// reason is not in the answer.
    #[test]
    fn a_peer_answers_in_the_dialect() {
        let dir = tempfile::tempdir().unwrap();
        let f = flags(&a(&[
            "--dir",
            dir.path().to_str().unwrap(),
            "--alone",
            "--user",
            "me",
        ]))
        .unwrap();
        let mut h = Headless::open(&f, Domain::new(&harken_domain::module())).unwrap();
        let before = json::decode(&h.handle(Cmd::Hash).line()).unwrap();
        let made = h.handle(
            parse(r#"{"cmd":"mutate","name":"create_playlist","args":{"name":"Road trip"}}"#)
                .unwrap(),
        );
        let made = json::decode(&made.line()).unwrap();
        assert_eq!(made.as_struct()["ok"], Value::Bool(true));
        let Value::Id(id) = made.as_struct()["id"] else {
            panic!("{made:?}")
        };
        let refused = json::decode(
            &h.handle(
                parse(r#"{"cmd":"mutate","name":"create_playlist","args":{"name":" "}}"#).unwrap(),
            )
            .line(),
        )
        .unwrap();
        assert_eq!(refused.as_struct()["ok"], Value::Bool(false));
        assert_eq!(
            refused.as_struct()["why"],
            Value::text("a playlist needs a name")
        );
        let rows = json::decode(
            &h.handle(parse(r#"{"cmd":"query","name":"playlists"}"#).unwrap())
                .line(),
        )
        .unwrap();
        let Value::List(rows) = &rows.as_struct()["rows"] else {
            panic!("{rows:?}")
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].as_struct()["name"], Value::text("Road trip"));
        let after = json::decode(&h.handle(Cmd::Hash).line()).unwrap();
        assert_ne!(
            before.as_struct()["hash"],
            after.as_struct()["hash"],
            "alone, the confirmed store moved"
        );
        assert_eq!(after.as_struct()["cursor"], Value::int(1));
        assert_eq!(
            h.handle(Cmd::Standing { id }).line(),
            r#"{"standing":"confirmed"}"#
        );
        let status = json::decode(&h.handle(Cmd::Status).line()).unwrap();
        assert_eq!(status.as_struct()["pending"], Value::int(0));
        assert_eq!(status.as_struct()["denied"], Value::Null);
        assert_eq!(
            h.handle(Cmd::SignIn { user: "x".into() }).line(),
            r#"{"ok":false,"why":"a peer alone has no server to sign in to"}"#
        );
        assert_eq!(
            h.handle(Cmd::Rejections { all: true }).line(),
            r#"{"rejections":[]}"#
        );
        assert_eq!(h.handle(Cmd::Quit).line(), r#"{"ok":true}"#);
    }

    /// With a server that is not there: a peer signed out holds its intent
    /// pending and durable, its confirmed hash is the empty store's while
    /// its view's is not, `wait` answers at its timeout rather than
    /// hanging, and a reopen from the directory finds the intent.
    /// Falsified by answering `hash` with the view's hash in both fields.
    #[test]
    fn a_peer_offline_keeps_what_it_made() {
        let dir = tempfile::tempdir().unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let server = format!("http://127.0.0.1:{port}");
        let f = flags(&a(&[
            "--dir",
            dir.path().to_str().unwrap(),
            "--server",
            &server,
        ]))
        .unwrap();
        let mut h = Headless::open(&f, Domain::new(&harken_domain::module())).unwrap();
        let made = h.handle(
            parse(r#"{"cmd":"mutate","name":"create_playlist","args":{"name":"Offline"}}"#)
                .unwrap(),
        );
        assert!(
            made.line().starts_with(r#"{"ok":true,"id":{"$id":"#),
            "{}",
            made.line()
        );
        let hash = json::decode(&h.handle(Cmd::Hash).line()).unwrap();
        assert_ne!(
            hash.as_struct()["hash"],
            hash.as_struct()["view"],
            "confirmed is empty, the view is not"
        );
        let waited = h.handle(Cmd::Wait {
            cursor: 1,
            timeout: Duration::from_millis(120),
        });
        assert_eq!(waited.line(), r#"{"ok":false,"cursor":0}"#);
        let status = json::decode(&h.handle(Cmd::Status).line()).unwrap();
        assert_eq!(status.as_struct()["signed_out"], Value::Bool(true));
        assert_eq!(status.as_struct()["pending"], Value::int(1));
        drop(h);
        let h = Headless::open(&f, Domain::new(&harken_domain::module())).unwrap();
        assert_eq!(h.peer.pending_len(), 1, "the intent was on disk");
    }
}
