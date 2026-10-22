//! The phones' screens as rows (`harken_player::screens`) through the
//! phone's FFI, against harken's own session: real peers in a [`Hall`], the
//! player screen's row, the picker and the diagnostics' numbers read, and a
//! screen's asks pressed. The picker's and the scrub's tests were the app's
//! (`ListenTests.kt`), moved here with what they hold.
//!
//! Each was falsified once by breaking what it holds in
//! `harken/player/src/screens.rs`; the comment beside each says how.

use std::sync::{Arc, Mutex};

use ark_ffi::Peer;
use harken_domain::host::Listening;
use harken_domain::{Id, Track};
use harken_ffi::{Hall, Loudness, PhoneSound, Player, SoundState};
use harken_player::{Ask, Line, Press, Where};
use serde_json::json;

fn track(title: &str, n: u8) -> Track {
    let mut id = [0; 16];
    id[15] = n;
    Track {
        album: "Water Music".into(),
        creator: "Handel".into(),
        duration_ms: 100_000,
        file: format!("music/{title}.mp3"),
        id: Id::new(id),
        title: title.into(),
    }
}

fn queue() -> Vec<Track> {
    vec![track("a", 1), track("b", 2), track("c", 3)]
}

/// A player holding a queue it was handed, and a media stream of a hundred
/// steps; nothing moves unless a test says so.
#[derive(Default)]
struct Held {
    queue: Vec<Track>,
    index: i64,
    playing: bool,
    from: String,
    stream: Option<(i64, bool)>,
}

struct Fake(Mutex<Held>);

impl PhoneSound for Fake {
    fn state(&self) -> SoundState {
        let s = self.0.lock().unwrap();
        SoundState {
            queue: s.queue.iter().map(|t| t.id.hex()).collect(),
            index: s.index,
            playing: s.playing,
            buffering: false,
            position_ms: 0,
            ended: false,
            from: s.from.clone(),
        }
    }
    fn take_stalled(&self) -> bool {
        false
    }
    fn tracks(&self) -> String {
        serde_json::to_string(&self.0.lock().unwrap().queue).unwrap()
    }
    fn load(&self, queue: String, at: i64, _: i64, playing: bool, from: String) {
        let mut s = self.0.lock().unwrap();
        (s.queue, s.index, s.playing, s.from) = (serde_json::from_str(&queue).unwrap(), at, playing, from);
    }
    fn jump(&self, at: i64, _: i64) {
        self.0.lock().unwrap().index = at;
    }
    fn resume(&self) {
        self.0.lock().unwrap().playing = true;
    }
    fn pause(&self) {
        self.0.lock().unwrap().playing = false;
    }
    fn seek(&self, _: i64) {}
    fn wake_in(&self, _: i64) {}
    fn volume(&self) -> Option<Loudness> {
        self.0.lock().unwrap().stream.map(|(level, muted)| Loudness { level, muted })
    }
    fn set_volume(&self, level: i64, muted: bool) {
        self.0.lock().unwrap().stream = Some((level, muted));
    }
}

struct Device<'h> {
    hall: &'h Hall,
    peer: Arc<Peer>,
    player: Arc<Player>,
    _dir: tempfile::TempDir,
}

fn on<'h>(hall: &'h Hall, id: &str, can_play: bool, kind: &str) -> Device<'h> {
    let dir = tempfile::tempdir().unwrap();
    let said = json!({ "can_play": can_play, "id": "", "kind": kind, "name": id, "user": "" }).to_string();
    let peer = hall.device(dir.path().to_str().unwrap().into(), "alice".into(), id.into(), said).unwrap();
    let player = Player::new(peer.clone(), id.into(), None);
    hall.carry();
    Device {
        hall,
        peer,
        player,
        _dir: dir,
    }
}

impl Device<'_> {
    fn press(&self, p: Press) {
        assert_eq!(self.player.press(serde_json::to_string(&p).unwrap()), None, "{p:?}");
        self.hall.carry();
    }

    fn ask(&self, a: Ask) {
        assert_eq!(self.player.ask(a.clone()), None, "{a:?}");
        self.hall.carry();
    }

    /// A speaker with a volume, of a family, at `level`.
    fn equip(&self, family: Option<&str>, level: i64) {
        let e = json!({ "can_volume": true, "family": family, "muted": false, "volume": level });
        self.peer.call("listening.equip".into(), e.to_string()).unwrap();
        self.hall.carry();
    }
}

