//! `harken-peer`: a harken peer with no screen (`docs/plan-fleet.md` §1).
//!
//! ```text
//! harken-peer --dir DIR --server URL [--user NAME]     a peer of that server
//! harken-peer --dir DIR --alone [--user NAME]          its own authority
//!   [--pump-ms 50] [--ping-ms 20000] [--backoff-ms 500,30000]
//!   [--auth-patience-ms N] [--roles ROLE,ROLE]
//!   [--device KIND,NAME,CAN_PLAY]
//! ```
//!
//! It is an `ark_peer::Peer` and its link, driven as a phone's is
//! (`ark_ffi::Peer`, which pumps it on a thread of its own and speaks JSON),
//! minus the screen: the replica in `DIR/replica` (the chain on disk), signed in the
//! way the desktop signs in under dev auth (`ark_auth::client::login` with a
//! name, which a dev server answers with a code and no browser), the login
//! remembered in `DIR/logins` so a restart reuses it. Without `--user`
//! it opens signed out, authoring as nobody and dialling nothing, until
//! `sign_in`.
//!
//! `--device` declares this peer a device of the account's listening
//! session in its volatile `Hello`: `phone,Phone,true` is a phone called
//! Phone that can play. Its id is its login's session, so a device is a
//! login, as every client declares it. Without it the peer is no device and
//! has no volatile machine.
//!
//! `--roles` names roles this device's entries carry beside the ones its
//! login says (`docs/plan-auth.md`): a peer alone, or signed out, has no
//! login to say any. They are what `has_role` answers in this device's
//! preview; the hub stamps every entry with the roles its own sign-in gives
//! the connection, whatever this says.
//!
//! It runs harken's domain as this build compiled it
//! (`harken_server::hosted`), and reads a function's arguments by its
//! input's own checks (`Headless::args`).
//!
//! `--auth-patience-ms` bounds each sign-in request — its connection and
//! each read — where `ark_auth::client::Patience::DEFAULT` is ten seconds
//! and twenty; a sign-in the server does not answer in time fails, and at
//! start that ends the process (exit 1), as any other failed sign-in does.
//! `--ping-ms` is read and has no effect: the hub pings, and the link
//! answers.
//!
//! **Alone, then a server, is one directory.** A directory used `--alone`
//! and then opened with `--server` is a peer joining that server: the chain
//! it sequenced alone is pushed as intents on its first connection and is
//! pending until the server answers it — as the login `--user` signs in,
//! or as nobody until `sign_in` — and the other way round a directory
//! opened `--alone` after a server goes alone from where it is. `join` and
//! `leave` do the same without a restart.
//!
//! **It reads commands as JSON lines on stdin and answers each with one JSON
//! line on stdout**, so a test — or a shell — drives it with no socket of
//! its own. Arguments and rows are the domain's own JSON: an id its hex, an
//! integer a number, an `Option` `null` or its value, a sum externally
//! tagged (`{"Playing":{"at_ms":…,"since":…}}`, a variant with no fields
//! its name); an entry's id its hex.
//!
//! ```text
//! {"cmd":"mutate","name":"create_playlist","args":{"name":"Road trip"}}
//!                              {"ok":true,"id":"<hex>"} | {"ok":false,"why":"…"}
//! {"cmd":"status"}             {"cursor":N,"pending":K,"held":0,"behind":false,"linked":b,
//!                               "denied":null|"…","user":"…",…}
//! {"cmd":"hash"}               {"cursor":N,"hash":"hex","view":"hex"}
//! {"cmd":"wait","cursor":N,"timeout_ms":T}      {"ok":b,"cursor":N}
//! {"cmd":"settle","timeout_ms":T}               {"ok":b,"cursor":N,"pending":K}
//! {"cmd":"query","name":"playlists","args":{}}  {"rows":…} | {"ok":false,"why":"…"}
//! {"cmd":"rejections"}         {"rejections":[{"id":…,"reason":"…"}]} — since last asked;
//!                              `"all":true` for every one this directory has seen
//! {"cmd":"standing","id":…}    {"standing":"pending|confirmed|rejected|unknown","why":…}
//! {"cmd":"disconnect"} {"cmd":"reconnect"}      {"ok":true}
//! {"cmd":"sign_in","user":"…"} {"cmd":"sign_out"}  {"ok":b,…}
//! {"cmd":"join","server":URL,"user":"…"}   {"ok":b,"pending":K,…} — alone → that server;
//!                              without `user`, signed out
//! {"cmd":"leave"}              {"ok":b,"cursor":N} — the server → alone
//! {"cmd":"call","live":"listening","transition":"pause","input":{}}
//!                              {"ok":true,"kind":"ordered|ephemeral"} | {"ok":false,"why":"…"}
//!                              — previewed at once, and sent if linked
//! {"cmd":"live","name":"listening"}
//!                              {"value":…|null,"device":"…","seq":N,"pending":K,"synced":b}
//! {"cmd":"live_refusals"}      {"refusals":["…"]} — the server's, since last asked
//! {"cmd":"persist"}            {"ok":true}  — a pump, now (the chain is on disk as it moves)
//! {"cmd":"quit"}               {"ok":true}, then exit 0
//! ```
//!
//! A line that is not a command is answered `{"ok":false,"why":"…"}` and
//! the peer carries on; the end of stdin is a `quit`. What happens on the
//! link — opened, dropped, denied — goes to stderr, one line each, prefixed
//! `harken-peer:`, so a test can attach it to a failure.
//!
//! The peer pumps on its own thread whenever the link says something;
//! `wait` and `settle` look at it every few milliseconds and read no more
//! commands until they answer: a driver sends one command and reads one
//! answer. `--pump-ms` is how long `settle` waits with nothing new before it
//! counts the cursor as still.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ark::format::Encode as Bytes;
use ark::hash::hex;
use ark::log::EntryId;
use ark::peer::{Journal, Role, Who};
use ark_auth::client::Patience;
use ark_auth::Login;
use ark_ffi::Standing;
use ark_peer::{Config, Files, Live, Peer};
use harken_server::domain::host::Listening;
use harken_server::domain::Device;
use harken_server::hosted::Hosted;
use serde_json::{json, Map, Value};

