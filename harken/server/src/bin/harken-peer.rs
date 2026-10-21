//! `harken-peer`: a harken peer with no screen (`docs/plan-fleet.md` §1).
//!
//! ```text
//! harken-peer --dir DIR --server URL [--user NAME]     a peer of that server
//! harken-peer --dir DIR --alone [--user NAME]          its own authority
//!   [--pump-ms 50] [--ping-ms 20000] [--backoff-ms 500,30000]
//!   [--auth-patience-ms N] [--module FILE] [--roles ROLE,ROLE]
//!   [--device KIND,NAME,CAN_PLAY]
//! ```
//!
//! It is an `ark_peer::Peer` and its link, as the desktop is, minus the
//! window: the replica in `DIR/replica` (the chain on disk), signed in the
//! way the desktop signs in under dev auth (`ark_auth::client::login` with a
//! name, which a dev server answers with a code and no browser), the login
//! remembered in `DIR/login.json` so a restart reuses it. Without `--user`
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
//! `--module FILE` runs the module in that file instead of harken's own
//! (`harken_server::hosted`, the server's `HARKEN_MODULE`). Arguments and
//! rows cross as JSON for the functions this binary knows the types of
//! ([`input`], [`rows`]): harken's own.
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
//! it sequenced alone is handed over on its first connection and is
//! pending until the server answers it — as the login `--user` signs in,
//! or as nobody until `sign_in` — and the other way round a directory
//! opened `--alone` after a server goes alone from where it is. `join` and
//! `leave` do the same without a restart.
//!
//! **It reads commands as JSON lines on stdin and answers each with one JSON
//! line on stdout**, so a test — or a shell — drives it with no socket of
//! its own. An id is `{"$id":"8-4-4-4-12"}`; an integer a JSON number, or
//! `{"$int":"…"}`; an `Option` `null` or its value; a sum
//! `{"variant":"Name", …fields}`.
//!
//! ```text
//! {"cmd":"mutate","name":"create_playlist","args":{"name":"Road trip"}}
//!                              {"ok":true,"id":{"$id":"…"}} | {"ok":false,"why":"…"}
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
//! The peer pumps when the link says something (it waits on the link a few
//! milliseconds at a time between lines) and after every command; `wait`
//! and `settle` pump while they wait and read no more commands until they
//! answer: a driver sends one command and reads one answer. `--pump-ms` is
//! how long `settle` waits with nothing new before it counts the cursor as
//! still.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use ark::hash::hex;
use ark::ir::Node;
use ark::log::EntryId;
use ark::peer::{Ctx, Journal, Role, Who};
use ark_auth::client::Patience;
use ark_auth::Login;
use ark_peer::{now_ms, Config, Files, Live, Peer};
use harken_server::domain::host::Listening;
use harken_server::domain::Id;
use harken_server::hosted::Hosted;
use serde_json::{json, Map, Value};

const USAGE: &str =
    "usage: harken-peer --dir DIR (--server URL [--user NAME] | --alone [--user NAME])
                   [--pump-ms 50] [--ping-ms 20000] [--backoff-ms 500,30000]
                   [--auth-patience-ms N] [--module FILE] [--roles ROLE,ROLE]
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
    module: Option<PathBuf>,
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
        module: None,
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
            "--module" => f.module = Some(PathBuf::from(it.next().ok_or("--module needs a file")?)),
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

/// `DIR/login.json`: the login for one server. Not `ark_auth::remember`,
/// which keeps one file per user account in `$XDG_CONFIG_HOME` — a fleet
/// of peers on one machine is several devices, and each keeps its own.
fn login_file(dir: &Path) -> PathBuf {
    dir.join("login.json")
}

fn recall(dir: &Path, server: &str) -> Option<Login> {
    let text = std::fs::read_to_string(login_file(dir)).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    if v.get("server")?.as_str()? != server {
        return None;
    }
    serde_json::from_value(v.get("login")?.clone()).ok()
}

fn remember(dir: &Path, server: &str, login: &Login) -> Result<(), String> {
    let body = json!({ "server": server, "login": login });
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

// -- JSON, under harken's types -------------------------------------------------

/// A value of a harken type as JSON and back: what `args` and `rows` are.
trait Json: Sized {
    fn from_json(v: &Value) -> Result<Self, String>;
    fn to_json(&self) -> Value;
}

impl Json for String {
    fn from_json(v: &Value) -> Result<Self, String> {
        v.as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("not text: {v}"))
    }
    fn to_json(&self) -> Value {
        json!(self)
    }
}

