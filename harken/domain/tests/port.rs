//! harken's domain in plain Rust is the one its ArkLang text was (§1.16):
//! N1 ported it and held the two to each other — the same schema, byte for
//! byte; over seeded scripts of every mutator and live transition, by
//! several users and devices, refused and not, the same rows after every
//! step and the same refusals; and every view answering what its query
//! did. With the text gone, what the two agreed on is pinned: the schema's
//! hash and, per seed, the state the script ends at and a hash of its
//! whole transcript — every step's refusal or how many changes it made,
//! the state after it, and every view's answer at every fiftieth step. The
//! values became JSON after the text went (N1, step e), which moved every
//! pin: these are the JSON run's, the scripts and the code as they were.
//! Falsified by numbering a playlist "Favorites [1]"
//! (`playlists::numbered`): seed 1's pins moved.

use ark::format::Encode;
use ark::hash::sha256;
use ark::log::EntryId;
use ark::peer::Ctx;
use ark::store::Store;
use harken_domain::*;

use arkdb::sim::Rng;

/// One step: who, on which device, holding which roles, calls what with
/// what.
struct Step {
    user: &'static str,
    device: &'static str,
    roles: Vec<String>,
    name: &'static str,
    input: Vec<u8>,
}

const USERS: [&str; 2] = ["alice", "bob"];
const DEVICES: [&str; 3] = ["phone", "laptop", "speaker"];
const WORDS: [&str; 6] = ["Aria", "Goldberg", "  Suite ", "Water Music", "", "Air"];
const PEOPLE: [&str; 5] = ["Bach", "Gould", "Handel", " Pinnock ", "!!!"];

fn media_ids(s: &Store) -> Vec<Id<Media>> {
    s.rows("media").map(|b| Media::from_bytes(b).unwrap().id).collect()
}

fn playlist_ids(s: &Store) -> Vec<Id<Playlist>> {
    s.rows("playlist").map(|b| Playlist::from_bytes(b).unwrap().id).collect()
}

fn track(r: &mut Rng, s: &Store) -> Track {
    let ids = media_ids(s);
    Track {
        album: r.pick(&WORDS).into(),
        creator: r.pick(&PEOPLE).into(),
        duration_ms: r.below(300_000) as i64,
        file: format!("f{}", r.below(4)),
        id: ids.get(r.below(ids.len() as u64 + 1) as usize).copied().unwrap_or_default(),
        title: r.pick(&WORDS).into(),
    }
}

