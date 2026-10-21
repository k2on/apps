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

/// The schema's hash (its bytes', postcard; `edd9a961…` its JSON's).
const SCHEMA: &str = match ark::format::NAME.as_bytes() {
    b"json" => "edd9a96187efea9d3cd816afb5ffc383839bb2d246be32354e6a601bff012751",
    _ => "748e53c5ffac28aa4f032b7b8496da2052231f57e47a5289dd8ba7bd8dade7dd",
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
        "b8db5481d025690ef3240fa4ffe20acfe4c91a43d448a1bd774e6d9c30fe6041",
        "cd6056003a23bd7c36f28dd1c0789e06df572c43436c5ab284ae36065cdeef19",
    ),
    (
        2,
        "5d04ba61b069ff408f2a80fe4a195d373732a678386179e56e6ce430d562d859",
        "80fecada08e8c6da8233d8db3b1f1eb3a393567641f1443e1cf580a41a79642e",
    ),
    (
        3,
        "d36fa93b102932483bdd57d1f7c642f1f9b1458784d1f502fca172ae1727770b",
        "3c290b3ad5e3aa0d6e2ab90b384fcc35e65ca82e304d89219aeb575d05da8ac9",
    ),
    (
        4,
        "93030d16cc32086826fd0c7bd08e3f707f04a3c57c4ade7e2bd783813aba4351",
        "d1ed728c3e0cb8a86b50a82c2db6d640c2f805d09301caef24c5389e5b07260e",
    ),
    (
        5,
        "5c53715f73f2c3744296839633c32187bfbee001f0abcb94d015eb0f5109cdc0",
        "eab09f5a198fb9757fa01ecdc0f9620490aab919eded399dad3091992054ca1d",
    ),
    (
        6,
        "3dc080acbffe1ffa9507511f885389180d9c5fa781c7c0659df95d7a9236cdbe",
        "ff4c98e3d23b749f355ace9236a44e3acbdc8ad75232050a16f0a1a181ee0c07",
    ),
    (
        7,
        "ca207f22558cd21442fc08b94ab0f4ca55fd6466b12488a3e1fa119b89d9e8ec",
        "7bddcaeed1a50b72c9f73c1c38e10ecdd758edb69fdad799edc6e1e2c2e3c13d",
    ),
    (
        8,
        "68b0531b863ecd24349c2e4c95f165e56f3665b156bfa069171923de045ee501",
        "106f7a0513e128cb142298015bb1d704965065642df81fa4eae6332415d810c4",
    ),
    (
        9,
        "f97d3f3be9193c114bbc38a8c585d3c2701f65175a1f9b0414742ef50341b8b9",
        "d79154b4be42b1d1e83427fd317b168544dfbc12aa91be00fb0348d6e681fe21",
    ),
    (
        10,
        "eb5d676854e3c6884d9f519fe2ecc887cc1f33fbe4ad57494dc28ee468f50403",
        "36254f4dd26e566321122c31b409853c9f0e849e5452d05b921c1c87e92a5ea4",
    ),
    (
        11,
        "3702ccd0524af1f41f5eae7b387d277ba80bb330f27bb33aa97d68da2947c885",
        "9b57cc2da737b8d1ee4b020217c1bc910b7db060a38fcb042efe2a31766fc153",
    ),
    (
        12,
        "71c894f8d45ded711a3fe8a60e152d64b4daf352b21da10cf49965a6941b5211",
        "998c1291d10a94949ebc1e85f372ef1d0bf14f2d491bb3db569b8abd6c3330a0",
    ),
];

const PINS_JSON: [(u64, &str, &str); 12] = [
    (
        1,
        "2fce4484670ac1b7569a55629eafc93d43e7b846ced34ff698c1c38a53dd94ef",
        "0e4559745b58ec0f580f3d40a56539f2813d4415e35a0c555f94a055ccde238a",
    ),
    (
        2,
        "c22c99d3b50d9082c9382053d70fd962d10f334126b5cdd74fd892ca6e0c7845",
        "09ba9c822a842ec49bbfaa13a416c673cef65ad17500bf2f7ad1d9bead8875c7",
    ),
    (
        3,
        "d146635e694769f50d14824238cf7b747919b36a2eafc0acd2268d47d74b5cbf",
        "5ddc6bfff3e0a4c7fcce2652a5af16bc4fb80a6803f9305f80259631ab885877",
    ),
    (
        4,
        "7fb2c255505fc0a8b79517e558968fe2aa2173c29c039982cd44241e8c5eed96",
        "bb6441911cb525d68344659336f9377d7212c3b366706c01fd548304382329d2",
    ),
    (
        5,
        "12e81dddd75c70e9b97e309d9bd4866f661587fbe3dc4b37912a5211e0988db2",
        "3e12576650c01a4aaa46de5f261bbcd209a232a01983bc376f0a13e2c9b14876",
    ),
    (
        6,
        "98acd2926b5026fc3bcf657cb60f90dd6970f0cbad96549ba3d00ae0be103773",
        "e1ee63dd4db360d6bc8faf09054c3fa5b4148daa5d0edb6049b9302e1661f95c",
    ),
    (
        7,
        "6b54361e357c947e332e6e37c89fdc2cd4b6d1cef0ff766fa82bf85ab29f66be",
        "d1b39eee5350cdfdac07b4b5a3bb5cce720f9f7577f06742face5c25d4437165",
    ),
    (
        8,
        "0b6b5c3861689a57a2e476c7e41b99a5b590b3fc8b8522ea6affc7613d7341d8",
        "55c9e4cd19f151a62496e5085b3d8d628a0137f7c88fa9c20e5f27b39d605634",
    ),
    (
        9,
        "18966b40b913c2eba94a3d384571631b61156d12456929071c303b04b8d2da12",
        "07af033a093bcbcf8aab6eb1707c100c73433f17d5bf4934122cb1cbdccbe8b5",
    ),
    (
        10,
        "ad38c47ba5dd115dadcbfa94806355575d54ccb1e8ffa91cfa73952aa1e804e7",
        "8c5dc54248e83113128257cf90e1f8f317d8a06c6e1b56b0ff98162aee775088",
    ),
    (
        11,
        "1ac78295a5f4dcf86b8ec6dac03884e8c2fd8cbf5d4bfe40ff125c93a737855f",
        "cc87b1458313de4802295e38b6bdb004ffc1fc734e77135e5d123206675eaa6e",
    ),
    (
        12,
        "35aa2fc37d4387f97ba1c63bf80f0fec72afe162eeaeb94f1df531eed1d898e5",
        "a055d1becee91a1bfea9c9d3811ff8997afcda99638421fd8fcf8203989ebd54",
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
