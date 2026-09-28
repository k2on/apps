//! A speaker reaches the picker the way it does in production: through the
//! whole server, a room the desk announced, a peer the server stands in for,
//! and a Home Assistant that answers over HTTP.
//!
//! `tests/listening.rs` holds the desk's rules against a bare hub; the
//! bridge's own rules are unit tests beside it. This is the plumbing above
//! both, which neither can see and which is where a device goes missing
//! without any rule being wrong — a `Watch` nobody receives, a `Hello` that
//! never entered a room, a frame addressed to a peer nobody holds, a service
//! call with the wrong body.
//!
//! The Home Assistant here is a stand-in, written from its REST API's
//! documentation: `POST /api/services/media_player/<service>` with a JSON
//! body, `GET /api/states/<entity>`, a bearer token on both. What a real one
//! and a real Sonos make of these calls is not verified here.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ark_client::Peer;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use common::*;
use harken_domain::listening::{Command, Hear, Kind, Say, Session, Track};
use harken_server::assistant::ha;
use serde_json::{json, Value as Json_};

/// Every service call the house was asked for, and what each player says
/// it is doing.
#[derive(Clone, Default)]
struct Fake {
    calls: Arc<Mutex<Vec<(String, Json_)>>>,
    states: Arc<Mutex<BTreeMap<String, Json_>>>,
    unauthorised: Arc<Mutex<usize>>,
}

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
    fake.calls.lock().unwrap().push((service, body));
    StatusCode::OK
}