fn state(hall: &Hall) -> Listening {
    let v: serde_json::Value = serde_json::from_str(&hall.listening("alice".into())).unwrap();
    Listening {
        devices: serde_json::from_value(v["devices"].clone()).unwrap(),
        scrub: serde_json::from_value(v["scrub"].clone()).unwrap(),
        session: serde_json::from_value(v["session"].clone()).unwrap(),
    }
}

fn start(q: &[Track], at: i64) -> Press {
    Press::Start {
        queue: q.to_vec(),
        at,
        position_ms: 0,
        from: "#album/Water%20Music".into(),
    }
}

/// The picker, as the desktop's: while nothing plays, this phone is the
/// default — ticked, the cursor on it — and there is no row that stops it
/// everywhere; while something plays, the output is ticked and the cursor
/// on it; a device that cannot play is drawn and not selectable; the output
/// that has gone is drawn, not answering; and picking a device is a
/// hand-off, which the session shows as connecting until that device says
/// it has started.
///
/// Falsified by ticking the output alone, with no default (`ticked` the
/// output's id or ""): nothing was ticked while nothing played.
#[test]
fn the_picker_defaults_to_this_phone_and_transfers() {
    let hall = Hall::new();
    let desk = on(&hall, "desk", false, "computer");
    let phone = on(&hall, "phone", true, "phone");
    let laptop = on(&hall, "laptop", true, "computer");
    let ids = |rows: &[harken_player::DeviceRow], f: fn(&harken_player::DeviceRow) -> bool| {
        rows.iter().filter(|r| f(r)).map(|r| r.id.clone()).collect::<Vec<_>>()
    };

    let idle = phone.player.picker();
    assert_eq!(ids(&idle.rows, |_| true), ["desk", "laptop", "phone"], "a row per device and no other");
    assert_eq!(ids(&idle.rows, |r| r.ticked), ["phone"], "nothing plays: this phone is ticked");
    assert_eq!(idle.rows[idle.cursor as usize].id, "phone", "and the cursor is on it");
    assert!(!idle.rows[0].takeable, "a desktop with no speakers is not selectable");

    phone.press(start(&queue(), 0));
    assert_eq!(ids(&phone.player.picker().rows, |r| r.ticked), ["phone"], "the output is ticked");
    phone.ask(Ask::Pick { id: "phone".into() });
    phone.ask(Ask::Pick { id: "desk".into() });
    assert!(
        state(&hall).outputs("phone"),
        "picking the device that has it, or one that cannot play, asks nothing"
    );
    phone.ask(Ask::Pick { id: "laptop".into() });
    let moving = phone.player.picker();
    let row = moving.rows.iter().find(|r| r.id == "laptop").unwrap();
    assert!(row.moving && row.ticked, "connecting, until it says it has started");
    assert_eq!(moving.rows[moving.cursor as usize].id, "laptop");

    // The laptop's socket goes: it keeps the sound, and is drawn not answering.
    laptop.peer.set_online(false);
    hall.carry();
    let away = phone.player.picker();
    let row = away.rows.iter().find(|r| r.id == "laptop").unwrap();
    assert!(row.away && row.ticked && !row.takeable, "drawn, away, ticked, and not selectable");
    assert!(desk.peer.status().linked);
    assert_eq!(phone.player.ask(Ask::Pick { id: "nobody".into() }).as_deref(), Some("no such row"));
}

/// Three speakers of one family in the house — the kitchen at 40, the
/// bedroom at 60, the study at 20.
fn house(hall: &Hall) -> Vec<Device<'_>> {
    [("kitchen", 40), ("bedroom", 60), ("study", 20)]
        .into_iter()
        .map(|(id, level)| {
            let d = on(hall, id, true, "speaker");
            d.equip(Some("sonos"), level);
            d
        })
        .collect()
}