impl Json for i64 {
    fn from_json(v: &Value) -> Result<Self, String> {
        match v {
            Value::Number(n) => n.as_i64().ok_or_else(|| format!("not an integer: {v}")),
            Value::Object(m) => m
                .get("$int")
                .and_then(|s| s.as_str()?.parse().ok())
                .ok_or_else(|| format!("not an integer: {v}")),
            _ => Err(format!("not an integer: {v}")),
        }
    }
    fn to_json(&self) -> Value {
        json!(self)
    }
}

impl Json for bool {
    fn from_json(v: &Value) -> Result<Self, String> {
        v.as_bool().ok_or_else(|| format!("not a bool: {v}"))
    }
    fn to_json(&self) -> Value {
        json!(self)
    }
}

impl<T: Json> Json for Option<T> {
    fn from_json(v: &Value) -> Result<Self, String> {
        match v {
            Value::Null => Ok(None),
            v => T::from_json(v).map(Some),
        }
    }
    fn to_json(&self) -> Value {
        self.as_ref().map_or(Value::Null, T::to_json)
    }
}

impl<T: Json> Json for Vec<T> {
    fn from_json(v: &Value) -> Result<Self, String> {
        v.as_array()
            .ok_or_else(|| format!("not a list: {v}"))?
            .iter()
            .map(T::from_json)
            .collect()
    }
    fn to_json(&self) -> Value {
        Value::Array(self.iter().map(T::to_json).collect())
    }
}

impl Json for () {
    fn from_json(_: &Value) -> Result<Self, String> {
        Ok(())
    }
    fn to_json(&self) -> Value {
        json!({})
    }
}

/// `8-4-4-4-12` hex, as `text_of_id` writes an id.
fn id_text(b: &[u8; 16]) -> String {
    let h = hex(b);
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

fn id_bytes(v: &Value) -> Result<[u8; 16], String> {
    let t = v
        .get("$id")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("not an id: {v}"))?;
    let h: String = t.chars().filter(|c| *c != '-').collect();
    let b = ark::hash::unhex(&h)
        .filter(|b| b.len() == 16)
        .ok_or_else(|| format!("not an id: {t}"))?;
    Ok(b.try_into().expect("sixteen bytes"))
}

fn id_json(b: &[u8; 16]) -> Value {
    json!({ "$id": id_text(b) })
}

impl<R> Json for Id<R> {
    fn from_json(v: &Value) -> Result<Self, String> {
        id_bytes(v).map(Id::new)
    }
    fn to_json(&self) -> Value {
        id_json(&self.0)
    }
}

/// A record by its fields' names.
macro_rules! record {
    ($($t:path { $($f:ident),* $(,)? })*) => {$(
        impl Json for $t {
            fn from_json(v: &Value) -> Result<Self, String> {
                let o = v.as_object().ok_or_else(|| format!("not an object: {v}"))?;
                Ok(Self { $($f: Json::from_json(o.get(stringify!($f)).unwrap_or(&Value::Null)).map_err(|e| format!("`{}`: {e}", stringify!($f)))?,)* })
            }
            fn to_json(&self) -> Value {
                let mut o = Map::new();
                $(o.insert(stringify!($f).into(), self.$f.to_json());)*
                Value::Object(o)
            }
        }
    )*};
}

use harken_server::domain::*;

record! {
    CreatePlaylist { name }
    AddToPlaylist { playlist_id, media_id }
    AddAllToPlaylist { playlist_id }
    RemoveFromPlaylist { playlist_id, media_id }
    PlaylistsOf { media_id }
    PlaylistInput { playlist_id }
    Library { playlist_id }
    AddSong { title, artist, album, duration_ms, file, track, part, catalogue, performer, bpm, album_art, artist_art, disc, work_title, movement_no }
    RemoveMedia { id }
    Playlist { id, name, pos, created_ms, user_id }
    LibraryEntry { added_ms, creator, duration_ms, file, id, kind, playlist_pos, pos, title, user_id }
    Play { id, user_id, track, title, creator, album, duration_ms, file, played_ms, from }
    RecordPlay { track, title, creator, album, duration_ms, file, from }
    Track { album, creator, duration_ms, file, id, title }
    Start { at, from, position, queue }
    Seek { ms }
    Skip { by }
    Jump { index }
    Transfer { to }
    At { at_ms }
    Report { at_ms, since }
    Advance { from_track }
    Enqueue { next, tracks }
    Remove { index }
    Move { from, to }
    Scrub { at_ms }
    Equip { can_volume, family, muted, volume }
    SetVolume { device, level }
    NudgeVolume { delta, device }
    SetMute { device, muted }
    ReportVolume { level, muted }
    Member { device }
    ReportGroup { members }
    GroupVolume { level }
    GroupNudge { delta }
    GroupMute { muted }
    Present { can_play, can_volume, family, follows, id, kind, muted, name, user, volume }
    Active { at, clock, from, output, queue }
}

