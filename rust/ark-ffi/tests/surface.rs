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
    assert!(
        alice.call("point".into(), r#"{"x":"three"}"#.into()).is_err(),
        "an input not of its type refused at once"
    );
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

/// A login the server turns away stops the link and says why until the
/// next login. Falsified by the driver dropping `Pumped::denied`: the
/// status never said it.
#[test]
fn a_denied_login_is_said() {
    let hub = server();
    let d = tempfile::tempdir().unwrap();
    let p = device(&d, "phone", Some(login("alice@phone")));
    // Pumped by hand, so the next denial is not taken in before the
    // status is read.
    p.by_hand();
    p.connect_with("", hub.dialer(Arc::new(|_: &str| None)));
    until("denied", || {
        p.pump();
        p.status().denied.is_some()
    });
    let s = p.status();
    assert_eq!((s.link.as_str(), s.denied.as_deref()), ("signed out", Some("not signed in")));
    p.login(login("alice@phone"));
    assert_eq!(p.status().denied, None);
}

pub mod jobs {
    use std::sync::atomic::{AtomicBool, Ordering};

    use ark::effect::Job;
    use ark::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Row, Effect)]
    pub struct Fetch {
        #[ark(key)]
        pub id: String,
        pub state: Job<String>,
    }

    #[derive(Schema)]
    pub struct Jobs {
        pub fetch: Fetch,
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
    pub struct Ask {
        pub id: String,
        pub after_ms: i64,
    }

    fn ask(ctx: &Ctx, db: &mut Db<Jobs>, i: Ask) -> Result<(), Refusal> {
        let state = Job::Pending {
            due_ms: ctx.now + i.after_ms,
        };
        db.fetch.insert(Fetch { id: i.id, state })
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Input)]
    pub struct Of {
        pub id: String,
    }

    /// Whether `fetch.every` refuses the next time it is made.
    pub static REFUSE: AtomicBool = AtomicBool::new(false);

    /// Every job, as a plan that reads the row `id` names, so that a write
    /// to it makes the plan again — which refuses once when asked to.
    fn every(_: &Ctx, db: &Db<Jobs>, i: Of) -> Result<Plan<Jobs, Fetch>, Refusal> {
        let _ = db.fetch.get(i.id);
        match REFUSE.swap(false, Ordering::SeqCst) {
            true => Err(Refusal("refused once".into())),
            false => Ok(scan::<Jobs, Fetch>()),
        }
    }

    pub fn domain() -> Domain {
        Domain::new::<Jobs>("jobs", "1")
            .mutator("fetch.ask", ask)
            .view("fetch.every", every)
            .with(Fetch::register::<Jobs>)
    }
}

/// A handler in the app asks which jobs of a kind are due: the rows a
/// claim would take now, each its JSON, and when the next falls due.
/// Falsified by `<kind>.due` answering every open row: the job due in an
/// hour was among those due now.
#[test]
fn a_kinds_due_jobs_are_asked_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let d = Peer::open(
        Config::of(Arc::new(jobs::domain())),
        dir.path().to_str().unwrap(),
        Some(login("alice@phone")),
    )
    .unwrap();
    d.mutate("fetch.ask".into(), r#"{"id":"now","after_ms":0}"#.into()).unwrap();
    d.mutate("fetch.ask".into(), r#"{"id":"later","after_ms":3600000}"#.into()).unwrap();
    let due = d.due("fetch".into()).unwrap();
    assert_eq!(due.rows.len(), 1, "{due:?}");
    assert!(due.rows[0].contains(r#""id":"now""#), "{due:?}");
    assert!(due.next_ms.is_some_and(|t| t > ark_peer::now_ms() + 3_000_000));
}

/// A kept view that could not be kept up is made again from its input,
/// and reads as it should. Falsified by `Peer::view` keeping its input's
/// JSON as the bytes it is made again from (`input.into_bytes()`): the
/// view made again read nothing.
#[test]
fn a_view_made_again_after_a_reset_is_made_from_its_input() {
    let dir = tempfile::tempdir().unwrap();
    let d = Peer::open(Config::of(Arc::new(jobs::domain())), dir.path().to_str().unwrap(), None).unwrap();
    let every = d.view("fetch.every".into(), r#"{"id":"a"}"#.into()).unwrap();
    d.mutate("fetch.ask".into(), r#"{"id":"b","after_ms":0}"#.into()).unwrap();
    assert!(matches!(every.take(), ViewUpdate::Patched { .. }));
    jobs::REFUSE.store(true, Ordering::SeqCst);
    d.mutate("fetch.ask".into(), r#"{"id":"a","after_ms":0}"#.into()).unwrap();
    assert!(!jobs::REFUSE.load(Ordering::SeqCst), "the plan was made again, and refused");
    let ViewUpdate::Reset { rows } = every.take() else { panic!("reset") };
    assert_eq!(rows.len(), 2, "made again from its input: {rows:?}");
    d.mutate("fetch.ask".into(), r#"{"id":"c","after_ms":0}"#.into()).unwrap();
    assert!(matches!(every.take(), ViewUpdate::Patched { .. }), "and kept up after");
}
