//! The whole bridge in one process: devices opened on disk, linked to a
//! server in this process, called by name with JSON — a mutate and its
//! refusal, a check, a query, a view's patches, the live space's call — and
//! a device alone that joins; each driven by its own thread, as on a phone.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ark_dev::demo::{arrival, demo, live, room, Playlist};
use ark_ffi::{Listener, Login, Patch, Peer, Refusal, Standing, ViewUpdate};
use ark_peer::{dev, Config, Handle, Live, Memory, Module};

fn config() -> Config {
    Config::of(Arc::new(demo()))
}

fn server() -> Handle {
    let mut c = config();
    c.live = Some(Live {
        on_join: Some("room.on_join".into()),
        on_leave: Some("room.on_leave".into()),
        arrive: Arc::new(arrival),
        ..Live::new(Module::of(&room()))
    });
    Handle::spawn(ark_peer::Peer::serve(c, Box::new(Memory::default()), 0))
}

/// The login the dev server reads `token` as.
fn login(token: &str) -> Login {
    let w = dev(token).expect("a dev login");
    Login {
        token: token.into(),
        session: w.session,
        user: w.actor,
        name: String::new(),
        email: String::new(),
        roles: w.roles,
        expires_ms: 0,
    }
}

/// A device on a disk of its own, saying `name` in the live space.
fn device(dir: &tempfile::TempDir, name: &str, who: Option<Login>) -> Arc<Peer> {
    let c = Config {
        live: Some(live(name, 10)),
        ..config()
    };
    Peer::open(c, dir.path().to_str().unwrap(), who).expect("opened")
}

fn until(what: &str, f: impl Fn() -> bool) {
    for _ in 0..2000 {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("never: {what}");
}

struct Count(AtomicUsize);

impl Listener for Count {
    fn changed(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

fn playlists(p: &Peer) -> Vec<Playlist> {
    p.with(|p| ark::plan::rows::<Playlist>(&p.log.store))
}

/// Everything a phone calls, over a server in this process. Falsified by
/// `State::settle` dropping the patches it took from the peer: the view
/// said `Unchanged` after the item was added.
#[test]
fn the_bridge_does_what_the_kit_does() {
    let hub = server();
    let dial = || hub.dialer(Arc::new(dev));
    let (da, db, dc) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());

    let alice = device(&da, "phone", Some(login("alice@phone")));
    let told = Arc::new(Count(AtomicUsize::new(0)));
    alice.set_listener(Some(told.clone()));
    alice.connect_with("", dial());
    until("alice linked", || alice.status().linked);
    assert_eq!(alice.status().link, "online");

    // A mutate, a refusal and the form's check, by name with JSON.
    let made = alice.mutate("create_playlist".into(), r#"{"name":" Road "}"#.into()).unwrap();
    let refused = alice.mutate("create_playlist".into(), r#"{"name":"  "}"#.into());
    assert_eq!(
        refused,
        Err(Refusal::Refused {
            why: "a playlist needs a name".into()
        })
    );
    assert_eq!(
        alice.check("create_playlist".into(), r#"{"name":""}"#.into()).as_deref(),
        Some("a playlist needs a name")
    );
    assert_eq!(alice.check("create_playlist".into(), r#"{"name":"x"}"#.into()), None);
    assert!(alice.mutate("nothing".into(), "null".into()).is_err());
    until("confirmed", || alice.standing(made.clone()) == Standing::Confirmed);
    let road = playlists(&alice)[0].id.hex();

    // A view and its patches; a query answering the same.
    let args = format!(r#"{{"playlist_id":"{road}"}}"#);
    let items = alice.view("items".into(), args.clone()).unwrap();
    assert_eq!(items.rows(), Vec::<String>::new());
    let before = told.0.load(Ordering::Relaxed);
    let add = format!(r#"{{"playlist_id":"{road}","track_id":"t1"}}"#);
    alice.mutate("add_to_playlist".into(), add).unwrap();
    let ViewUpdate::Patched { patches } = items.take() else {
        panic!("patched")
    };
    let [Patch::Insert { at: 0, row }] = &patches[..] else {
        panic!("one insert: {patches:?}")
    };
    assert!(row.contains(r#""track_id":"t1""#), "{row}");
    assert_eq!(items.take(), ViewUpdate::Unchanged);
    assert!(told.0.load(Ordering::Relaxed) > before, "the listener was told");
    assert_eq!(alice.query("items".into(), args.clone()).unwrap(), format!("[{row}]"));

    // Another device sees it; the live space carries a call between them.
    let bob = device(&db, "laptop", Some(login("bob@laptop")));
    bob.connect_with("", dial());
    until("bob has the item", || {
        bob.query("items".into(), args.clone()).is_ok_and(|r| r.contains("t1"))
    });
    until("both here", || bob.live_rows("here".into()).len() == 2);
    alice.call("point".into(), r#"{"x":3}"#.into()).unwrap();
    until("bob sees the pointer", || {
        bob.live_rows("pointer".into()).iter().any(|r| r.contains(r#""x":3"#))
    });
    alice.call("point".into(), r#"{"x":"three"}"#.into()).unwrap();
    until("an ephemeral call refused as it is relayed", || !alice.take_live_refusals().is_empty());
    assert!(alice.call("rename".into(), r#"{"name":7}"#.into()).is_err(), "any other refused at once");

    // A device alone keeps a history of its own, and hands it over joining.
    let carol = device(&dc, "tablet", None);
    carol.connect_with("", dial());
    carol.alone();
    carol.mutate("create_playlist".into(), r#"{"name":"Alone"}"#.into()).unwrap();
    let s = carol.status();
    assert!(s.alone && s.cursor == 1 && s.link == "alone", "{s:?}");
    carol.join(String::new());
    carol.login(login("carol@tablet"));
    until("carol's playlist reached bob", || playlists(&bob).iter().any(|p| p.name == "Alone"));
    until("carol has nothing pending", || carol.status().pending == 0 && !carol.status().alone);

    // Signed out: nothing dialled, the history kept.
    alice.logout();
    assert_eq!(alice.status().link, "signed out");
    assert_eq!(alice.query("items".into(), args).unwrap(), format!("[{row}]"));
    assert!(alice.take_rejections().is_empty());
}

/// A device opened again reads what its disk held.
/// Falsified by `Peer::open` opening its disk in memory: the reopened
/// device held no playlist.
#[test]
fn a_device_reopens_from_its_disk() {
    let d = tempfile::tempdir().unwrap();
    let p = device(&d, "phone", None);
    p.mutate("create_playlist".into(), r#"{"name":"Kept"}"#.into()).unwrap();
    drop(p);
    let p = device(&d, "phone", None);
    assert_eq!(playlists(&p).len(), 1);
    assert_eq!(p.status().pending, 1, "pending, signed out");
}
