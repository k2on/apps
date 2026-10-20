//! The volume controls and the groups, through this window: which rows the
//! device picker draws for a session with a group in it, what each key does
//! on a row, and which transition a control calls. The session is set by
//! hand — what the reconciler would have read — because these are the
//! window's rules over a session, not the session's (those are the domain's
//! tests, and the reconciler's are `listening`'s).
use ark::author::Id;

use super::device;
use crate::listening::{Active, Clock, Device, Kind, Listening, Output, Track};
use crate::{App, Change, Level, Message, Volumed, NUDGE};

const KITCHEN: &str = "media_player.kitchen";
const STUDY: &str = "media_player.study";
const DEN: &str = "media_player.den";

fn speaker(id: &str, name: &str, volume: i64, follows: Option<&str>) -> Device {
    Device {
        can_volume: true,
        volume,
        family: Some("sonos".into()),
        follows: follows.map(str::to_string),
        ..device(id, name, Kind::Speaker, true)
    }
}

/// A window whose device is the phone, over a session playing on the
/// kitchen with the study joined to it, the den beside them, and the phone,
/// which has said nothing of a volume.
fn app() -> App {
    let mut app = App::with_peer(super::alone("me"), String::new(), None);
    app.listening.open("phone");
    let track = Track {
        id: Id::new([1; 16]),
        title: "Air".into(),
        creator: "Bach".into(),
        album: String::new(),
        duration_ms: 300_000,
        file: "music/air.mp3".into(),
    };
    app.session = Some(Listening {
        devices: vec![
            device("phone", "Phone", Kind::Phone, true),
            speaker(KITCHEN, "Kitchen", 30, None),
            Device {
                muted: true,
                ..speaker(STUDY, "Study", 60, Some(KITCHEN))
            },
            speaker(DEN, "Den", 10, None),
        ],
        scrub: Default::default(),
        session: Some(Active {
            queue: vec![track],
            at: 0,
            clock: Clock::Paused { at_ms: 0 },
            output: Output::Device {
                id: KITCHEN.into(),
                moving: false,
            },
            from: String::new(),
        }),
    });
    app
}

/// The picker of a session playing on a group: the group first, as itself,
/// at its members' level; then every device by name, ticked where it plays
/// what the output plays, with its own volume where it has one — and a `+`
/// on the speaker that can join, a `−` on the one that can leave.
///
/// Falsified by a `picker_rows` that drew no group row: the first row was
/// the den.
#[test]
fn the_picker_draws_the_group_its_members_and_who_can_join() {
    let app = app();
    let rows = app.picker_rows();
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Kitchen + 1", "Den", "Kitchen", "Phone", "Study"]);
    let group = &rows[0];
    assert!(group.group && group.playing && !group.takeable);
    assert_eq!(
        (group.volume, group.muted),
        (Some(45), false),
        "the mean of 30 and 60; one of two muted is not muted"
    );
    let by = |name: &str| rows.iter().find(|r| r.name == name).unwrap();
    assert_eq!((by("Den").join, by("Den").playing), (Some(true), false));
    assert_eq!((by("Study").join, by("Study").playing), (Some(false), true));
    assert_eq!(
        (by("Kitchen").join, by("Kitchen").playing),
        (None, true),
        "the leader neither joins nor leaves"
    );
    assert_eq!(
        (by("Phone").join, by("Phone").volume),
        (None, None),
        "a phone joins nothing and said no volume"
    );
    assert_eq!((by("Study").volume, by("Study").muted), (Some(60), true));
}

/// On a picker row, `J` joins the speaker or takes it out, `+` and `-` move
/// its volume — the group's on the group's row — and `M` mutes it; a row
/// with no volume takes no volume key, and one that can neither join nor
/// leave takes no `J`.
///
/// Falsified by a `device_key` that sent `Join` for every row with a `join`:
/// the study was joined again rather than taken out.
#[test]
fn the_keys_on_a_picker_row_join_it_or_move_its_volume() {
    let mut app = app();
    let at = |app: &App, name: &str| app.picker_rows().iter().position(|r| r.name == name).unwrap();
    app.devices = Some(at(&app, "Den"));
    assert!(matches!(app.device_key('J'), Some(Message::Join(id)) if id == DEN));
    app.devices = Some(at(&app, "Study"));
    assert!(matches!(app.device_key('J'), Some(Message::Unjoin(id)) if id == STUDY));
    assert!(matches!(app.device_key('+'), Some(Message::Nudge(Level::Device(id), NUDGE)) if id == STUDY));
    assert!(matches!(app.device_key('-'), Some(Message::Nudge(Level::Device(_), by)) if by == -NUDGE));
    assert!(matches!(app.device_key('M'), Some(Message::Mute(Level::Device(id))) if id == STUDY));
    app.devices = Some(0);
    assert!(matches!(app.device_key('='), Some(Message::Nudge(Level::Group, NUDGE))));
    assert!(app.device_key('J').is_none(), "a group is not joined to itself");
    app.devices = Some(at(&app, "Phone"));
    assert!(app.device_key('+').is_none() && app.device_key('J').is_none());
}

/// What a control calls: the bar's is the group's while the output is one,
/// a row's is its own device's, and a mute is the other way from how it is.
/// With nothing playing, the bar is this device's — and a device that has
/// said no volume has no control at all.
///
/// Falsified by a `volume_call` that muted whatever the mute was: the
/// study, muted, was muted again.
#[test]
fn a_control_calls_the_groups_transition_or_its_devices() {
    let mut app = app();
    assert_eq!(app.volumed(&Level::Output), Some(Volumed::Group));
    let (t, _) = app.volume_call(&Volumed::Group, Change::To(50)).unwrap();
    assert_eq!(t, "set_group_volume");
    let (t, _) = app.volume_call(&Volumed::Group, Change::By(-5)).unwrap();
    assert_eq!(t, "nudge_group_volume");
    let study = Volumed::Device(STUDY.into());
    let (t, input) = app.volume_call(&study, Change::Mute).unwrap();
    assert_eq!(t, "set_mute");
    assert_eq!(input, crate::listening::input::set_mute(STUDY, false), "muted, so unmuted");
    let (t, input) = app.volume_call(&study, Change::To(20)).unwrap();
    assert_eq!((t, input), ("set_volume", crate::listening::input::set_volume(STUDY, 20)));
    assert_eq!(app.volumed(&Level::Device("phone".into())), None, "the phone said no volume");

    // Nothing playing: the bar is this device's, which has no volume yet.
    if let Some(a) = app.session.as_mut().and_then(|s| s.session.as_mut()) {
        a.output = Output::Nowhere;
    }
    assert_eq!(app.volumed(&Level::Output), None);
    if let Some(s) = app.session.as_mut() {
        s.devices[0].can_volume = true;
    }
    assert_eq!(app.volumed(&Level::Output), Some(Volumed::Device("phone".into())));
}
