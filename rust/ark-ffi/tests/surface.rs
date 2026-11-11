//! The whole bridge in one process: devices opened on disk, linked to a
//! server in this process, called by name with JSON — a mutate and its
//! refusal, a check, a query, a view's patches, the live space's call — and
//! a device alone that joins; each driven by its own thread, as on a phone.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ark_ffi::{Listener, Login, Patch, Peer, Refusal, Standing, ViewUpdate};
use ark_peer::{dev, Config, Handle, Live, Memory, Module};
use arkdb::demo::{arrival, demo, live, room, Playlist};

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
        picture: String::new(),
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
    /// Held by every test that makes `fetch.every`: `REFUSE` is the
    /// process's, and tests run side by side.
    pub static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    let _one = jobs::ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
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

/// A tap in the app is the root of what it causes: what is mutated between
/// `begin` and `end` reaches the server under the span `begin` said.
/// Falsified by `begin` opening nothing on the peer: the server's entry had
/// no cause.
#[test]
fn a_tap_begun_through_the_bridge_is_the_servers_cause() {
    let hub = server();
    let d = tempfile::tempdir().unwrap();
    let alice = device(&d, "phone", Some(login("alice@phone")));
    alice.connect_with("", hub.dialer(Arc::new(dev)));
    until("alice linked", || alice.status().linked);
    let span = alice.begin("tap new playlist".into());
    let made = alice.mutate("create_playlist".into(), r#"{"name":"Road"}"#.into()).unwrap();
    alice.end();
    until("confirmed", || alice.standing(made.clone()) == Standing::Confirmed);
    let cause = hub
        .with(move |p| {
            p.log
                .log()
                .entries()
                .into_iter()
                .find(|l| ark::hash::hex(&l.entry.id.0) == made)
                .and_then(|l| l.entry.cause)
        })
        .flatten()
        .expect("the server's entry has a cause");
    assert_eq!(format!("{}/{}", ark::hash::hex(&cause.trace.0), ark::hash::hex(&cause.id)), span);
}

/// An app's mutate with no span opened is somebody's doing too: it reaches
/// the server as the root of a trace of its own, not as an answer to
/// whatever the device was last told. Falsified by `mutate` authoring
/// under the peer's waking span: the entry had no root of its own.
#[test]
fn an_apps_mutate_is_the_root_of_its_trace() {
    let hub = server();
    let d = tempfile::tempdir().unwrap();
    let alice = device(&d, "phone", Some(login("alice@phone")));
    alice.connect_with("", hub.dialer(Arc::new(dev)));
    until("alice linked", || alice.status().linked);
    let made = alice.mutate("create_playlist".into(), r#"{"name":"Road"}"#.into()).unwrap();
    until("confirmed", || alice.standing(made.clone()) == Standing::Confirmed);
    let entry = hub
        .with(move |p| {
            p.log
                .log()
                .entries()
                .into_iter()
                .find(|l| ark::hash::hex(&l.entry.id.0) == made)
                .map(|l| l.entry)
        })
        .flatten()
        .unwrap();
    let cause = entry.cause.expect("a root span");
    assert_eq!(entry.span().trace, cause.trace, "in the root's trace");
    assert_ne!(cause.trace.0, entry.id.0, "a trace begun by the app, not by the entry");
}

/// A view's rows read as they stand are its base: what it did before they
/// were read is in them, and the next `take` says only what came after.
/// Falsified by `rows` leaving the patches gathered since the view opened
/// (the driver's thread settling the server's rows as an app opens): the
/// next `take` said the same insert again, and a list spliced from the rows
/// held the row twice (money's budget, a category twice, the app gone).
#[test]
fn a_views_rows_are_its_base_and_take_says_what_came_after() {
    let _one = jobs::ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let d = Peer::open(Config::of(Arc::new(jobs::domain())), dir.path().to_str().unwrap(), None).unwrap();
    let every = d.view("fetch.every".into(), r#"{"id":"a"}"#.into()).unwrap();
    // Settled into the view before the app reads its rows.
    d.mutate("fetch.ask".into(), r#"{"id":"b","after_ms":0}"#.into()).unwrap();
    let rows = every.rows();
    assert_eq!(rows.len(), 1, "the rows as they stand: {rows:?}");
    assert_eq!(every.take(), ViewUpdate::Unchanged, "nothing came after the rows were read");
    d.mutate("fetch.ask".into(), r#"{"id":"c","after_ms":0}"#.into()).unwrap();
    let ViewUpdate::Patched { patches } = every.take() else {
        panic!("patched")
    };
    assert_eq!(patches.len(), 1, "only what came after: {patches:?}");
}

/// The process's one runtime: two UIs attached share one peer, opened
/// once; detaching both leaves it running; a sign-out puts it away and the
/// next open makes it again. Falsified by opening a peer per attachment, or
/// closing it when the last UI detaches.
#[test]
fn two_attaches_share_one_runtime_and_detaching_leaves_it_running() {
    let dir = tempfile::tempdir().unwrap();
    let process = ark_ffi::ArkProcess::apart();
    let open = || Peer::open(config(), dir.path().to_str().unwrap(), None);
    let first = process.clone().attach();
    let a = process.open("one", open).unwrap();
    let second = process.clone().attach();
    let b = process.open("one", || panic!("opened twice")).unwrap();
    assert!(Arc::ptr_eq(&a, &b), "one peer for both");
    assert_eq!((process.attached(), process.opened()), (2, 1));
    let part = process.part("player", || 7_u32);
    assert!(Arc::ptr_eq(&part, &process.part("player", || 8_u32)), "a part made once");
    first.detach();
    first.detach();
    drop(second);
    assert_eq!(process.attached(), 0);
    assert!(process.running(), "the runtime outlives its UI");
    assert!(Arc::ptr_eq(&process.peer().unwrap(), &a));
    process.sign_out();
    assert!(!process.running());
    process.open("one", open).unwrap();
    assert_eq!(process.opened(), 2, "made again after a sign-out");
    let other = tempfile::tempdir().unwrap();
    let c = process
        .open("two", || Peer::open(config(), other.path().to_str().unwrap(), None))
        .unwrap();
    assert_eq!(process.opened(), 3, "another server is another runtime");
    assert!(Arc::ptr_eq(&process.peer().unwrap(), &c));
    assert!(
        Arc::ptr_eq(&ark_ffi::ArkProcess::shared(), &ark_ffi::ArkProcess::shared()),
        "one a process"
    );
}

/// A wake: the app's runtime hears it, the process is online for it, and
/// the woken component is told to finish once the runtime is idle — not
/// before — then the process goes offline again with no UI attached.
/// Falsified by `woken` not waiting on `idle`: `done` was told while the
/// runtime still synced.
#[test]
fn a_wake_is_work_until_the_runtime_is_idle() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    struct App {
        heard: Mutex<Vec<ark_app::Woken>>,
        busy: AtomicBool,
    }
    impl ark_ffi::Wakeful for App {
        fn woken(&self, reason: ark_app::Woken) -> bool {
            self.heard.lock().unwrap().push(reason);
            self.busy.store(true, Ordering::SeqCst);
            true
        }
        fn idle(&self) -> bool {
            !self.busy.load(Ordering::SeqCst)
        }
    }
    struct Done(std::sync::mpsc::Sender<bool>);
    impl ark_ffi::WakeDone for Done {
        fn finished(&self, idle: bool) {
            let _ = self.0.send(idle);
        }
    }

    let process = ark_ffi::ArkProcess::apart();
    let online = Arc::new(Mutex::new(vec![]));
    let o = online.clone();
    process.with_link(move |on| o.lock().unwrap().push(on));
    let app = Arc::new(App {
        heard: Mutex::new(vec![]),
        busy: AtomicBool::new(false),
    });
    process.set_wakeful(Some(app.clone()));
    let (tx, rx) = std::sync::mpsc::channel();
    process.clone().wake(ark_ffi::WakeReason::Sync, 5_000, Arc::new(Done(tx)));
    assert_eq!(*app.heard.lock().unwrap(), vec![ark_app::Woken::Sync]);
    assert!(process.online(), "online for the wake");
    assert!(rx.recv_timeout(std::time::Duration::from_millis(200)).is_err(), "not done while it syncs");
    app.busy.store(false, Ordering::SeqCst);
    assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(2)), Ok(true));
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert!(!process.online(), "offline again: no UI, no work");
    assert_eq!(*online.lock().unwrap(), vec![true, false]);
    // Out of time: told so.
    app.busy.store(true, Ordering::SeqCst);
    let (tx, rx) = std::sync::mpsc::channel();
    process.clone().settle(100, Arc::new(Done(tx)));
    assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(2)), Ok(false));
}
