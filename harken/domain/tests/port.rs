//! harken's domain in plain Rust is its domain in ArkLang (§1.16): the same
//! schema, byte for byte, and over seeded scripts of every mutator and live
//! transition — by several users and devices, refused and not — the same
//! rows after every step, table by table, and the same refusals; and every
//! view answers what its query did. Falsified by numbering a playlist
//! "Favorites [1]" (`playlists::numbered`): the second "Favorites" moved
//! the rows; by `add_song` keeping a stale album's art: the album table
//! moved; and by `skip` going two tracks: the listening table moved.

use std::sync::Arc;

use ark::ir::Node;
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

fn digests(s: &Store) -> Vec<(String, [u8; 32])> {
    s.schema_arc()
        .tables
        .iter()
        .map(|t| (t.name.clone(), s.digest(&t.name).unwrap()))
        .collect()
}

/// The schema the structs derive is the ArkLang schema, byte for byte.
/// Falsified by declaring `playlist`'s indexes in another order.
#[test]
fn the_schema_is_the_arklang_schema() {
    let (native, lang) = (domain(), harken_domain::text());
    assert_eq!(*native.schema, *lang.schema);
    assert_eq!(native.schema_hash(), lang.schema.hash());
}

/// Over seeded scripts, both stores hold the same rows after every step
/// and refuse the same steps with the same words; and every view answers
/// what the query of its name did, for each user.
#[test]
fn every_step_moves_the_same_rows_and_every_view_answers_the_same() {
    let (native, lang) = (domain(), harken_domain::text());
    let schema = Arc::new((*native.schema).clone());
    let views: Vec<&str> = native
        .fns
        .values()
        .filter(|f| f.kind == ark::domain::Kind::View)
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(views.len(), 18);
    let (mut refused, mut moved) = (0, 0);
    for seed in 1..=12u64 {
        let (mut a, mut b) = (Store::new(schema.clone()), Store::new(schema.clone()));
        let mut r = Rng(seed);
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
            let ran = f.apply(&mut a, &ctx, &s.input);
            let was = lang.run(&b, &ctx, s.name, &s.input).and_then(|cs| b.apply_all(&cs).map(|_| cs));
            match (&ran, &was) {
                (Ok(x), Ok(y)) => {
                    assert_eq!(x.len(), y.len(), "seed {seed} step {i} {}: the changes", s.name);
                    moved += !x.is_empty() as usize;
                }
                (Err(x), Err(y)) => {
                    assert_eq!(x.0, y.0, "seed {seed} step {i} {}", s.name);
                    refused += 1;
                }
                _ => panic!("seed {seed} step {i} {}: {ran:?} against {was:?}", s.name),
            }
            assert_eq!(digests(&a), digests(&b), "seed {seed} step {i} {}", s.name);
            if i % 50 == 49 {
                for user in USERS {
                    let ctx = Ctx {
                        actor: user.into(),
                        ..ctx.clone()
                    };
                    let lists = playlist_ids(&a);
                    let on = lists.first().copied().unwrap_or_default();
                    for v in &views {
                        let input = match *v {
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
                                if *v == "library.work" {
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
                                if *v == "library.credits" {
                                    Credits { recording_id: rec }.to_bytes()
                                } else {
                                    RecordingInput { playlist_id: on, id: rec }.to_bytes()
                                }
                            }
                            "playlists.playlists_of" => PlaylistsOf {
                                media_id: media_ids(&a).first().copied().unwrap_or_default(),
                            }
                            .to_bytes(),
                            "playlists.playlist" => PlaylistInput { playlist_id: on }.to_bytes(),
                            _ => vec![],
                        };
                        let x = native.ask(&a, &ctx, v, &input).map_err(|e| e.0);
                        let y = lang.ask(&b, &ctx, v, &input).map_err(|e| e.0);
                        assert_eq!(x, y, "seed {seed} step {i}: {v} as {user}");
                    }
                }
            }
        }
    }
    assert!(refused > 400 && moved > 1500, "{refused} refused, {moved} moved");
}