const USAGE: &str =
    "usage: harken-peer --dir DIR (--server URL [--user NAME] | --alone [--user NAME])
                   [--pump-ms 50] [--ping-ms 20000] [--backoff-ms 500,30000]
                   [--auth-patience-ms N] [--roles ROLE,ROLE]
                   [--device KIND,NAME,CAN_PLAY]
Commands are JSON lines on stdin; see the source's first page.";

/// What `status` says of a login the server no longer knows.
const REVOKED: &str = "the server does not know this login: signed out, or revoked";

/// What `--device` says: its kind, its name, whether it can play.
#[derive(Clone, Debug, PartialEq, Eq)]
struct DeviceFlag {
    kind: String,
    name: String,
    can_play: bool,
}

/// What the command line says.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Flags {
    dir: PathBuf,
    server: Option<String>,
    user: Option<String>,
    alone: bool,
    pump: Duration,
    backoff: (u64, u64),
    patience: Patience,
    roles: Vec<String>,
    device: Option<DeviceFlag>,
}

fn flags(args: &[String]) -> Result<Flags, String> {
    let mut f = Flags {
        dir: PathBuf::new(),
        server: None,
        user: None,
        alone: false,
        pump: Duration::from_millis(50),
        backoff: (500, 30_000),
        patience: Patience::DEFAULT,
        roles: vec![],
        device: None,
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
            "--ping-ms" => {
                ms(it.next(), "--ping-ms")?;
            }
            "--backoff-ms" => {
                let v = it.next().ok_or("--backoff-ms needs FIRST,MAX")?;
                let (a, b) = v.split_once(',').ok_or("--backoff-ms takes FIRST,MAX")?;
                f.backoff = (
                    a.trim()
                        .parse()
                        .map_err(|_| "--backoff-ms takes milliseconds")?,
                    b.trim()
                        .parse()
                        .map_err(|_| "--backoff-ms takes milliseconds")?,
                );
            }
            "--auth-patience-ms" => {
                f.patience = Patience::of(Duration::from_millis(
                    ms(it.next(), "--auth-patience-ms")?.max(1),
                ))
            }
            "--roles" => {
                f.roles = it
                    .next()
                    .ok_or("--roles needs ROLE,ROLE")?
                    .split(',')
                    .map(str::trim)
                    .filter(|r| !r.is_empty())
                    .map(str::to_string)
                    .collect()
            }
            "--device" => {
                let v = it.next().ok_or("--device needs KIND,NAME,CAN_PLAY")?;
                // The kind before the first comma, whether it can play after the last, and
                // the name — which may hold commas of its own — between.
                let (Some((kind, rest)), true) = (v.split_once(','), v.matches(',').count() >= 2)
                else {
                    return Err("--device takes KIND,NAME,CAN_PLAY".into());
                };
                let (name, can) = rest.rsplit_once(',').expect("two commas");
                let can_play = match can {
                    "true" => true,
                    "false" => false,
                    _ => return Err("--device's CAN_PLAY is true or false".into()),
                };
                f.device = Some(DeviceFlag {
                    kind: kind.into(),
                    name: name.into(),
                    can_play,
                });
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

/// `DIR/logins`: the logins of this directory's device, one per server
/// (`ark_peer::Logins` at a directory of its own, not the app's — a
/// fleet of peers on one machine is several devices, and each keeps its
/// own).
fn logins(dir: &Path) -> ark_peer::Logins {
    ark_peer::Logins::at(&dir.join("logins"))
}

/// Sign `user` in at `server` the desktop's way. A dev server answers with
/// a code and no page; anything else needs a person, so the URL is said on
/// stderr for one to open.
fn log_in(server: &str, user: &str, patience: Patience) -> Result<Login, String> {
    ark_auth::client::login_with(
        server,
        Some(user),
        |url| eprintln!("harken-peer: open {url} to sign in"),
        patience,
    )
}

/// Nobody: a peer signed out, or one alone with no name.
fn nobody(name: &str) -> Who {
    Who {
        actor: name.into(),
        session: name.into(),
        roles: vec![],
    }
}

// -- JSON: the domain's own --------------------------------------------------------

/// One account's session as JSON: its devices, scrubbers and what plays.
fn listening_json(l: &Listening) -> Value {
    json!({ "devices": l.devices, "scrub": l.scrub, "session": l.session })
}

// -- a command, and its answer ----------------------------------------------------

/// One line from stdin, read.
#[derive(Clone, Debug, PartialEq)]
enum Cmd {
    Mutate {
        name: String,
        args: Value,
    },
    Status,
    Hash,
    Wait {
        cursor: i64,
        timeout: Duration,
    },
    Settle {
        timeout: Duration,
    },
    Query {
        name: String,
        args: Value,
    },
    Rejections {
        all: bool,
    },
    Standing {
        id: [u8; 16],
    },
    Disconnect,
    Reconnect,
    SignIn {
        user: String,
    },
    SignOut,
    Join {
        server: String,
        user: Option<String>,
    },
    Leave,
    Call {
        live: String,
        transition: String,
        input: Value,
    },
    Live {
        name: String,
    },
    LiveRefusals,
    Persist,
    Quit,
}

/// How long a `wait` or `settle` waits when the line does not say.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

fn parse(line: &str) -> Result<Cmd, String> {
    let v: Value = serde_json::from_str(line).map_err(|e| format!("not JSON: {e}"))?;
    let Value::Object(m) = v else {
        return Err("a command is an object".into());
    };
    let text = |k: &str| match m.get(k) {
        Some(Value::String(t)) => Ok(t.to_string()),
        Some(_) => Err(format!("`{k}` is text")),
        None => Err(format!("`{k}` is missing")),
    };
    let int = |k: &str| match m.get(k) {
        Some(v) => v
            .as_i64()
            .map(Some)
            .ok_or_else(|| format!("`{k}` is an integer")),
        None => Ok(None),
    };
    let object = |k: &str| match m.get(k) {
        Some(Value::Object(a)) => Ok(Value::Object(a.clone())),
        Some(_) => Err(format!("`{k}` is an object")),
        None => Ok(json!({})),
    };
    let timeout = || -> Result<Duration, String> {
        Ok(int("timeout_ms")?.map_or(DEFAULT_TIMEOUT, |n| Duration::from_millis(n.max(0) as u64)))
    };
    Ok(match text("cmd")?.as_str() {
        "mutate" => Cmd::Mutate {
            name: text("name")?,
            args: object("args")?,
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
            args: object("args")?,
        },
        "rejections" => Cmd::Rejections {
            all: m.get("all") == Some(&Value::Bool(true)),
        },
        "standing" => match m
            .get("id")
            .and_then(Value::as_str)
            .and_then(ark::hash::unhex)
            .and_then(|b| b.try_into().ok())
        {
            Some(id) => Cmd::Standing { id },
            _ => return Err("`id` is an entry's id, its hex".into()),
        },
        "disconnect" => Cmd::Disconnect,
        "reconnect" => Cmd::Reconnect,
        "sign_in" => Cmd::SignIn {
            user: text("user")?,
        },
        "sign_out" => Cmd::SignOut,
        "join" => Cmd::Join {
            server: text("server")?.trim_end_matches('/').to_string(),
            user: match m.get("user") {
                None | Some(Value::Null) => None,
                Some(_) => Some(text("user")?),
            },
        },
        "leave" => Cmd::Leave,
        "call" => Cmd::Call {
            live: text("live")?,
            transition: text("transition")?,
            input: object("input")?,
        },
        "live" => Cmd::Live {
            name: text("name")?,
        },
        "live_refusals" => Cmd::LiveRefusals,
        "persist" => Cmd::Persist,
        "quit" => Cmd::Quit,
        other => return Err(format!("no command {other:?}")),
    })
}

/// An answer, a field at a time.
#[derive(Default)]
struct Answer(Map<String, Value>);

impl Answer {
    fn ok(ok: bool) -> Answer {
        Answer::default().set("ok", json!(ok))
    }
    fn refused(why: impl std::fmt::Display) -> Answer {
        Answer::ok(false).set("why", json!(why.to_string()))
    }
    fn set(mut self, k: &str, v: Value) -> Answer {
        self.0.insert(k.into(), v);
        self
    }
    fn line(&self) -> String {
        Value::Object(self.0.clone()).to_string()
    }
}

// -- the peer --------------------------------------------------------------------------

struct Headless {
    /// The peer, driven by its own thread (`ark_ffi::Peer`): its link
    /// pumped whenever it rings, a command answered meanwhile.
    peer: Arc<ark_ffi::Peer>,
    hosted: Hosted,
    dir: PathBuf,
    server: Option<String>,
    token: Option<String>,
    patience: Patience,
    /// How long `settle` waits with nothing new before the cursor counts
    /// as still for a turn (`--pump-ms`).
    still: Duration,
    /// Verdicts collected and not yet asked for. Every one is also appended
    /// to `DIR/rejections.jsonl` as it arrives, since a client keeps a
    /// verdict in memory only.
    unasked: Vec<Value>,
    /// `--roles`: what this device's entries carry beside its login's.
    roles: Vec<String>,
}

/// How many of the link's events the client is given in one turn: a server
/// confirming two thousand intents one entry at a time is two thousand
/// frames, each a rebase of what is still pending; taken a chunk at a time,
/// a command is answered between chunks rather than after all of them.
const CHUNK: usize = 32;

/// How often a `wait` or a `settle` looks again.
const LOOK: Duration = Duration::from_millis(2);

impl Headless {
    fn open(f: &Flags, hosted: Hosted) -> Result<Headless, String> {
        std::fs::create_dir_all(&f.dir).map_err(|e| format!("{}: {e}", f.dir.display()))?;
        let login = match (&f.server, f.alone) {
            (Some(server), false) => {
                let remembered = logins(&f.dir)
                    .recall(server)
                    .filter(|l| f.user.as_ref().is_none_or(|u| *u == l.user.id));
                match (remembered, &f.user) {
                    (Some(l), _) => Some(l),
                    (None, Some(user)) => {
                        let l = log_in(server, user, f.patience)
                            .map_err(|e| format!("signing {user} in at {server}: {e}"))?;
                        logins(&f.dir).remember(server, &l);
                        Some(l)
                    }
                    (None, None) => None,
                }
            }
            _ => None,
        };
        let mut who = match (&login, f.alone) {
            (Some(l), _) => ark_peer::who(l),
            (None, true) => nobody(f.user.as_deref().unwrap_or("me")),
            (None, false) => nobody(""),
        };
        with_roles(&mut who, &f.roles);
        let live = f.device.as_ref().map(|d| Live {
            device: Some(
                Device {
                    can_play: d.can_play,
                    id: String::new(),
                    kind: d.kind.clone(),
                    name: d.name.clone(),
                    user: String::new(),
                }
                .to_bytes(),
            ),
            ephemeral: hosted.get("listening.scrub").into_iter().collect(),
            every_ms: 250,
            ..Live::new(hosted.module.clone())
        });
        let config = Config {
            live,
            ..Config::of(hosted.domain.clone())
        };
        let disk = Files::new(f.dir.join("replica"))
            .map_err(|e| format!("opening {}: {e}", f.dir.display()))?;
        let mut client = Peer::open(config, who.clone(), Box::new(disk), false);
        client.link.backoff = (f.backoff.0.max(1), f.backoff.1.max(f.backoff.0).max(1));
        client.link.chunk = CHUNK;
        match (f.alone, client.is_alone()) {
            (true, false) => client.alone(),
            (false, true) => client.join(),
            _ => {}
        }
        // A directory that was another login's, or alone: what it has
        // pending is authored again as this one.
        if login.is_some()
            && client
                .log
                .pending
                .iter()
                .any(|p| p.entry.session != who.session)
        {
            reauthor(&mut client, who);
        }
        let peer = ark_ffi::Peer::of(client).map_err(|e| e.to_string())?;
        peer.set_listener(Some(Arc::new(Stderr {
            peer: Arc::downgrade(&peer),
            was: Default::default(),
        })));
        let h = Headless {
            peer,
            hosted,
            dir: f.dir.clone(),
            server: f.server.clone(),
            token: login.map(|l| l.token),
            patience: f.patience,
            still: f.pump,
            unasked: vec![],
            roles: f.roles.clone(),
        };
        h.dial();
        Ok(h)
    }

    /// The link up, when there is a server and a login; down otherwise.
    fn dial(&self) {
        let (server, token) = (self.server.clone(), self.token.clone());
        self.peer.with(|p| {
            p.set_token(None);
            if let (Some(server), Some(token), false) = (server, token, p.is_alone()) {
                p.set_token(Some(token));
                p.connect(&ark_auth::socket_url(&server));
            }
        });
    }

    /// A function's arguments as the peer reads them: the command's `args`
    /// as they are, or `null` for a function that takes nothing; refused
    /// here when the input's own checks would refuse it.
    fn args(&self, name: &str, v: &Value) -> Result<String, String> {
        let f = self
            .hosted
            .domain
            .get(name)
            .ok_or_else(|| format!("harken-peer knows no function {name}"))?;
        let v = match f.input {
            ark::row::Type::Unit => Value::Null,
            _ => v.clone(),
        };
        let text = v.to_string();
        match self.peer.check(name.into(), text.clone()) {
            Some(why) => Err(format!("`args`: {why}")),
            None => Ok(text),
        }
    }

    /// Take the verdicts that arrived, and write each down.
    fn collect(&mut self) {
        let fresh: Vec<Value> = (self.peer.take_rejections().into_iter())
            .map(|r| json!({ "id": r.id, "reason": r.why }))
            .collect();
        if fresh.is_empty() {
            return;
        }
        let file = self.dir.join("rejections.jsonl");
        let text: Vec<String> = fresh.iter().map(Value::to_string).collect();
        let wrote = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file)
            .and_then(|mut f| f.write_all(format!("{}\n", text.join("\n")).as_bytes()));
        if let Err(e) = wrote {
            eprintln!("harken-peer: {}: {e}", file.display());
        }
        self.unasked.extend(fresh);
    }

    /// Looks at the peer every `step` until `done`, or the timeout:
    /// whether it was done.
    fn until(
        &self,
        timeout: Duration,
        step: Duration,
        mut done: impl FnMut(&mut Peer) -> bool,
    ) -> bool {
        ark_peer::link::until(timeout, step, || self.peer.with(&mut done))
    }

    fn status(&self) -> Answer {
        let st = self.peer.status();
        let (alone, signed_out) = (st.alone, self.token.is_none() && !st.alone);
        let (dialling, diverged, fork) = self.peer.with(|p| {
            let fork = match &p.log.role {
                Role::Authority {
                    journal:
                        Journal {
                            fork: Some((n, _)), ..
                        },
                } => *n,
                _ => 0,
            };
            (!p.is_signed_out(), p.log.diverged.len(), fork)
        });
        let link = match (alone, dialling, st.linked) {
            (true, _, _) => "alone",
            (_, false, _) if signed_out => "signed out",
            (_, false, _) => "offline",
            (_, true, true) => "linked",
            (_, true, false) => "dialling",
        };
        Answer::default()
            .set("cursor", json!(st.cursor))
            .set("pending", json!(st.pending))
            .set("held", json!(0))
            .set("behind", json!(false))
            .set("linked", json!(st.linked))
            .set("denied", st.denied.map_or(Value::Null, |_| json!(REVOKED)))
            .set("user", json!(st.user))
            .set("session", json!(st.session))
            .set("signed_out", json!(signed_out))
            .set("epoch", json!(st.epoch))
            .set("link", json!(link))
            .set("opens", json!(st.epoch))
            .set("diverged", json!(diverged))
            .set("fork", json!(fork))
    }

    /// Signs in as `login`: the client authors as it, the link dials with it.
    fn signed_in(&mut self, login: &Login) {
        let mut l = ark_ffi::Login::from(login.clone());
        l.roles.extend(self.roles.iter().cloned());
        l.roles.sort();
        l.roles.dedup();
        self.peer.login(l);
        self.token = Some(login.token.clone());
        self.dial();
    }

    /// Answer one command. `quit` is answered like `persist`; the loop is
    /// what stops.
    fn handle(&mut self, cmd: Cmd) -> Answer {
        let binding = |name: &str| self.peer.with(|p| p.binding(name));
        match cmd {
            Cmd::Mutate { name, args } => {
                let Some(name) = binding(&name) else {
                    return Answer::refused(format!("no function {name}"));
                };
                match self
                    .args(&name, &args)
                    .and_then(|a| self.peer.mutate(name, a).map_err(|e| e.to_string()))
                {
                    Err(why) => Answer::refused(why),
                    Ok(id) => Answer::ok(true).set("id", json!(id)),
                }
            }
            Cmd::Status => self.status(),
            Cmd::Hash => self.peer.with(|p| {
                Answer::default()
                    .set("cursor", json!(p.cursor()))
                    .set("hash", json!(p.log.confirmed_hash().hex()))
                    .set("view", json!(p.log.store.state_hash().hex()))
            }),
            Cmd::Wait { cursor, timeout } => {
                let ok = self.until(timeout, LOOK, |p| p.cursor() >= cursor);
                Answer::ok(ok).set("cursor", json!(self.peer.status().cursor))
            }
            Cmd::Settle { timeout } => {
                // Linked (or alone), nothing pending, and the cursor still
                // for two steps of `--pump-ms`: what "caught up" can mean
                // from this end alone. The fleet compares against the
                // server's head as well.
                let (mut last, mut still) = (-1, 0);
                let ok = self.until(timeout, self.still, |p| {
                    if p.cursor() == last {
                        still += 1;
                    } else {
                        (last, still) = (p.cursor(), 0);
                    }
                    (p.linked || p.is_alone()) && p.pending_len() == 0 && still >= 2
                });
                let st = self.peer.status();
                Answer::ok(ok)
                    .set("cursor", json!(st.cursor))
                    .set("pending", json!(st.pending))
            }
            Cmd::Query { name, args } => {
                let Some(name) = binding(&name) else {
                    return Answer::refused(format!("no function {name}"));
                };
                let answer = self
                    .args(&name, &args)
                    .and_then(|a| self.peer.query(name, a).map_err(|e| e.to_string()))
                    .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()));
                match answer {
                    Ok(v) => Answer::default().set("rows", v),
                    Err(e) => Answer::refused(e),
                }
            }
            Cmd::Rejections { all } => {
                self.collect();
                let asked = std::mem::take(&mut self.unasked);
                let items: Vec<Value> = if all {
                    std::fs::read_to_string(self.dir.join("rejections.jsonl"))
                        .unwrap_or_default()
                        .lines()
                        .filter_map(|l| serde_json::from_str(l).ok())
                        .collect()
                } else {
                    asked
                };
                Answer::default().set("rejections", Value::Array(items))
            }
            Cmd::Standing { id } => {
                // The kit's standing, and what is in the log: a confirmed
                // entry stays so across a restart.
                let logged = || {
                    self.peer.with(|p| {
                        p.log
                            .log()
                            .entries()
                            .iter()
                            .any(|l| l.entry.id == EntryId(id))
                    })
                };
                let (standing, why) = match self.peer.standing(hex(&id)) {
                    Standing::Pending => ("pending", None),
                    Standing::Rejected { why } => ("rejected", Some(why)),
                    Standing::Confirmed => ("confirmed", None),
                    Standing::Unknown if logged() => ("confirmed", None),
                    Standing::Unknown => ("unknown", None),
                };
                match why {
                    Some(why) => Answer::default()
                        .set("standing", json!(standing))
                        .set("why", json!(why)),
                    None => Answer::default().set("standing", json!(standing)),
                }
            }
            Cmd::Disconnect => {
                self.peer.with(|p| p.set_token(None));
                Answer::ok(true)
            }
            Cmd::Reconnect => {
                self.dial();
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
                logins(&self.dir).remember(&server, &login);
                self.signed_in(&login);
                Answer::ok(true)
                    .set("user", json!(login.user.id))
                    .set("session", json!(login.session))
            }
            Cmd::SignOut => {
                // The session revoked through the domain, while linked.
                self.peer.logout();
                self.token = None;
                self.peer.with(|p| p.sign_in(nobody("")));
                self.dial();
                if let Some(server) = &self.server {
                    logins(&self.dir).forget(server);
                }
                Answer::ok(true)
            }
            Cmd::Join { server, user } => {
                // The login this directory already has for that server and
                // person, when it has one: the chain it made alone is its
                // session's, and a server adopts it from that session as it
                // is. Another login authors it again, with ids of its own.
                let remembered = user.as_ref().and_then(|u| {
                    logins(&self.dir)
                        .recall(&server)
                        .filter(|l| l.user.id == *u)
                });
                let login = match (&user, remembered) {
                    (None, _) => None,
                    (Some(_), Some(l)) => Some(l),
                    (Some(user), None) => match log_in(&server, user, self.patience) {
                        Ok(l) => Some(l),
                        Err(e) => {
                            return Answer::refused(format!("signing {user} in at {server}: {e}"))
                        }
                    },
                };
                let took = Instant::now();
                self.peer.with(|p| {
                    if p.is_alone() {
                        p.join();
                    }
                });
                self.server = Some(server.clone());
                match &login {
                    Some(l) => {
                        logins(&self.dir).remember(&server, l);
                        self.signed_in(l);
                    }
                    None => {
                        self.token = None;
                        self.dial();
                    }
                }
                let st = self.peer.status();
                eprintln!(
                    "harken-peer: joined {server}: {} pending in {:?}",
                    st.pending,
                    took.elapsed()
                );
                Answer::ok(true)
                    .set("pending", json!(st.pending))
                    .set("cursor", json!(st.cursor))
            }
            Cmd::Leave => {
                if self.peer.status().alone {
                    return Answer::refused("already alone");
                }
                self.peer.with(|p| {
                    p.set_token(None);
                    p.alone();
                });
                self.server = None;
                Answer::ok(true).set("cursor", json!(self.peer.status().cursor))
            }
            Cmd::Call {
                live,
                transition,
                input,
            } => {
                let name = format!("{live}.{transition}");
                let ephemeral = self
                    .peer
                    .with(|p| p.config.live.as_ref().map(|l| l.ephemeral.contains(&name)));
                let Some(ephemeral) = ephemeral else {
                    return Answer::refused("this peer is no device (--device)");
                };
                if self.hosted.get(&name).is_none() {
                    return Answer::refused(format!("no transition {name}"));
                }
                match self
                    .args(&name, &input)
                    .and_then(|a| self.peer.call(name, a).map_err(|e| e.to_string()))
                {
                    Err(why) => Answer::refused(why),
                    Ok(()) => Answer::ok(true).set(
                        "kind",
                        json!(if ephemeral { "ephemeral" } else { "ordered" }),
                    ),
                }
            }
            Cmd::Live { name } => {
                if name != "listening" {
                    return Answer::refused(format!("no live state {name}"));
                }
                self.peer.with(|p| {
                    let me = p.me().clone();
                    let (value, seq, pending) = match &p.live {
                        Some(l) => (
                            Some(Listening::of(&l.store, &me.actor)),
                            l.log().head().0,
                            l.pending.len(),
                        ),
                        None => (None, 0, 0),
                    };
                    // The hub has paged it since the connection opened:
                    // this device stands in the session it holds.
                    let synced = p.linked
                        && value
                            .as_ref()
                            .is_some_and(|l| l.device(&me.session).is_some());
                    Answer::default()
                        .set("value", value.as_ref().map_or(Value::Null, listening_json))
                        .set("device", json!(me.session))
                        .set("seq", json!(seq))
                        .set("pending", json!(pending))
                        .set("synced", json!(synced))
                })
            }
            Cmd::LiveRefusals => {
                Answer::default().set("refusals", json!(self.peer.take_live_refusals()))
            }
            Cmd::Persist | Cmd::Quit => {
                self.peer.pump();
                match self.peer.status().failure {
                    None => Answer::ok(true),
                    Some(e) => Answer::refused(e),
                }
            }
        }
    }
}

