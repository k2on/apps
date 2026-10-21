//! A peer as a server: devices over the in-process transport and over a
//! WebSocket converge; a token proving nobody is closed; a revoked session
//! is closed and its link dials again; the log survives a restart; a disk
//! that refuses holds every delivery until it writes; the journal keeps a
//! refusal across a restart; a web build is served with its build as the
//! validator.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

use ark::frames::{Frame, Verdict};
use ark::json::Json;
use ark::log::{Entry, EntryId, Origin};
use ark::peer::Who;
use ark::schema::Space;
use ark_peer::link::{bell, socket, wait, Dial, Event, Pumped};
use ark_peer::{dev, now_ms, Config, Disk, Files, Handle, Live, Memory, Module, Peer, Server};

/// `create_playlist`'s input.
fn named(name: &str) -> Vec<u8> {
    ark_dev::demo::CreatePlaylist { name: name.into() }.to_bytes()
}

fn config() -> Config {
    Config::of(Arc::new(ark_dev::demo::demo()))
}

fn peer(who: Who) -> Peer {
    Peer::open(config(), who, Box::new(Memory::default()), false)
}

/// `token`'s peer, dialling `url` through `dial`, and its bell.
fn linked(token: &str, url: &str, dial: impl Dial + 'static) -> (Peer, Receiver<()>) {
    let mut c = peer(dev(token).unwrap_or(dev("x@y").unwrap()));
    let (wake, rx) = bell();
    c.set_waker(wake);
    c.link.backoff = (5, 20);
    c.set_token(Some(token.into()));
    c.connect_with(url, dial);
    (c, rx)
}

/// Pumps a peer until `f` holds, or panics after two seconds.
fn until(c: &mut (Peer, Receiver<()>), what: &str, f: impl Fn(&Peer, &Pumped) -> bool) {
    for _ in 0..400 {
        let p = c.0.pump();
        if f(&c.0, &p) {
            return;
        }
        wait(&c.0, &c.1, Duration::from_millis(5));
    }
    panic!("never: {what}");
}

fn create(c: &mut Peer, name: &str) {
    let _ = c.author(Space::Durable, "demo.create_playlist", named(name), now_ms());
    c.pump();
}

fn two_clients(url: &str, a: impl Dial + 'static, b: impl Dial + 'static) {
    let (mut alice, mut bob) = (linked("alice@laptop", url, a), linked("bob@phone", url, b));
    until(&mut alice, "alice linked", |c, _| c.linked);
    until(&mut bob, "bob linked", |c, _| c.linked);
    create(&mut alice.0, "Road");
    until(&mut bob, "bob has alice's playlist", |c, _| c.log.log().head().0 == 1);
    until(&mut alice, "alice confirmed", |c, _| c.log.pending.is_empty());
    assert_eq!(alice.0.log.store, bob.0.log.store);
    assert_eq!(bob.0.log.log().entries()[0].entry.actor, "alice");
}

fn hub() -> Handle {
    Handle::spawn(Peer::serve(config(), Box::new(Memory::default()), 0))
}

/// Two peers, each dialling the server in this process, converge.
/// Falsified by the in-process wire never forwarding the peer's frames:
/// alice's playlist never reached bob.
#[test]
fn in_process_clients_converge() {
    let hub = hub();
    two_clients("", hub.dialer(Arc::new(dev)), hub.dialer(Arc::new(dev)));
}

