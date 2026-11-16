//! harken's phones against the server the owner runs (ui/docs/app.md,
//! Servers Already Deployed): this build's phone host — harken's part of
//! its host under ark-ffi's `Hosted` and the app host every Ark app has, as
//! its generated library puts them together — dialling a harken server
//! built from an older revision, which `ARK_DEPLOYED_SERVER` names (its base
//! URL, a dev server: `HARKEN_DEV_AUTH=1`). `.github/compat-check.sh` builds
//! that server from the revision k2on/os pins, starts it and runs this; with
//! no server named the test says so and passes.
//!
//! The walk is a person's: the first page, the address typed, the server
//! found to be harken and this domain, the sign-in (the dev server sends
//! the browser straight back, which the shell here does for it), the
//! library synced (the server's own Favorites, made at the first sign-in);
//! then a playlist made on the phone, confirmed by the server, and seen on a
//! second device signed in after it. Then the listening session, on that
//! old server: both devices stand in it, each as its host says it is (the
//! phone and a computer), and each sees the other there; the second plays,
//! and the phone's player shows what plays and where; the phone pauses,
//! and the second sees it paused.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ark::format::Encode;
use ark_app::host::{Form, ThisDevice};
use ark_ffi::hosted::{Hosted, PhoneApp};
use ark_ffi::{ArkHost, DeviceForm, HostShell, Launch, ShotsTook, Standing};
use harken_app::host::Harken;
use harken_app::player::{Player, PlayerMsg};
use harken_domain::host::Listening;
use harken_domain::{Clock, Playlist, Start, Track};

struct HarkenPhone;

impl PhoneApp for HarkenPhone {
    type App = Harken;
    const NAME: &'static str = "Harken";
    const SCHEME: &'static str = "harken";
    const DEMO: bool = true;
    fn domain() -> Arc<ark::domain::Domain> {
        harken_domain::shared()
    }
    fn demo(today: &str, now_ms: i64) -> ark_peer::Peer {
        harken_seed::open(harken_seed::seeded_disk(today, now_ms))
    }
}

/// A phone's shell: what it is asked to open in the browser.
#[derive(Default)]
struct Shell {
    browsed: Mutex<Vec<String>>,
}

impl HostShell for Shell {
    fn changed(&self) {}
    fn browse(&self, url: String) {
        self.browsed.lock().unwrap().push(url);
    }
}

