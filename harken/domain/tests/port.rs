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

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) % n.max(1)
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len() as u64) as usize]
    }
}

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

/// The schema's hash (its bytes', postcard; `9ccaa3bb…` its JSON's).
const SCHEMA: &str = match ark::format::NAME.as_bytes() {
    b"json" => "9ccaa3bb710008e648b2c55636e2c7ceb4af5bcdc5535827adf07daed52997f8",
    _ => "5866fbdb4fabf5d7e2ecbd0d07c5f1b7034351543f66f53b92d1170654a5914c",
};

/// Per seed, the state after 400 steps and the transcript's hash, as the
/// value format writes them: postcard (N4), and JSON as N1 pinned them.
const PINS: [(u64, &str, &str); 12] = match ark::format::NAME.as_bytes() {
    b"json" => PINS_JSON,
    _ => PINS_POSTCARD,
};

const PINS_POSTCARD: [(u64, &str, &str); 12] = [
    (
        1,
        "edbb1611b95c7524f223206f67f41821248e6527f3d3105da693bf267cae0af9",
        "02ea91a1a665087c09ff4661d6c18da7d457c54353f9354e0fb9475181b1c057",
    ),
    (
        2,
        "d17e82a9561eae93fe63dbf48ebdfa022dc45919a40adba734506567d12a1b29",
        "cf623ea4943706719dadc13e71295911b36ae7972e545ae225d6250197b8161c",
    ),
    (
        3,
        "50ab7e238c8dd280004beeca8ef44a6c7dd967df557b0a3e8f41d16d9d85b957",
        "bd548fbd6c757056cfa4348e22a7e7ffc80b7f6383ef8356ec0e3f5bc767f985",
    ),
    (
        4,
        "f62be8f3a4369d767a442072358b7b01a103b9d853d6772d699cfb798c97d4dd",
        "5a5f29976c242ccc62a902419e293165f64701193b5fbc0f218c9a419d284240",
    ),
    (
        5,
        "1e9830e2d4d7550d4fd0d157a771ad778248f3175c8da05ee56acebcd361abb5",
        "6ff2a6016c5ee89a987bc9b8d0a2b8e8eb9908ef5d8c40d87e83334c79430e4e",
    ),
    (
        6,
        "bde7e25e542ec8b0866f84a8b574e3e7718ff1b85e60b3b0663af0e56ae0c7d3",
        "083e409d728039ca59d16ddd1dcc467d66030c11665dbebb92aba3b99b805827",
    ),
    (
        7,
        "af8f9f52591686cb50e58de83113df2339cb2dce1cfef3d862a6f42479060f21",
        "f5a603b58577cd2c1e68f59ac331fca48aab9786bf3155678353bcc7638d5a95",
    ),
    (
        8,
        "e39346f48bce68fc19fe6b1393600d774bd039a928d38bfdf6d759098b0bbb03",
        "dc4c15c4bfcfeb9865964dad253a67c937deb98d57a78a385c700aea89fceb55",
    ),
    (
        9,
        "149f9daa4de2c629dc803245dc6a403b2788e6ccd7cbaa4b9bd7a45db10c0709",
        "4d2194bea23778e09e9e65f41a5bacc0029985ae0266bf87b29a9d70a4ae379d",
    ),
    (
        10,
        "285bd3d8a37307af6e02f2d1245809ea77a535e230b0c55709c7a487b4511f04",
        "8d6446842b0596f28f36932115217baa42c16e6a38347200014e1e3a359a85cc",
    ),
    (
        11,
        "6ff564156fda5f3e319408c927aabd99f8a218f25ba72d83477f03c49c78162b",
        "e1b55cd908e92c226eb65bdbf663c6c9834deaa0dda4d97bd09c6f019b7b3421",
    ),
    (
        12,
        "c824f4d3f2bf8143274c90596a1a3eb200de17974a38900f45c9f9203615d1cd",
        "c6b55d22ddb452a7089720b618da0867c6a2b7b45364a9acd0ff285900cbcaa6",
    ),
];