/// What happens on the link, said on stderr as it moves: opened, dropped,
/// denied.
struct Stderr {
    peer: std::sync::Weak<ark_ffi::Peer>,
    /// Linked, and denied, when last told.
    was: std::sync::Mutex<(bool, bool)>,
}

impl ark_ffi::Listener for Stderr {
    fn changed(&self) {
        let Some(st) = self.peer.upgrade().map(|p| p.status()) else {
            return;
        };
        let mut was = self.was.lock().unwrap_or_else(|e| e.into_inner());
        match (was.0, st.linked) {
            (false, true) => eprintln!(
                "harken-peer: linked (epoch {}, cursor {})",
                st.epoch, st.cursor
            ),
            (true, false) => eprintln!("harken-peer: dropped"),
            _ => {}
        }
        if st.denied.is_some() && !was.1 {
            eprintln!("harken-peer: denied: {REVOKED}");
        }
        *was = (st.linked, st.denied.is_some());
    }
}

/// `--roles` beside a login's.
fn with_roles(who: &mut Who, roles: &[String]) {
    who.roles.extend(roles.iter().cloned());
    who.roles.sort();
    who.roles.dedup();
}

/// What is pending authored again as `who`.
///
/// The same session is no new login: its intents are its own already, and
/// authoring them again from its first `local` would name entries the
/// server has answered.
fn reauthor(client: &mut Peer, who: Who) {
    let log = &client.log;
    let mut theirs = log.pending.iter().map(|p| &p.entry.session);
    if who.session == log.me.session && theirs.all(|s| *s == who.session) {
        return;
    }
    client.sign_in(who);
}