/// A linked round trip through the server in this process waits on
/// nothing: a playlist created and pumped until confirmed, 100 times, is
/// within 0.5 ms a round trip (median) of the same over the in-process
/// wire carried by hand. Falsified by the link's thread sending only
/// between 5 ms waits for a frame, as before: the linked median was 5.6 ms
/// against 0.9.
#[test]
fn a_linked_round_trip_waits_on_nothing() {
    let hub = hub();
    let median = |mut ts: Vec<Duration>| {
        ts.sort();
        ts[ts.len() / 2]
    };
    let mut c = linked("alice@laptop", "", hub.dialer(Arc::new(dev)));
    until(&mut c, "linked", |c, _| c.linked);
    let linked: Vec<Duration> = (0..100)
        .map(|i| {
            let t = std::time::Instant::now();
            create(&mut c.0, &format!("L{i}"));
            while !c.0.log.pending.is_empty() {
                wait(&c.0, &c.1, Duration::from_millis(1));
                c.0.pump();
            }
            t.elapsed()
        })
        .collect();
    let mut d = peer(dev("bob@phone").unwrap());
    let mut wire = hub.dialer(Arc::new(dev)).dial("", "bob@phone", None);
    let direct: Vec<Duration> = (0..100)
        .map(|i| {
            let t = std::time::Instant::now();
            let _ = d.author(Space::Durable, "demo.create_playlist", named(&format!("D{i}")), now_ms());
            loop {
                for f in d.take_frames() {
                    wire.send(f);
                }
                if d.log.pending.is_empty() {
                    break;
                }
                for e in wire.events() {
                    match e {
                        Event::Open => d.connected(),
                        Event::Frame(b) => d.receive(b),
                        Event::Closed(_) => {}
                    }
                }
            }
            t.elapsed()
        })
        .collect();
    let (linked, direct) = (median(linked), median(direct));
    assert!(
        linked <= direct + Duration::from_micros(500),
        "linked {linked:?}, carried by hand {direct:?}"
    );
}

/// The same over WebSockets, the token the first message; and an empty
/// token, which proves nobody, is closed with 4401, sent nothing, and not
/// dialled again. Falsified by the socket taking its first message as a
/// frame: nobody was signed in and no client converged; and by the link
/// not reading close code 4401 as a denial: nobody was closed 13 times.
#[tokio::test(flavor = "multi_thread")]
async fn socket_clients_converge_and_nobody_is_closed() {
    let running = Server::new(config()).listen("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/sync", running.addr);
    let u = url.clone();
    tokio::task::spawn_blocking(move || {
        two_clients(&u, socket, socket);
        let mut nobody = linked("", &u, socket);
        let mut closed = 0;
        for _ in 0..40 {
            let p = nobody.0.pump();
            assert_eq!(p.frames, [0, 0], "nobody is sent nothing");
            closed += p.closed.len();
            assert!(p.denied.is_none() || nobody.0.is_signed_out(), "and stops dialling");
            wait(&nobody.0, &nobody.1, Duration::from_millis(5));
        }
        assert_eq!(closed, 1, "and is closed, once");
    })
    .await
    .unwrap();
    running.stop().await;
}

/// A sign-out closes that session's connections and no other; its link
/// dials again and is a replica again. Falsified by `Revoke` disconnecting
/// in the hub but keeping the sink: alice's link never closed.
#[test]
fn a_revoked_session_is_closed_and_dials_again() {
    let hub = hub();
    let mut alice = linked("alice@laptop", "", hub.dialer(Arc::new(dev)));
    let mut bob = linked("bob@phone", "", hub.dialer(Arc::new(dev)));
    until(&mut alice, "alice linked", |c, _| c.linked);
    until(&mut bob, "bob linked", |c, _| c.linked);
    hub.revoke("alice@laptop");
    until(&mut alice, "alice closed", |_, p| !p.closed.is_empty());
    until(&mut alice, "alice open again", |_, p| p.opened);
    create(&mut bob.0, "Mix");
    until(&mut alice, "alice hears bob", |c, _| c.log.log().head().0 == 1);
    assert!(bob.0.pump().closed.is_empty(), "bob stayed");
}

/// The log on disk: a hub stopped and opened again from its directory holds
/// the chain it had, and its journal; the authority's own `local` goes on.
/// Falsified by `Peer::serve` not loading its disk: the reopened hub's log
/// was empty.
#[test]
fn the_log_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mut hub = Peer::serve(config(), Box::new(Files::new(dir.path()).unwrap()), 0);
    for name in ["a", "b"] {
        hub.author(Space::Durable, "demo.create_playlist", named(name), 1).unwrap();
    }
    let store = hub.log.store.clone();
    drop(hub);
    let mut hub = Peer::serve(config(), Box::new(Files::new(dir.path()).unwrap()), 0);
    assert_eq!((hub.log.log(), &hub.log.store), (store.log(), &store));
    hub.author(Space::Durable, "demo.create_playlist", named("c"), 1).unwrap();
    assert_eq!(hub.log.log().entries()[2].entry.local, 3, "its own count goes on");
}

/// A disk that refuses whenever `fail` is set.
struct Flaky(Memory, Arc<AtomicBool>);

