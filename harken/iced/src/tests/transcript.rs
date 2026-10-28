//! An episode's transcript, following what plays: a window joined to a
//! harken hub in this process while the phone plays the episode, so where
//! it is in it is the session's clock, moved by a seek from either.
use arkui::vim::{Action, Motion};

use super::now_playing::{carry, dial, hub, phone_who};
use super::who;
use crate::client::Replica;
use crate::domain::{AddEpisode, AddShow, CueAt, SetTranscript, VoiceAt};
use crate::places::{Pane, Source};
use crate::{App, Message};

const FILE: &str = "https://media.transistor.fm/66c2ec4c/cb887ff2.mp3";

/// Where the first lines of Full Time Nix's Nix 2.23.0 episode start, as
/// its SubRip times them.
const STARTS: [i64; 5] = [3_096, 6_736, 18_520, 20_590, 23_096];

/// The episode, and the start of its transcript, in the window's library.
fn episode(p: &mut Replica) {
    let show = AddShow {
        kind: "podcast".into(),
        title: "Full Time Nix".into(),
        ..AddShow::default()
    };
    p.mutate("add_show", &show).unwrap();
    let e = AddEpisode {
        show: "podcast/full-time-nix".into(),
        title: "Nix 2.23.0 release with Jacek Galowicz".into(),
        duration_ms: 3_128_000,
        file: FILE.into(),
        ..AddEpisode::default()
    };
    p.mutate("add_episode", &e).unwrap();
    let said = [
        "Hello and welcome to Full Time Nix.",
        "This is an episode about release notes of Nix 2.23.0 with Jacek Galowicz.",
        "Hi, Jacek.",
        "Hey, Shahar how are you?",
        "Pretty good, pretty good.",
    ];
    let cues = STARTS
        .iter()
        .zip(said)
        .map(|(&start_ms, text)| CueAt {
            start_ms,
            end_ms: start_ms + 2_000,
            speaker: String::new(),
            text: text.into(),
        })
        .collect();
    p.mutate(
        "set_transcript",
        &SetTranscript {
            file: FILE.into(),
            cues,
            voices: vec![],
        },
    )
    .unwrap();
}

/// A window that cannot play, joined to the hub, and the phone playing the
/// episode from its start; the window on its transcript.
fn playing_on_the_phone(hub: &ark_peer::Handle) -> (App, Option<Replica>) {
    let mut client = crate::client::memory_as(who("me"), Some("dev"), crate::client::device(false, "Browser"));
    client.set_roles([crate::domain::LIBRARY]);
    episode(&mut client);
    client.connect_with("hub", dial(hub));
    let mut app = App::with_peer(crate::peer::Peer::open(client), String::new(), None);
    app.listening.open("me-device");
    let mut phone = crate::client::memory_as(phone_who(), Some("phone"), crate::client::device(true, "Phone"));
    phone.connect_with("hub", dial(hub));
    let mut phone = Some(phone);
    carry(hub, &mut app, &mut phone);
    let _ = app.update(Message::Select(Source::Show("podcast/full-time-nix".into(), "Full Time Nix".into())));
    let i = app.peer.rows()[0].clone();
    let queue = vec![crate::listening::Track {
        id: crate::peer::key(i.id),
        title: i.title,
        creator: i.creator,
        album: String::new(),
        duration_ms: i.duration_ms,
        file: i.file,
    }];
    let start = harken_player::Press::Start {
        queue,
        at: 0,
        position_ms: 0,
        from: String::new(),
    };
    let mut player = harken_player::Player::new("phone", None);
    assert_eq!(player.press(phone.as_mut().unwrap(), &start, None), None);
    carry(hub, &mut app, &mut phone);
    let _ = app.update(Message::Select(Source::Transcript));
    (app, phone)
}

/// The seek bar dragged to `secs` and let go.
fn seek(app: &mut App, secs: f32) {
    let _ = app.update(Message::Seek(secs));
    let _ = app.update(Message::SeekDone);
}

