//! The hub as a server: clients over the in-process transport and over a
//! WebSocket converge; a token proving nobody is closed; a revoked session
//! is closed and its link dials again; the log survives a restart; a disk
//! that refuses holds every delivery until it writes; the journal keeps a
//! refusal across a restart; a web build is served with its build as the
//! validator.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ark::codec::bytes_enc;
use ark::frames::{Frame, Verdict};
use ark::log::{Entry, EntryId, Origin};
use ark::peer::Who;
use ark::schema::Space;
use ark_client::link::{Backoff, Dial, Event, Link, Recv, Socket};
use ark_client::storage::Stored;
use ark_client::{Client, Config, Disk, Files, Memory};
use ark_dev::demo::{domain, module};
use ark_server::handle::now;
use ark_server::{builder, dev, Handle, Hub};

fn config() -> ark_server::Config {
    ark_server::Config::new(module(&domain()))
}

fn client(name: &str) -> Client {
    let who = dev(name).unwrap();
    Client::open(
        Config {
            log: module(&domain()),
            live: None,
        },
        who,
        Box::new(Memory::default()),
        false,
    )
}

fn fast() -> Backoff {
    Backoff {
        first: Duration::from_millis(5),
        most: Duration::from_millis(20),
    }
}

/// Pumps a client until `f` holds, or panics after two seconds.
fn until(c: &mut Client, link: &Link, what: &str, f: impl Fn(&Client, &[Event]) -> bool) {
    for _ in 0..400 {
        let ev = link.wait(Duration::from_millis(5));
        let hit = f(c, &ev);
        c.pump(link, ev, now());
        if hit || f(c, &[]) {
            return;
        }
    }
    panic!("never: {what}");
}

fn create(c: &mut Client, link: &Link, name: &str) {
    c.mutate(domain().get("demo.create_playlist"), bytes_enc(name.as_bytes()), now());
    c.pump(link, vec![], now());
}

fn two_clients(a: Box<dyn Dial>, b: Box<dyn Dial>) {
    let (mut alice, mut bob) = (client("alice@laptop"), client("bob@phone"));
    let (la, lb) = (Link::start(a, "alice@laptop".into(), fast()), Link::start(b, "bob@phone".into(), fast()));
    until(&mut alice, &la, "alice linked", |c, _| c.linked);
    until(&mut bob, &lb, "bob linked", |c, _| c.linked);
    create(&mut alice, &la, "Road");
    until(&mut bob, &lb, "bob has alice's playlist", |c, _| c.log.log.head().0 == 1);
    until(&mut alice, &la, "alice confirmed", |c, _| c.log.pending.is_empty());
    assert_eq!(alice.log.store, bob.log.store);
    assert_eq!(bob.log.log.entries[0].entry.actor, "alice");
}

/// Two clients, each on its own link to the hub in this process, converge.
/// Falsified by `InProcess` never forwarding the client's frames: alice's
/// playlist never reached bob.
#[test]
fn in_process_clients_converge() {
    let hub = Handle::spawn(Hub::open(config(), Box::new(Memory::default()), 0));
    two_clients(Box::new(hub.dial(Arc::new(dev))), Box::new(hub.dial(Arc::new(dev))));
}

/// A linked round trip through the hub in this process waits on nothing:
/// a playlist created and pumped until confirmed, 100 times, is within
/// 0.5 ms a round trip (median) of the same over the `Conn` with no link
/// thread. Falsified by the link's thread sending only between 5 ms waits
/// for a frame, as before: the linked median was 5.6 ms against 0.9.
#[test]
fn a_linked_round_trip_waits_on_nothing() {
    let hub = Handle::spawn(Hub::open(config(), Box::new(Memory::default()), 0));
    let median = |mut ts: Vec<Duration>| {
        ts.sort();
        ts[ts.len() / 2]
    };
    let mut c = client("alice@laptop");
    let link = Link::start(Box::new(hub.dial(Arc::new(dev))), "alice@laptop".into(), fast());
    until(&mut c, &link, "linked", |c, _| c.linked);
    let linked: Vec<Duration> = (0..100)
        .map(|i| {
            let t = std::time::Instant::now();
            create(&mut c, &link, &format!("L{i}"));
            while !c.log.pending.is_empty() {
                let ev = link.wait(Duration::from_millis(1));
                c.pump(&link, ev, now());
            }
            t.elapsed()
        })
        .collect();
    let mut d = client("bob@phone");
    let mut conn = hub.dial(Arc::new(dev)).dial("bob@phone").unwrap();
    d.connected();
    let direct: Vec<Duration> = (0..100)
        .map(|i| {
            let t = std::time::Instant::now();
            d.mutate(domain().get("demo.create_playlist"), bytes_enc(format!("D{i}").as_bytes()), now());
            loop {
                for f in d.take_frames() {
                    conn.send(f);
                }
                if d.log.pending.is_empty() {
                    break;
                }
                if let Recv::Frame(b) = conn.recv(Duration::from_millis(1)) {
                    d.receive(b);
                }
            }
            t.elapsed()
        })
        .collect();
    let (linked, direct) = (median(linked), median(direct));
    assert!(
        linked <= direct + Duration::from_micros(500),
        "linked {linked:?}, no link thread {direct:?}"
    );
}

