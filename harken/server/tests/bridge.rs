//! The listening session over real sockets, the way production runs it:
//! the whole server, clients that sign in and declare themselves devices,
//! the session as the domain's live state on `/sync`, a speaker the Home
//! Assistant bridge stands in for, and a Home Assistant that answers over
//! HTTP.
//!
//! The session's rules are transition tests in `harken/domain/tests/
//! listening.rs`, and the bridge's are unit tests beside it. This is the
//! plumbing above both, which neither can see and which is where a device
//! goes missing without any rule being wrong — a participant that never
//! stood, a frame addressed to a connection nobody holds, a session read
//! before it moved, a service call with the wrong body.
//!
//! The Home Assistant here is a stand-in, written from its REST API's
//! documentation: `POST /api/services/media_player/<service>` with a JSON
//! body, `GET /api/states/<entity>`, a bearer token on both. What a real one
//! and a real Sonos make of these calls is not verified here.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ark::live::state::DeviceDecl;
use ark_auth::Account;
use ark_client::{Options, Peer};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use common::*;
use harken_domain::listening::read::{input, Clock, Kind, Listening, Output, Track, STATE};
use harken_server::assistant::ha;
use harken_server::Server;
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

    /// Wait for the house to have been asked for `n` calls, then a little
    /// longer for any it should not have been asked for.
    fn calls(&self, n: usize) -> Vec<(String, Json_)> {
        eventually(5, || (self.calls.lock().unwrap().len() >= n).then_some(()));
        std::thread::sleep(Duration::from_millis(150));
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

/// `user` signed in on this server as a device: what a browser, a desktop
/// or a phone declares at `hello`.
fn device(server: &Server, user: &str, kind: Kind, name: &str, can_play: bool) -> Peer {
    let login = server
        .auth
        .issue(&Account {
            id: user.into(),
            ..Account::default()
        })
        .unwrap();
    let opts = Options::server(login.user.id, login.session, Some(login.token))
        .with_timing(quick())
        .with_device(DeviceDecl::new("", kind.name(), name, can_play));
    let mut p = Peer::open_memory(server.domain.clone(), opts).unwrap();
    p.connect(&server.running.sync_url());
    p
}

/// This peer's device id: its login.
fn me(p: &Peer) -> String {
    p.ctx().session.clone()
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
        let settled = p.linked()
            && p.live_status(STATE)
                .is_some_and(|s| s.synced && s.pending == 0);
        if let Some(l) = p.live(STATE).and_then(|v| Listening::from_value(&v)) {
            if settled && want(&l) {
                return l;
            }
            last = Some(l);
        }
        assert!(
            std::time::Instant::now() < deadline,
            "never told {what}; last {last:#?}; {:?}; refused {:?}",
            p.live_status(STATE),
            p.take_live_refusals()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn call(p: &mut Peer, transition: &str, i: ark::eval::Args) {
    if let Err(e) = p.call(STATE, transition, i) {
        let view = p.live(STATE).and_then(|v| Listening::from_value(&v));
        panic!(
            "{transition}: {e}; the view was {view:#?}, {:?}",
            p.live_status(STATE)
        );
    }
    p.pump();
}

fn track(t: &str, n: u8) -> Track {
    let mut id = [0u8; 16];
    id[15] = n;
    Track {
        id,
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
    let user = user.to_string();
    server
        .running
        .hub
        .read_blocking(move |h| {
            h.live()
                .instances
                .iter()
                .find(|(i, _)| i.state == STATE && i.key == user)
                .and_then(|(_, held)| Listening::from_value(&held.value))
        })
        .unwrap()
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
/// phone went.
#[test]
fn a_speaker_is_a_device_the_whole_way_round() {
    let rt = runtime();
    let fake = Fake::default();
    let url = fake.serve(&rt);
    let data = tempfile::tempdir().unwrap();
    let server = serve(&rt, data.path(), |c| c.house = Some(house(url)));

    let mut phone = device(&server, "max", Kind::Phone, "Pixel", true);
    let me = me(&phone);
    let session = told(&mut phone, "the kitchen standing beside the phone", |s| {
        s.devices.len() == 2
    });
    let kitchen = session
        .device(KITCHEN)
        .expect("the speaker is in the picker");
    assert_eq!(
        (kitchen.name.as_str(), kitchen.kind, kitchen.can_play),
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
        ["clear_playlist", "play_media", "play_media", "media_seek"],
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
    assert_eq!(
        calls[2].1["media_content_id"],
        json!(media_url("music/c.mp3"))
    );
    assert_eq!(
        calls[2].1["enqueue"],
        json!("add"),
        "the rest after it, so the speaker's own next works"
    );
    let at = calls[3].1["seek_position"].as_f64().unwrap();
    assert!(
        (42.5..44.5).contains(&at),
        "and from where it had got to, counted on: {at}"
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

    // A pause on the phone is a pause in the kitchen. (The stand-in house
    // does not act on what it is told, so it is told to be paused first, as
    // a real speaker would be a moment after.)
    std::thread::sleep(Duration::from_millis(200));
    fake.take();
    fake.playing(KITCHEN, "paused", &media_url("music/c.mp3"), 1.2);
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
/// took the speaker: alice's session still named the kitchen.
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
        let me = me(p);
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
    // A library of two batches, authored by the library's own account.
    let login = server
        .auth
        .issue(&Account {
            id: "library".into(),
            roles: vec![harken_domain::schema::LIBRARY.into()],
            ..Account::default()
        })
        .unwrap();
    let opts =
        Options::server(login.user.id, login.session, Some(login.token)).with_timing(quick());
    let mut library = Peer::open_memory(server.domain.clone(), opts).unwrap();
    library.set_roles([harken_domain::schema::LIBRARY]);
    library.connect(&server.running.sync_url());
    for i in 0..(ark::protocol::BATCH_LIMIT + 44) {
        let song = |t: &str| {
            ark::eval::Args::from_iter(
                [
                    ("title", ark::value::Value::text(t)),
                    ("artist", ark::value::Value::text("The Fleet")),
                    ("album", ark::value::Value::text("Loopback")),
                    ("duration_ms", ark::value::Value::int(1000)),
                    ("file", ark::value::Value::text(format!("typed/{t}.wav"))),
                    ("track", ark::value::Value::int(0)),
                    ("part", ark::value::Value::text("")),
                    ("catalogue", ark::value::Value::text("")),
                    ("performer", ark::value::Value::text("")),
                    ("bpm", ark::value::Value::int(0)),
                    ("album_art", ark::value::Value::text("")),
                    ("artist_art", ark::value::Value::text("")),
                    ("disc", ark::value::Value::int(0)),
                    ("work_title", ark::value::Value::text("")),
                    ("movement_no", ark::value::Value::int(0)),
                ]
                .map(|(k, v)| (k.to_string(), v)),
            )
        };
        library
            .mutate("add_song", song(&format!("song-{i:03}")))
            .unwrap();
    }
    pump_until(&mut [&mut library], 30, |ps| {
        ps[0].linked() && ps[0].pending_len() == 0
    });

    let mut browser = device(&server, "max", Kind::Computer, "Browser", true);
    told(&mut browser, "the browser and the kitchen", |s| {
        s.devices.len() == 2
    });
    let seq = browser.live_status(STATE).unwrap().seq;
    pump_until(&mut [&mut browser], 30, |ps| {
        ps[0].cursor() as usize >= ark::protocol::BATCH_LIMIT + 44
    });
    let session = told(&mut browser, "the session", |_| true);
    let mut ids: Vec<&str> = session.devices.iter().map(|d| d.id.as_str()).collect();
    ids.sort_unstable();
    let me = me(&browser);
    let mut want = vec![me.as_str(), KITCHEN];
    want.sort_unstable();
    assert_eq!(ids, want);
    assert_eq!(
        browser.live_status(STATE).unwrap().seq,
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
    let b = me(&browser);
    told_among(&mut [&mut desktop, &mut browser], 0, "three devices", |s| {
        s.devices.len() == 3
    });
    let d = me(&desktop);
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
    fake.playing(KITCHEN, "paused", &media_url("music/a.mp3"), 11.0);
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
    fake.playing(KITCHEN, "paused", &media_url("music/a.mp3"), 13.0);
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