/// The picker's volumes and groups: every row with a volume carries it, the
/// group that plays comes first as a whole — "kitchen + 1", every member
/// ticked — and its slider is the group's; a row's mute is that device's; a
/// speaker of the leader's family that is here and not in the group can
/// join, one in it can leave, and nothing else offers either — not the
/// leader, not this phone, not a laptop. The diagnostics say the group.
///
/// Falsified by the group row's slider being a device's (`Level::Device`
/// of its id, ""): refused, "that device is not here, or has no volume". And
/// by offering a join to every speaker that can play, family or none: the
/// phone was offered one ("not this phone").
#[test]
fn the_picker_moves_volumes_and_joins_speakers() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone");
    let _laptop = on(&hall, "laptop", true, "computer");
    phone.equip(None, 50);
    let _house = house(&hall);
    phone.press(start(&queue(), 0));
    phone.press(Press::Transfer { to: "kitchen".into() });
    phone.press(Press::Join { device: "bedroom".into() });

    let p = phone.player.picker();
    let group = &p.rows[0];
    assert!(group.group, "the group first");
    assert_eq!(group.name, "kitchen + 1", "named for its leader and how many follow it");
    assert_eq!(group.volume, Some(50), "its level the mean of its members'");
    let ticked: Vec<&str> = p.rows.iter().filter(|r| r.ticked).map(|r| r.id.as_str()).collect();
    assert_eq!(ticked, ["bedroom", "kitchen"], "every member ticked");
    assert_eq!(p.rows[p.cursor as usize].id, "kitchen", "and the cursor on the output");
    let row = |id: &str| phone.player.picker().rows.into_iter().find(|r| r.id == id).unwrap();
    assert_eq!(row("study").join, Some(true), "a speaker of the family, here, outside the group: join");
    assert_eq!(row("bedroom").join, Some(false), "one in it: leave");
    assert_eq!(row("kitchen").join, None, "not the leader");
    assert_eq!(row("phone").join, None, "not this phone");
    assert_eq!(row("laptop").join, None, "not a laptop");
    assert_eq!(row("phone").volume, Some(50), "a row with a volume carries it");
    assert_eq!(row("laptop").volume, None, "one without, none");
    let numbers = phone.player.numbers();
    let line = |name: &str, value: &str| Line {
        name: name.into(),
        value: value.into(),
    };
    assert!(
        numbers.contains(&line("group", "kitchen + bedroom · volume 50")),
        "the diagnostics say the group"
    );
    assert!(
        numbers.contains(&line("  volume", "60 · family sonos · follows kitchen")),
        "and a member's leader"
    );

    phone.ask(Ask::Pick { id: "".into() });
    assert!(state(&hall).outputs("kitchen"), "the group row hands nothing over");
    phone.ask(Ask::RowVolume { id: "".into(), level: 70 });
    let s = state(&hall);
    assert_eq!(s.group_volume("kitchen"), Some(70), "the group's slider is the group's");
    assert_eq!(
        [s.device("kitchen").unwrap().volume, s.device("bedroom").unwrap().volume],
        [56, 84],
        "both moved"
    );
    phone.ask(Ask::RowMute { id: "bedroom".into() });
    let s = state(&hall);
    assert!(
        s.device("bedroom").unwrap().muted && !s.device("kitchen").unwrap().muted,
        "a row's mute is that device's"
    );
    phone.ask(Ask::RowVolume {
        id: "study".into(),
        level: 35,
    });
    assert_eq!(state(&hall).device("study").unwrap().volume, 35, "a row's slider that device's");

    phone.ask(Ask::RowJoin { id: "study".into() });
    assert_eq!(state(&hall).device("study").unwrap().follows.as_deref(), Some("kitchen"), "joined");
    assert_eq!(phone.player.picker().rows[0].name, "kitchen + 2");
    phone.ask(Ask::RowJoin { id: "bedroom".into() });
    assert_eq!(state(&hall).device("bedroom").unwrap().follows, None, "and left");
    assert_eq!(row("bedroom").join, Some(true));
}