impl Disk for Flaky {
    fn append(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
        if self.1.load(Ordering::Relaxed) && n == "log" {
            return Err(io::Error::other("the disk is full"));
        }
        self.0.append(n, b)
    }
    fn put(&mut self, n: &str, b: &[u8]) -> io::Result<()> {
        self.0.put(n, b)
    }
    fn get(&mut self, n: &str) -> io::Result<Option<Vec<u8>>> {
        self.0.get(n)
    }
}

fn push(who: &Who, local: i64, name: &str) -> Vec<u8> {
    let entry = Entry {
        id: EntryId::of(Origin::named(&who.session), local),
        actor: who.actor.clone(),
        session: who.session.clone(),
        origin: Origin::named(&who.session),
        local,
        roles: vec![],
        func: "demo.create_playlist".into(),
        input: named(name),
        now: 1,
    };
    Frame::push(Space::Durable, [entry]).enc()
}

/// An acknowledgement follows the write: while the disk refuses, nothing a
/// push caused is delivered, and once it writes, all of it is, in order.
/// Falsified by `release` delivering whatever the disk said: the ack went
/// out with nothing on the disk.
#[test]
fn nothing_is_delivered_before_it_is_written() {
    let fail = Arc::new(AtomicBool::new(true));
    let mut hub = Peer::serve(config(), Box::new(Flaky(Memory::default(), fail.clone())), 0);
    let alice = dev("alice@laptop").unwrap();
    let c = hub.accept(alice.clone());
    assert!(hub.frame(c, push(&alice, 1, "Road"), 1).is_empty());
    assert!(hub.failure.as_deref().is_some_and(|f| f.contains("the disk is full")));
    fail.store(false, Ordering::Relaxed);
    let out = hub.release();
    let frames: Vec<Frame> = out.iter().map(|(_, b)| Frame::dec(b).unwrap()).collect();
    assert!(
        matches!(
            &frames[..],
            [
                Frame::Page { .. },
                Frame::Verdict {
                    verdict: Verdict::Ok { seq: 1 },
                    ..
                }
            ]
        ),
        "{frames:?}"
    );
    assert!(hub.failure.is_none());
}

/// A refusal is kept beside the log: after a restart the session's next
/// intent is taken, not held for a gap where the refused one was, and the
/// refused one pushed again is answered again. Falsified by the hub not
/// writing its journal: the next push waited for the refused one, which
/// would never come.
#[test]
fn the_journal_keeps_a_refusal_across_a_restart() {
    let alice = dev("alice@laptop").unwrap();
    let mut hub = Peer::serve(config(), Box::new(Memory::default()), 0);
    let c = hub.accept(alice.clone());
    let refused = |out: &[(u64, Vec<u8>)]| {
        out.iter().any(|(_, b)| {
            matches!(
                Frame::dec(b),
                Ok(Frame::Verdict {
                    verdict: Verdict::Refused { .. },
                    ..
                })
            )
        })
    };
    assert!(refused(&hub.frame(c, push(&alice, 1, " "), 1)), "a playlist needs a name");
    let disk = std::mem::replace(&mut hub.disk, Box::new(Memory::default()));
    let mut hub = Peer::serve(config(), disk, 0);
    let c = hub.accept(alice.clone());
    let out = hub.frame(c, push(&alice, 2, "Road"), 1);
    assert!(
        out.iter().any(|(_, b)| matches!(
            Frame::dec(b),
            Ok(Frame::Verdict {
                verdict: Verdict::Ok { seq: 1 },
                ..
            })
        )),
        "taken"
    );
    assert!(refused(&hub.frame(c, push(&alice, 1, " "), 1)), "refused again");
}

/// A web build's tag is the store hash on the store, and off it the
/// module's or every file's stamp, moving when they do — named, only the
/// module's. Falsified by `build_tag` ignoring a named module: a page
/// written beside it moved the tag.
#[test]
fn a_web_build_is_its_own_validator() {
    use ark_peer::web::build_tag;
    let store = std::path::Path::new("/nix/store/6ilgmw07dbmi7y4klpgc9v5k1nmfbs66-app-web-0.1.0");
    assert_eq!(build_tag(store, None), "6ilgmw07dbmi7y4klpgc9v5k1nmfbs66");
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("pkg")).unwrap();
    let m = Some("pkg/app_bg.wasm");
    assert_eq!((build_tag(dir.path(), m), build_tag(dir.path(), None)), ("0".into(), "0".into()));
    std::fs::write(dir.path().join("pkg/app_bg.wasm"), b"one").unwrap();
    let one = build_tag(dir.path(), m);
    std::fs::write(dir.path().join("pkg/app_bg.wasm"), b"another build").unwrap();
    assert_ne!(build_tag(dir.path(), m), one);
    let (named, whole) = (build_tag(dir.path(), m), build_tag(dir.path(), None));
    std::fs::write(dir.path().join("index.html"), b"a page").unwrap();
    assert_eq!(build_tag(dir.path(), m), named, "named, only the module is the build");
    assert_ne!(build_tag(dir.path(), None), whole);
}