/// A random step over what the store holds.
fn step(r: &mut Rng, s: &Store) -> Step {
    let user = r.pick(&USERS);
    let device = r.pick(&DEVICES);
    let mut roles = vec![];
    if r.below(5) > 0 {
        roles.push(LIBRARY.to_string());
    }
    let media = media_ids(s);
    let some_media = |r: &mut Rng| media.get(r.below(media.len() as u64 + 1) as usize).copied().unwrap_or_default();
    let lists = playlist_ids(s);
    let some_list = |r: &mut Rng| lists.get(r.below(lists.len() as u64 + 1) as usize).copied().unwrap_or_default();
    let n = |r: &mut Rng, k: u64| r.below(k) as i64;
    let (name, input): (&'static str, Vec<u8>) = match r.below(40) {
        0..=7 => (
            "library.add_song",
            AddSong {
                title: r.pick(&WORDS).into(),
                artist: r.pick(&PEOPLE).into(),
                album: r.pick(&WORDS).into(),
                duration_ms: n(r, 400_000),
                file: format!("f{}", r.below(6)),
                track: n(r, 4) - 1,
                part: r.pick(&["", "I", "II"]).into(),
                catalogue: r.pick(&["", "BWV 988", "HWV 349"]).into(),
                performer: r.pick(&["", "Gould", "Pinnock"]).into(),
                bpm: n(r, 400),
                album_art: r.pick(&["", "a.jpg", "b.jpg"]).into(),
                artist_art: r.pick(&["", "p.jpg"]).into(),
                disc: n(r, 120) - 2,
                work_title: r.pick(&["", "Goldberg Variations", "Suite"]).into(),
                movement_no: n(r, 3),
                artist_banner: String::new(),
                released: 0,
            }
            .to_bytes(),
        ),
        8 => (
            "library.describe_work",
            DescribeWork {
                id: r.pick(&["bach/bwv-988", "handel/hwv-349", "nobody"]).into(),
                opus: r.pick(&["", "op. 1"]).into(),
                composed: n(r, 3) * 1700,
                ..DescribeWork::default()
            }
            .to_bytes(),
        ),
        9 => (
            "library.describe_person",
            DescribePerson {
                name: r.pick(&PEOPLE).into(),
                sort_name: r.pick(&["", "Bach, J. S."]).into(),
                born: n(r, 2) * 1685,
                ..DescribePerson::default()
            }
            .to_bytes(),
        ),
        10 => {
            let ids: Vec<String> = s.rows("recording").map(|b| Recording::from_bytes(b).unwrap().id).collect();
            let id = ids.get(r.below(ids.len() as u64 + 1) as usize).cloned().unwrap_or("none".into());
            match r.below(2) {
                0 => (
                    "library.describe_recording",
                    DescribeRecording {
                        id,
                        venue: r.pick(&["", "Toronto"]).into(),
                        recorded: n(r, 2) * 1955,
                        ..DescribeRecording::default()
                    }
                    .to_bytes(),
                ),
                _ => (
                    "library.credit_recording",
                    CreditRecording {
                        recording_id: id,
                        person_name: r.pick(&PEOPLE).into(),
                        role: r.pick(&["", "piano", "composer"]).into(),
                        instrument: r.pick(&["", "harpsichord"]).into(),
                        pos: n(r, 3),
                    }
                    .to_bytes(),
                ),
            }
        }
        11 => ("library.remove_media", RemoveMedia { id: some_media(r) }.to_bytes()),
        12..=13 => (
            "playlists.create_playlist",
            CreatePlaylist {
                name: r.pick(&["Favorites", "Favorites (1)", " Road ", "", "Favorites (2)"]).into(),
            }
            .to_bytes(),
        ),
        14..=16 => (
            "playlists.add_to_playlist",
            AddToPlaylist {
                playlist_id: some_list(r),
                media_id: some_media(r),
            }
            .to_bytes(),
        ),
        17 => ("playlists.add_all_to_playlist", AddAllToPlaylist { playlist_id: some_list(r) }.to_bytes()),
        18 => (
            "playlists.remove_from_playlist",
            RemoveFromPlaylist {
                playlist_id: some_list(r),
                media_id: some_media(r),
            }
            .to_bytes(),
        ),
        19 => (
            "listening.record_play",
            RecordPlay {
                track: some_media(r),
                title: r.pick(&WORDS).into(),
                duration_ms: n(r, 10) - 1,
                ..RecordPlay::default()
            }
            .to_bytes(),
        ),
        20..=21 => (
            "listening.on_join",
            Device {
                can_play: r.below(4) > 0,
                id: device.into(),
                kind: r.pick(&["phone", "speaker"]).into(),
                name: device.into(),
                user: user.into(),
            }
            .to_bytes(),
        ),
        22 => (
            "listening.on_leave",
            Device {
                id: device.into(),
                name: device.into(),
                user: user.into(),
                ..Device::default()
            }
            .to_bytes(),
        ),
        23 => (
            "listening.start",
            Start {
                at: n(r, 3),
                from: r.pick(&["", "album/Air"]).into(),
                position: n(r, 5000) - 10,
                queue: (0..r.below(3)).map(|_| track(r, s)).collect(),
            }
            .to_bytes(),
        ),
        24 => (
            r.pick(&[
                "listening.play",
                "listening.pause",
                "listening.stop",
                "listening.release",
                "listening.clear_upcoming",
            ]),
            vec![],
        ),
        25 => ("listening.seek", Seek { ms: n(r, 9000) - 10 }.to_bytes()),
        26 => ("listening.skip", Skip { by: n(r, 5) - 2 }.to_bytes()),
        27 => ("listening.jump", Jump { index: n(r, 4) - 1 }.to_bytes()),
        28 => ("listening.transfer", Transfer { to: r.pick(&DEVICES).into() }.to_bytes()),
        29 => (
            "listening.enqueue",
            Enqueue {
                next: r.below(2) == 0,
                tracks: (0..r.below(3)).map(|_| track(r, s)).collect(),
            }
            .to_bytes(),
        ),
        30 => match r.below(2) {
            0 => ("listening.remove", Remove { index: n(r, 4) - 1 }.to_bytes()),
            _ => ("listening.move", Move { from: n(r, 4), to: n(r, 4) }.to_bytes()),
        },
        31 => (
            r.pick(&["listening.started", "listening.stalled"]),
            At { at_ms: n(r, 9000) - 5 }.to_bytes(),
        ),
        32 => ("listening.advance", Advance { from_track: n(r, 3) }.to_bytes()),
        33 => (
            "listening.report_position",
            Report {
                at_ms: n(r, 9000),
                since: n(r, 3) * 1000,
            }
            .to_bytes(),
        ),
        34 => (
            "listening.equip",
            Equip {
                can_volume: r.below(3) > 0,
                family: [None, Some("sonos".to_string()), Some(String::new())][r.below(3) as usize].clone(),
                muted: r.below(2) == 0,
                volume: n(r, 120),
            }
            .to_bytes(),
        ),
        35 => match r.below(4) {
            0 => (
                "listening.set_volume",
                SetVolume {
                    device: r.pick(&DEVICES).into(),
                    level: n(r, 110),
                }
                .to_bytes(),
            ),
            1 => (
                "listening.nudge_volume",
                NudgeVolume {
                    delta: n(r, 60) - 30,
                    device: r.pick(&DEVICES).into(),
                }
                .to_bytes(),
            ),
            2 => (
                "listening.set_mute",
                SetMute {
                    device: r.pick(&DEVICES).into(),
                    muted: r.below(2) == 0,
                }
                .to_bytes(),
            ),
            _ => (
                "listening.report_volume",
                ReportVolume {
                    level: n(r, 110),
                    muted: r.below(2) == 0,
                }
                .to_bytes(),
            ),
        },
        36 => (
            r.pick(&["listening.join", "listening.unjoin"]),
            Member {
                device: r.pick(&DEVICES).into(),
            }
            .to_bytes(),
        ),
        37 => (
            "listening.report_group",
            ReportGroup {
                members: DEVICES.iter().filter(|_| r.below(2) == 0).map(|d| d.to_string()).collect(),
            }
            .to_bytes(),
        ),
        38 => match r.below(4) {
            0 => ("listening.set_group_volume", GroupVolume { level: n(r, 101) }.to_bytes()),
            1 => ("listening.nudge_group_volume", GroupNudge { delta: n(r, 40) - 20 }.to_bytes()),
            2 => ("listening.set_group_mute", GroupMute { muted: r.below(2) == 0 }.to_bytes()),
            _ => ("listening.on_restore", vec![]),
        },
        _ => (
            "listening.scrub",
            Scrub {
                at_ms: [None, Some(n(r, 900) - 5)][r.below(2) as usize],
            }
            .to_bytes(),
        ),
    };
    Step {
        user,
        device,
        roles,
        name,
        input,
    }
}