/// A scrubber dragged on a tablet reaches this phone's player screen as the
/// slot it is, and is what its scrubber shows; its own slot is not
/// another's, and letting go — a seek — empties it.
///
/// Falsified by `scrubbing` reading every slot, this phone's own included:
/// the phone's own drag came back to it as somebody else's.
#[test]
fn a_scrub_on_another_device_is_shown_here() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone");
    let tablet = on(&hall, "tablet", true, "computer");
    phone.press(start(&queue(), 0));
    tablet.ask(Ask::Scrub { ms: Some(61_000) });
    let shown = || phone.player.now_playing("Phone".into(), "https://m".into()).scrubbing;
    assert_eq!(shown(), Some(61_000), "the tablet's scrubber, here");
    phone.ask(Ask::Scrub { ms: Some(10_000) });
    assert_eq!(shown(), Some(61_000), "this phone's own slot is not another's");
    tablet.ask(Ask::Seek { ms: 61_000 });
    // Within the relay's window of the last: held, and sent by the next pump past it.
    std::thread::sleep(std::time::Duration::from_millis(harken_ffi::THROTTLE_MS as u64 + 10));
    hall.carry();
    phone.ask(Ask::Scrub { ms: None });
    assert_eq!(shown(), None, "let go: nobody is scrubbing");
    assert_eq!(state(&hall).session.unwrap().clock.position(0), 61_000, "and the seek is the session's");
}

/// The player screen joined: the session's track with its cover from the
/// log, what comes after it, where it plays — this phone, a speaker, a
/// group named for its leader, a laptop gone — and what is playing's
/// volume, remote while it is another device's; a position that counts
/// only while it plays and is not being handed over. And its asks: the one
/// button, next, a track of what comes up, a volume moved and muted.
///
/// Falsified by an up-next that started at the track playing (`skip(at)`):
/// "b" came after "b". And by the device button naming the leader alone:
/// "kitchen" where "kitchen + 1" was wanted.
#[test]
fn the_player_screen_is_the_sessions_while_joined() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone");
    let laptop = on(&hall, "laptop", true, "computer");
    phone.equip(None, 50);
    let np = || phone.player.now_playing("Phone".into(), "https://m".into());
    let idle = np();
    assert!(idle.joined && idle.now.is_none(), "nothing queued: nothing drawn");
    assert_eq!(
        idle.on,
        Some(Where {
            name: "Phone".into(),
            kind: "phone".into(),
            here: true,
            away: false,
            moving: false
        }),
        "the next press plays here"
    );
    assert_eq!(
        idle.volume.map(|v| (v.level, v.remote)),
        Some((50, false)),
        "this phone's own volume, where the next press plays"
    );

    laptop.press(start(&queue(), 1));
    let n = np();
    let now = n.now.clone().unwrap();
    assert_eq!(
        (now.title.as_str(), now.art.as_str()),
        ("b", ""),
        "the session's track; an album the log has not got has no cover"
    );
    assert_eq!(
        n.up_next.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(),
        ["c"],
        "what comes after it"
    );
    assert_eq!(n.on.as_ref().map(|w| (w.name.as_str(), w.here, w.moving)), Some(("laptop", false, false)));
    assert!(n.playing && n.since.is_some(), "playing on the laptop, counting");
    assert_eq!(n.volume, None, "a laptop has no volume anybody can move");

    phone.ask(Ask::Toggle);
    let n = np();
    assert!(!n.playing && n.since.is_none(), "the one button pauses what sounds; paused, it stands");
    phone.ask(Ask::UpNext { n: 0 });
    assert_eq!(state(&hall).session.unwrap().at, 2, "a track of what comes up is a jump to it");
    phone.ask(Ask::Previous);
    assert_eq!(state(&hall).session.unwrap().at, 1);

    let house = house(&hall);
    phone.ask(Ask::Pick { id: "kitchen".into() });
    let n = np();
    assert!(
        n.on.as_ref().unwrap().moving && n.since.is_none(),
        "on its way to the kitchen: connecting, and standing"
    );
    assert_eq!(
        n.volume.map(|v| (v.level, v.remote, v.group)),
        Some((40, true, false)),
        "the kitchen's volume, a remote's"
    );
    phone.ask(Ask::Nudge { delta: 5 });
    phone.ask(Ask::Mute { muted: None });
    let k = state(&hall).device("kitchen").cloned().unwrap();
    assert_eq!((k.volume, k.muted), (45, true), "a button's step and the mute are the kitchen's");
    phone.press(Press::Join { device: "bedroom".into() });
    let n = np();
    assert_eq!(n.on.as_ref().unwrap().name, "kitchen + 1", "a group named for its leader");
    assert!(n.volume.unwrap().group, "and the volume the group's");
    phone.ask(Ask::Volume { level: 70 });
    let level = state(&hall).group_volume("kitchen").unwrap();
    assert!(
        (69..=71).contains(&level),
        "the slider moves the group, each member in proportion: {level}"
    );
    drop(house);
}