/// A line of the transcript pressed plays the episode from it, and the
/// line being said is lit and followed as playback moves — the cursor on
/// it, wherever a seek took it.
///
/// Falsified by `Message::Cue` playing nothing: the phone's place stayed at
/// the start; and by `follow` not moving the cursor: it stayed on the line
/// pressed after the seek bar moved on.
#[test]
fn a_line_pressed_plays_from_it_and_the_line_said_is_followed() {
    let hub = hub();
    let (mut app, mut phone) = playing_on_the_phone(&hub);
    assert_eq!(app.cues().len(), STARTS.len(), "the bar offers what plays' transcript");
    assert_eq!(app.cue_now(), None, "nothing said yet");

    let _ = app.update(Message::Cue(STARTS[3]));
    carry(&hub, &mut app, &mut phone);
    let at = app.now().unwrap().position;
    assert!((20.59..23.096).contains(&at), "the episode from the line pressed: {at}");
    assert_eq!(app.cue_now(), Some(3));
    assert_eq!(app.at(Pane::Tracks), 3, "the cursor on the line being said");

    seek(&mut app, 7.0);
    carry(&hub, &mut app, &mut phone);
    assert_eq!((app.cue_now(), app.at(Pane::Tracks)), (Some(1), 1), "…and on it as it moves");
    assert!(app.following);
}

/// The cursor moved by hand stops following: playback moving on leaves it
/// where it was put, until the line being said is the one it was left on —
/// or Follow is pressed. Falsified by `land` leaving `following` alone: the
/// cursor moved by hand still followed.
#[test]
fn a_cursor_moved_by_hand_stays_until_playback_reaches_it() {
    let hub = hub();
    let (mut app, mut phone) = playing_on_the_phone(&hub);
    seek(&mut app, 4.0);
    carry(&hub, &mut app, &mut phone);
    assert_eq!(app.at(Pane::Tracks), 0);

    let _ = app.act(Action::Move(Motion::Down(3)));
    assert!(!app.following, "moved by hand");
    seek(&mut app, 19.0);
    carry(&hub, &mut app, &mut phone);
    assert_eq!((app.cue_now(), app.at(Pane::Tracks)), (Some(2), 3), "left where it was put");
    seek(&mut app, 21.0);
    carry(&hub, &mut app, &mut phone);
    assert!(app.following, "playback reached the line it was left on");

    let _ = app.act(Action::Move(Motion::Up(3)));
    assert!(!app.following);
    let _ = app.act(Action::Key('f'));
    carry(&hub, &mut app, &mut phone);
    assert_eq!((app.at(Pane::Tracks), app.following), (3, true), "`f` is back on the line being said");
}

/// A voice's name clicked asks who it is, and a name typed is who says
/// that voice's lines from then on — every one of them, and only those.
/// Falsified by naming the line's speaker rather than its voice: the
/// typed name was not the voice's, and the lines stayed `Speaker 2`'s.
#[test]
fn a_voice_named_from_its_line_says_all_its_lines() {
    let hub = hub();
    let (mut app, _phone) = playing_on_the_phone(&hub);
    let voices = ["Speaker 1", "Speaker 1", "Speaker 1", "Speaker 2", "Speaker 1"];
    let cues = STARTS
        .iter()
        .zip(voices)
        .map(|(&start_ms, v)| CueAt {
            start_ms,
            end_ms: start_ms + 2_000,
            speaker: v.into(),
            text: format!("Said at {start_ms}."),
        })
        .collect();
    let voices = ["Speaker 1", "Speaker 2"]
        .map(|v| VoiceAt {
            name: v.into(),
            print: vec![1, 2, 3],
        })
        .to_vec();
    app.peer
        .client
        .mutate(
            "set_transcript",
            &SetTranscript {
                file: FILE.into(),
                cues,
                voices,
            },
        )
        .unwrap();
    app.peer.refresh();

    let _ = app.update(Message::NameVoice(3));
    assert_eq!(app.ctx.picker.as_ref().map(|p| p.title.as_str()), Some("Who is Speaker 2?"));
    let _ = app.update(Message::PickerName("Jacek Galowicz".into()));
    let _ = app.update(Message::PickerCreate);
    assert!(app.ctx.picker.is_none(), "named, the picker is down");
    let said: Vec<String> = app.cues().into_iter().map(|c| c.speaker).collect();
    assert_eq!(said, ["Speaker 1", "Speaker 1", "Speaker 1", "Jacek Galowicz", "Speaker 1"]);
}