/// The schema's hash (its bytes', postcard; `edd9a961…` its JSON's when
/// a build could write values as JSON).
const SCHEMA: &str = "3b5ce74c2fc660653588dded7e131a9246cfbe7e4c88a62d30a808e8908b9571";

/// Per seed, the state after 400 steps and the transcript's hash, as the
/// value format writes them (postcard, N4).
const PINS: [(u64, &str, &str); 12] = [
    (
        1,
        "ab7b7c34b8922679d4b9999638d88c908c9e97d694259420720985f31d08adf5",
        "7f249616ed9c8d9c615164f08dbd2a2c449bb08839906c909ee29427b28545f4",
    ),
    (
        2,
        "eeb604de803e98e3cb03d8e6739bf43310589348ff27b2bd81d49289a98b337e",
        "96fe35040f3ed18e9b95857981237b391bbb3d4bdcf335a95b8db329827fc40b",
    ),
    (
        3,
        "9ba4cd2ae10ace8c64b91a0092d19c5ffe19f29ab74f28710bf18878d4123c27",
        "c1b41806ee5be6bf800cd7511291540764d9fff819d2ed1c361f081a1b46c5fc",
    ),
    (
        4,
        "0235905a49b9e7fa20ce90d468468871f86b8f2fceaad61148af571b1f022af1",
        "6590188dd7539cb47e29b1e7fcedb2eaa13ea2027f1057dbc2a63548a901f5de",
    ),
    (
        5,
        "cd40af4bb37448e9f090c3d4d03b1962c8a52370e6de118a5a074466ae44df4d",
        "8b8d51d4ec2a0a626461ddd46c0e1043dfc1eb5152daa9ea7163685c69987a75",
    ),
    (
        6,
        "9e95c6c1ec7ae0e1d87eaf460fa6cfc756b4864b7792a83517aeb42e833fe8b7",
        "2e3db855d4a9bfb56ca8675230f8cf5beadced236112ad19e26126bb4b24d9f7",
    ),
    (
        7,
        "8afb17dbf1bd9f88cc225dc4d18db2e7f0cddfccfba178fcf593ab29cc79c10f",
        "fb89441ab79a23b856494baa44917402cb3dd2b63c90242d007e3e92d13846a9",
    ),
    (
        8,
        "18b3b6e6edda0e7e678ca7ece78ee255b7606ae5c1f83faeba94abc0f032629a",
        "5f3b49642f6b4d298e119679ec2ca37175b2a11f7fe2a0cbd9bbf5a7c3302169",
    ),
    (
        9,
        "6e5a731c91028360aed8a587bd58a95f9bf720bb03d77e282ea5401cff2c6c56",
        "3a7eb221104e03ff60a2f480a6186a6de561150cc79e862eebd5ae41ed7b4168",
    ),
    (
        10,
        "692eb6f9237d7882f1e94bdd9d5da26974f498c01285947abc13042ad9c7a6eb",
        "07cf8b54ebe6f65bea7f0663a2c4de561d364150fa91bdb55157d2911594ce3c",
    ),
    (
        11,
        "48ceaeed87c9f28ed11ac9d847cc62b4950f9f0f06b878a27f875132e1d82338",
        "a80c6762894e9ec70dc4e3acdea70813fa1c928dd333919cfae20b8885608ce3",
    ),
    (
        12,
        "0c59514299edacbb3e3628d0eb35586b250a139ce61b9d56adf777629ad8c323",
        "b50c2a5bcb4da6c3dab70b6310ddcff06646685e4d9179b8fd1e8e3f1dd0a43a",
    ),
];

