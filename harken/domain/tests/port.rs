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
                folder: String::new(),
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
const SCHEMA: &str = "b0f869fb912d16806b2340366f5b5f865e1ebfa4b6fd852050f1c0c35602249c";

/// Per seed, the state after 400 steps and the transcript's hash, as the
/// value format writes them (postcard, N4).
const PINS: [(u64, &str, &str); 12] = [
    (
        1,
        "ddb40130e6bb64f67f83ed19045e1ae429bc1be6adacb4522d8f1934ac868957",
        "995333e50012f205de5a9fb993d880b2ff6ed8472a12352887a84bd6363d8355",
    ),
    (
        2,
        "fb0cae3eb3e23fbe05e24a665b926715a63a2ac71bc13c60d44d9d49816c710d",
        "f349a611188a595e0c8d4b5589ca58103ff126b4333cc7a2cbc8d61068c90038",
    ),
    (
        3,
        "9ab57014d03333c09065ee49f231afa429df6683bb283d1fa186fe25d035c9fb",
        "97651a07b7a4f069180fc5c377165b9dc2ba211151a498d50e57f9d6ba4ce8cf",
    ),
    (
        4,
        "3215a8336650a0c51b3ed28229ce7fabb6162e1e3847b334b22252e5c38a4e0a",
        "207e8f2303a5746689352f40bb03d0b2944db746e4a95aaae68ce2c5395ec013",
    ),
    (
        5,
        "e8c3d5ed130384c3e42c515cab76e78b5b6cd9a18b1ff5d00be7bfcd7d3be1a8",
        "252829a7f5a3ae7ec8ec63138accbd4281fd12b4d3019dbac746978367d5d3a8",
    ),
    (
        6,
        "e7dbf908e08ea732474cfc4204818f17820695eb0f33d4fe465e7e57325f8c9c",
        "bd34d50788eb3ab7620d068db0443ae76d4f6358d36ad29c6e6e5f710d980591",
    ),
    (
        7,
        "642b50a654b9468c232651f439ed9371ed24cfce82d8e84ffe9ed41f1ac478b4",
        "5d79d8dd3f88d0e0ee51ad8c332a2ddc3fea079dfaef8b684ad1b451c265cfb4",
    ),
    (
        8,
        "20971f16633bc12c5eeb2d78ee532b233162f3248eec3c2c8d469571d7d0a169",
        "6129819c80ee3c933e69fffc2e85087666098e0b53bd0ae47b4378b1b1e6da74",
    ),
    (
        9,
        "fc7289984c604b1f59fbc464175dcca0ac2bb389add8eb1c3a73232844ff6a5a",
        "34d0fb8ae67fef6daa42d720ba5ad46882913df373671398125742c3fd4bcbd0",
    ),
    (
        10,
        "04d16261e621af38a4147036efedbd64f21f6dc1f9d4f492bc98fd974b86a9e4",
        "c2a07ef344bc190bf42f83ff0ef98649a8baf41f5ef78515feff837db0ba0108",
    ),
    (
        11,
        "e9c8a413014eabb09b0eea6eff72824efaeeab92572425c261e3602fb13a578a",
        "08919aefbe158a075957ca0380c8c3ae4df1304f9f1e1f8e9a666117ceee3fef",
    ),
    (
        12,
        "dcd4714f86d627b57204d3b5c07a904ecbba6147340597c3219685a852407cd3",
        "d7b0645d53bf5277e5d3f02100a03391e1f9e3b69b406dec719ddec409978fec",
    ),
];

/// The schema the structs derive hashes as the ArkLang schema did.
/// Falsified by declaring `playlist`'s indexes in another order.
#[test]
fn the_schema_hash_is_pinned() {
    assert_eq!(domain().schema_hash().hex(), SCHEMA);
}

/// The schema reads every row the one a server last stood on wrote
/// (`tests/schema.json`): as they are (`ark::schema::compat`), or through
/// the domain's migrations (`src/migrations.rs`). A column appended with
/// neither an `Option` type nor a default, and no step that reads the rows
/// before it, is a server that does not start. `ARK_WRITE_GEN=1` writes
/// this schema there, once it is the one the server runs.
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
    let read = ark::migrate::migrate(&was, &vec![], &now, &domain().migrations);
    assert!(read.is_ok(), "{read:?}");
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
    assert_eq!(views.len(), 32);
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
            id: "/air".into(),
        }
        .to_bytes(),
        "library.artist" => Artist {
            playlist_id: on,
            name: "Bach".into(),
        }
        .to_bytes(),
        "library.album_credits" => AlbumCredits { id: "/air".into() }.to_bytes(),
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