const PINS_JSON: [(u64, &str, &str); 12] = [
    (
        1,
        "307579a46e5a7c85ee64573c457257296edf136c3274c1d8824ed88d22b352f0",
        "84629fd4ee3eade85f5fbaff06668f216b14725ef13cb569a2019604c4c70aa0",
    ),
    (
        2,
        "0ae89efec6fa17d5cbeb4ecf2990c9ae49ed854895a005248261c90400278cbf",
        "44e05bab4a0d012cbb6c16e6fb354f2ab2ad4728f087074192b7b015870e92de",
    ),
    (
        3,
        "6283966f284818ab3cf4ebdb5e2098b1e9e5e15cb99458c55b0292cc972e8c31",
        "9d7d0e3dc48ea1aaf13620ed8edf98df997c7fd545510f106a8134dd8c90880b",
    ),
    (
        4,
        "237fbdb1d5f5ca4e6ff9a31ddc7b7157b100ead2dc971637e75440790e955c15",
        "0aaa7ca9d29ca443327acddba1540eedbd2241f45d295a7db5402f5b5c76a4b0",
    ),
    (
        5,
        "7de8b878ac351ddeea3922d3cd2768fbc607d019609f14f9ce4993b2039d57c4",
        "e7bf1afc54fcd115784afb590e0b913d582c991155f3322614f8d2998efc2284",
    ),
    (
        6,
        "e4c1dd224d901582c29dd6fdc0c1cdb87f2da079242405cdea5b96366d67b203",
        "89f723979306c3cff904af624ae033c376494d1d0ca75b298f3cd176e89e0fd0",
    ),
    (
        7,
        "fef062d2c3aca71a2fd933df0a12082059d0c17f69ab62d7cb806cce1535dcdd",
        "b84fa518abe5f35e8845be3de933afbe6932a38d3e238054760d507d68e4111f",
    ),
    (
        8,
        "8380992eb028f3628325d4ab0dbe6ad88e4d69dcae55bf9ee259378f860e73e8",
        "11ad10d86d3b2f91fd0e62900da81f6c5be57522272e78ac385dbc8356875b64",
    ),
    (
        9,
        "5dc12cece6c8932f8a3e5f7f55db31e9124f73885859e6650391bb9cee10e232",
        "6d06db67f7d287948572cf15b859933ceee643a00f600edd7aa3d5c3242fb885",
    ),
    (
        10,
        "4156bf877fe1885bdaa922a75070e3c9040be0ec0fb59fcf841722b74564878d",
        "aaa72228a3348f9cbc618e94b962b294e7ccc9856b60a67199265c9a24b6d69f",
    ),
    (
        11,
        "d79cafad0fac46622b406dedad0f13f9286a47433f3bb52fae5ef66777669b03",
        "980fef6ca17ceba6c82d84921e14c8a74bc92207a3658ad496ab85694aa51639",
    ),
    (
        12,
        "15a2fb5159f9633c4ce2db611b20e90d08890cc7d627ff1d49e7b007d4fb1468",
        "71a802e4f0bf2c46a5fa075158680662097af0c48e0e28c8598c44d0c109a5ca",
    ),
];

/// The schema the structs derive hashes as the ArkLang schema did.
/// Falsified by declaring `playlist`'s indexes in another order.
#[test]
fn the_schema_hash_is_pinned() {
    assert_eq!(domain().schema_hash().hex(), SCHEMA);
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
    assert_eq!(views.len(), 19);
    let (mut refused, mut moved) = (0, 0);
    let mut a = Store::new(native.schema.clone());
    let mut r = Rng(seed);
    let mut said: Vec<u8> = vec![];
    for i in 0..400 {
        let s = step(&mut r, &a);
        let ctx = Ctx {
            draws: Default::default(),
            id: EntryId::of("port", i),
            actor: s.user.into(),
            session: s.device.into(),
            roles: s.roles.clone(),
            now: 10_000 + i * 977,
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