/// The schema the structs derive hashes as the ArkLang schema did.
/// Falsified by declaring `playlist`'s indexes in another order.
#[test]
fn the_schema_hash_is_pinned() {
    assert_eq!(domain().schema_hash().hex(), SCHEMA);
}

/// The schema reads every row the one a server last stood on wrote
/// (`tests/schema.json`, `ark::schema::compat`). A column appended with
/// neither an `Option` type nor a default is a log that does not open: the
/// server opens what it can read of it and carries on from there, and what
/// it could not read is gone. `ARK_WRITE_GEN=1` writes this schema there,
/// once it is the one the server runs.
///
/// Falsified by taking `#[serde(default)]` off `person.banner`: "banner is
/// appended with neither an Option type nor a default".
#[test]
fn the_schema_reads_what_the_last_one_wrote() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/schema.json");
    let now = domain().schema.clone();
    if std::env::var_os("ARK_WRITE_GEN").is_some() {
        std::fs::write(path, serde_json::to_string_pretty(&*now).unwrap() + "\n").unwrap();
    }
    let was = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}; ARK_WRITE_GEN=1 writes it"));
    let was: ark::schema::Schema = serde_json::from_str(&was).unwrap();
    assert_eq!(ark::schema::compat(&was, &now), Ok(()));
}