/// The same over WebSockets, the token the first message; and an empty
/// token, which proves nobody, is closed, dialled again and closed again,
/// and sent nothing. Falsified by the socket taking its first message as a
/// frame: nobody was signed in and no client converged.
#[tokio::test(flavor = "multi_thread")]
async fn socket_clients_converge_and_nobody_is_closed() {
    let running = builder(config()).build().unwrap().serve("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/sync", running.addr);
    let u = url.clone();
    tokio::task::spawn_blocking(move || {
        two_clients(Box::new(Socket(u.clone())), Box::new(Socket(u.clone())));
        let mut nobody = client("x@y");
        let l = Link::start(Box::new(Socket(u)), String::new(), fast());
        let mut closed = 0;
        for _ in 0..40 {
            let ev = l.wait(Duration::from_millis(5));
            assert!(!ev.iter().any(|e| matches!(e, Event::Frame(_))), "nobody is sent nothing");
            closed += ev.iter().filter(|e| matches!(e, Event::Closed(_))).count();
            nobody.pump(&l, ev, now());
        }
        assert!(closed > 0, "and is closed");
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
    let hub = Handle::spawn(Hub::open(config(), Box::new(Memory::default()), 0));
    let (mut alice, mut bob) = (client("alice@laptop"), client("bob@phone"));
    let la = Link::start(Box::new(hub.dial(Arc::new(dev))), "alice@laptop".into(), fast());
    let lb = Link::start(Box::new(hub.dial(Arc::new(dev))), "bob@phone".into(), fast());
    until(&mut alice, &la, "alice linked", |c, _| c.linked);
    until(&mut bob, &lb, "bob linked", |c, _| c.linked);
    hub.revoke("alice@laptop");
    until(&mut alice, &la, "alice closed", |_, ev| ev.iter().any(|e| matches!(e, Event::Closed(_))));
    until(&mut alice, &la, "alice open again", |_, ev| ev.contains(&Event::Open));
    create(&mut bob, &lb, "Mix");
    until(&mut alice, &la, "alice hears bob", |c, _| c.log.log.head().0 == 1);
    assert!(lb.events().iter().all(|e| !matches!(e, Event::Closed(_))), "bob stayed");
}

/// The log on disk: a hub stopped and opened again from its directory holds
/// the chain it had, and its journal; the authority's own `local` goes on.
/// Falsified by `Hub::open` not loading its disk: the reopened hub's log
/// was empty.
#[test]
fn the_log_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mut hub = Hub::open(config(), Box::new(Files::new(dir.path()).unwrap()), 0);
    for name in ["a", "b"] {
        hub.author(Space::Durable, domain().get("demo.create_playlist"), bytes_enc(name.as_bytes()), 1)
            .0
            .unwrap();
    }
    let (log, store) = (hub.log.log.clone(), hub.log.store.clone());
    drop(hub);
    let mut hub = Hub::open(config(), Box::new(Files::new(dir.path()).unwrap()), 0);
    assert_eq!((&hub.log.log, &hub.log.store), (&log, &store));
    hub.author(Space::Durable, domain().get("demo.create_playlist"), bytes_enc(b"c"), 1)
        .0
        .unwrap();
    assert_eq!(hub.log.log.entries[2].entry.local, 3, "its own count goes on");
}

/// A disk that refuses whenever `fail` is set.
struct Flaky(Memory, Arc<AtomicBool>);

impl Disk for Flaky {
    fn load(&mut self) -> io::Result<Stored> {
        self.0.load()
    }
    fn record(&mut self, b: &[u8]) -> io::Result<()> {
        if self.1.load(Ordering::Relaxed) {
            return Err(io::Error::other("the disk is full"));
        }
        self.0.record(b)
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
        func: domain().get("demo.create_playlist"),
        input: bytes_enc(name.as_bytes()),
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
    let mut hub = Hub::open(config(), Box::new(Flaky(Memory::default(), fail.clone())), 0);
    let alice = dev("alice@laptop").unwrap();
    let c = hub.connect(alice.clone());
    assert!(hub.frame(c, push(&alice, 1, "Road"), 1).is_empty());
    assert!(hub.failure.as_deref().is_some_and(|f| f.contains("the disk is full")));
    fail.store(false, Ordering::Relaxed);
    let out = hub.flushed();
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
    let mut hub = Hub::open(config(), Box::new(Memory::default()), 0);
    let c = hub.connect(alice.clone());
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
    let mut hub = Hub::open(config(), disk, 0);
    let c = hub.connect(alice.clone());
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
    use ark_server::web::build_tag;
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
    let running = builder(config()).auth(auth.clone()).build().unwrap().serve("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/sync", running.addr);
    let (token, session) = (login.token.clone(), login.session.clone());
    let hub = running.hub.clone();
    tokio::task::spawn_blocking(move || {
        let who = ark_client::auth::who(&login);
        let mut c = Client::open(
            Config {
                log: module(&domain()),
                live: None,
            },
            who,
            Box::new(Memory::default()),
            false,
        );
        let l = Link::start(Box::new(Socket(url)), token.clone(), fast());
        until(&mut c, &l, "carol linked", |c, _| c.linked);
        create(&mut c, &l, "Carol's");
        until(&mut c, &l, "confirmed", |c, _| c.log.log.head().0 == 1);
        assert_eq!(c.log.log.entries[0].entry.session, session);
        assert_eq!(hub.with(|h| h.log.log.entries[0].entry.actor.clone()).unwrap(), "carol");
        auth.revoke(&token).unwrap();
        until(&mut c, &l, "closed", |_, ev| ev.iter().any(|e| matches!(e, Event::Closed(_))));
    })
    .await
    .unwrap();
    running.stop().await;
}

/// The live space across a restart: what the hub held when it closed is
/// what it opens with, and `on_restore` is authored over it — the room
/// keeps who was here and lets every pointer go. Falsified by `Hub::open`
/// not reading what `close` wrote: the room came back empty.
#[test]
fn the_live_space_is_restored_and_on_restore_authored() {
    use ark_dev::demo::{arrival, room};
    let r = room();
    let mut config = config();
    config.live = Some(ark_server::Live {
        module: module(&r),
        on_join: Some(r.get("room.on_join")),
        on_leave: Some(r.get("room.on_leave")),
        on_restore: Some(r.get("room.on_restore")),
        device: Arc::new(arrival),
        keep: 8,
    });
    let mut hub = Hub::open(config.clone(), Box::new(Memory::default()), 0);
    let alice = dev("alice@phone").unwrap();
    let c = hub.connect(alice.clone());
    let hello = Frame::Hello {
        space: Space::Volatile,
        cursor: 0,
        hash: None,
        device: Some(b"phone".to_vec()),
    };
    hub.frame(c, hello.enc(), 1);
    hub.author(Space::Volatile, r.get("room.point"), ark::codec::int_enc(7).to_vec(), 2)
        .0
        .unwrap();
    let live = |h: &Hub| h.live.as_ref().unwrap().store.snapshot_rows();
    assert_eq!(
        live(&hub).iter().map(|(_, rs)| rs.len()).collect::<Vec<_>>(),
        vec![1, 1],
        "here, and a pointer"
    );
    hub.close();
    let disk = std::mem::replace(&mut hub.disk, Box::new(Memory::default()));
    let hub = Hub::open(config, disk, 3);
    assert_eq!(
        live(&hub).iter().map(|(_, rs)| rs.len()).collect::<Vec<_>>(),
        vec![1, 0],
        "here kept, the pointer let go"
    );
    let l = hub.live.as_ref().unwrap();
    assert_eq!(l.log.head().0, l.log.base.seq + 1, "on_restore, the one entry since the open");
}