/// Signing in through `ark-auth`: a login's token is the session it was
/// issued, and a sign-out closes it. Falsified by the builder reading
/// tokens the dev way whatever `auth` says: the entry's actor was the
/// token itself, not carol.
#[tokio::test(flavor = "multi_thread")]
async fn a_login_is_the_connection_and_a_sign_out_closes_it() {
    use ark_auth::server::{Auth, Mode};
    let auth = Arc::new(Auth::new(ark_auth::session::SessionStore::memory(), Mode::Dev, "http://127.0.0.1"));
    let login = auth
        .issue(&ark_auth::Account {
            id: "carol".into(),
            ..Default::default()
        })
        .unwrap();
    let running = Server::new(config()).auth(auth.clone()).listen("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/sync", running.addr);
    let (token, session) = (login.token.clone(), login.session.clone());
    let hub = running.hub.clone();
    tokio::task::spawn_blocking(move || {
        let (wake, rx) = bell();
        let mut c = (peer(ark_peer::who(&login)), rx);
        c.0.set_waker(wake);
        c.0.link.backoff = (5, 20);
        c.0.set_token(Some(token.clone()));
        c.0.connect(&url);
        until(&mut c, "carol linked", |c, _| c.linked);
        create(&mut c.0, "Carol's");
        until(&mut c, "confirmed", |c, _| c.log.log().head().0 == 1);
        assert_eq!(c.0.log.log().entries()[0].entry.session, session);
        assert_eq!(hub.with(|h| h.log.log().entries()[0].entry.actor.clone()).unwrap(), "carol");
        auth.revoke(&token).unwrap();
        until(&mut c, "closed", |_, p| !p.closed.is_empty());
    })
    .await
    .unwrap();
    running.stop().await;
}

/// The live space across a restart: what the hub held when it closed is
/// what it opens with, and `on_restore` is authored over it — the room
/// keeps who was here and lets every pointer go. Falsified by `Peer::serve`
/// not reading what `close` wrote: the room came back empty.
#[test]
fn the_live_space_is_restored_and_on_restore_authored() {
    use ark_dev::demo::{arrival, room};
    let r = room();
    let mut config = config();
    config.live = Some(Live {
        on_join: Some("room.on_join".into()),
        on_leave: Some("room.on_leave".into()),
        on_restore: Some("room.on_restore".into()),
        arrive: Arc::new(arrival),
        keep: Some(8),
        ..Live::new(Module::of(&r))
    });
    let mut hub = Peer::serve(config.clone(), Box::new(Memory::default()), 0);
    let alice = dev("alice@phone").unwrap();
    let c = hub.accept(alice.clone());
    let hello = Frame::Hello {
        space: Space::Volatile,
        cursor: 0,
        hash: None,
        device: Some(b"phone".to_vec()),
        schema: Default::default(),
        version: String::new(),
    };
    hub.frame(c, hello.enc(), 1);
    hub.author(Space::Volatile, "room.point", ark_dev::demo::Point { x: 7 }.to_bytes(), 2)
        .unwrap();
    let live = |h: &Peer| h.live.as_ref().unwrap().store.snapshot_rows();
    assert_eq!(
        live(&hub).iter().map(|(_, rs)| rs.len()).collect::<Vec<_>>(),
        vec![1, 1],
        "here, and a pointer"
    );
    hub.close();
    let disk = std::mem::replace(&mut hub.disk, Box::new(Memory::default()));
    let hub = Peer::serve(config, disk, 3);
    assert_eq!(
        live(&hub).iter().map(|(_, rs)| rs.len()).collect::<Vec<_>>(),
        vec![1, 0],
        "here kept, the pointer let go"
    );
    let l = hub.live.as_ref().unwrap();
    assert_eq!(l.log().head().0, l.log().base_seq() + 1, "on_restore, the one entry since the open");
}