/// One seed's script run: the state it ends at, and its transcript's hash;
/// and how many steps were refused and how many moved rows.
fn run(seed: u64) -> (String, String, usize, usize) {
    let native = domain();
    let views: Vec<&str> = native
        .fns
        .values()
        .filter(|f| f.kind == ark::domain::Kind::View)
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(views.len(), 28);
    let (mut refused, mut moved) = (0, 0);
    let mut a = Store::new(native.schema.clone(), ark::schema::Space::Durable);
    let mut r = Rng(seed);
    let mut said: Vec<u8> = vec![];
    for i in 0..400 {
        let s = step(&mut r, &a);
        let ctx = Ctx {
            id: EntryId::of("port", i),
            actor: s.user.into(),
            session: s.device.into(),
            roles: s.roles.clone(),
            now: 10_000 + i * 977,
            ..Default::default()
        };
        let f = native.get(s.name).unwrap_or_else(|| panic!("{}", s.name)).proc.clone().unwrap();
        match f.apply(&mut a, &ctx, &s.input) {
            Ok(x) => {
                moved += !x.is_empty() as usize;
                said.extend(format!("{} {} changes\n", s.name, x.len()).bytes());
            }
            Err(x) => {
                refused += 1;
                said.extend(format!("{} refused: {}\n", s.name, x.0).bytes());
            }
        }
        said.extend(a.state_hash().0);
        if i % 50 == 49 {
            for user in USERS {
                let ctx = Ctx {
                    actor: user.into(),
                    ..ctx.clone()
                };
                for v in &views {
                    let input = view_input(&a, v);
                    match native.ask(&a, &ctx, v, &input) {
                        Ok(x) => said.extend(x),
                        Err(e) => said.extend(e.0.bytes()),
                    }
                }
            }
        }
    }
    (a.state_hash().hex(), ark::hash::hex(&sha256(&[&said])), refused, moved)
}

/// A view's input over what the store holds.
fn view_input(a: &Store, v: &str) -> Vec<u8> {
    let lists = playlist_ids(a);
    let on = lists.first().copied().unwrap_or_default();
    match v {
        "library.library" => Library { playlist_id: on }.to_bytes(),
        "library.search" => Search {
            playlist_id: on,
            needle: "a".into(),
        }
        .to_bytes(),
        "library.album" => AlbumInput {
            playlist_id: on,
            name: "Air".into(),
        }
        .to_bytes(),
        "library.artist" => Artist {
            playlist_id: on,
            name: "Bach".into(),
        }
        .to_bytes(),
        "library.album_credits" => AlbumCredits { name: "Air".into() }.to_bytes(),
        "library.works" => Works { composer: "Bach".into() }.to_bytes(),
        "library.work" | "library.recordings" => {
            let w = a.rows("work").next().map(|b| Work::from_bytes(b).unwrap().id).unwrap_or_default();
            if v == "library.work" {
                WorkInput { id: w }.to_bytes()
            } else {
                Recordings { work_id: w }.to_bytes()
            }
        }
        "library.credits" | "library.recording" => {
            let rec = a
                .rows("recording")
                .last()
                .map(|b| Recording::from_bytes(b).unwrap().id)
                .unwrap_or_default();
            if v == "library.credits" {
                Credits { recording_id: rec }.to_bytes()
            } else {
                RecordingInput { playlist_id: on, id: rec }.to_bytes()
            }
        }
        "playlists.playlists_of" => PlaylistsOf {
            media_id: media_ids(a).first().copied().unwrap_or_default(),
        }
        .to_bytes(),
        "playlists.playlist" => PlaylistInput { playlist_id: on }.to_bytes(),
        _ => vec![],
    }
}

/// Over twelve seeded scripts of 400 steps, the domain ends where the
/// ArkLang text did and says on the way what it said — refusing over 400
/// steps and moving rows in over 1,500.
#[test]
fn every_seed_reaches_its_pins() {
    let (mut refused, mut moved) = (0, 0);
    let mut got = vec![];
    for (seed, state, said) in PINS {
        let (s, t, r, m) = run(seed);
        (refused, moved) = (refused + r, moved + m);
        got.push(format!("({seed}, \"{s}\", \"{t}\")"));
        assert_eq!((s.as_str(), t.as_str()), (state, said), "seed {seed}; all: [{}]", got.join(", "));
    }
    assert!(refused > 400 && moved > 1500, "{refused} refused, {moved} moved");
}
