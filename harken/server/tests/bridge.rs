//! The listening session over real sockets, the way production runs it:
//! the whole server, clients that sign in and declare themselves devices,
//! the session as the domain's live state on `/sync`, a speaker the Home
//! Assistant bridge stands in for, and a Home Assistant that answers over
//! HTTP.
//!
//! The session's rules are transition tests in `harken/domain/tests/
//! listening.rs`, and the bridge's are unit tests beside it. This is the
//! plumbing above both, which neither can see and which is where a device
//! goes missing without any rule being wrong — a speaker that never stood,
//! a frame addressed to a connection nobody holds, a session read before it
//! moved, a service call with the wrong body.
//!
//! A speaker's id in a session is `user@entity` (`ha::speaker`); the tests
//! name it by its entity, as the bridge does: [`call`] says a speaker a
//! transition names by its id, and every view read here names it back by
//! its entity (`ha::as_entities`).
//!
//! The Home Assistant here is a stand-in, written from its REST API's
//! documentation: `POST /api/services/media_player/<service>` with a JSON
//! body, `GET /api/states/<entity>`, `POST /api/template`, a bearer token on
//! each. Its players take `volume_set`, `volume_mute`, `join` and `unjoin`
//! and say `volume_level`, `is_volume_muted`, `group_members` and
//! `supported_features` as the documentation says a Sonos does. What a real
//! one and a real Sonos make of these calls is not verified here.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ark::ir::Node;
use ark_auth::Account;
use ark_server::handle::now;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use common::*;
use harken_domain::listening::read::{Kind, Listening};
use harken_domain::listening::{
    Clock, Member, NudgeVolume, Output, SetMute, SetVolume, Track, Transfer,
};
use harken_server::assistant::{ha, input};
use harken_server::Server;
use serde_json::{json, Value as Json_};

/// Every service call the house was asked for, and what each player says
/// it is doing: what a test sets, and a pause or a play it was told.
#[derive(Clone, Default)]
struct Fake {
    calls: Arc<Mutex<Vec<(String, Json_)>>>,
    states: Arc<Mutex<BTreeMap<String, Json_>>>,
    unauthorised: Arc<Mutex<usize>>,
    /// The players that keep a queue of their own, as a Sonos does: what
    /// `clear_playlist` and `play_media` do to it, and how the speaker spells
    /// back what it was given. A player not here only says what a test sets.
    queues: Arc<Mutex<BTreeMap<String, Queue>>>,
    /// How many times each player was asked what it is doing.
    reads: Arc<Mutex<BTreeMap<String, usize>>>,
}

/// A Sonos's own queue, as Home Assistant's Sonos integration drives it
/// (`homeassistant/components/sonos/media_player.py`, `_play_media`):
/// `enqueue: play` inserts after the integration's `queue_position` — what
/// it last heard, which a `clear_playlist` a moment before has not reached
/// yet — and plays from there; `enqueue: add` appends; `enqueue: next`
/// inserts after the one playing.
struct Queue {
    urls: Vec<String>,
    /// The item playing, from 0.
    at: Option<usize>,
    /// `queue_position` as the integration last heard it, 1-based: stale
    /// across a `clear_playlist`, which it learns of from an event later.
    heard: Option<usize>,
    /// What the speaker reports a URL it was given as: its `TrackURI`.
    spell: fn(&str) -> String,
    /// Until when the item it is on is still loading: a seek before then is
    /// dropped, as a Sonos drops one sent before it has read the stream.
    loading: Option<std::time::Instant>,
    /// Whether a new queue arrives the way a Sonos's does through Home
    /// Assistant ([`Fake::keeps_a_queue_as_a_sonos`]), and the one arriving.
    sonos: bool,
    arriving: Option<Arrival>,
    /// What it said it was playing before the last `clear_playlist`.
    before: Json_,
}

/// A queue a Sonos has just been handed, on its way to playing — as Home
/// Assistant reports it, a [`STEP`] at a time ([`Fake::arrive`]).
struct Arrival {
    from: std::time::Instant,
    /// What it will say it plays, spelt its way, and what it said before.
    url: String,
    stale: Json_,
}

/// How long each step of a Sonos taking a new queue lasts, as Home Assistant
/// says it: two or so of the bridge's polls here.
const STEP: Duration = Duration::from_millis(90);

const TOKEN: &str = "a-long-lived-token";

fn authorised(fake: &Fake, headers: &HeaderMap) -> bool {
    let ok = headers.get("authorization").and_then(|v| v.to_str().ok())
        == Some(&format!("Bearer {TOKEN}"));
    if !ok {
        *fake.unauthorised.lock().unwrap() += 1;
    }
    ok
}

async fn service(
    State(fake): State<Fake>,
    Path(service): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Json_>,
) -> StatusCode {
    if !authorised(&fake, &headers) {
        return StatusCode::UNAUTHORIZED;
    }
    // A pause or a play is done at once, as a speaker would a moment
    // later: otherwise the poll after it reads the speaker as moved by
    // hand, and the bridge says so. So is a volume, a mute, a join and an
    // unjoin.
    let now = match service.as_str() {
        "media_pause" => Some("paused"),
        "media_play" => Some("playing"),
        _ => None,
    };
    let entity = body["entity_id"].as_str().unwrap_or_default().to_string();
    fake.arrive(&entity);
    fake.queue_service(&entity, &service, &body);
    {
        let mut states = fake.states.lock().unwrap();
        if let (Some(now), Some(st)) = (now, states.get_mut(&entity)) {
            // A change of state is when the house dates the position again:
            // where it had got to, as of now.
            let at = position_now(st);
            st["state"] = json!(now);
            date_position(st, at);
        }
        match service.as_str() {
            "media_seek" if fake.still_loading(&entity) => {}
            "media_seek" => {
                if let Some(st) = states.get_mut(&entity) {
                    let at = body["seek_position"].as_f64().unwrap_or(0.0);
                    date_position(st, at);
                }
            }
            "volume_set" => {
                if let Some(st) = states.get_mut(&entity) {
                    st["attributes"]["volume_level"] = body["volume_level"].clone();
                }
            }
            "volume_mute" => {
                if let Some(st) = states.get_mut(&entity) {
                    st["attributes"]["is_volume_muted"] = body["is_volume_muted"].clone();
                }
            }
            "join" => {
                let mut group = vec![entity.clone()];
                for m in body["group_members"].as_array().into_iter().flatten() {
                    group.extend(m.as_str().map(str::to_string));
                }
                for e in &group {
                    if let Some(st) = states.get_mut(e) {
                        st["attributes"]["group_members"] = json!(group);
                    }
                }
            }
            "unjoin" => {
                let group: Vec<String> = states
                    .get(&entity)
                    .and_then(|st| st["attributes"]["group_members"].as_array().cloned())
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|m| m.as_str().map(str::to_string))
                    .filter(|m| *m != entity)
                    .collect();
                for e in &group {
                    if let Some(st) = states.get_mut(e) {
                        st["attributes"]["group_members"] = json!(group);
                    }
                }
                if let Some(st) = states.get_mut(&entity) {
                    st["attributes"]["group_members"] = json!([entity]);
                }
            }
            _ => {}
        }
    }
    fake.calls.lock().unwrap().push((service, body));
    StatusCode::OK
}