/// The player screen alone: the phone's own player drawn as if it were a
/// session — its queue, each track's cover from the log, its stream's
/// volume, no device button, nobody's scrubber — the player's buttons the caller's to do, and the volume and
/// the mute put on the stream here.
///
/// Falsified by alone asks taken as the session's (`ask` not looking at
/// `joined`): the volume and the mute went to a session that has no
/// volume, and the stream stayed at 30.
#[test]
fn the_player_screen_alone_is_the_players_own() {
    let dir = tempfile::tempdir().unwrap();
    let device = r#"{"can_play":true,"id":"","kind":"phone","name":"Pixel","user":""}"#;
    let login = ark_ffi::Login {
        token: String::new(),
        session: "phone".into(),
        user: "library".into(),
        name: String::new(),
        email: String::new(),
        roles: vec!["library".into()],
        expires_ms: 0,
    };
    let peer = harken_ffi::harken_open(dir.path().to_str().unwrap().into(), Some(login), device.into()).unwrap();
    peer.alone();
    let add = json!({ "title": "a", "artist": "Handel", "album": "Water Music", "duration_ms": 100_000, "file": "music/a.mp3", "track": 1,
        "part": "", "catalogue": "", "performer": "", "bpm": 0, "album_art": "covers/water music.jpg", "artist_art": "", "disc": 1,
        "work_title": "", "movement_no": 0 });
    peer.mutate("add_song".into(), add.to_string()).unwrap();
    let player = Player::new(peer, "phone".into(), None);
    let sound = Arc::new(Fake(Mutex::new(Held {
        index: -1,
        stream: Some((30, false)),
        ..Held::default()
    })));
    player.attach(Some(sound.clone()));
    sound.load(serde_json::to_string(&queue()).unwrap(), 1, 0, true, "#library".into());
    player.step();
    let n = player.now_playing("Phone".into(), "https://m".into());
    assert!(
        !n.joined && n.on.is_none() && n.scrubbing.is_none(),
        "alone: no device button, nobody else"
    );
    let now = n.now.unwrap();
    assert_eq!(
        (now.title.as_str(), now.art.as_str()),
        ("b", "https://m/media/covers/water%20music.jpg"),
        "its own track, its cover from the log"
    );
    assert_eq!(n.up_next.len(), 1);
    assert_eq!(n.volume.map(|v| (v.level, v.remote)), Some((30, false)), "its own stream");
    assert_eq!(player.ask(Ask::Toggle).as_deref(), Some(harken_player::NOT_PART), "the player's to do");
    assert_eq!(player.ask(Ask::Volume { level: 80 }), None);
    assert_eq!(player.ask(Ask::Mute { muted: None }), None);
    assert_eq!(
        sound.0.lock().unwrap().stream,
        Some((80, true)),
        "the volume and the mute put on the stream"
    );
    assert!(player.picker().rows.is_empty(), "no picker alone");
    let numbers = player.numbers();
    assert!(numbers.contains(&Line {
        name: "session".into(),
        value: "—".into()
    }));
}

/// A session given whole — what a screenshot draws from, with no peer
/// behind it — is the same picker and the same lines as the phone's own.
///
/// Falsified by `listening_of` reading no session (`session: None`): the
/// whole session's picker ticked this phone where the phone's ticked the
/// laptop.
#[test]
fn a_session_given_whole_draws_the_same_picker() {
    let hall = Hall::new();
    let phone = on(&hall, "phone", true, "phone");
    let _laptop = on(&hall, "laptop", true, "computer");
    phone.press(start(&queue(), 0));
    phone.ask(Ask::Pick { id: "laptop".into() });
    let whole = hall.listening("alice".into());
    assert_eq!(harken_ffi::harken_picker(whole.clone(), "phone".into()), phone.player.picker());
    let lines = harken_ffi::harken_lines(whole, "phone".into());
    assert!(lines.contains(&Line {
        name: "output".into(),
        value: "laptop (moving)".into()
    }));
}
