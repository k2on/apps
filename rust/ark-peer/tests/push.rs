//! The hub wakes the devices not connected when the app's rows change,
//! through a fake `PushSender`: a device writes its endpoint as its
//! session's push row over its own connection; a change wakes the devices
//! whose sessions are not connected, once, with nothing of the rows in what
//! is sent; a connected device is not pushed to; a gone endpoint's row is
//! taken away.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ark::format::Encode;
use ark::prelude::*;
use ark::schema::Space;
use ark_auth::server::{dev_account, Auth, Mode};
use ark_auth::{Push, Role, SetPush, SignIn};
use ark_peer::link::{bell, wait};
use ark_peer::{Config, Handle, Memory, Peer, Server};
use ark_push::fake::Fake;
use ark_push::{Endpoint, Wake};
use arkdb::demo::{CreatePlaylist, Playlist};

#[derive(Schema)]
pub struct App {
    pub playlist: Playlist,
    pub sign_in: SignIn,
    pub role: Role,
    pub push: Push,
}

ark_auth::install!(App);

#[mutator("app.create_playlist")]
fn create_playlist(ctx: &Ctx, db: &mut Db<App>, input: CreatePlaylist) -> Result<(), Refusal> {
    db.playlist.insert(Playlist {
        id: ctx.new_id(),
        name: input.name,
        user_id: ctx.user().into(),
    })
}

fn config() -> Config {
    Config::of(Arc::new(ark::register!(
        Domain::new::<App>("app", "1").with(ark_auth::register::<App>); create_playlist)))
}

fn apns(token: &str) -> Endpoint {
    Endpoint::Apns {
        token: token.into(),
        topic: "us.koon.app".into(),
        sandbox: false,
    }
}

/// A device of `name`'s, signed in and linked to `hub`.
fn device(hub: &Handle, auth: &Arc<Auth>, name: &str) -> (Peer, std::sync::mpsc::Receiver<()>) {
    let login = auth.issue(&dev_account(name), "a phone").unwrap();
    let who = auth.authenticate()(&login.token).unwrap();
    let mut d = Peer::open(config(), who, Box::new(Memory::default()), false);
    let (wake, rx) = bell();
    d.set_waker(wake);
    d.set_token(Some(login.token));
    d.connect_with("", hub.dialer(auth.authenticate()));
    (d, rx)
}

/// `func` authored by `d` and confirmed by the hub.
fn author(d: &mut Peer, rx: &std::sync::mpsc::Receiver<()>, func: &str, input: Vec<u8>) {
    d.author(Space::Durable, func, input, 1).unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        d.pump();
        if d.linked && d.log.pending.is_empty() {
            return;
        }
        wait(d, rx, Duration::from_millis(5));
    }
    panic!("{func} was not confirmed");
}

/// What the fake was sent, waited for: the hub sends from a thread of its own.
fn sent(fake: &Fake, n: usize) -> Vec<ark_push::fake::Sent> {
    let mut got = vec![];
    let until = Instant::now() + Duration::from_secs(5);
    while got.len() < n && Instant::now() < until {
        got.extend(fake.take());
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(100));
    got.extend(fake.take());
    got
}

fn pushes(hub: &Handle) -> Vec<Push> {
    hub.with(|p| ark::db::read::<App, _>(&p.log.store, |db| Ok::<_, Refusal>(db.push.scan())).unwrap())
        .unwrap()
}

/// Falsified by not skipping connected sessions in `to_wake`: the laptop,
/// online, was woken too; and by putting the app's rows into the payload:
/// the playlist's name is in what was sent.
#[test]
fn a_change_wakes_the_devices_not_connected_and_says_nothing_of_it() {
    let fake = Fake::new();
    let auth = Arc::new(Auth::new::<App>(Mode::Dev, "http://localhost", b"key").with_ttl(86_400_000));
    let server = Server::new(config())
        .auth(auth.clone())
        .push(Arc::new(fake.clone()), Wake::of("app").shown("App", "New activity in App"));
    let (hub, _router) = server.build().unwrap();

    // The phone hands its endpoint over its own connection, and goes away.
    let (mut phone, prx) = device(&hub, &auth, "alice");
    let set = SetPush {
        endpoint: apns("aa01").to_json(),
    }
    .to_bytes();
    author(&mut phone, &prx, ark::auth::PUSH, set);
    let phone_session = phone.me().session.clone();
    drop(phone);
    // The laptop hands its own and stays.
    let (mut laptop, lrx) = device(&hub, &auth, "alice");
    author(
        &mut laptop,
        &lrx,
        ark::auth::PUSH,
        SetPush {
            endpoint: apns("bb02").to_json(),
        }
        .to_bytes(),
    );
    assert_eq!(pushes(&hub).len(), 2, "each device's endpoint is a row");
    assert!(sent(&fake, 0).is_empty(), "who is signed in wakes nobody");

    // A change of the app's rows: the phone is woken, once, the laptop not.
    let secret = "Groceries at Trader Joe's";
    author(
        &mut laptop,
        &lrx,
        "app.create_playlist",
        CreatePlaylist { name: secret.into() }.to_bytes(),
    );
    author(
        &mut laptop,
        &lrx,
        "app.create_playlist",
        CreatePlaylist { name: "Another".into() }.to_bytes(),
    );
    let got = sent(&fake, 1);
    assert_eq!(got.len(), 1, "one wake for a burst, to the phone alone: {got:?}");
    assert_eq!(got[0].to, apns("aa01"));
    let payload = String::from_utf8(got[0].payload.clone()).unwrap();
    assert!(
        !payload.contains("Groceries") && !payload.contains("Another"),
        "nothing of the rows: {payload}"
    );
    assert!(!payload.contains("alice") && !payload.contains(&phone_session), "nor who: {payload}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&payload).unwrap()["ark"],
        serde_json::json!({"wake": "app"})
    );
    hub.stop();
}

/// An endpoint the push service says is gone has its row taken away.
#[test]
fn a_gone_endpoint_is_taken_away() {
    let fake = Fake::new();
    fake.gone(apns("dead"));
    let auth = Arc::new(Auth::new::<App>(Mode::Dev, "http://localhost", b"key").with_ttl(86_400_000));
    let (hub, _router) = Server::new(config())
        .auth(auth.clone())
        .push(Arc::new(fake.clone()), Wake::of("app"))
        .build()
        .unwrap();
    let (mut tablet, trx) = device(&hub, &auth, "bob");
    author(
        &mut tablet,
        &trx,
        ark::auth::PUSH,
        SetPush {
            endpoint: apns("dead").to_json(),
        }
        .to_bytes(),
    );
    drop(tablet);
    let (mut other, orx) = device(&hub, &auth, "carol");
    author(&mut other, &orx, "app.create_playlist", CreatePlaylist { name: "x".into() }.to_bytes());
    assert_eq!(sent(&fake, 1).len(), 1);
    let until = Instant::now() + Duration::from_secs(5);
    while !pushes(&hub).is_empty() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(pushes(&hub).is_empty(), "the gone endpoint's row is taken away");
    hub.stop();
}