fn until(what: &str, f: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(60);
    while !f() {
        assert!(Instant::now() < end, "never: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A new install of harken, walked through its first run to `server` and
/// signed in there: its host.
fn signed_in(root: &std::path::Path, server: &str) -> (Arc<ArkHost>, Arc<Hosted<HarkenPhone>>) {
    let launch = Launch {
        root: root.to_string_lossy().into(),
        version: "0.1 (1)".into(),
        demo: false,
        demo_day: None,
        shots: true,
        walk: true,
        scan: false,
        args: vec![],
        form: DeviceForm::Phone,
        device: "Compat Phone".into(),
    };
    let app = Hosted::<HarkenPhone>::new();
    let host = ArkHost::new(app.clone(), launch, None);
    let shell = Arc::new(Shell::default());
    host.start(shell.clone());
    for step in [
        r#"{"tap": "welcome.next"}"#.to_string(),
        r#"{"tap": "welcome.typed"}"#.to_string(),
        serde_json::json!({ "type": server }).to_string(),
        r#"{"tap": "welcome.check"}"#.to_string(),
    ] {
        until(&format!("{step} taken"), || host.shots_step(step.clone()) == ShotsTook::Done);
    }
    // The sign-in, in the browser: a dev server sends it straight back to
    // the app's scheme, as the phone's browser sheet would.
    until("the sign-in opened in the browser", || {
        if host.shows_app() {
            return true;
        }
        let Some(url) = shell.browsed.lock().unwrap().pop() else {
            return false;
        };
        let back = ark_ffi::ark_auth::client::straight_back(&url, HarkenPhone::SCHEME)
            .unwrap_or_else(|| panic!("{url} is not a dev server's sign-in that comes straight back"));
        host.returned(Some(back), None);
        true
    });
    until("the app shown, signed in", || host.shows_app());
    (host, app)
}

/// The account's listening session as `peer` holds it.
fn listening(peer: &ark_ffi::Peer) -> Listening {
    peer.with(|p| match &p.live {
        Some(live) => Listening::of(&live.store, &p.me().actor),
        None => Listening::default(),
    })
}

#[test]
fn the_phones_sync_with_the_deployed_server() {
    let Some(server) = std::env::var("ARK_DEPLOYED_SERVER").ok().filter(|s| !s.is_empty()) else {
        eprintln!("ARK_DEPLOYED_SERVER names no server: nothing to check (.github/compat-check.sh starts one)");
        return;
    };
    let server = server.trim_end_matches('/').to_string();

    // The server runs this domain: its /healthz names the schema this build
    // has, so the phone finds it the same, not newer.
    let health = match ark_ffi::ark_auth::probe::get(&format!("{server}/healthz")) {
        ark_ffi::ark_auth::probe::Answer::Answered { status: 200, body } => body,
        other => panic!("{server}/healthz: {other:?}"),
    };
    let line = ark_ffi::hosted::domain_line::<HarkenPhone>();
    let ours = line.split(' ').next().unwrap();
    assert!(
        health.contains(&format!("domain {ours} ")),
        "the server runs another schema:\n{health}\nthis build: {line}"
    );

    // One phone: the first run, the sign-in, the library synced. Its screen
    // is up (a UI attached to the process's runtime), so it dials.
    let (a_dir, b_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let _ui = ark_ffi::ArkProcess::shared().attach();
    let (phone, app) = signed_in(a_dir.path(), &server);
    let peer = phone.peer().expect("a session open");
    let st = peer.status();
    assert!(!st.alone && !st.user.is_empty(), "{st:?}");
    let playlists = |p: &ark_ffi::Peer| p.view_typed::<Playlist>("playlists", "null").unwrap().rows();
    until("the server's Favorites", || playlists(&peer).iter().any(|p| p.name == "Favorites"));

    // An edit, judged by the server: confirmed, not held or refused.
    let name = format!("Compat {}", ark_peer::now_ms());
    let id = peer
        .mutate("create_playlist".into(), serde_json::json!({ "name": name }).to_string())
        .unwrap();
    until("the server confirms the playlist", || {
        let s = peer.standing(id.clone());
        assert!(!matches!(s, Standing::Rejected { .. }), "refused: {s:?}");
        s == Standing::Confirmed
    });

    // A second device, signed in after it (the desktop's sign-in: the dev
    // server answers the loopback with a code), its own replica, has it
    // from the server.
    let login = ark_ffi::ark_auth::client::login(&server, None, |_| {}).expect("the second device's sign-in");
    let desk = ThisDevice {
        form: Form::Computer,
        name: "Compat Desk".into(),
    };
    let other = ark_ffi::hosted::open::<HarkenPhone>(b_dir.path().to_str().unwrap(), Some(login.into()), &desk).unwrap();
    other.connect(ark_ffi::ark_auth::socket_url(&server));
    other.set_online(true);
    until("the second device has the playlist", || playlists(&other).iter().any(|p| p.name == name));
    assert_eq!(peer.status().pending, 0, "nothing of the first phone's held");

    // The listening session: each stands in it as its host said it is, and
    // sees the other there.
    let named = |l: &Listening, n: &str, kind: &str| l.devices.iter().any(|d| d.name == n && d.kind == kind && d.can_play);
    until("the phone sees both devices in the session", || {
        let l = listening(&peer);
        named(&l, "Compat Phone", "phone") && named(&l, "Compat Desk", "computer")
    });
    until("the second device sees the phone in the session", || {
        named(&listening(&other), "Compat Phone", "phone")
    });
    until("the phone's player stands in the session", || {
        let p = app.page::<Player>();
        p.joined && p.devices.iter().any(|d| d.name == "Compat Desk") && p.devices.iter().any(|d| d.mine)
    });

    // The second device plays; the phone's player shows it, and where.
    let track = Track {
        album: "Compat".into(),
        creator: "Nobody".into(),
        duration_ms: 180_000,
        file: "music/compat.flac".into(),
        id: harken_domain::Id::new([7; 16]),
        title: format!("Compat Track {}", ark_peer::now_ms()),
    };
    let start = Start {
        at: 0,
        from: String::new(),
        position: 0,
        queue: vec![track.clone()],
    };
    other
        .change_encoded("listening.start", start.to_bytes())
        .expect("the second device plays");
    until("the phone's player shows what the second device plays", || {
        let p = app.page::<Player>();
        p.has && p.now.title == track.title && p.now.playing && p.output == "Compat Desk"
    });

    // The phone pauses it; the second device sees it paused.
    app.send::<Player>(PlayerMsg::Toggle);
    until("the second device sees it paused", || {
        listening(&other)
            .session
            .is_some_and(|a| a.queue.first() == Some(&track) && matches!(a.clock, Clock::Paused { .. }))
    });
    until("the phone's player shows it paused", || {
        let p = app.page::<Player>();
        p.has && !p.now.playing
    });
}