/// The template API, for the one template the bridge renders: which
/// integration a player is. Every player here is a Sonos.
async fn template(
    State(fake): State<Fake>,
    headers: HeaderMap,
    Json(body): Json<Json_>,
) -> Result<String, StatusCode> {
    if !authorised(&fake, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let asked = body["template"].as_str().unwrap_or_default();
    match asked.contains("config_entry_attr(config_entry_id(") {
        true => Ok("sonos".into()),
        false => Err(StatusCode::BAD_REQUEST),
    }
}

/// The time as Home Assistant writes it: RFC 3339, in UTC, to the
/// microsecond.
fn stamp(ms: i64) -> String {
    let (secs, ms) = (ms.div_euclid(1000), ms.rem_euclid(1000));
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{ms:03}000+00:00",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

fn wall_ms() -> i64 {
    now()
}

/// Where a player is now, as a real house would know it: its dated
/// position, counted on while it plays.
fn position_now(st: &Json_) -> f64 {
    let at = st["attributes"]["media_position"].as_f64().unwrap_or(0.0);
    let since = st["attributes"]["media_position_updated_at"]
        .as_str()
        .and_then(harken_server::assistant::when::rfc3339_ms);
    match (st["state"].as_str(), since) {
        (Some("playing"), Some(since)) => at + (wall_ms() - since).max(0) as f64 / 1000.0,
        _ => at,
    }
}

/// The position set, and dated now — what Home Assistant does at a change of
/// state, and only then.
fn date_position(st: &mut Json_, at_s: f64) {
    st["attributes"]["media_position"] = json!(at_s);
    st["attributes"]["media_position_updated_at"] = json!(stamp(wall_ms()));
}

async fn state(
    State(fake): State<Fake>,
    Path(entity): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Json_>, StatusCode> {
    if !authorised(&fake, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    *fake
        .reads
        .lock()
        .unwrap()
        .entry(entity.clone())
        .or_default() += 1;
    fake.arrive(&entity);
    fake.states
        .lock()
        .unwrap()
        .get(&entity)
        .cloned()
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

impl Fake {
    /// This player keeps a queue of its own and spells back what it was
    /// given through `spell`.
    fn keeps_a_queue(&self, entity: &str, spell: fn(&str) -> String) {
        self.queues.lock().unwrap().insert(
            entity.into(),
            Queue {
                urls: vec![],
                at: None,
                heard: None,
                spell,
                loading: None,
                sonos: false,
                arriving: None,
                before: json!(""),
            },
        );
        self.sonos(entity, 0.3, &[entity]);
    }

    /// …and takes a new one the way a Sonos does through Home Assistant,
    /// which is not at once. After `clear_playlist` and `play_media` the
    /// house says `idle` with nothing playing; then `buffering`, still naming
    /// whatever it played before; then `playing` the new track, with no
    /// position at all — the integration clears it while the stream's length
    /// is unknown; and only then a position, 0, dated at that moment. A seek
    /// sent before that last step is dropped, and one sent after it lands,
    /// dated again.
    fn keeps_a_queue_as_a_sonos(&self, entity: &str) {
        self.keeps_a_queue(entity, as_given);
        self.queues.lock().unwrap().get_mut(entity).unwrap().sonos = true;
    }

    /// Where a Sonos taking a new queue has got to, written into what the
    /// house says of it: called before anything reads or tells it anything.
    fn arrive(&self, entity: &str) {
        let (step, url, stale) = {
            let mut queues = self.queues.lock().unwrap();
            let Some(a) = queues.get_mut(entity).and_then(|q| q.arriving.as_ref()) else {
                return;
            };
            let elapsed = a.from.elapsed();
            let step = (elapsed.as_millis() / STEP.as_millis()).min(3);
            let (url, stale) = (a.url.clone(), a.stale.clone());
            if step == 3 {
                queues.get_mut(entity).unwrap().arriving = None;
            }
            (step, url, stale)
        };
        let mut states = self.states.lock().unwrap();
        let st = states.get_mut(entity).unwrap();
        let unplaced = |st: &mut Json_| {
            let attrs = st["attributes"].as_object_mut().unwrap();
            attrs.remove("media_position");
            attrs.remove("media_position_updated_at");
        };
        match step {
            0 => {
                st["state"] = json!("idle");
                st["attributes"]["media_content_id"] = json!("");
                unplaced(st);
            }
            1 => {
                st["state"] = json!("buffering");
                st["attributes"]["media_content_id"] = stale;
                unplaced(st);
            }
            2 => {
                st["state"] = json!("playing");
                st["attributes"]["media_content_id"] = json!(url);
                unplaced(st);
            }
            _ => {
                st["state"] = json!("playing");
                st["attributes"]["media_content_id"] = json!(url);
                date_position(st, 0.0);
            }
        }
    }

    /// What a service call does to a player that keeps a queue.
    fn queue_service(&self, entity: &str, service: &str, body: &Json_) {
        let mut queues = self.queues.lock().unwrap();
        let Some(q) = queues.get_mut(entity) else {
            return;
        };
        match service {
            "clear_playlist" => {
                q.urls.clear();
                q.at = None;
                q.before = self
                    .states
                    .lock()
                    .unwrap()
                    .get(entity)
                    .map_or(json!(""), |st| st["attributes"]["media_content_id"].clone());
                drop(queues);
                self.on_item(entity, None);
            }
            "play_media" => {
                let url = body["media_content_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                match body["enqueue"].as_str() {
                    Some("add") => q.urls.push(url),
                    Some("next") => {
                        let at = q.at.map_or(q.urls.len(), |a| a + 1).min(q.urls.len());
                        q.urls.insert(at, url);
                    }
                    Some("play") => {
                        let pos = q.heard.unwrap_or(0) + 1;
                        let at = (pos - 1).min(q.urls.len());
                        q.urls.insert(at, url);
                        q.at = Some(at);
                        let sonos = q.sonos;
                        drop(queues);
                        self.on_item(entity, Some(at));
                        if sonos {
                            self.arriving(entity);
                        }
                    }
                    _ => {}
                }
            }
            "media_next_track" => {
                let next = q.at.map(|a| a + 1).filter(|n| *n < q.urls.len());
                if next.is_some() {
                    q.at = next;
                    drop(queues);
                    self.on_item(entity, next);
                }
            }
            _ => {}
        }
    }

    /// The speaker on item `at` of its queue, from its start, playing — or
    /// on nothing, idle — as the house then says it: the item's URL as the
    /// speaker spells it, `queue_position`, and the position dated now.
    fn on_item(&self, entity: &str, at: Option<usize>) {
        let (url, position) = {
            let mut queues = self.queues.lock().unwrap();
            let q = queues.get_mut(entity).unwrap();
            q.at = at;
            q.heard = at.map(|a| a + 1).or(q.heard);
            q.loading = Some(std::time::Instant::now() + LOADING);
            (at.map(|a| (q.spell)(&q.urls[a])), at.map(|a| a + 1))
        };
        let mut states = self.states.lock().unwrap();
        let st = states.get_mut(entity).unwrap();
        st["state"] = json!(if url.is_some() { "playing" } else { "idle" });
        st["attributes"]["media_content_id"] = json!(url.unwrap_or_default());
        st["attributes"]["queue_position"] = json!(position);
        date_position(st, 0.0);
    }

    /// The item just put on is a new queue arriving, the way a Sonos's does.
    fn arriving(&self, entity: &str) {
        let mut queues = self.queues.lock().unwrap();
        let q = queues.get_mut(entity).unwrap();
        let stale = q.before.clone();
        let url = q.at.map(|a| (q.spell)(&q.urls[a])).unwrap_or_default();
        q.loading = Some(std::time::Instant::now() + STEP * 3);
        q.arriving = Some(Arrival {
            from: std::time::Instant::now(),
            url,
            stale,
        });
        drop(queues);
        self.arrive(entity);
    }

    /// The track playing runs out, and the speaker moves to the next item of
    /// its queue by itself — after `blink` of saying it is playing nothing,
    /// as a Sonos between two tracks can; at the end of its queue it stops.
    fn track_ends(&self, entity: &str, blink: Duration) {
        if !blink.is_zero() {
            let mut states = self.states.lock().unwrap();
            let st = states.get_mut(entity).unwrap();
            st["attributes"]["media_content_id"] = json!("");
            drop(states);
            std::thread::sleep(blink);
        }
        let next = {
            let queues = self.queues.lock().unwrap();
            let q = &queues[entity];
            q.at.map(|a| a + 1).filter(|n| *n < q.urls.len())
        };
        self.on_item(entity, next);
    }

    /// How many times the house was asked what any player is doing.
    fn reads(&self) -> usize {
        self.reads.lock().unwrap().values().sum()
    }

    /// Whether the item this player is on is still loading.
    fn still_loading(&self, entity: &str) -> bool {
        self.queues
            .lock()
            .unwrap()
            .get(entity)
            .and_then(|q| q.loading)
            .is_some_and(|until| std::time::Instant::now() < until)
    }

    /// Where a player is now, in milliseconds.
    fn at_ms(&self, entity: &str) -> i64 {
        (position_now(&self.states.lock().unwrap()[entity]) * 1000.0) as i64
    }

    /// The URLs a player's own queue holds, as it was given them.
    fn queued(&self, entity: &str) -> Vec<String> {
        self.queues.lock().unwrap()[entity].urls.clone()
    }

    fn serve(&self, rt: &tokio::runtime::Runtime) -> String {
        let router = Router::new()
            .route("/api/services/media_player/:service", post(service))
            .route("/api/states/:entity", get(state))
            .route("/api/template", post(template))
            .with_state(self.clone());
        let listener = rt
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let addr = listener.local_addr().unwrap();
        rt.spawn(async move { axum::serve(listener, router).await });
        format!("http://{addr}")
    }

    fn take(&self) -> Vec<(String, Json_)> {
        std::mem::take(&mut *self.calls.lock().unwrap())
    }

    /// Wait for the house to have been asked for `n` calls, then a little
    /// longer for any it should not have been asked for.
    fn calls(&self, n: usize) -> Vec<(String, Json_)> {
        eventually(5, || (self.calls.lock().unwrap().len() >= n).then_some(()));
        std::thread::sleep(Duration::from_millis(150));
        self.take()
    }

    /// What a player is playing, from `position_s`, dated now — and from
    /// then on saying that same position, as Home Assistant does until the
    /// next change of state. Its volume and its group stay as they were.
    fn playing(&self, entity: &str, state: &str, url: &str, position_s: f64) {
        let mut states = self.states.lock().unwrap();
        let st = states
            .entry(entity.into())
            .or_insert_with(|| json!({ "entity_id": entity, "attributes": {} }));
        st["state"] = json!(state);
        st["attributes"]["media_content_id"] = json!(url);
        date_position(st, position_s);
    }

    /// A Sonos, idle: its volume, its group, and what Home Assistant says a
    /// Sonos supports — `PAUSE | SEEK | VOLUME_SET | VOLUME_MUTE |
    /// PREVIOUS_TRACK | NEXT_TRACK | PLAY_MEDIA | CLEAR_PLAYLIST | PLAY |
    /// GROUPING`, among others.
    fn sonos(&self, entity: &str, volume: f64, members: &[&str]) {
        self.states.lock().unwrap().insert(
            entity.into(),
            json!({
                "entity_id": entity,
                "state": "idle",
                "attributes": {
                    "volume_level": volume,
                    "is_volume_muted": false,
                    "group_members": members,
                    "supported_features": 1 | 2 | 4 | 8 | 16 | 32 | 512 | 8192 | 16384 | 524_288,
                },
            }),
        );
    }

    /// One attribute of a player, as the house now has it.
    fn attribute(&self, entity: &str, name: &str) -> Json_ {
        self.states.lock().unwrap()[entity]["attributes"][name].clone()
    }

    /// A hand on the house: an attribute changed outside harken.
    fn set(&self, entity: &str, name: &str, value: Json_) {
        self.states.lock().unwrap().get_mut(entity).unwrap()["attributes"][name] = value;
    }
}

const KITCHEN: &str = "media_player.kitchen";

/// How long a player that keeps a queue takes to load a track: longer than
/// it takes the bridge to ask it to seek.
const LOADING: Duration = Duration::from_millis(150);
const MEDIA: &str = "http://10.0.0.2:8787";

fn house(url: String) -> ha::Config {
    ha::Config {
        url,
        token: TOKEN.into(),
        players: vec![(KITCHEN.into(), "Kitchen".into())],
        media: MEDIA.into(),
        tick: Duration::from_millis(40),
        idle: ha::IDLE,
        trace: ha::Trace::Off,
    }
}

const STUDY: &str = "media_player.study";

/// The house with two Sonos in it.
fn two_sonos(url: String) -> ha::Config {
    ha::Config {
        players: vec![
            (KITCHEN.into(), "Kitchen".into()),
            (STUDY.into(), "Study".into()),
        ],
        ..house(url)
    }
}

/// The speakers the tests name, by their entities, as the bridge does.
const SPEAKERS: &[&str] = &[KITCHEN, STUDY];

/// A speaker's entity as its id in `user`'s session, anything else as it is.
fn id_of(user: &str, d: &str) -> String {
    match SPEAKERS.contains(&d) {
        true => ha::speaker(user, d),
        false => d.to_string(),
    }
}

/// Pump until this peer's view of the session, confirmed by the server,
/// satisfies `want`.
fn told(p: &mut Peer, what: &str, want: impl FnMut(&Listening) -> bool) -> Listening {
    told_among(&mut [p], 0, what, want)
}

/// [`told`] of `peers[who]`, every one of them pumped meanwhile.
fn told_among(
    peers: &mut [&mut Peer],
    who: usize,
    what: &str,
    mut want: impl FnMut(&Listening) -> bool,
) -> Listening {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut last = None;
    loop {
        for p in peers.iter_mut() {
            p.pump();
        }
        let p = &mut *peers[who];
        let settled = p.live_settled();
        if let Some(l) = p.listening(SPEAKERS) {
            if settled && want(&l) {
                return l;
            }
            last = Some(l);
        }
        assert!(
            std::time::Instant::now() < deadline,
            "never told {what}; last {last:#?}; linked {}; refused {:?}",
            p.linked(),
            p.refused
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A transition of the session from `p`; a speaker it names, by its entity,
/// said as its id in `p`'s session.
fn call(p: &mut Peer, transition: &str, i: Vec<u8>) {
    let user = p.user();
    let i = match transition {
        "transfer" => Transfer::from_bytes(&i).map_or(i, |t| {
            Transfer {
                to: id_of(&user, &t.to),
            }
            .to_bytes()
        }),
        "join" | "unjoin" => Member::from_bytes(&i).map_or(i, |m| {
            Member {
                device: id_of(&user, &m.device),
            }
            .to_bytes()
        }),
        "set_volume" => SetVolume::from_bytes(&i).map_or(i, |v| {
            SetVolume {
                device: id_of(&user, &v.device),
                ..v
            }
            .to_bytes()
        }),
        "nudge_volume" => NudgeVolume::from_bytes(&i).map_or(i, |v| {
            NudgeVolume {
                device: id_of(&user, &v.device),
                ..v
            }
            .to_bytes()
        }),
        "set_mute" => SetMute::from_bytes(&i).map_or(i, |v| {
            SetMute {
                device: id_of(&user, &v.device),
                ..v
            }
            .to_bytes()
        }),
        _ => i,
    };
    if let Err(e) = p.call(transition, i) {
        let view = p.listening(SPEAKERS);
        panic!("{transition}: {e}; the view was {view:#?}");
    }
    p.pump();
}

fn track(t: &str, n: u8) -> Track {
    let mut id = [0u8; 16];
    id[15] = n;
    Track {
        id: ark::author::Id::new(id),
        title: t.into(),
        creator: "Bach".into(),
        album: String::new(),
        duration_ms: 300_000,
        file: format!("music/{t}.mp3"),
    }
}

fn queue(names: &[&str]) -> Vec<Track> {
    names
        .iter()
        .enumerate()
        .map(|(i, n)| track(n, i as u8))
        .collect()
}

fn media_url(file: &str) -> String {
    format!("{MEDIA}/media/{file}")
}

/// The session as the hub holds it.
fn held(server: &Server, user: &str) -> Option<Listening> {
    let u = user.to_string();
    let s = server
        .running
        .hub
        .with(move |h| Listening::of(&h.live.as_ref().unwrap().store, &u))
        .unwrap();
    let speakers: Vec<String> = SPEAKERS.iter().map(|s| s.to_string()).collect();
    let s = ha::as_entities(s, user, &speakers);
    (s.session.is_some() || !s.devices.is_empty()).then_some(s)
}

/// More songs than one page of the log (`ark::peer::PAGE`).
const SONGS: usize = ark::peer::PAGE + 44;

/// How far into the session's volatile log a client has heard.
fn live_head(p: &Peer) -> i64 {
    p.client.live.as_ref().unwrap().log.head().0
}

/// The whole way round: a phone arrives and the kitchen stands beside it;
/// the phone hands the kitchen the sound, and the house is asked to play the
/// whole queue from where it was; the kitchen says it has started and where
/// it goes next, and the phone hears it; a pause on the phone is a pause in
/// the kitchen; taking the sound back pauses the kitchen; and when the phone
/// goes, so does the kitchen — a speaker does not keep a session's presence
/// open — and the sound stays with the phone, away.
///
/// Falsified by the bridge standing in every session it can read rather
/// than only where a client is: the kitchen stayed in `devices` after the
/// phone went. On v5, by a speaker's machine not taking the frames the hub
/// sends it: its `started` was refused by its own preview, every test but
/// two failed; and by reading the speakers' frames before the sessions
/// rather than after: a speaker's preview was behind what the bridge read,
/// and one test in two runs failed never told the kitchen played.
#[test]
fn a_speaker_is_a_device_the_whole_way_round() {
    let rt = runtime();
    let fake = Fake::default();
    let url = fake.serve(&rt);
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |c| c.house = Some(house(url)));

    let mut phone = device(&server, "max", Kind::Phone, "Pixel", true);
    let me = phone.me();
    let session = told(&mut phone, "the kitchen standing beside the phone", |s| {
        s.devices.len() == 2
    });
    let kitchen = session
        .device(KITCHEN)
        .expect("the speaker is in the picker");
    assert_eq!(
        (
            kitchen.name.as_str(),
            Kind::parse(&kitchen.kind),
            kitchen.can_play
        ),
        ("Kitchen", Kind::Speaker, true)
    );
    assert!(
        session.device(&me).is_some(),
        "and so is the phone, as its login"
    );
    assert!(
        fake.take().is_empty(),
        "offering a speaker asks the house nothing"
    );

    // Playing on the phone, then handed to the kitchen halfway through.
    call(
        &mut phone,
        "start",
        input::start(&queue(&["a", "b", "c"]), 1, 42_500, "album/Brandenburg"),
    );
    told(&mut phone, "the phone as the output", |s| s.outputs(&me));
    call(&mut phone, "transfer", input::transfer(KITCHEN));
    let session = told(&mut phone, "the kitchen, connecting", |s| {
        s.moving_to(KITCHEN)
    });
    assert_eq!(session.session.as_ref().unwrap().at, 1);

    let calls = fake.calls(4);
    let names: Vec<&str> = calls.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(
        names,
        ["clear_playlist", "play_media", "media_seek", "play_media"],
        "{calls:#?}"
    );
    assert_eq!(calls[0].1, json!({ "entity_id": KITCHEN }));
    assert_eq!(
        calls[1].1,
        json!({
            "entity_id": KITCHEN,
            "media_content_id": media_url("music/b.mp3"),
            "media_content_type": "music",
            "enqueue": "play",
        }),
        "from the track it was on, at the address a *speaker* can reach"
    );
    let at = calls[2].1["seek_position"].as_f64().unwrap();
    assert!(
        (42.5..44.5).contains(&at),
        "from where it had got to, before the rest of the queue: {at}"
    );
    assert_eq!(
        calls[3].1["media_content_id"],
        json!(media_url("music/c.mp3"))
    );
    assert_eq!(
        calls[3].1["enqueue"],
        json!("add"),
        "the rest after it, so the speaker's own next works"
    );

    // The kitchen starts, and says so; then it moves on by itself, and the
    // phone hears that too.
    fake.playing(KITCHEN, "playing", &media_url("music/b.mp3"), 43.0);
    let session = told(&mut phone, "the kitchen started", |s| {
        s.outputs(KITCHEN) && !s.moving_to(KITCHEN)
    });
    assert!(session.sounding());
    fake.playing(KITCHEN, "playing", &media_url("music/c.mp3"), 1.0);
    let session = told(&mut phone, "the kitchen on the third track", |s| {
        s.session.as_ref().is_some_and(|a| a.at == 2)
    });
    assert!(session.sounding());
    fake.take();

    // A pause on the phone is a pause in the kitchen.
    std::thread::sleep(Duration::from_millis(200));
    fake.take();
    call(&mut phone, "pause", input::none());
    told(&mut phone, "paused", |s| !s.sounding());
    let calls = fake.calls(1);
    assert_eq!(
        calls,
        [("media_pause".to_string(), json!({ "entity_id": KITCHEN }))]
    );

    // Taking the sound back: the kitchen reads the session, and a speaker
    // has nobody to go quiet for it, so the bridge pauses it.
    call(&mut phone, "transfer", input::transfer(&me));
    told(&mut phone, "the phone as the output again", |s| {
        s.outputs(&me)
    });
    let calls = fake.calls(1);
    assert_eq!(
        calls,
        [("media_pause".to_string(), json!({ "entity_id": KITCHEN }))]
    );
    assert_eq!(
        *fake.unauthorised.lock().unwrap(),
        0,
        "every call carried the token"
    );

    // The phone goes. The kitchen goes with it, and the sound stays with the
    // phone, away.
    drop(phone);
    let left = eventually(10, || held(&server, "max").filter(|s| s.devices.is_empty()));
    let left = left.unwrap_or_else(|| {
        panic!(
            "a speaker kept the session's presence open: {:?}",
            held(&server, "max")
        )
    });
    assert_eq!(left.output().and_then(Output::id), Some(me.as_str()));
    assert!(matches!(left.output(), Some(Output::Away { .. })));
    rt.block_on(server.stop());
}

/// A speaker is one piece of hardware and a session is one account's, so two
/// people can both pick the kitchen: the second gets it, and the first is
/// told — by the speaker, in their session — rather than left drawing a
/// transport for somebody else's music. And a speaker that goes on playing
/// something that is not ours is let go the same way, after its grace.
///
/// Falsified by the bridge not releasing the first session when the second
/// took the speaker: alice's session still named the kitchen. On v5, by a
/// speaker's session being its entity in every account: bob's kitchen
/// shared alice's kitchen's session, and the take failed.
#[test]
fn a_speaker_taken_or_sent_elsewhere_is_let_go_out_loud() {
    let rt = runtime();
    let fake = Fake::default();
    let url = fake.serve(&rt);
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |c| c.house = Some(house(url)));

    let mut alice = device(&server, "alice", Kind::Phone, "Alice's phone", true);
    let mut bob = device(&server, "bob", Kind::Phone, "Bob's phone", true);
    told(&mut alice, "the kitchen in alice's session", |s| {
        s.device(KITCHEN).is_some()
    });
    told(&mut bob, "the kitchen in bob's session", |s| {
        s.device(KITCHEN).is_some()
    });

    let take = |p: &mut Peer, t: &str, fake: &Fake| {
        let me = p.me();
        call(p, "start", input::start(&queue(&[t]), 0, 0, ""));
        told(p, "the phone as the output", |s| s.outputs(&me));
        // A quick speaker: playing what it is handed by the first poll.
        fake.playing(
            KITCHEN,
            "playing",
            &media_url(&format!("music/{t}.mp3")),
            0.5,
        );
        call(p, "transfer", input::transfer(KITCHEN));
        told(p, "the kitchen playing it", |s| {
            s.outputs(KITCHEN) && !s.moving_to(KITCHEN)
        });
    };
    take(&mut alice, "a", &fake);
    take(&mut bob, "b", &fake);
    let session = told(&mut alice, "alice let go of the kitchen", |s| {
        matches!(s.output(), Some(Output::Nowhere))
    });
    assert!(!session.sounding());
    assert_eq!(
        session.session.unwrap().queue,
        queue(&["a"]),
        "her queue stays"
    );
    let calls = fake.take();
    assert!(
        calls
            .iter()
            .any(|(s, b)| s == "play_media"
                && b["media_content_id"] == json!(media_url("music/b.mp3"))),
        "{calls:#?}"
    );

    // Somebody puts the radio on in the kitchen. Once the grace is spent,
    // bob's session is told the kitchen is not his any more.
    fake.playing(KITCHEN, "playing", "http://radio.example/stream", 0.0);
    let session = told(&mut bob, "bob let go of the kitchen", |s| {
        matches!(s.output(), Some(Output::Nowhere))
    });
    assert!(!session.sounding());
    rt.block_on(server.stop());
}

/// The bug the debug screen found, which the engine now owns: a library
/// longer than one batch has the client say `Hello` a *second* time on the
/// same socket to ask for the rest, and that must not be a departure and an
/// arrival — or the session drops the browser from its own presence, and
/// with it the speaker beside it. Three hundred songs is two batches.
///
/// Falsified by `Desk::hello` parting a connection that says the same
/// device again: the session's sequence moved by the browser's leave and
/// its join while it paged.
#[test]
fn asking_for_the_next_batch_does_not_leave_the_session() {
    let rt = runtime();
    let fake = Fake::default();
    let url = fake.serve(&rt);
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |c| c.house = Some(house(url)));
    // A library of two pages, authored by the library's own account.
    let login = server
        .auth
        .issue(&Account {
            id: "library".into(),
            roles: vec![harken_domain::schema::LIBRARY.into()],
            ..Account::default()
        })
        .unwrap();
    let mut library = open(ark_client::auth::who(&login), None);
    library.connect(&server.url(), &login.token);
    for i in 0..SONGS {
        let t = format!("song-{i:03}");
        library.mutate(
            "library.add_song",
            &harken_domain::library::AddSong {
                title: t.clone(),
                artist: "The Fleet".into(),
                album: "Loopback".into(),
                duration_ms: 1000,
                file: format!("typed/{t}.wav"),
                ..Default::default()
            },
        );
    }
    pump_until(&mut [&mut library], 30, |ps| {
        ps[0].linked() && ps[0].pending() == 0
    });

    let mut browser = device(&server, "max", Kind::Computer, "Browser", true);
    told(&mut browser, "the browser and the kitchen", |s| {
        s.devices.len() == 2
    });
    let seq = live_head(&browser);
    pump_until(&mut [&mut browser], 30, |ps| {
        ps[0].cursor() as usize >= SONGS
    });
    let session = told(&mut browser, "the session", |_| true);
    let mut ids: Vec<&str> = session.devices.iter().map(|d| d.id.as_str()).collect();
    ids.sort_unstable();
    let me = browser.me();
    let mut want = vec![me.as_str(), KITCHEN];
    want.sort_unstable();
    assert_eq!(ids, want);
    assert_eq!(
        live_head(&browser),
        seq,
        "nobody left and nobody came while it paged"
    );
    rt.block_on(server.stop());
}

/// One account, three devices, over real sockets: a browser that can play,
/// a desktop that cannot (the remote control it is), and the kitchen.
/// The desktop hands the browser's sound to the kitchen and pauses it from
/// where it sits, which pauses the kitchen; hands it back to the browser;
/// and the browser's socket drops, which leaves the sound with the browser,
/// away, paused. Then the decided rule for coming back, both ways: with the
/// kitchen playing meanwhile, the browser's return takes nothing and the
/// kitchen plays on; and with nothing playing, the browser — which was
/// playing on its own while it was away — offers what it was doing as a
/// `start`, and the session becomes that, on the browser.
///
/// The browser's half of the rule — its own sound going quiet when the
/// session is sounding elsewhere — is its reconciler's, and tested there
/// (`harken-iced`'s `listening` tests); what is held here is that the
/// session the browser comes back to says so, and takes its `start` when
/// nothing is sounding.
///
/// Falsified by an `on_join` that gave a present output to whoever joined
/// (`returned`'s `device` arm answering `here(id)`): the browser's return
/// took the sound from the playing kitchen.
#[test]
fn a_browser_a_desktop_and_a_speaker_share_one_session() {
    let rt = runtime();
    let fake = Fake::default();
    let url = fake.serve(&rt);
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |c| c.house = Some(house(url)));

    let mut browser = device(&server, "max", Kind::Computer, "Browser", true);
    let mut desktop = device(&server, "max", Kind::Computer, "Desktop", false);
    let b = browser.me();
    told_among(&mut [&mut desktop, &mut browser], 0, "three devices", |s| {
        s.devices.len() == 3
    });
    // …and so does the browser, before it calls on what it holds.
    told_among(
        &mut [&mut browser, &mut desktop],
        0,
        "three devices, to the browser",
        |s| s.devices.len() == 3,
    );
    let d = desktop.me();
    assert!(
        !told_among(&mut [&mut desktop, &mut browser], 0, "the desktop", |_| {
            true
        })
        .device(&d)
        .unwrap()
        .can_play
    );

    call(
        &mut browser,
        "start",
        input::start(&queue(&["a", "b"]), 0, 10_000, "album/Water Music"),
    );
    told_among(
        &mut [&mut desktop, &mut browser],
        0,
        "the browser playing",
        |s| s.outputs(&b) && s.sounding(),
    );

    // The desktop moves the sound to the kitchen, and pauses it there.
    fake.playing(KITCHEN, "playing", &media_url("music/a.mp3"), 10.0);
    call(&mut desktop, "transfer", input::transfer(KITCHEN));
    told_among(
        &mut [&mut desktop, &mut browser],
        0,
        "the kitchen playing",
        |s| s.outputs(KITCHEN) && !s.moving_to(KITCHEN) && s.sounding(),
    );
    fake.take();
    call(&mut desktop, "pause", input::none());
    told_among(
        &mut [&mut browser, &mut desktop],
        0,
        "paused, the browser hears",
        |s| s.outputs(KITCHEN) && !s.sounding(),
    );
    let calls = fake.calls(1);
    assert!(calls.iter().any(|(c, _)| c == "media_pause"), "{calls:#?}");

    // Back to the browser, which says it has it.
    call(&mut desktop, "transfer", input::transfer(&b));
    told_among(
        &mut [&mut browser, &mut desktop],
        0,
        "the browser connecting",
        |s| s.moving_to(&b),
    );
    call(&mut browser, "started", input::at(11_000));
    told_among(
        &mut [&mut desktop, &mut browser],
        0,
        "the browser has it",
        |s| s.outputs(&b) && !s.moving_to(&b),
    );
    call(&mut desktop, "play", input::none());
    told_among(
        &mut [&mut browser, &mut desktop],
        0,
        "playing on the browser",
        |s| s.outputs(&b) && s.sounding(),
    );

    // The browser's socket drops: the sound stays with it, away and paused.
    browser.disconnect();
    let s = told_among(
        &mut [&mut desktop, &mut browser],
        0,
        "the browser away",
        |s| matches!(s.output(), Some(Output::Away { .. })),
    );
    assert_eq!(s.output().and_then(Output::id), Some(b.as_str()));
    assert!(
        matches!(s.output(), Some(Output::Away { name, .. }) if name == "Browser"),
        "named, for the picker"
    );
    assert!(!s.sounding());
    assert!(s.device(&b).is_none());

    // Meanwhile the desktop plays the kitchen: a press that means a sound on
    // a device that cannot make one goes to… nobody here, so it picks the
    // kitchen first.
    fake.playing(KITCHEN, "playing", &media_url("music/a.mp3"), 12.0);
    call(&mut desktop, "transfer", input::transfer(KITCHEN));
    call(&mut desktop, "play", input::none());
    told_among(
        &mut [&mut desktop, &mut browser],
        0,
        "the kitchen sounding",
        |s| s.outputs(KITCHEN) && !s.moving_to(KITCHEN) && s.sounding(),
    );

    // The browser comes back while the kitchen is playing: it takes nothing.
    browser.reconnect();
    let s = told_among(
        &mut [&mut browser, &mut desktop],
        0,
        "the browser back, a controller",
        |s| s.device(&b).is_some(),
    );
    assert!(
        s.outputs(KITCHEN) && s.sounding(),
        "the kitchen plays on: {s:?}"
    );

    // …and away again; the kitchen is stopped; and the browser, which went
    // on playing on its own, comes back to nothing sounding and offers what
    // it was doing.
    browser.disconnect();
    told_among(
        &mut [&mut desktop, &mut browser],
        0,
        "the browser gone",
        |s| s.device(&b).is_none(),
    );
    call(&mut desktop, "pause", input::none());
    told_among(
        &mut [&mut desktop, &mut browser],
        0,
        "nothing sounding",
        |s| !s.sounding(),
    );
    browser.reconnect();
    let s = told_among(&mut [&mut browser, &mut desktop], 0, "back", |s| {
        s.device(&b).is_some()
    });
    assert!(
        !s.sounding(),
        "nothing is sounding, so the browser's own sound becomes the session"
    );
    call(
        &mut browser,
        "start",
        input::start(&queue(&["x", "y"]), 1, 95_000, ""),
    );
    let s = told_among(
        &mut [&mut desktop, &mut browser],
        0,
        "the browser's queue, on the browser",
        |s| s.outputs(&b) && s.sounding(),
    );
    let a = s.session.unwrap();
    assert_eq!((a.queue, a.at), (queue(&["x", "y"]), 1));
    assert!(matches!(a.clock, Clock::Playing { at_ms: 95_000, .. }));
    rt.block_on(server.stop());
}

/// Volume and groups the whole way round, over real sockets, with a client,
/// the bridge and two stand-in Sonos: each speaker says what it can do and
/// where its volume is; the phone joins the study to the kitchen it is
/// playing on, and the house is asked to join them; the phone moves the
/// group's volume, and each speaker is told its own share of it; somebody
/// turns the study's knob, and the phone hears it; the Sonos app groups and
/// ungroups by hand, and the session follows; and the phone unjoins the
/// study, and the house is asked to let it go.
///
/// Falsified by a bridge whose `tune` was never called: the speakers stood
/// in the session with no volume at all.
#[test]
fn a_group_of_speakers_and_its_volume_over_real_sockets() {
    let rt = runtime();
    let fake = Fake::default();
    fake.sonos(KITCHEN, 0.3, &[KITCHEN]);
    fake.sonos(STUDY, 0.6, &[STUDY]);
    let url = fake.serve(&rt);
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |c| c.house = Some(two_sonos(url)));

    let mut phone = device(&server, "max", Kind::Phone, "Pixel", true);
    let s = told(&mut phone, "two speakers that say what they can do", |s| {
        [KITCHEN, STUDY].iter().all(|e| {
            s.device(e)
                .is_some_and(|d| d.can_volume && d.family.is_some())
        })
    });
    let (k, st) = (s.device(KITCHEN).unwrap(), s.device(STUDY).unwrap());
    assert_eq!(
        (k.volume, st.volume),
        (30, 60),
        "the levels the house reports"
    );
    assert_eq!(
        k.family.as_deref(),
        Some("sonos"),
        "the integration, from the template"
    );
    assert!(
        fake.take().is_empty(),
        "standing and saying so tells the house nothing"
    );

    // Playing on the kitchen.
    call(&mut phone, "start", input::start(&queue(&["a"]), 0, 0, ""));
    fake.playing(KITCHEN, "playing", &media_url("music/a.mp3"), 0.5);
    call(&mut phone, "transfer", input::transfer(KITCHEN));
    told(&mut phone, "the kitchen playing", |s| {
        s.outputs(KITCHEN) && !s.moving_to(KITCHEN)
    });
    fake.take();

    // The study joins it.
    call(&mut phone, "join", input::member(STUDY));
    told(&mut phone, "the study in the kitchen's group", |s| {
        s.grouped()
    });
    let calls = fake.calls(1);
    assert_eq!(
        calls,
        [(
            "join".to_string(),
            json!({ "entity_id": KITCHEN, "group_members": [STUDY] })
        )]
    );
    assert_eq!(
        fake.attribute(STUDY, "group_members"),
        json!([KITCHEN, STUDY])
    );

    // The group's volume: 30 and 60 are a group at 45, so 90 is 60 and 120,
    // the second clamped — each told to its own speaker.
    call(&mut phone, "set_group_volume", input::set_group_volume(90));
    let s = told(&mut phone, "the group at 90", |s| {
        s.group_volume(KITCHEN) == Some(80)
    });
    assert_eq!(
        (
            s.device(KITCHEN).unwrap().volume,
            s.device(STUDY).unwrap().volume
        ),
        (60, 100)
    );
    let mut calls = fake.calls(2);
    calls.sort_by(|a, b| a.1["entity_id"].as_str().cmp(&b.1["entity_id"].as_str()));
    assert_eq!(
        calls,
        [
            (
                "volume_set".to_string(),
                json!({ "entity_id": KITCHEN, "volume_level": 0.6 })
            ),
            (
                "volume_set".to_string(),
                json!({ "entity_id": STUDY, "volume_level": 1.0 })
            ),
        ]
    );
    // Quiet past the house catching up: nothing comes back as a hand.
    std::thread::sleep(Duration::from_millis(300));
    let s = told(&mut phone, "still 60 and 100", |_| true);
    assert_eq!(
        (
            s.device(KITCHEN).unwrap().volume,
            s.device(STUDY).unwrap().volume
        ),
        (60, 100)
    );
    assert!(
        fake.take().is_empty(),
        "and the house is told nothing twice"
    );

    // A hand on the study's knob.
    fake.set(STUDY, "volume_level", json!(0.25));
    fake.set(STUDY, "is_volume_muted", json!(true));
    let s = told(&mut phone, "the study turned down by hand", |s| {
        s.device(STUDY).is_some_and(|d| d.volume == 25 && d.muted)
    });
    assert_eq!(s.device(KITCHEN).unwrap().volume, 60);
    assert!(
        fake.take().is_empty(),
        "a hand is reported, not argued with"
    );

    // The Sonos app takes the study out, then puts it back.
    fake.set(KITCHEN, "group_members", json!([KITCHEN]));
    fake.set(STUDY, "group_members", json!([STUDY]));
    told(&mut phone, "the study ungrouped by hand", |s| !s.grouped());
    fake.set(KITCHEN, "group_members", json!([KITCHEN, STUDY]));
    fake.set(STUDY, "group_members", json!([KITCHEN, STUDY]));
    told(&mut phone, "the study grouped by hand", |s| s.grouped());
    assert!(fake.take().is_empty());

    // The phone unjoins it.
    call(&mut phone, "unjoin", input::member(STUDY));
    told(&mut phone, "the study alone", |s| !s.grouped());
    let calls = fake.calls(1);
    assert_eq!(
        calls,
        [("unjoin".to_string(), json!({ "entity_id": STUDY }))]
    );
    assert_eq!(fake.attribute(STUDY, "group_members"), json!([STUDY]));
    assert_eq!(
        *fake.unauthorised.lock().unwrap(),
        0,
        "every call carried the token"
    );
    rt.block_on(server.stop());
}

/// The bug as somebody heard it: a track played on the speaker, playing
/// fine, while every bar's position ran to six seconds and went back to
/// nothing, over and over. The house says a playing speaker's position as it
/// was at the last change of state, with its date — as this stand-in now
/// does — and the bridge read it as now, so once the session had counted
/// three seconds past it the speaker was "somewhere else in the track" and
/// the session was sought back to 0.
///
/// Here the speaker plays on for five seconds — the old bug fired at about
/// three — and the session's position runs on with it, unsought.
///
/// Falsified by the old raw read of `media_position`: "the session was
/// sought back: 1504 ms, five seconds in".
#[test]
fn a_speaker_playing_on_is_never_sought_back_by_its_own_stale_position() {
    let rt = runtime();
    let fake = Fake::default();
    let url = fake.serve(&rt);
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |c| c.house = Some(house(url)));

    let mut phone = device(&server, "max", Kind::Phone, "Pixel", true);
    told(&mut phone, "the kitchen beside the phone", |s| {
        s.devices.len() == 2
    });
    call(&mut phone, "start", input::start(&queue(&["a"]), 0, 0, ""));
    fake.playing(KITCHEN, "playing", &media_url("music/a.mp3"), 0.0);
    call(&mut phone, "transfer", input::transfer(KITCHEN));
    told(&mut phone, "the kitchen playing", |s| {
        s.outputs(KITCHEN) && !s.moving_to(KITCHEN) && s.sounding()
    });
    fake.take();
    let started = std::time::Instant::now();
    let mut lowest_after = i64::MAX;
    while started.elapsed() < Duration::from_secs(5) {
        phone.pump();
        if let Some(s) = phone.listening(SPEAKERS) {
            let a = s.session.as_ref().unwrap();
            let at = a.clock.position(now());
            if started.elapsed() > Duration::from_millis(4_500) {
                lowest_after = lowest_after.min(at);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        lowest_after >= 4_000,
        "the session was sought back: {lowest_after} ms, five seconds in"
    );
    let calls = fake.take();
    assert!(
        !calls.iter().any(|(c, _)| c == "media_seek"),
        "nothing was sought on the speaker after the hand-off either: {calls:#?}"
    );
    rt.block_on(server.stop());
}

/// What a speaker gives back for a URL it was handed, as it spells it: the
/// identity, for a speaker that says exactly what it was given.
fn as_given(url: &str) -> String {
    url.to_string()
}

/// …and as a Sonos might spell the same address back: a space, a
/// parenthesis and an apostrophe decoded, an ampersand decoded and then
/// escaped for the XML it travelled in, every other escape in lower-case
/// hex, and the scheme and host in capitals. Every one of these is the same
/// address to the server that serves it.
fn respelt(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap();
    let (host, path) = rest.split_at(rest.find('/').unwrap());
    let mut out = format!("{}://{}", scheme.to_uppercase(), host.to_uppercase());
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let b = u8::from_str_radix(&path[i + 1..i + 3], 16).unwrap();
            match b {
                b' ' | b'(' | b')' | b'\'' => out.push(b as char),
                b'&' => out.push_str("&amp;"),
                _ => out.push_str(&format!("%{b:02x}")),
            }
            i += 3;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// A browser starts three tracks, hands them to the kitchen, and the
/// kitchen plays through all three by itself — the way the bug was
/// reported, "it stops at the first song" — with the session following it
/// track by track on every device, and nothing let go, paused or handed
/// again. The kitchen here keeps a queue of its own as a Sonos does
/// ([`Fake::keeps_a_queue`]), moves to its next item when a track runs out,
/// says it is playing nothing for a moment between the two, and reports
/// what it plays as `spell` spells it.
fn a_speaker_plays_through(spell: fn(&str) -> String) {
    let rt = runtime();
    let fake = Fake::default();
    fake.keeps_a_queue(KITCHEN, spell);
    let url = fake.serve(&rt);
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |c| c.house = Some(house(url)));

    let mut browser = device(&server, "max", Kind::Computer, "Browser", true);
    let mut desktop = device(&server, "max", Kind::Computer, "Desktop", false);
    let b = browser.me();
    for who in 0..2 {
        told_among(
            &mut [&mut browser, &mut desktop],
            who,
            "three devices",
            |s| s.devices.len() == 3,
        );
    }
    // Names a real library has, each spelt three ways by somebody.
    let names = ["Air (BWV 1068)", "Ballade & Polonaise", "Café d'Été"];
    call(
        &mut browser,
        "start",
        input::start(&queue(&names), 0, 0, "album/Mixed"),
    );
    told_among(
        &mut [&mut browser, &mut desktop],
        0,
        "the browser playing",
        |s| s.outputs(&b) && s.sounding(),
    );
    call(&mut browser, "transfer", input::transfer(KITCHEN));
    for at in 0..3 {
        if at > 0 {
            // Two polls of nothing, which is fewer than the three a speaker
            // that was playing ours is allowed.
            fake.track_ends(KITCHEN, Duration::from_millis(80));
        }
        for who in 0..2 {
            told_among(
                &mut [&mut browser, &mut desktop],
                who,
                &format!("the kitchen playing track {at}"),
                |s| {
                    s.outputs(KITCHEN)
                        && !s.moving_to(KITCHEN)
                        && s.sounding()
                        && s.session.as_ref().is_some_and(|a| a.at == at)
                },
            );
        }
    }
    let files: Vec<String> = queue(&names)
        .iter()
        .map(|t| harken_domain::listening::url(MEDIA, &t.file))
        .collect();
    assert_eq!(fake.queued(KITCHEN), files, "the speaker's queue is ours");
    let calls = fake.take();
    let named = |n: &str| calls.iter().filter(|(c, _)| c == n).count();
    assert_eq!(named("clear_playlist"), 1, "handed once: {calls:#?}");
    assert_eq!(named("media_pause"), 0, "never paused: {calls:#?}");
    let s = held(&server, "max").unwrap();
    assert!(s.outputs(KITCHEN) && s.sounding(), "never let go: {s:#?}");
    rt.block_on(server.stop());
}

/// A speaker that says back exactly the URL it was handed.
#[test]
fn a_speaker_plays_through_the_queue_it_was_handed() {
    a_speaker_plays_through(as_given);
}

/// …and one that spells the same addresses its own way. The bridge compared
/// what the speaker reported with what it was handed byte for byte, so an
/// address spelt the speaker's way was "not ours": a few polls of it and
/// the session let the speaker go — `release`, paused, `nowhere` — on the
/// first track, every bar stuck at the first song while the speaker played
/// on by itself. (A first track whose name has nothing to spell otherwise
/// starts, and the same happens at the first one that has.)
///
/// Falsified by comparing them byte for byte again: "never told the kitchen
/// playing track 0", the session let go of it.
#[test]
fn a_speaker_plays_through_the_queue_however_it_spells_it() {
    a_speaker_plays_through(respelt);
}

/// Where `p`'s session stands now, by its own clock of the server's, if
/// anything is queued.
fn standing(p: &mut Peer) -> Option<i64> {
    let now = now();
    let s = p.listening(SPEAKERS)?;
    Some(s.session?.position(now))
}

/// The second bug as somebody heard it: playing a track on one device and
/// picking a speaker, the first device played on until the speaker answered,
/// and then the speaker started the track again from the beginning. A
/// hand-off is a pause where the track was and the same point picked up on
/// the device it went to.
///
/// Here a browser is forty seconds into a track and hands it to the kitchen,
/// which drops a seek sent before it has loaded the track ([`LOADING`]), as
/// a Sonos does. From the transfer on the browser is not the output — its
/// reconciler goes quiet on that, which `harken-iced`'s tests hold — and the
/// session stands still where the track was while the kitchen fetches; the
/// kitchen ends up there and not at 0; and on no device does the session's
/// position ever go back. Then the same handed back to the browser, which
/// says it has started where the kitchen was, and the kitchen is paused.
///
/// Falsified by a `transfer` that left the clock as it was (the session
/// stood at where the track had been started, behind where it had got to:
/// "went back 344 ms on device 1"), and by a bridge that said `started` at
/// the first position the kitchen reported ("went back 39857 ms on device
/// 0": the kitchen at the beginning, and every device with it).
#[test]
fn a_hand_off_picks_up_where_the_track_stopped() {
    let rt = runtime();
    let fake = Fake::default();
    fake.keeps_a_queue(KITCHEN, as_given);
    let url = fake.serve(&rt);
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |c| c.house = Some(house(url)));

    let mut browser = device(&server, "max", Kind::Computer, "Browser", true);
    let mut desktop = device(&server, "max", Kind::Computer, "Desktop", false);
    let b = browser.me();
    for who in 0..2 {
        told_among(
            &mut [&mut browser, &mut desktop],
            who,
            "three devices",
            |s| s.devices.len() == 3,
        );
    }
    call(
        &mut browser,
        "start",
        input::start(&queue(&["a", "b"]), 0, 40_000, "album/Mixed"),
    );
    for who in 0..2 {
        told_among(
            &mut [&mut browser, &mut desktop],
            who,
            "the browser forty seconds in",
            |s| s.outputs(&b) && s.sounding(),
        );
    }
    std::thread::sleep(Duration::from_millis(300));
    call(&mut browser, "transfer", input::transfer(KITCHEN));
    let s = browser.listening(SPEAKERS).unwrap();
    assert!(
        !s.outputs(&b) && s.moving_to(KITCHEN),
        "the browser is not the output from the transfer on, before anybody answers"
    );
    let handed = standing(&mut browser).unwrap();
    assert!(
        (40_000..41_000).contains(&handed),
        "handed where it was: {handed}"
    );

    // Until the kitchen has started: every device's position, sampled.
    let mut last = [handed; 2];
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        for (i, p) in [&mut browser, &mut desktop].into_iter().enumerate() {
            p.pump();
            let Some(at) = standing(p) else { continue };
            assert!(
                at + 300 >= last[i],
                "went back {} ms on device {i}",
                last[i] - at
            );
            last[i] = last[i].max(at);
        }
        let settled = [&mut browser, &mut desktop].into_iter().all(|p| {
            p.listening(SPEAKERS)
                .is_some_and(|s| s.outputs(KITCHEN) && !s.moving_to(KITCHEN))
        });
        if settled && std::time::Instant::now() > deadline - Duration::from_secs(8) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the kitchen never started"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let kitchen = fake.at_ms(KITCHEN);
    assert!(
        (handed..handed + 4_000).contains(&kitchen),
        "the kitchen picked up where the track was: {kitchen} ms, handed at {handed}"
    );
    let calls = fake.take();
    let seeks = calls.iter().filter(|(c, _)| c == "media_seek").count();
    assert!(
        seeks >= 2,
        "the first seek was dropped and it was told again: {calls:#?}"
    );

    // …and back to the browser, which starts where the kitchen was.
    call(&mut desktop, "transfer", input::transfer(&b));
    let s = told_among(
        &mut [&mut browser, &mut desktop],
        0,
        "the browser connecting",
        |s| s.moving_to(&b),
    );
    assert!(!s.outputs(KITCHEN));
    let from = standing(&mut browser).unwrap();
    assert!(
        from + 300 >= last[0],
        "the hand-back stands where the kitchen was: {from}"
    );
    call(&mut browser, "started", input::at(from));
    let s = told_among(
        &mut [&mut desktop, &mut browser],
        0,
        "the browser playing",
        |s| s.outputs(&b) && !s.moving_to(&b) && s.sounding(),
    );
    assert!(s.session.unwrap().position(now()) + 300 >= from);
    let calls = fake.calls(1);
    assert!(
        calls.iter().any(|(c, _)| c == "media_pause"),
        "the kitchen is quiet once it is not the output: {calls:#?}"
    );
    rt.block_on(server.stop());
}

/// Where `p` draws the session now, and the speaker's own position, apart.
fn apart(p: &mut Peer, fake: &Fake) -> Option<i64> {
    let at = standing(p)?;
    Some(at - fake.at_ms(KITCHEN))
}

/// The two reports as somebody heard them, after the hand-off stood still:
/// "it stops, then starts playing on the speaker, but the position still
/// isn't moving, until it starts moving, but it's a few seconds behind what
/// the speaker is playing", and "it still resets when I start casting".
///
/// The kitchen here takes the queue the way a Sonos does through Home
/// Assistant ([`Fake::keeps_a_queue_as_a_sonos`]): idle with nothing
/// playing, then buffering still naming the radio it played before, then
/// playing the track with no position, then a position of 0 dated at that
/// moment — and a seek sent before that is dropped. A browser forty seconds
/// into a track hands it over. From the transfer to the end no device ever
/// draws less than where the track was handed, by more than the bound; the
/// hand-off stands still until the kitchen is where the track was, and then
/// moves on from where the kitchen *measurably is*; from then on every
/// device is within the bound of what the kitchen plays. Then the kitchen
/// drifts a second ahead by itself, and every device follows it without the
/// house being told anything. And the trace says each poll.
///
/// Falsified by a bridge that said `started` at the hand-off's position off
/// a playing report the house had not dated (the old `unwrap_or(from_ms)`):
/// the session ran on from forty seconds while the kitchen played from the
/// beginning — "device 0 came out of the hand-off 40321 ms from the kitchen",
/// which is the few seconds behind, and then the reset, as reported. And by
/// a bridge that said nothing of a drift under three seconds (the old
/// `DRIFT_MS` as the only bound): "after the drift, device 0 is -1022 ms from
/// the kitchen".
#[test]
fn a_cast_is_where_the_speaker_is_and_never_the_beginning() {
    let rt = runtime();
    let fake = Fake::default();
    fake.keeps_a_queue_as_a_sonos(KITCHEN);
    // Paused on somebody's radio, which it goes on naming for a moment.
    fake.playing(KITCHEN, "paused", "http://radio.example/stream", 12.0);
    let url = fake.serve(&rt);
    let data = tempfile::tempdir().unwrap();
    let (lines, traced) = std::sync::mpsc::channel();
    let server = serve(&rt, data.path(), |c| {
        c.house = Some(ha::Config {
            trace: ha::Trace::Lines(lines),
            ..house(url)
        })
    });
    let bound = harken_domain::listening::read::SYNC_MS;

    let mut browser = device(&server, "max", Kind::Computer, "Browser", true);
    let mut desktop = device(&server, "max", Kind::Computer, "Desktop", false);
    let b = browser.me();
    for who in 0..2 {
        told_among(
            &mut [&mut browser, &mut desktop],
            who,
            "three devices",
            |s| s.devices.len() == 3,
        );
    }
    call(
        &mut browser,
        "start",
        input::start(&queue(&["a", "b"]), 0, 40_000, "album/Mixed"),
    );
    for who in 0..2 {
        told_among(
            &mut [&mut browser, &mut desktop],
            who,
            "the browser forty seconds in",
            |s| s.outputs(&b) && s.sounding(),
        );
    }
    std::thread::sleep(Duration::from_millis(300));
    call(&mut browser, "transfer", input::transfer(KITCHEN));
    let handed = standing(&mut browser).unwrap();

    // Until it has started: never the beginning, on any device.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let started = loop {
        let mut settled = true;
        for (i, p) in [&mut browser, &mut desktop].into_iter().enumerate() {
            p.pump();
            let Some(at) = standing(p) else { continue };
            assert!(
                at + bound >= handed,
                "device {i} drew {at} ms, handed at {handed}"
            );
            settled &= p
                .listening(SPEAKERS)
                .is_some_and(|s| s.outputs(KITCHEN) && !s.moving_to(KITCHEN));
        }
        if settled {
            break std::time::Instant::now();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the kitchen never started"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    // The moment it ends, it ends where the kitchen is, measured.
    for (i, p) in [&mut browser, &mut desktop].into_iter().enumerate() {
        let off = apart(p, &fake).unwrap();
        assert!(
            off.abs() <= bound,
            "device {i} came out of the hand-off {off} ms from the kitchen"
        );
    }
    let calls = fake.take();
    assert!(
        calls.iter().filter(|(c, _)| c == "media_seek").count() >= 2,
        "the first seek was dropped and it was told again: {calls:#?}"
    );

    // From then on, within the bound of what the kitchen plays.
    let watch = |browser: &mut Peer, desktop: &mut Peer, what: &str, until: Duration| {
        let from = std::time::Instant::now();
        while from.elapsed() < until {
            for (i, p) in [&mut *browser, &mut *desktop].into_iter().enumerate() {
                p.pump();
                let off = apart(p, &fake).unwrap();
                assert!(
                    off.abs() <= bound + 100,
                    "{what}, device {i} is {off} ms from the kitchen"
                );
                assert!(
                    standing(p).unwrap() + bound >= handed,
                    "{what}, device {i} went back"
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    watch(
        &mut browser,
        &mut desktop,
        "playing on",
        Duration::from_secs(2),
    );
    assert!(started.elapsed() > Duration::from_secs(2));

    // A second ahead, by itself — the house dates where it now is.
    fake.take();
    {
        let mut states = fake.states.lock().unwrap();
        let st = states.get_mut(KITCHEN).unwrap();
        let ahead = position_now(st) + 1.0;
        date_position(st, ahead);
    }
    std::thread::sleep(Duration::from_millis(400));
    watch(
        &mut browser,
        &mut desktop,
        "after the drift",
        Duration::from_secs(1),
    );
    let told: Vec<String> = fake.take().into_iter().map(|(c, _)| c).collect();
    assert!(
        told.is_empty(),
        "followed without the house being told anything: {told:?}"
    );

    // And the trace said every poll: what the house said, and what was done.
    let lines: Vec<String> = traced.try_iter().collect();
    assert!(lines
        .iter()
        .all(|l| l.starts_with("harken-server: house: trace: media_player.kitchen: ")));
    let started_line = lines
        .iter()
        .find(|l| l.contains("did started "))
        .unwrap_or_else(|| panic!("no poll said started: {lines:#?}"));
    for said in [
        "state \"playing\"",
        "media_content_id \"http://10.0.0.2:8787/media/music/a.mp3\"",
        "media_position 4",
        "media_position_updated_at \"",
        "Date ",
        "speaker ",
        "session ",
    ] {
        assert!(started_line.contains(said), "{said} in {started_line}");
    }
    assert!(
        lines.iter().any(|l| l.contains("did report_position ")),
        "the drift: {lines:#?}"
    );
    assert!(lines.iter().any(|l| l.contains("state \"buffering\"")));
    rt.block_on(server.stop());
}

/// The house with one Sonos that keeps a queue, and a silent speaker let go
/// after `idle` where nobody is listening.
fn quick_to_idle(url: String, idle: Duration) -> ha::Config {
    ha::Config { idle, ..house(url) }
}

/// The third thing the user asked for: the music in the kitchen is the
/// kitchen's, not the browser's that sent it there. A browser hands three
/// tracks to the kitchen and is closed; the kitchen goes on standing in the
/// session with nobody listening, plays on through the queue — the session
/// following it track by track, said by the bridge as the kitchen's own
/// `advance` — and is never paused, well past the `idle` that would let a
/// silent one go; and a browser opened later finds the session on the third
/// track, playing, from where the kitchen is.
///
/// Falsified by a bridge that stood only where a client is (the old rule):
/// the kitchen left with the browser, away and paused, and its `advance`
/// was never said — "never on track 1".
#[test]
fn a_speaker_plays_on_through_the_queue_with_nobody_listening() {
    let rt = runtime();
    let fake = Fake::default();
    fake.keeps_a_queue(KITCHEN, as_given);
    let url = fake.serve(&rt);
    let data = tempfile::tempdir().unwrap();
    let idle = Duration::from_millis(300);
    let server = serve(&rt, data.path(), |c| {
        c.house = Some(quick_to_idle(url, idle))
    });

    let mut browser = device(&server, "max", Kind::Computer, "Browser", true);
    told(&mut browser, "the kitchen beside the browser", |s| {
        s.devices.len() == 2
    });
    call(
        &mut browser,
        "start",
        input::start(&queue(&["a", "b", "c"]), 0, 0, "album/Mixed"),
    );
    call(&mut browser, "transfer", input::transfer(KITCHEN));
    told(&mut browser, "the kitchen playing", |s| {
        s.outputs(KITCHEN) && !s.moving_to(KITCHEN) && s.sounding()
    });
    drop(browser);
    let alone = eventually(10, || {
        held(&server, "max").filter(|s| s.devices.iter().all(|d| d.id == KITCHEN))
    })
    .expect("the browser left");
    assert!(
        alone.device(KITCHEN).is_some(),
        "the kitchen goes on standing: {alone:#?}"
    );
    // Well past `idle`, and still playing: a speaker making the sound is not
    // a silent one.
    std::thread::sleep(idle * 3);
    for at in 1..3 {
        fake.track_ends(KITCHEN, Duration::ZERO);
        let s = eventually(10, || {
            held(&server, "max").filter(|s| s.session.as_ref().is_some_and(|a| a.at == at))
        })
        .unwrap_or_else(|| panic!("never on track {at}: {:#?}", held(&server, "max")));
        assert!(
            s.outputs(KITCHEN) && s.sounding(),
            "playing on, on the kitchen: {s:#?}"
        );
    }
    let calls = fake.take();
    assert!(
        !calls.iter().any(|(c, _)| c == "media_pause"),
        "never paused: {calls:#?}"
    );
    // Somebody opens a browser again.
    std::thread::sleep(Duration::from_millis(500));
    let mut later = device(&server, "max", Kind::Computer, "Browser", true);
    let s = told(&mut later, "the session as the kitchen has it", |s| {
        s.device(KITCHEN).is_some()
    });
    let a = s.session.as_ref().unwrap();
    assert_eq!(a.at, 2, "on the third track");
    assert!(s.outputs(KITCHEN) && s.sounding());
    let at = a.position(now());
    let kitchen = fake.at_ms(KITCHEN);
    assert!(
        (at - kitchen).abs() < 1_500,
        "where the kitchen is: {at} against {kitchen}"
    );
    rt.block_on(server.stop());
}

/// …and a kitchen left paused with nobody listening stands only for `idle`:
/// then it leaves the session — kept, paused, with the sound away on the
/// kitchen, as any session is when its output goes — and the house is asked
/// nothing more about it.
///
/// Falsified by a bridge that never let a silent speaker go: "the kitchen
/// never went".
#[test]
fn a_speaker_left_paused_with_nobody_listening_goes_after_a_while() {
    let rt = runtime();
    let fake = Fake::default();
    fake.keeps_a_queue(KITCHEN, as_given);
    let url = fake.serve(&rt);
    let data = tempfile::tempdir().unwrap();
    let idle = Duration::from_millis(1_200);
    let server = serve(&rt, data.path(), |c| {
        c.house = Some(quick_to_idle(url, idle))
    });

    let mut browser = device(&server, "max", Kind::Computer, "Browser", true);
    told(&mut browser, "the kitchen beside the browser", |s| {
        s.devices.len() == 2
    });
    call(
        &mut browser,
        "start",
        input::start(&queue(&["a", "b"]), 0, 0, ""),
    );
    call(&mut browser, "transfer", input::transfer(KITCHEN));
    told(&mut browser, "the kitchen playing", |s| {
        s.outputs(KITCHEN) && !s.moving_to(KITCHEN) && s.sounding()
    });
    call(&mut browser, "pause", input::none());
    told(&mut browser, "paused", |s| !s.sounding());
    drop(browser);
    let gone_at = std::time::Instant::now();
    eventually(10, || {
        held(&server, "max").filter(|s| s.devices.iter().all(|d| d.id == KITCHEN))
    })
    .expect("the browser left");
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        held(&server, "max").unwrap().device(KITCHEN).is_some(),
        "still standing a moment after the browser went"
    );
    let left = eventually(10, || held(&server, "max").filter(|s| s.devices.is_empty()))
        .unwrap_or_else(|| panic!("the kitchen never went: {:#?}", held(&server, "max")));
    assert!(gone_at.elapsed() >= idle, "not before `idle`");
    assert!(
        matches!(left.output(), Some(Output::Away { id, .. }) if id == KITCHEN) && !left.sounding(),
        "kept, paused, the sound away on the kitchen: {left:#?}"
    );
    std::thread::sleep(Duration::from_millis(200));
    let reads = fake.reads();
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(fake.reads(), reads, "and asked nothing more");
    rt.block_on(server.stop());
}

/// A house nobody is listening in, and in which nothing plays, is asked
/// nothing: not before anybody has signed in, and not once the only client
/// has gone while the sound was its own.
///
/// Falsified by polling every configured speaker whether or not it stands:
/// the house was asked about the kitchen thirteen times before anybody had
/// signed in.
#[test]
fn an_idle_house_is_asked_nothing() {
    let rt = runtime();
    let fake = Fake::default();
    fake.sonos(KITCHEN, 0.3, &[KITCHEN]);
    let url = fake.serve(&rt);
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |c| c.house = Some(house(url)));
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(fake.reads(), 0, "nobody has signed in");

    let mut browser = device(&server, "max", Kind::Computer, "Browser", true);
    told(&mut browser, "the kitchen beside the browser", |s| {
        s.devices.len() == 2
    });
    assert!(
        eventually(5, || (fake.reads() > 0).then_some(())).is_some(),
        "a speaker standing beside somebody is asked about"
    );
    let b = browser.me();
    call(
        &mut browser,
        "start",
        input::start(&queue(&["a"]), 0, 0, ""),
    );
    told(&mut browser, "the browser playing", |s| s.outputs(&b));
    drop(browser);
    eventually(10, || held(&server, "max").filter(|s| s.devices.is_empty()))
        .expect("the kitchen went with the browser, since the sound was the browser's");
    std::thread::sleep(Duration::from_millis(200));
    let reads = fake.reads();
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(fake.reads(), reads, "and the house is asked nothing more");
    assert!(
        fake.take().iter().all(|(c, _)| c != "media_pause"),
        "nor told anything"
    );
    rt.block_on(server.stop());
}

/// The session's queue edited while the kitchen plays it: a track put next
/// and one added at the end reach the kitchen's own queue in place — `play_media`
/// with `enqueue: next` and `enqueue: add` — without a fresh hand-off or a
/// gap; a track taken out is a fresh hand-off from where the track playing
/// is, picked up there; and the kitchen then plays on through the queue as
/// the session now has it.
///
/// Falsified by an `enqueued` that read only a pure append: the track put
/// next was a fresh hand-off, the kitchen's queue cleared ("handed once").
#[test]
fn the_queue_edited_reaches_the_speaker_without_a_gap() {
    let rt = runtime();
    let fake = Fake::default();
    fake.keeps_a_queue(KITCHEN, as_given);
    let url = fake.serve(&rt);
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |c| c.house = Some(house(url)));

    let mut browser = device(&server, "max", Kind::Computer, "Browser", true);
    told(&mut browser, "the kitchen beside the browser", |s| {
        s.devices.len() == 2
    });
    call(
        &mut browser,
        "start",
        input::start(&queue(&["a", "b"]), 0, 0, ""),
    );
    call(&mut browser, "transfer", input::transfer(KITCHEN));
    told(&mut browser, "the kitchen playing", |s| {
        s.outputs(KITCHEN) && !s.moving_to(KITCHEN) && s.sounding()
    });
    let url_of = |t: &str| harken_domain::listening::url(MEDIA, &format!("music/{t}.mp3"));
    call(
        &mut browser,
        "enqueue",
        input::enqueue(&[track("n", 7)], true),
    );
    call(
        &mut browser,
        "enqueue",
        input::enqueue(&[track("z", 9)], false),
    );
    let want = [url_of("a"), url_of("n"), url_of("b"), url_of("z")];
    assert!(
        eventually(10, || (fake.queued(KITCHEN) == want).then_some(())).is_some(),
        "the kitchen's queue is the session's: {:?}",
        fake.queued(KITCHEN)
    );
    let calls = fake.take();
    let named = |n: &str| calls.iter().filter(|(c, _)| c == n).count();
    assert_eq!(named("clear_playlist"), 1, "handed once: {calls:#?}");
    assert!(calls
        .iter()
        .any(|(c, b)| c == "play_media" && b["enqueue"] == json!("next")));
    // `b` taken out while `a` plays: handed again from `a`, where it is.
    call(&mut browser, "remove", input::remove(2));
    let want = [url_of("a"), url_of("n"), url_of("z")];
    assert!(
        eventually(10, || (fake.queued(KITCHEN) == want).then_some(())).is_some(),
        "{:?}",
        fake.queued(KITCHEN)
    );
    // A track does not end a moment after it was handed: the bridge hears
    // the kitchen start it first.
    std::thread::sleep(Duration::from_millis(400));
    for (at, t) in [(1, "n"), (2, "z")] {
        fake.track_ends(KITCHEN, Duration::ZERO);
        let s = told(&mut browser, &format!("the kitchen on {t}"), |s| {
            s.session.as_ref().is_some_and(|a| a.at == at)
        });
        assert_eq!(s.session.unwrap().now().title, t);
    }
    assert!(!fake.take().iter().any(|(c, _)| c == "media_pause"));
    rt.block_on(server.stop());
}
