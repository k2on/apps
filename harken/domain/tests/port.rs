//! harken's domain in plain Rust is the one its ArkLang text was (§1.16):
//! N1 ported it and held the two to each other — the same schema, byte for
//! byte; over seeded scripts of every mutator and live transition, by
//! several users and devices, refused and not, the same rows after every
//! step and the same refusals; and every view answering what its query
//! did. With the text gone, what the two agreed on is pinned: the schema's
//! hash and, per seed, the state the script ends at and a hash of its
//! whole transcript — every step's refusal or how many changes it made,
//! the state after it, and every view's answer at every fiftieth step.
//! Falsified by numbering a playlist "Favorites [1]"
//! (`playlists::numbered`): seed 1's pins moved.

use ark::codec::Node;
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

/// The schema's hash, as the ArkLang text's was.
const SCHEMA: &str = "d9900c967063c82175edd94123cb7d785852656375e91be7bf49a97e3b24c3e9";

/// Per seed, the state after 400 steps and the transcript's hash.
const PINS: [(u64, &str, &str); 12] = [
    (
        1,
        "5f9221c52c7fce766be36b9264000a9b31b65b0b9a34091b2fef0c8bcc077825",
        "97505f7c639b8ba729ae4250271f2be9b8d778c9dfea9fb8da7ec42669cf1867",
    ),
    (
        2,
        "8467c96aa0fb8e3634f00730ee349cb1650299dbac7e95dc9b94749b2da7a182",
        "850717b03b32a32177582e3f842eeadd60c8cdf63552f0f168fba2c9308571d9",
    ),
    (
        3,
        "f36f7f9af883cf404f1a5ef64c6434efd4f981e0a7d2802a7dbeeda294569e81",
        "0a1d254b52e40ed2dac5788a0d04e5544eafaf9e20cac647ddda07925aba14b5",
    ),
    (
        4,
        "7a6d337e10635c3bbd74fe7d01afd20ed7d1d79cdc009e42b2201df87befe0fc",
        "20572b4c3caaf4d78ced14fc215609ce7336c81da84440e96de9aaabde873591",
    ),
    (
        5,
        "865f99af4c73ae399ccd120c6754188c4baacf666be8275dcfaa398c4c0b5d05",
        "7ee0e294a2aa98db58099f509cdeeb810586bb5aaaee6ae2df77bb51e7646a39",
    ),
    (
        6,
        "84c1f92adfc3b3c9e2699e53587d5016969d6b01b5c419f6f8f9fee614d1da41",
        "aa1287a4aa11f64cceaf0082f591106dc35d605db939105a8b77192292bb7313",
    ),
    (
        7,
        "1b4ddea79c2284e9bb05d7d054b098f9601466793b876d79a7f4af08db83ca53",
        "50bf855538fb711f9407225c3f361926109c6adcb518964089f818826aa734f5",
    ),
    (
        8,
        "50e9ed1cba5dc32473521a3d00975fd178c78db37f65097e54e8e0f9486835c7",
        "e6ee354dc97b6cf33ec706b02d1eee31c02844b91a08b47c46aa67ef3f2619be",
    ),
    (
        9,
        "f80441f07580fe5e511399b1b6d9ce1f11d7dfb47a73cc5d6b95f4d9dd96710a",
        "26808001f8ab8abb53921e0617496887d39f3164856de593086c88dd4b3ad663",
    ),
    (
        10,
        "0a831c7a60b4f6ad64d9422a7868f879d8d0ee3aaa3dbab203bfdfacc7825137",
        "2e565e860e4b1aa280df40a3ecfb8e5f7f5a0341474b790da4768cc0dbdf18be",
    ),
    (
        11,
        "64ad2af1b2f78b78251d65e9fdd9aef07f5d604b0f67b8d9e304067210497cfc",
        "eaf522f01f649f0296a0664de19316f77bb1a79f7eaa9475451e2da2a0aa5d0e",
    ),
    (
        12,
        "ad45845319ff8b0bc07c38110c6c7f8a44644f27d51334f2a1490a02aa6f42f8",
        "bb9f95def54af526e3332b38fa33f6e1fa9bd5d1176c3f8eed0fd2700094f565",
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
    assert_eq!(views.len(), 18);
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