async fn state(
    State(fake): State<Fake>,
    Path(entity): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Json_>, StatusCode> {
    if !authorised(&fake, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    fake.states
        .lock()
        .unwrap()
        .get(&entity)
        .cloned()
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

impl Fake {
    fn serve(&self, rt: &tokio::runtime::Runtime) -> String {
        let router = Router::new()
            .route("/api/services/media_player/:service", post(service))
            .route("/api/states/:entity", get(state))
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

    /// Wait for the house to have been asked for `n` calls.
    fn calls(&self, n: usize) -> Vec<(String, Json_)> {
        eventually(5, || (self.calls.lock().unwrap().len() >= n).then_some(()));
        std::thread::sleep(Duration::from_millis(100));
        self.take()
    }

    fn playing(&self, entity: &str, state: &str, url: &str, position_s: f64) {
        self.states.lock().unwrap().insert(
            entity.into(),
            json!({
                "entity_id": entity,
                "state": state,
                "attributes": { "media_content_id": url, "media_position": position_s },
            }),
        );
    }
}

const KITCHEN: &str = "media_player.kitchen";
const MEDIA: &str = "http://10.0.0.2:8787";

fn house(url: String) -> ha::Config {
    ha::Config {
        url,
        token: TOKEN.into(),
        players: vec![(KITCHEN.into(), "Kitchen".into())],
        media: MEDIA.into(),
        tick: Duration::from_millis(40),
    }
}

/// Say something, and send it: a frame goes out on the next pump.
fn say(p: &mut Peer, s: Say) {
    p.say(s.encode());
    p.pump();
}

/// Pump until this peer has been told a session that satisfies `want`.
fn told(p: &mut Peer, what: &str, mut want: impl FnMut(&Session) -> bool) -> Session {
    let mut last = None;
    let mut found = None;
    pump_until(&mut [p], 10, |ps| {
        for f in ps[0].heard() {
            if let Ok(Hear::State { session }) = Hear::decode(&f) {
                if want(&session) {
                    found = Some(session.clone());
                }
                last = Some(session);
            }
        }
        found.is_some()
    });
    found.unwrap_or_else(|| panic!("never told {what}; last {last:?}"))
}

fn track(t: &str) -> Track {
    Track {
        id: t.into(),
        title: t.into(),
        creator: "Bach".into(),
        album: String::new(),
        duration_ms: 300_000,
        file: format!("music/{t}.mp3"),
    }
}

fn here(p: &mut Peer, name: &str) {
    say(
        p,
        Say::Here {
            name: name.into(),
            audible: true,
            kind: Kind::Phone,
        },
    );
}

/// The whole way round: a phone arrives and the kitchen stands beside it;
/// the phone hands the kitchen the sound, and the house is asked to play the
/// whole queue from where it was; the kitchen says where it is and the phone
/// hears it; a pause on the phone is a pause in the kitchen; taking the sound
/// back pauses the kitchen; and when the phone goes, so does the kitchen, and
/// the room closes rather than being kept open by a speaker.
#[test]
fn a_speaker_is_a_device_the_whole_way_round() {
    let rt = runtime();
    let fake = Fake::default();
    let url = fake.serve(&rt);
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |c| c.house = Some(house(url)));
    let hub = server.running.hub.clone();

    let mut phone = signed_in(&server, "max");
    let me = phone.ctx().session.clone();
    pump_until(&mut [&mut phone], 10, |ps| ps[0].linked());
    here(&mut phone, "Pixel");
    let session = told(&mut phone, "the kitchen standing beside the phone", |s| {
        s.devices.len() == 2
    });
    let kitchen = session
        .device(KITCHEN)
        .expect("the speaker is in the picker");
    assert_eq!(
        (kitchen.name.as_str(), kitchen.kind, kitchen.here),
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
    say(
        &mut phone,
        Say::Do {
            command: Command::Start {
                queue: vec![track("a"), track("b"), track("c")],
                at: 1,
                position_ms: 0,
                playing: true,
            },
        },
    );
    told(&mut phone, "the phone as the output", |s| s.outputs(&me));
    say(
        &mut phone,
        Say::Report {
            queue: vec![track("a"), track("b"), track("c")],
            at: 1,
            playing: true,
            position_ms: 42_500,
        },
    );
    told(&mut phone, "where the phone got to", |s| {
        s.position_ms == 42_500
    });
    say(
        &mut phone,
        Say::Transfer {
            to: Some(KITCHEN.into()),
        },
    );
    let session = told(&mut phone, "the kitchen, connecting", |s| {
        s.outputs(KITCHEN)
    });
    assert_eq!(session.moving.as_deref(), Some(KITCHEN));

    let calls = fake.calls(4);
    let names: Vec<&str> = calls.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(
        names,
        ["clear_playlist", "play_media", "play_media", "media_seek"],
        "{calls:#?}"
    );
    assert_eq!(calls[0].1, json!({ "entity_id": KITCHEN }));
    assert_eq!(
        calls[1].1,
        json!({
            "entity_id": KITCHEN,
            "media_content_id": format!("{MEDIA}/media/music/b.mp3"),
            "media_content_type": "music",
            "enqueue": "play",
        }),
        "from the track it was on, at the address a *speaker* can reach"
    );
    assert_eq!(
        calls[2].1["media_content_id"],
        json!(format!("{MEDIA}/media/music/c.mp3"))
    );
    assert_eq!(
        calls[2].1["enqueue"],
        json!("add"),
        "the rest after it, so the speaker's own next works"
    );
    assert_eq!(
        calls[3].1["seek_position"],
        json!(42.5),
        "and from where it had got to"
    );

    // The kitchen starts, and says so: the phone hears where it is.
    fake.playing(
        KITCHEN,
        "playing",
        &format!("{MEDIA}/media/music/c.mp3"),
        3.0,
    );
    let session = told(&mut phone, "the kitchen playing the third track", |s| {
        s.at == 2 && s.moving.is_none()
    });
    assert!(session.playing);
    assert_eq!(session.position_ms, 3_000);

    // A pause on the phone is a pause in the kitchen.
    say(
        &mut phone,
        Say::Do {
            command: Command::Pause,
        },
    );
    let calls = fake.calls(1);
    assert_eq!(
        calls,
        [("media_pause".to_string(), json!({ "entity_id": KITCHEN }))]
    );

    // Taking the sound back: the kitchen is told by the broadcast, and a
    // speaker has nobody to go quiet for it, so the bridge pauses it.
    say(
        &mut phone,
        Say::Transfer {
            to: Some(me.clone()),
        },
    );
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

    // The phone goes. The kitchen is taken out with it, so the room closes
    // and is written down, rather than being held open by a speaker.
    drop(phone);
    let closed = eventually(10, || {
        let rooms = hub.read_blocking(|h| h.rooms()).unwrap();
        rooms.is_empty().then_some(())
    });
    assert!(
        closed.is_some(),
        "a standing speaker kept the room open: {:?}",
        hub.read_blocking(|h| h.rooms())
    );
    let kept = hub
        .read_blocking(|h| h.kept("max").map(<[u8]>::to_vec))
        .unwrap()
        .expect("the room is kept");
    let woken = harken_server::listening::woken(&kept).unwrap();
    assert_eq!(woken.output.as_deref(), Some(me.as_str()));
    rt.block_on(server.stop());
}

/// A speaker is one piece of hardware and a room is one account, so two
/// people can both pick the kitchen: the second gets it, and the first is
/// told — by the speaker, in their room — rather than left drawing a
/// transport for somebody else's music. And a speaker that goes on playing
/// something that is not ours is let go the same way, after its grace.
#[test]
fn a_speaker_taken_or_sent_elsewhere_is_let_go_out_loud() {
    let rt = runtime();
    let fake = Fake::default();
    let url = fake.serve(&rt);
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |c| c.house = Some(house(url)));

    let mut alice = signed_in(&server, "alice");
    let mut bob = signed_in(&server, "bob");
    pump_until(&mut [&mut alice, &mut bob], 10, |ps| {
        ps.iter().all(|p| p.linked())
    });
    here(&mut alice, "Alice's phone");
    here(&mut bob, "Bob's phone");
    told(&mut alice, "the kitchen in alice's room", |s| {
        s.device(KITCHEN).is_some()
    });
    told(&mut bob, "the kitchen in bob's room", |s| {
        s.device(KITCHEN).is_some()
    });

    // Pressing a track, and then — as the output, which is what pressing it
    // made the phone — saying what it is playing, the way a client does.
    let start = |p: &mut Peer, t: &str| {
        let me = p.ctx().session.clone();
        say(
            p,
            Say::Do {
                command: Command::Start {
                    queue: vec![track(t)],
                    at: 0,
                    position_ms: 0,
                    playing: true,
                },
            },
        );
        told(p, "the phone as the output", |s| s.outputs(&me));
        say(
            p,
            Say::Report {
                queue: vec![track(t)],
                at: 0,
                playing: true,
                position_ms: 0,
            },
        );
        told(p, "the phone playing", |s| s.queue.len() == 1);
    };
    start(&mut alice, "a");
    // A quick speaker: playing what it is handed by the first poll.
    fake.playing(
        KITCHEN,
        "playing",
        &format!("{MEDIA}/media/music/a.mp3"),
        1.0,
    );
    say(
        &mut alice,
        Say::Transfer {
            to: Some(KITCHEN.into()),
        },
    );
    told(&mut alice, "alice's kitchen", |s| s.outputs(KITCHEN));
    assert_eq!(
        fake.calls(2).len(),
        2,
        "the queue cleared and alice's track put on"
    );
    told(&mut alice, "the kitchen playing alice's", |s| {
        s.moving.is_none() && s.outputs(KITCHEN)
    });

    start(&mut bob, "b");
    fake.playing(
        KITCHEN,
        "playing",
        &format!("{MEDIA}/media/music/b.mp3"),
        1.0,
    );
    say(
        &mut bob,
        Say::Transfer {
            to: Some(KITCHEN.into()),
        },
    );
    told(&mut bob, "bob's kitchen", |s| s.outputs(KITCHEN));
    let session = told(&mut alice, "alice let go of the kitchen", |s| {
        s.output.is_none()
    });
    assert!(!session.playing);
    let calls = fake.calls(2);
    assert_eq!(calls.len(), 2, "{calls:#?}");
    assert_eq!(
        calls[1].1["media_content_id"],
        json!(format!("{MEDIA}/media/music/b.mp3")),
        "{calls:#?}"
    );
    told(&mut bob, "the kitchen playing bob's", |s| {
        s.moving.is_none() && s.outputs(KITCHEN)
    });

    // Somebody puts the radio on in the kitchen. Once the grace is spent,
    // bob's room is told the kitchen is not his any more.
    fake.playing(KITCHEN, "playing", "http://radio.example/stream", 0.0);
    let session = told(&mut bob, "bob let go of the kitchen", |s| {
        s.output.is_none()
    });
    assert!(!session.playing);
    rt.block_on(server.stop());
}

/// The bug the debug screen found. A library longer than one batch has the
/// client send a *second* `Hello` on the same socket to ask for the rest —
/// and a `Hello` used to be a departure and an arrival, so the desk dropped
/// the browser from its own picker, saw nobody listening, sent the speakers
/// away, and answered every `State` after that with no devices at all.
#[test]
fn asking_for_the_next_batch_does_not_leave_the_room() {
    let rt = runtime();
    let fake = Fake::default();
    let url = fake.serve(&rt);
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |c| c.house = Some(house(url)));
    let mut browser = signed_in(&server, "max");
    pump_until(&mut [&mut browser], 10, |ps| ps[0].linked());
    here(&mut browser, "Browser");
    told(&mut browser, "the browser and the kitchen", |s| {
        s.devices.len() == 2
    });

    // The next batch, please: another Hello on the same connection.
    browser.connected();
    let mut heard = vec![];
    let deadline = std::time::Instant::now() + Duration::from_millis(600);
    while std::time::Instant::now() < deadline {
        browser.pump();
        heard.extend(
            browser
                .heard()
                .into_iter()
                .filter_map(|f| Hear::decode(&f).ok()),
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    for h in &heard {
        if let Hear::State { session } = h {
            assert_eq!(
                session.devices.len(),
                2,
                "a resumed connection is still in the room: {session:?}"
            );
        }
    }
    let rooms = server.running.hub.read_blocking(|h| h.rooms()).unwrap();
    assert_eq!(
        rooms["max"].len(),
        2,
        "the browser and the kitchen, each once"
    );
    here(&mut browser, "Browser");
    let session = told(&mut browser, "the room", |_| true);
    let mut ids: Vec<&str> = session.devices.iter().map(|d| d.id.as_str()).collect();
    ids.sort_unstable();
    let me = browser.ctx().session.clone();
    let mut want = vec![me.as_str(), KITCHEN];
    want.sort_unstable();
    assert_eq!(ids, want);
    rt.block_on(server.stop());
}