impl Json for Clock {
    fn from_json(v: &Value) -> Result<Self, String> {
        let at_ms = i64::from_json(&v["at_ms"])?;
        match v["variant"].as_str() {
            Some("Paused") => Ok(Clock::Paused { at_ms }),
            Some("Playing") => Ok(Clock::Playing {
                at_ms,
                since: i64::from_json(&v["since"])?,
            }),
            _ => Err(format!("not a clock: {v}")),
        }
    }
    fn to_json(&self) -> Value {
        match self {
            Clock::Paused { at_ms } => json!({ "variant": "Paused", "at_ms": at_ms }),
            Clock::Playing { at_ms, since } => {
                json!({ "variant": "Playing", "at_ms": at_ms, "since": since })
            }
        }
    }
}

impl Json for Output {
    fn from_json(v: &Value) -> Result<Self, String> {
        match v["variant"].as_str() {
            Some("Away") => Ok(Output::Away {
                id: String::from_json(&v["id"])?,
                name: String::from_json(&v["name"])?,
            }),
            Some("Device") => Ok(Output::Device {
                id: String::from_json(&v["id"])?,
                moving: bool::from_json(&v["moving"])?,
            }),
            Some("Nowhere") => Ok(Output::Nowhere),
            _ => Err(format!("not an output: {v}")),
        }
    }
    fn to_json(&self) -> Value {
        match self {
            Output::Away { id, name } => json!({ "variant": "Away", "id": id, "name": name }),
            Output::Device { id, moving } => {
                json!({ "variant": "Device", "id": id, "moving": moving })
            }
            Output::Nowhere => json!({ "variant": "Nowhere" }),
        }
    }
}

/// One account's session as JSON: its devices, scrubbers and what plays.
fn listening_json(l: &Listening) -> Value {
    json!({
        "devices": l.devices.to_json(),
        "scrub": l.scrub.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<Map<_, _>>(),
        "session": l.session.to_json(),
    })
}

fn enc<T: Json + Node>(v: &Value) -> Result<Vec<u8>, String> {
    T::from_json(v).map(|t| t.to_bytes())
}

fn dec<T: Json + Node>(b: &[u8]) -> Result<Value, String> {
    T::from_bytes(b).map(|t| t.to_json()).map_err(|e| e.0)
}

/// A function's input, from JSON, by its binding.
fn input(name: &str, v: &Value) -> Result<Vec<u8>, String> {
    match name {
        "playlists.create_playlist" => enc::<CreatePlaylist>(v),
        "playlists.add_to_playlist" => enc::<AddToPlaylist>(v),
        "playlists.add_all_to_playlist" => enc::<AddAllToPlaylist>(v),
        "playlists.remove_from_playlist" => enc::<RemoveFromPlaylist>(v),
        "playlists.playlists" | "listening.last_played" | "listening.recently_played" => {
            enc::<()>(v)
        }
        "playlists.playlists_of" => enc::<PlaylistsOf>(v),
        "playlists.playlist" => enc::<PlaylistInput>(v),
        "library.library" => enc::<Library>(v),
        "library.add_song" => enc::<AddSong>(v),
        "library.remove_media" => enc::<RemoveMedia>(v),
        "listening.record_play" => enc::<RecordPlay>(v),
        "listening.start" => enc::<Start>(v),
        "listening.seek" => enc::<Seek>(v),
        "listening.skip" => enc::<Skip>(v),
        "listening.jump" => enc::<Jump>(v),
        "listening.transfer" => enc::<Transfer>(v),
        "listening.started" | "listening.stalled" => enc::<At>(v),
        "listening.report_position" => enc::<Report>(v),
        "listening.advance" => enc::<Advance>(v),
        "listening.enqueue" => enc::<Enqueue>(v),
        "listening.remove" => enc::<Remove>(v),
        "listening.move" => enc::<Move>(v),
        "listening.scrub" => enc::<Scrub>(v),
        "listening.equip" => enc::<Equip>(v),
        "listening.set_volume" => enc::<SetVolume>(v),
        "listening.nudge_volume" => enc::<NudgeVolume>(v),
        "listening.set_mute" => enc::<SetMute>(v),
        "listening.report_volume" => enc::<ReportVolume>(v),
        "listening.join" | "listening.unjoin" => enc::<Member>(v),
        "listening.report_group" => enc::<ReportGroup>(v),
        "listening.set_group_volume" => enc::<GroupVolume>(v),
        "listening.nudge_group_volume" => enc::<GroupNudge>(v),
        "listening.set_group_mute" => enc::<GroupMute>(v),
        "listening.play"
        | "listening.pause"
        | "listening.stop"
        | "listening.clear_upcoming"
        | "listening.release" => enc::<()>(v),
        other => Err(format!("harken-peer knows no input type for {other}")),
    }
}