fn run(f: Flags) -> Result<(), String> {
    let mut h = Headless::open(&f, Hosted::harken())?;
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
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
        // A reader that has gone is a driver that has gone: stop.
        if writeln!(out, "{}", answer.line())
            .and_then(|_| out.flush())
            .is_err()
            || quit
        {
            return Ok(());
        }
        h.collect();
    }
    // The end of stdin is a quit.
    h.handle(Cmd::Quit);
    Ok(())
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

    fn harken() -> Hosted {
        static H: std::sync::OnceLock<Hosted> = std::sync::OnceLock::new();
        H.get_or_init(Hosted::harken).clone()
    }

    fn line(h: &mut Headless, cmd: &str) -> Value {
        serde_json::from_str(&h.handle(parse(cmd).unwrap()).line()).unwrap()
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
        assert_eq!(f.backoff, (20, 200));
        assert!(
            flags(&a(&["--server", "http://h:1"])).is_err(),
            "a directory is required"
        );
        assert!(flags(&a(&["--dir", "/d", "--alone", "--server", "http://h:1"])).is_err());
        assert!(flags(&a(&["--dir", "/d"])).is_err());
        assert!(flags(&a(&["--dir", "/d", "--alone", "--what"])).is_err());
        assert!(flags(&a(&["--dir", "/d", "--alone"])).unwrap().alone);
        assert_eq!(
            flags(&a(&[
                "--dir",
                "/d",
                "--alone",
                "--device",
                "phone,Ana's phone, with a comma,true"
            ]))
            .unwrap()
            .device,
            Some(DeviceFlag {
                kind: "phone".into(),
                name: "Ana's phone, with a comma".into(),
                can_play: true
            }),
            "the name may hold commas; the last field is whether it can play"
        );
        assert!(flags(&a(&["--dir", "/d", "--alone", "--device", "phone,Phone"])).is_err());
        assert!(flags(&a(&[
            "--dir",
            "/d",
            "--alone",
            "--device",
            "phone,Phone,maybe"
        ]))
        .is_err());
    }

    /// Every command reads, an entry's id as its hex; what is not a
    /// command says why. Falsified by reading `cursor` as text:
    /// `wait` fails to parse.
    #[test]
    fn the_commands_read() {
        let id = "0000000000000000000000000000002a";
        assert_eq!(
            parse(r#"{"cmd":"mutate","name":"create_playlist","args":{"name":"Road trip"}}"#)
                .unwrap(),
            Cmd::Mutate {
                name: "create_playlist".into(),
                args: json!({"name": "Road trip"}),
            }
        );
        assert_eq!(
            parse(r#"{"cmd":"wait","cursor":7,"timeout_ms":250}"#).unwrap(),
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
                args: json!({})
            }
        );
        let mut want = [0u8; 16];
        want[15] = 42;
        assert_eq!(
            parse(&format!(r#"{{"cmd":"standing","id":"{id}"}}"#)).unwrap(),
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
        assert_eq!(
            parse(r#"{"cmd":"join","server":"http://h:1/","user":"alice"}"#).unwrap(),
            Cmd::Join {
                server: "http://h:1".into(),
                user: Some("alice".into())
            }
        );
        assert_eq!(
            parse(r#"{"cmd":"join","server":"http://h:1"}"#).unwrap(),
            Cmd::Join {
                server: "http://h:1".into(),
                user: None
            }
        );
        assert_eq!(
            parse(r#"{"cmd":"call","live":"listening","transition":"seek","input":{"ms":5000}}"#)
                .unwrap(),
            Cmd::Call {
                live: "listening".into(),
                transition: "seek".into(),
                input: json!({"ms": 5000}),
            }
        );
        assert_eq!(
            parse(r#"{"cmd":"live","name":"listening"}"#).unwrap(),
            Cmd::Live {
                name: "listening".into()
            }
        );
        for (line, simple) in [
            ("live_refusals", Cmd::LiveRefusals),
            ("status", Cmd::Status),
            ("hash", Cmd::Hash),
            ("rejections", Cmd::Rejections { all: false }),
            ("disconnect", Cmd::Disconnect),
            ("reconnect", Cmd::Reconnect),
            ("sign_out", Cmd::SignOut),
            ("leave", Cmd::Leave),
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
            (r#"{"cmd":"standing","id":"x"}"#, "`id` is an entry's id"),
            (r#"{"cmd":"join"}"#, "`server` is missing"),
        ] {
            let e = parse(bad).unwrap_err();
            assert!(e.contains(says), "{bad}: {e}");
        }
    }

    /// Arguments cross as the domain's own JSON — an id its hex, a sum
    /// externally tagged — checked by the input's own refinements; a
    /// function that takes nothing takes `null` whatever was sent.
    /// Falsified by sending `playlists` its `args` as they came (`{}`): the
    /// input did not read as `()`.
    #[test]
    fn harkens_types_cross_as_json() {
        let dir = tempfile::tempdir().unwrap();
        let f = flags(&a(&["--dir", dir.path().to_str().unwrap(), "--alone"])).unwrap();
        let h = Headless::open(&f, harken()).unwrap();
        let input = |name: &str, v: Value| {
            let text = h.args(name, &v)?;
            h.peer.with(|p| p.input_json(name, &text))
        };
        let id = json!("0000000000000000000000000000002a");
        let b = input(
            "playlists.add_to_playlist",
            json!({"playlist_id": id, "media_id": id}),
        )
        .unwrap();
        let back = harken_server::domain::AddToPlaylist::from_bytes(&b).unwrap();
        assert_eq!(back.media_id.0[15], 42);
        assert_eq!(
            input("playlists.playlists", json!({})).unwrap(),
            ().to_bytes()
        );
        let start = json!({"at": 0, "from": "", "position": 0, "queue": [{"album": "", "creator": "", "duration_ms": 1, "file": "a", "id": id, "title": "A"}]});
        assert!(input("listening.start", start).is_ok());
        let why = input("playlists.create_playlist", json!({"name": " "})).unwrap_err();
        assert!(why.contains("a playlist needs a name"), "{why}");
        assert!(input("playlists.create_playlist", json!({"name": 3})).is_err());
        assert!(input("nothing.here", json!({})).is_err());
    }

    /// A peer alone, driven through `handle` exactly as stdin would: an
    /// intent and its id, a refusal, the rows, the hashes of a store that
    /// moved, the standing, and an answer that reads back as JSON.
    /// Falsified by `Answer::refused` leaving out `why`: the refusal's
    /// reason is not in the answer.
    #[test]
    fn a_peer_answers_in_json() {
        let dir = tempfile::tempdir().unwrap();
        let f = flags(&a(&[
            "--dir",
            dir.path().to_str().unwrap(),
            "--alone",
            "--user",
            "me",
        ]))
        .unwrap();
        let mut h = Headless::open(&f, harken()).unwrap();
        let before = line(&mut h, r#"{"cmd":"hash"}"#);
        let made = line(
            &mut h,
            r#"{"cmd":"mutate","name":"create_playlist","args":{"name":"Road trip"}}"#,
        );
        assert_eq!(made["ok"], json!(true), "{made}");
        let id = made["id"].clone();
        let refused = line(
            &mut h,
            r#"{"cmd":"mutate","name":"create_playlist","args":{"name":" "}}"#,
        );
        assert_eq!(refused["ok"], json!(false));
        assert!(
            refused["why"]
                .as_str()
                .unwrap()
                .contains("a playlist needs a name"),
            "{refused}"
        );
        let rows = line(&mut h, r#"{"cmd":"query","name":"playlists"}"#);
        assert_eq!(rows["rows"].as_array().unwrap().len(), 1, "{rows}");
        assert_eq!(rows["rows"][0]["name"], json!("Road trip"));
        let after = line(&mut h, r#"{"cmd":"hash"}"#);
        assert_ne!(
            before["hash"], after["hash"],
            "alone, the confirmed store moved"
        );
        assert_eq!(after["cursor"], json!(1));
        assert_eq!(
            line(&mut h, &format!(r#"{{"cmd":"standing","id":{id}}}"#)),
            json!({"standing": "confirmed"})
        );
        let status = line(&mut h, r#"{"cmd":"status"}"#);
        assert_eq!(status["pending"], json!(0));
        assert_eq!(status["denied"], Value::Null);
        assert_eq!(status["link"], json!("alone"));
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

    /// A directory used `--alone` and then opened with `--server` is the
    /// join — what it sequenced alone is pending, handed over on the first
    /// connection — and `leave` and `join` walk the same two transitions
    /// without a restart. No server answers here, so nothing is pushed;
    /// the fleet has the rest. Falsified by `Cmd::Leave` answering without
    /// going alone: the cursor stayed at 0 with three pending.
    #[test]
    fn alone_then_a_server_is_one_directory() {
        let dir = tempfile::tempdir().unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let server = format!("http://127.0.0.1:{port}");
        let d = dir.path().to_str().unwrap();
        let alone = flags(&a(&["--dir", d, "--alone"])).unwrap();
        let mut h = Headless::open(&alone, harken()).unwrap();
        for name in ["one", "two", "three"] {
            let made = line(
                &mut h,
                &format!(
                    r#"{{"cmd":"mutate","name":"create_playlist","args":{{"name":"{name}"}}}}"#
                ),
            );
            assert_eq!(made["ok"], json!(true), "{made}");
        }
        let status = line(&mut h, r#"{"cmd":"status"}"#);
        assert_eq!(status["link"], json!("alone"));
        assert_eq!(status["cursor"], json!(3));
        drop(h);

        let online = flags(&a(&["--dir", d, "--server", &server])).unwrap();
        let mut h = Headless::open(&online, harken()).unwrap();
        let status = line(&mut h, r#"{"cmd":"status"}"#);
        assert_eq!(
            (&status["cursor"], &status["pending"]),
            (&json!(0), &json!(3)),
            "joined, signed out: the three are pending: {status}"
        );
        let left = line(&mut h, r#"{"cmd":"leave"}"#);
        assert_eq!(left["cursor"], json!(3), "{left}");
        let status = line(&mut h, r#"{"cmd":"status"}"#);
        assert_eq!(status["pending"], json!(0));
        assert_eq!(status["link"], json!("alone"));
        let joined = line(&mut h, &format!(r#"{{"cmd":"join","server":"{server}"}}"#));
        assert_eq!(joined["pending"], json!(3), "{joined}");
        assert_eq!(joined["cursor"], json!(0));
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
        let mut h = Headless::open(&f, harken()).unwrap();
        let made = line(
            &mut h,
            r#"{"cmd":"mutate","name":"create_playlist","args":{"name":"Offline"}}"#,
        );
        assert!(
            made["ok"] == json!(true) && made["id"].is_string(),
            "{made}"
        );
        let hash = line(&mut h, r#"{"cmd":"hash"}"#);
        assert_ne!(
            hash["hash"], hash["view"],
            "confirmed is empty, the view is not"
        );
        let waited = h.handle(Cmd::Wait {
            cursor: 1,
            timeout: Duration::from_millis(120),
        });
        assert_eq!(waited.line(), r#"{"cursor":0,"ok":false}"#);
        let status = line(&mut h, r#"{"cmd":"status"}"#);
        assert_eq!(status["signed_out"], json!(true));
        assert_eq!(status["pending"], json!(1));
        drop(h);
        let h = Headless::open(&f, harken()).unwrap();
        assert_eq!(h.peer.status().pending, 1, "the intent was on disk");
    }
}