/// A query's answer, as JSON, by its binding.
fn rows(name: &str, b: &[u8]) -> Result<Value, String> {
    match name {
        "playlists.playlists" | "playlists.playlists_of" => dec::<Vec<Playlist>>(b),
        "playlists.playlist" | "library.library" => dec::<Vec<LibraryEntry>>(b),
        "listening.last_played" | "listening.recently_played" => dec::<Vec<Play>>(b),
        other => Err(format!("harken-peer cannot print what {other} answers")),
    }
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
        Some(v) => i64::from_json(v)
            .map(Some)
            .map_err(|_| format!("`{k}` is an integer")),
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
        "standing" => match m.get("id").map(id_bytes) {
            Some(Ok(id)) => Cmd::Standing { id },
            _ => return Err("`id` is an id: {\"$id\":\"8-4-4-4-12\"}".into()),
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
    client: Peer,
    /// What the link rings.
    bell: Receiver<()>,
    hosted: Hosted,
    dir: PathBuf,
    server: Option<String>,
    token: Option<String>,
    patience: Patience,
    /// How long `settle` waits with nothing new before the cursor counts
    /// as still for a turn (`--pump-ms`).
    still: Duration,
    /// The lines of stdin, as they come; `None` is its end.
    lines: Receiver<Option<String>>,
    /// Lines that arrived while a `wait` or a `settle` was pumping.
    held: Vec<Option<String>>,
    /// Verdicts collected and not yet asked for. Every one is also appended
    /// to `DIR/rejections.jsonl` as it arrives, since a client keeps a
    /// verdict in memory only.
    unasked: Vec<Value>,
    /// The server's refusals of the volatile machine's intents.
    live_refusals: Vec<String>,
    /// Every verdict this run has heard, for `standing`.
    rejected: BTreeMap<EntryId, String>,
    /// `--roles`: what this device's entries carry beside its login's.
    roles: Vec<String>,
    /// Whether the last connection opened and has heard nothing yet (a
    /// token the hub does not know is closed before a word); why it was
    /// last turned away; whether the hub has paged the volatile machine
    /// since it opened.
    quiet_open: bool,
    denied: Option<String>,
    synced: bool,
}

/// How many of the link's events the client is given in one turn.
const CHUNK: usize = 32;

impl Headless {
    fn open(f: &Flags, hosted: Hosted) -> Result<Headless, String> {
        std::fs::create_dir_all(&f.dir).map_err(|e| format!("{}: {e}", f.dir.display()))?;
        let login = match (&f.server, f.alone) {
            (Some(server), false) => {
                let remembered = recall(&f.dir, server)
                    .filter(|l| f.user.as_ref().is_none_or(|u| *u == l.user.id));
                match (remembered, &f.user) {
                    (Some(l), _) => Some(l),
                    (None, Some(user)) => {
                        let l = log_in(server, user, f.patience)
                            .map_err(|e| format!("signing {user} in at {server}: {e}"))?;
                        remember(&f.dir, server, &l)?;
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
        who.roles.extend(f.roles.iter().cloned());
        who.roles.sort();
        who.roles.dedup();
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
        let (wake, bell) = ark_peer::link::bell();
        client.set_waker(wake);
        client.link.backoff = (f.backoff.0.max(1), f.backoff.1.max(f.backoff.0).max(1));
        client.link.chunk = CHUNK;
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if tx.send(Some(line)).is_err() {
                    return;
                }
            }
            let _ = tx.send(None);
        });
        let mut h = Headless {
            client,
            bell,
            hosted,
            dir: f.dir.clone(),
            server: f.server.clone(),
            token: login.map(|l| l.token),
            patience: f.patience,
            still: f.pump,
            lines,
            held: vec![],
            unasked: vec![],
            live_refusals: vec![],
            rejected: BTreeMap::new(),
            roles: f.roles.clone(),
            quiet_open: false,
            denied: None,
            synced: false,
        };
        match (f.alone, h.is_alone()) {
            (true, false) => h.client.alone(),
            (false, true) => h.client.join(),
            _ => {}
        }
        // A directory that was another login's, or alone: what it has
        // pending is authored again as this one.
        if h.token.is_some()
            && h.client
                .log
                .pending
                .iter()
                .any(|p| p.entry.session != who.session)
        {
            h.reauthor(who);
        }
        h.dial();
        Ok(h)
    }

    /// Off the network, with nothing of the old connection left to give
    /// the client.
    fn drop_link(&mut self) {
        self.client.set_token(None);
    }

    /// Whether the link is up or being dialled.
    fn dialling(&self) -> bool {
        self.client.link.token.is_some()
    }

    fn is_alone(&self) -> bool {
        self.client.is_alone()
    }

    /// The link up, when there is a server and a login.
    fn dial(&mut self) {
        self.drop_link();
        if let (Some(server), Some(token), false) = (&self.server, &self.token, self.is_alone()) {
            let url = ark_auth::socket_url(server);
            self.client.set_token(Some(token.clone()));
            self.client.connect(&url);
        }
    }

    /// One turn of the link, a chunk of what it said ([`CHUNK`]): a server
    /// confirming two thousand intents one entry at a time is two thousand
    /// frames, each a rebase of what is still pending; taken a chunk at a
    /// time, a command is answered between chunks rather than after all of
    /// them. What it came to goes on stderr.
    fn pump(&mut self) {
        let p = self.client.pump();
        if p.opened {
            (self.quiet_open, self.synced) = (true, false);
            eprintln!(
                "harken-peer: linked (epoch {}, cursor {})",
                self.client.link.epoch,
                self.client.log.log().head().0
            );
        }
        if p.frames != [0, 0] {
            (self.quiet_open, self.denied) = (false, None);
        }
        self.synced |= p.frames[1] > 0;
        for why in &p.closed {
            eprintln!("harken-peer: dropped: {why}");
        }
        if p.denied.is_some() || !p.closed.is_empty() && std::mem::take(&mut self.quiet_open) {
            self.ask_why(p.denied.is_some());
        }
        self.collect();
    }

    /// The hub closed a connection before saying a word, which is what it
    /// does to a token that proves nobody (with code 4401, `denied`) — or a
    /// server going down. Which one is asked of the sign-in routes when it
    /// did not say: a token they no longer know is denied, and the peer
    /// stops dialling with it until it signs in again; its store and its
    /// pending intents stay.
    fn ask_why(&mut self, denied: bool) {
        let (Some(server), Some(token)) = (&self.server, &self.token) else {
            return;
        };
        if denied || matches!(ark_auth::client::whoami(server, token), Ok(None)) {
            self.denied = Some(REVOKED.into());
            self.drop_link();
            eprintln!("harken-peer: denied: {REVOKED}");
        }
    }

    /// Take the verdicts that arrived, and write each down.
    fn collect(&mut self) {
        let mut fresh = vec![];
        for (id, why) in self.client.take_rejections() {
            fresh.push(json!({ "id": id_json(&id.0), "reason": why }));
            self.rejected.insert(id, why);
        }
        self.live_refusals.extend(self.client.take_live_refusals());
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

    /// Waits on the link for at most `wait` (on stdin when there is no
    /// link), and pumps.
    fn turn(&mut self, wait: Duration) {
        if !self.dialling() {
            if let Ok(l) = self.lines.recv_timeout(wait) {
                self.held.push(l);
            }
        } else if self.client.backlog() == 0 {
            ark_peer::link::wait(&self.client, &self.bell, wait);
        }
        self.pump();
    }

    /// Pumps until `done`, or the timeout; with `step`, `done` is asked
    /// once a step. Whether it was done.
    fn pump_until(
        &mut self,
        timeout: Duration,
        step: Option<Duration>,
        mut done: impl FnMut(&Headless) -> bool,
    ) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if done(self) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            let looked = Instant::now();
            loop {
                while let Ok(l) = self.lines.try_recv() {
                    self.held.push(l);
                }
                self.turn(Duration::from_millis(2));
                if step.is_none_or(|s| looked.elapsed() >= s) || Instant::now() >= deadline {
                    break;
                }
            }
        }
    }

    /// The next line to answer, pumping meanwhile; `None` at the end of
    /// stdin.
    fn next(&mut self) -> Option<String> {
        if !self.held.is_empty() {
            return self.held.remove(0);
        }
        loop {
            match self.lines.try_recv() {
                Ok(l) => return l,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return None,
                Err(_) => {}
            }
            if self.dialling() {
                self.turn(Duration::from_millis(2));
                continue;
            }
            match self.lines.recv_timeout(Duration::from_millis(50)) {
                Ok(l) => return l,
                Err(RecvTimeoutError::Disconnected) => return None,
                Err(RecvTimeoutError::Timeout) => self.pump(),
            }
        }
    }

    fn cursor(&self) -> i64 {
        self.client.log.log().head().0
    }

    fn pending(&self) -> usize {
        self.client.log.pending.len()
    }

    fn linked(&self) -> bool {
        self.client.linked
    }

    fn fork(&self) -> i64 {
        match &self.client.log.role {
            Role::Authority {
                journal: Journal {
                    fork: Some((n, _)), ..
                },
            } => *n,
            _ => 0,
        }
    }

    fn status(&self) -> Answer {
        let me = &self.client.log.me;
        let link = match (self.is_alone(), self.dialling(), self.linked()) {
            (true, _, _) => "alone",
            (_, false, _) if self.token.is_none() => "signed out",
            (_, false, _) => "offline",
            (_, true, true) => "linked",
            (_, true, false) => "dialling",
        };
        Answer::default()
            .set("cursor", json!(self.cursor()))
            .set("pending", json!(self.pending()))
            .set("held", json!(0))
            .set("behind", json!(false))
            .set("linked", json!(self.linked()))
            .set(
                "denied",
                self.denied.as_ref().map_or(Value::Null, |d| json!(d)),
            )
            .set("user", json!(me.actor))
            .set("session", json!(me.session))
            .set(
                "signed_out",
                json!(self.token.is_none() && !self.is_alone()),
            )
            .set("epoch", json!(self.client.link.epoch))
            .set("link", json!(link))
            .set("opens", json!(self.client.link.epoch))
            .set("diverged", json!(self.client.log.diverged.len()))
            .set(
                "joining",
                json!(self.client.log.joining.as_ref().map_or(0, |j| j.2.len())),
            )
            .set("fork", json!(self.fork()))
    }

    /// A bare name, `create_playlist`, as its binding,
    /// `playlists.create_playlist`, when exactly one binding ends so.
    fn binding(&self, name: &str) -> Result<String, String> {
        let m = &self.hosted.domain.module;
        if m.get(name).is_some() {
            return Ok(name.into());
        }
        let hit: Vec<&String> = m
            .bindings
            .iter()
            .map(|b| &b.0)
            .filter(|b| b.rsplit('.').next() == Some(name))
            .collect();
        match hit[..] {
            [b] => Ok(b.clone()),
            _ => Err(format!("no function {name}")),
        }
    }

    fn session(&self) -> Option<Listening> {
        let live = self.client.live.as_ref()?;
        Some(Listening::of(&live.store, &self.client.log.me.actor))
    }

    /// What is pending authored again as `who`. A chain a join was to
    /// hand over goes with it: its entries are another session's, which a
    /// server adopts from nobody but that session, so they are pushed as
    /// this login's intents instead, in their order.
    ///
    /// The same session is no new login: its intents are its own already,
    /// and authoring them again from its first `local` would name entries
    /// the server has answered.
    fn reauthor(&mut self, who: Who) {
        let log = &self.client.log;
        let theirs = log.pending.iter().map(|p| &p.entry.session).chain(
            log.joining
                .iter()
                .flat_map(|j| j.2.iter().map(|l| &l.entry.session)),
        );
        if who.session == log.me.session && theirs.into_iter().all(|s| *s == who.session) {
            return;
        }
        self.client.sign_in(who);
        self.client.log.joining = None;
    }

    /// Signs in as `login`: the client authors as it, the link dials with it.
    fn signed_in(&mut self, login: &Login) {
        let mut who = ark_peer::who(login);
        who.roles.extend(self.roles.iter().cloned());
        who.roles.sort();
        who.roles.dedup();
        self.reauthor(who);
        self.token = Some(login.token.clone());
        self.denied = None;
        self.dial();
    }

    /// Answer one command. `quit` is answered like `persist`; the loop is
    /// what stops.
    fn handle(&mut self, cmd: Cmd) -> Answer {
        match cmd {
            Cmd::Mutate { name, args } => {
                let name = match self.binding(&name) {
                    Ok(n) => n,
                    Err(e) => return Answer::refused(e),
                };
                let bytes = match input(&name, &args) {
                    Ok(b) => b,
                    Err(e) => return Answer::refused(e),
                };
                match self.client.mutate_bytes(&name, bytes) {
                    Err(why) => Answer::refused(why),
                    Ok(id) => Answer::ok(true).set("id", id_json(&id.0)),
                }
            }
            Cmd::Status => self.status(),
            Cmd::Hash => Answer::default()
                .set("cursor", json!(self.cursor()))
                .set("hash", json!(self.client.log.confirmed_hash().hex()))
                .set("view", json!(self.client.log.store.state_hash().hex())),
            Cmd::Wait { cursor, timeout } => {
                let ok = self.pump_until(timeout, None, |h| h.cursor() >= cursor);
                Answer::ok(ok).set("cursor", json!(self.cursor()))
            }
            Cmd::Settle { timeout } => {
                // Linked (or alone), nothing pending, and the cursor still
                // for two steps of `--pump-ms`: what "caught up" can mean
                // from this end alone. The fleet compares against the
                // server's head as well.
                let (mut last, mut still) = (-1, 0);
                let step = self.still;
                let ok = self.pump_until(timeout, Some(step), |h| {
                    if h.cursor() == last {
                        still += 1;
                    } else {
                        (last, still) = (h.cursor(), 0);
                    }
                    (h.linked() || h.is_alone()) && h.pending() == 0 && still >= 2
                });
                Answer::ok(ok)
                    .set("cursor", json!(self.cursor()))
                    .set("pending", json!(self.pending()))
            }
            Cmd::Query { name, args } => {
                let name = match self.binding(&name) {
                    Ok(n) => n,
                    Err(e) => return Answer::refused(e),
                };
                let me = &self.client.log.me;
                let ctx = Ctx {
                    draws: Default::default(),
                    id: EntryId::of("query", 0),
                    actor: me.actor.clone(),
                    session: me.session.clone(),
                    roles: me.roles.clone(),
                    now: now_ms(),
                };
                let answer = input(&name, &args)
                    .and_then(|b| {
                        self.hosted
                            .domain
                            .ask(&self.client.log.store, &ctx, &name, &b)
                            .map_err(|r| r.0)
                    })
                    .and_then(|b| rows(&name, &b));
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
                let id = EntryId(id);
                if self.client.log.pending.iter().any(|p| p.entry.id == id) {
                    Answer::default().set("standing", json!("pending"))
                } else if self
                    .client
                    .log
                    .log()
                    .entries()
                    .iter()
                    .any(|l| l.entry.id == id)
                {
                    Answer::default().set("standing", json!("confirmed"))
                } else if let Some(why) = self.rejected.get(&id) {
                    Answer::default()
                        .set("standing", json!("rejected"))
                        .set("why", json!(why))
                } else {
                    Answer::default().set("standing", json!("unknown"))
                }
            }
            Cmd::Disconnect => {
                self.drop_link();
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
                if let Err(e) = remember(&self.dir, &server, &login) {
                    eprintln!("harken-peer: the login is not remembered: {e}");
                }
                self.signed_in(&login);
                Answer::ok(true)
                    .set("user", json!(login.user.id))
                    .set("session", json!(login.session))
            }
            Cmd::SignOut => {
                self.token = None;
                self.client.sign_in(nobody(""));
                self.dial();
                forget(&self.dir);
                Answer::ok(true)
            }
            Cmd::Join { server, user } => {
                // The login this directory already has for that server and
                // person, when it has one: the chain it made alone is its
                // session's, and a server adopts it from that session as it
                // is. Another login authors it again, with ids of its own.
                let remembered = user
                    .as_ref()
                    .and_then(|u| recall(&self.dir, &server).filter(|l| l.user.id == *u));
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
                if self.is_alone() {
                    self.client.join();
                }
                self.server = Some(server.clone());
                match &login {
                    Some(l) => {
                        if let Err(e) = remember(&self.dir, &server, l) {
                            eprintln!("harken-peer: the login is not remembered: {e}");
                        }
                        self.signed_in(l);
                    }
                    None => {
                        self.token = None;
                        self.dial();
                    }
                }
                eprintln!(
                    "harken-peer: joined {server}: {} handed over in {:?}",
                    self.client.log.joining.as_ref().map_or(0, |j| j.2.len()),
                    took.elapsed()
                );
                Answer::ok(true)
                    .set("pending", json!(self.pending()))
                    .set("cursor", json!(self.cursor()))
            }
            Cmd::Leave => {
                if self.is_alone() {
                    return Answer::refused("already alone");
                }
                self.drop_link();
                self.client.alone();
                self.server = None;
                self.pump();
                Answer::ok(true).set("cursor", json!(self.cursor()))
            }
            Cmd::Call {
                live,
                transition,
                input: args,
            } => {
                if self.client.live.is_none() {
                    return Answer::refused("this peer is no device (--device)");
                }
                let name = format!("{live}.{transition}");
                let Some(func) = self.hosted.get(&name) else {
                    return Answer::refused(format!("no transition {name}"));
                };
                let bytes = match input(&name, &args) {
                    Ok(b) => b,
                    Err(e) => return Answer::refused(e),
                };
                let ephemeral = self
                    .client
                    .config
                    .live
                    .as_ref()
                    .is_some_and(|l| l.ephemeral.contains(&func));
                match self.client.call(&name, bytes) {
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
                let (seq, pending) = self
                    .client
                    .live
                    .as_ref()
                    .map_or((0, 0), |l| (l.log().head().0, l.pending.len()));
                Answer::default()
                    .set(
                        "value",
                        self.session().as_ref().map_or(Value::Null, listening_json),
                    )
                    .set("device", json!(self.client.log.me.session))
                    .set("seq", json!(seq))
                    .set("pending", json!(pending))
                    .set("synced", json!(self.linked() && self.synced))
            }
            Cmd::LiveRefusals => {
                Answer::default().set("refusals", json!(std::mem::take(&mut self.live_refusals)))
            }
            Cmd::Persist | Cmd::Quit => {
                self.pump();
                match self.client.failure.clone() {
                    None => Answer::ok(true),
                    Some(e) => Answer::refused(e),
                }
            }
        }
    }
}

fn run(f: Flags) -> Result<(), String> {
    let hosted = match &f.module {
        None => Hosted::harken(),
        Some(p) => Hosted::read(p).map_err(|e| format!("{e:#}"))?,
    };
    let mut h = Headless::open(&f, hosted)?;
    let mut out = std::io::stdout().lock();
    h.pump();
    loop {
        let Some(line) = h.next() else {
            // The end of stdin is a quit.
            h.handle(Cmd::Quit);
            return Ok(());
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
        // A reader that has gone is a driver that has gone: stop.
        if writeln!(out, "{}", answer.line())
            .and_then(|_| out.flush())
            .is_err()
            || quit
        {
            return Ok(());
        }
        h.pump();
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

    /// Every command reads, its integers plain or as `{"$int"}`; what is
    /// not a command says why. Falsified by reading `cursor` as text:
    /// `wait` fails to parse.
    #[test]
    fn the_commands_read() {
        let id = "00000000-0000-0000-0000-00000000002a";
        assert_eq!(
            parse(r#"{"cmd":"mutate","name":"create_playlist","args":{"name":"Road trip"}}"#)
                .unwrap(),
            Cmd::Mutate {
                name: "create_playlist".into(),
                args: json!({"name": "Road trip"}),
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
                args: json!({})
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
            (r#"{"cmd":"standing","id":"x"}"#, "`id` is an id"),
            (r#"{"cmd":"join"}"#, "`server` is missing"),
        ] {
            let e = parse(bad).unwrap_err();
            assert!(e.contains(says), "{bad}: {e}");
        }
    }

    /// Arguments and rows cross as JSON under harken's own types, and back:
    /// an id as `{"$id"}`, a sum by its variant's name. Falsified by
    /// printing a record's fields without their names (an array): the
    /// playlist read back had no `name`.
    #[test]
    fn harkens_types_cross_as_json() {
        let id = json!({"$id": "00000000-0000-0000-0000-00000000002a"});
        let b = input(
            "playlists.add_to_playlist",
            &json!({"playlist_id": id, "media_id": id}),
        )
        .unwrap();
        let back = AddToPlaylist::from_bytes(&b).unwrap();
        assert_eq!(back.media_id.0[15], 42);
        assert_eq!(back.to_json()["playlist_id"], id);
        let p = Playlist {
            id: Id::new([1; 16]),
            name: "Road trip".into(),
            pos: 3,
            created_ms: 9,
            user_id: "alice".into(),
        };
        let v = rows("playlists.playlists", &vec![p].to_bytes()).unwrap();
        assert_eq!(v[0]["name"], json!("Road trip"));
        assert_eq!(v[0]["pos"], json!(3));
        let c = Clock::Playing { at_ms: 5, since: 7 };
        assert_eq!(Clock::from_json(&c.to_json()).unwrap(), c);
        let o = Output::Away {
            id: "phone".into(),
            name: "Phone".into(),
        };
        assert_eq!(Output::from_json(&o.to_json()).unwrap(), o);
        assert!(input("playlists.create_playlist", &json!({"name": 3}))
            .unwrap_err()
            .contains("`name`"));
        assert!(input("nothing.here", &json!({})).is_err());
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
            (&status["cursor"], &status["pending"], &status["joining"]),
            (&json!(0), &json!(3), &json!(3)),
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
            made["ok"] == json!(true) && made["id"]["$id"].is_string(),
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
        assert_eq!(h.pending(), 1, "the intent was on disk");
    }
}
