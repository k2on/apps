//! The listening session's rules, against a real hub with no socket under it.
//!
//! Every device here is a peer the hub stands in for (`HubHandle::stand`),
//! which is what a device with no replica is and exactly what the Home
//! Assistant bridge does for a speaker — so the harness is the feature
//! rather than a stub of it. A read of the hub after each step is the
//! barrier: the hub handles one command at a time and hands a standing peer
//! its frames while handling it, so once a read comes back everything said
//! before it has been heard.
//!
//! What most of these are about is one sentence of a bug report: *"if I'm
//! playing something on a device, and then the connection goes, it switches
//! to the local device, when it should remain on the last device played."*

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use ark::live::ConnId;
use ark_server::{App, HubHandle};
use harken_domain::listening::{Command, Hear, Kind, Say, Session, Track};
use harken_server::listening::Desk;

type Heard = Arc<Mutex<Vec<(String, Hear)>>>;

/// One account's devices, and the hub they are all standing in.
struct House {
    _app: App,
    hub: HubHandle,
    user: String,
    conns: BTreeMap<String, ConnId>,
    heard: Heard,
}

fn hub(data: Option<&Path>) -> App {
    let mut b = ark_server::builder(ark_client::Domain::new(&harken_domain::module()))
        .name("test")
        .trusting()
        .live(Desk::new());
    if let Some(d) = data {
        b = b.data(d);
    }
    b.build().unwrap()
}

impl House {
    fn new(user: &str) -> House {
        House::on(hub(None), user)
    }

    fn on(app: App, user: &str) -> House {
        House {
            hub: app.hub.clone(),
            _app: app,
            user: user.into(),
            conns: BTreeMap::new(),
            heard: Heard::default(),
        }
    }

    fn settle(&self) -> Told {
        self.hub.read_blocking(|_| ()).unwrap();
        Told(std::mem::take(&mut *self.heard.lock().unwrap()))
    }

    /// A device of `user` stands in the room, named `device`, without
    /// saying anything yet.
    fn stand(&mut self, user: &str, device: &str) -> Told {
        let (heard, who) = (self.heard.clone(), device.to_string());
        let conn = self
            .hub
            .stand(user, device, move |frame| {
                if let Ok(h) = Hear::decode(&frame) {
                    heard.lock().unwrap().push((who.clone(), h));
                }
            })
            .unwrap();
        self.conns.insert(device.into(), conn);
        self.settle()
    }

    /// A device arrives and says what it is — the two halves a client does
    /// in one breath.
    fn arrive(&mut self, device: &str, audible: bool, kind: Kind) -> Told {
        let user = self.user.clone();
        self.stand(&user, device);
        self.say(
            device,
            Say::Here {
                name: device.into(),
                audible,
                kind,
            },
        )
    }

    /// Its connection went. Not a decision about the music — which is the
    /// whole point of the thing under test.
    fn leave(&mut self, device: &str) -> Told {
        if let Some(c) = self.conns.remove(device) {
            self.hub.detach(c).unwrap();
        }
        self.settle()
    }

    fn say(&mut self, device: &str, what: Say) -> Told {
        self.hub.say(self.conns[device], what.encode()).unwrap();
        self.settle()
    }

    /// The session as every device last saw it: a `Here` from a device that
    /// is already here changes nothing about the output, and goes through
    /// the same broadcast everything else does.
    fn session(&mut self) -> Session {
        let device = self.conns.keys().next().expect("somebody is here").clone();
        let told = self.say(
            &device,
            Say::Here {
                name: device.clone(),
                audible: true,
                kind: Kind::Computer,
            },
        );
        told.state(&device).expect("a broadcast").clone()
    }
}

#[derive(Debug, Default)]
struct Told(Vec<(String, Hear)>);

impl Told {
    /// What one device was told to do.
    fn dos(&self, device: &str) -> Vec<Command> {
        self.0
            .iter()
            .filter(|(who, _)| who == device)
            .filter_map(|(_, h)| match h {
                Hear::Do { command } => Some(command.clone()),
                Hear::State { .. } => None,
            })
            .collect()
    }

    /// The last session one device was told.
    fn state(&self, device: &str) -> Option<&Session> {
        self.0.iter().rev().find_map(|(who, h)| match h {
            Hear::State { session } if who == device => Some(session),
            _ => None,
        })
    }
}

fn track(title: &str) -> Track {
    Track {
        id: title.into(),
        title: title.into(),
        creator: "Bach".into(),
        album: String::new(),
        duration_ms: 300_000,
        file: format!("music/{title}.mp3"),
    }
}

fn playing_on(house: &mut House, device: &str, position_ms: i64) {
    house.say(
        device,
        Say::Do {
            command: Command::Start {
                queue: vec![track("a"), track("b")],
                at: 0,
                position_ms: 0,
                playing: true,
            },
        },
    );
    house.say(
        device,
        Say::Report {
            queue: vec![track("a"), track("b")],
            at: 0,
            playing: true,
            position_ms,
        },
    );
}

/// The bug, as one test. A laptop is playing; its connection goes; the
/// sound is still the laptop's.
#[test]
fn the_sound_stays_with_a_device_whose_socket_went() {
    let mut house = House::new("alice");
    house.arrive("laptop", true, Kind::Computer);
    house.arrive("phone", true, Kind::Phone);
    playing_on(&mut house, "laptop", 42_000);
    assert_eq!(house.session().output.as_deref(), Some("laptop"));

    let told = house.leave("laptop");
    let session = told.state("phone").expect("the phone is told");
    assert_eq!(
        session.output.as_deref(),
        Some("laptop"),
        "the sound belongs to the laptop; it is simply not answering"
    );
    assert!(
        !session.playing,
        "and it cannot be playing, there is nobody there"
    );
    assert!(
        !session.device("laptop").expect("still in the picker").here,
        "drawn as away rather than dropped"
    );
    assert_eq!(session.position_ms, 42_000, "and where it had got to");
}

/// A device that merely turns up takes nothing: with the laptop away, a
/// phone opening its app must not become the output just by existing.
#[test]
fn arriving_is_not_how_a_device_gets_the_sound() {
    let mut house = House::new("alice");
    house.arrive("laptop", true, Kind::Computer);
    playing_on(&mut house, "laptop", 10_000);
    house.arrive("phone", true, Kind::Phone);
    house.leave("laptop");

    let told = house.arrive("tablet", true, Kind::Phone);
    assert_eq!(
        told.state("tablet").unwrap().output.as_deref(),
        Some("laptop")
    );
    assert!(
        told.dos("tablet").is_empty(),
        "and it is told to do nothing"
    );
}

/// A command goes to the output, not to whoever asked.
#[test]
fn a_command_goes_to_the_device_making_the_sound() {
    let mut house = House::new("alice");
    house.arrive("laptop", true, Kind::Computer);
    house.arrive("phone", true, Kind::Phone);
    playing_on(&mut house, "laptop", 0);

    let told = house.say(
        "phone",
        Say::Do {
            command: Command::Pause,
        },
    );
    assert_eq!(told.dos("laptop"), [Command::Pause]);
    assert!(told.dos("phone").is_empty());
    assert_eq!(
        house.session().output.as_deref(),
        Some("laptop"),
        "and asking did not move it"
    );
}

/// …unless the output cannot be reached and the asker can make a sound —
/// and only for a press that *means* a sound.
#[test]
fn only_a_press_that_means_a_sound_takes_it_from_a_device_that_has_gone() {
    let mut house = House::new("alice");
    house.arrive("laptop", true, Kind::Computer);
    house.arrive("phone", true, Kind::Phone);
    playing_on(&mut house, "laptop", 77_000);
    house.leave("laptop");

    for quiet in [Command::Pause, Command::Seek { position_ms: 5 }] {
        let told = house.say("phone", Say::Do { command: quiet });
        assert!(told.dos("phone").is_empty());
        assert_eq!(
            told.state("phone").unwrap().output.as_deref(),
            Some("laptop"),
            "nothing to pause is not a reason to take the music"
        );
    }

    let told = house.say(
        "phone",
        Say::Do {
            command: Command::Next,
        },
    );
    assert_eq!(
        told.state("phone").unwrap().output.as_deref(),
        Some("phone"),
        "somebody pressed a button that makes a sound and nothing else can answer"
    );
    assert_eq!(
        told.dos("phone"),
        [
            Command::Start {
                queue: vec![track("a"), track("b")],
                at: 0,
                position_ms: 77_000,
                playing: true,
            },
            Command::Next,
        ],
        "handed the session first — a phone that has been watching a laptop all \
         evening holds no queue of its own — and then the press it made"
    );
}

/// A device that cannot make a sound never takes it, however hard it
/// presses: the desktop build has no audio device, so it is a remote.
#[test]
fn a_device_that_cannot_be_heard_never_takes_the_sound() {
    let mut house = House::new("alice");
    house.arrive("laptop", true, Kind::Computer);
    house.arrive("desktop", false, Kind::Computer);
    playing_on(&mut house, "laptop", 0);
    house.leave("laptop");

    let told = house.say(
        "desktop",
        Say::Do {
            command: Command::Play,
        },
    );
    assert!(told.dos("desktop").is_empty());
    assert_eq!(
        told.state("desktop").unwrap().output.as_deref(),
        Some("laptop")
    );
}

/// Picking a speaker halfway through a track picks up halfway through it,
/// and until the speaker says otherwise it is *connecting*, not playing.
#[test]
fn picking_a_speaker_picks_up_where_the_track_was() {
    let mut house = House::new("alice");
    house.arrive("phone", true, Kind::Phone);
    house.arrive("media_player.kitchen", true, Kind::Speaker);
    playing_on(&mut house, "phone", 91_500);

    let told = house.say(
        "phone",
        Say::Transfer {
            to: Some("media_player.kitchen".into()),
        },
    );
    assert_eq!(
        told.dos("media_player.kitchen"),
        [Command::Start {
            queue: vec![track("a"), track("b")],
            at: 0,
            position_ms: 91_500,
            playing: true,
        }]
    );
    assert!(
        told.dos("phone").is_empty(),
        "the old output is told nothing but the broadcast"
    );
    let session = told.state("phone").unwrap();
    assert_eq!(session.output.as_deref(), Some("media_player.kitchen"));
    assert_eq!(
        session.moving.as_deref(),
        Some("media_player.kitchen"),
        "still connecting"
    );

    // A report from somebody who is not the output is not believed.
    let told = house.say(
        "phone",
        Say::Report {
            queue: vec![track("z")],
            at: 0,
            playing: true,
            position_ms: 1,
        },
    );
    assert!(
        told.0.is_empty(),
        "nobody is told anything about a report from nowhere"
    );

    // …and the speaker's first report is what ends connecting, because taking
    // a hand-off is exactly "started playing".
    let told = house.say(
        "media_player.kitchen",
        Say::Report {
            queue: vec![track("a"), track("b")],
            at: 0,
            playing: true,
            position_ms: 92_100,
        },
    );
    let session = told.state("phone").unwrap();
    assert_eq!(session.moving, None);
    assert_eq!(session.position_ms, 92_100);

    // A report that changes nothing is not broadcast: a paused output sends
    // those once a second.
    let same = Say::Report {
        queue: vec![track("a"), track("b")],
        at: 0,
        playing: true,
        position_ms: 92_100,
    };
    assert!(house.say("media_player.kitchen", same).0.is_empty());
}

/// The sound cannot be handed to something that is not answering, which is
/// the one way to lose it entirely — nor to something that cannot be heard.
#[test]
fn the_sound_is_not_handed_to_a_device_that_has_gone_or_is_mute() {
    let mut house = House::new("alice");
    house.arrive("phone", true, Kind::Phone);
    house.arrive("media_player.kitchen", true, Kind::Speaker);
    house.arrive("desktop", false, Kind::Computer);
    playing_on(&mut house, "phone", 0);
    house.leave("media_player.kitchen");

    for to in ["media_player.kitchen", "desktop", "nobody"] {
        let told = house.say(
            "phone",
            Say::Transfer {
                to: Some(to.into()),
            },
        );
        let session = told.state("phone").unwrap();
        assert_eq!(
            session.output.as_deref(),
            Some("phone"),
            "not handed to {to}"
        );
        assert_eq!(session.moving, None);
        assert!(told.dos(to).is_empty());
    }
}

/// A device on two connections at once — a tab reloaded before its old
/// socket was noticed gone — is one device, and the old connection closing
/// does not mark it away.
#[test]
fn a_device_on_a_second_connection_is_still_here_when_the_first_goes() {
    let mut house = House::new("alice");
    house.arrive("laptop", true, Kind::Computer);
    house.arrive("phone", true, Kind::Phone);
    playing_on(&mut house, "laptop", 5_000);
    let old = house.conns["laptop"];
    house.stand("alice", "laptop");
    let told = {
        house.hub.detach(old).unwrap();
        house.settle()
    };
    assert!(
        told.0.is_empty(),
        "nothing changed, so nobody is told: {told:?}"
    );
    let session = house.session();
    assert!(session.device("laptop").unwrap().here);
    assert_eq!(session.output.as_deref(), Some("laptop"));
    let told = house.say(
        "phone",
        Say::Do {
            command: Command::Pause,
        },
    );
    assert_eq!(
        told.dos("laptop"),
        [Command::Pause],
        "and a button still reaches it"
    );
}

/// The last device out writes the room down; the next one in reads it
/// back. What is kept is where the music was and what it was — never that
/// it is playing, and never that anybody is there.
#[test]
fn an_empty_room_is_written_down_and_comes_back() {
    let mut house = House::new("alice");
    house.arrive("laptop", true, Kind::Computer);
    playing_on(&mut house, "laptop", 61_000);
    house.leave("laptop");
    assert!(
        house.hub.read_blocking(|h| h.rooms().is_empty()).unwrap(),
        "the room is closed"
    );
    assert!(
        house
            .hub
            .read_blocking(|h| h.kept("alice").is_some())
            .unwrap(),
        "and kept"
    );

    let told = house.arrive("phone", true, Kind::Phone);
    let session = told.state("phone").expect("the phone is told");
    assert_eq!(session.output.as_deref(), Some("laptop"));
    assert_eq!(session.queue, vec![track("a"), track("b")]);
    assert_eq!(session.position_ms, 61_000);
    assert!(
        !session.playing,
        "what was true is where it was, not that it is"
    );
    assert!(!session.device("laptop").expect("drawn as away").here);
}

/// …across a restart too: the kept room is on disk beside the log.
#[test]
fn a_kept_room_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut house = House::on(hub(Some(dir.path())), "alice");
        house.arrive("laptop", true, Kind::Computer);
        playing_on(&mut house, "laptop", 33_000);
        house.leave("laptop");
    }
    let mut house = House::on(hub(Some(dir.path())), "alice");
    let told = house.arrive("phone", true, Kind::Phone);
    let session = told.state("phone").unwrap();
    assert_eq!(session.output.as_deref(), Some("laptop"));
    assert_eq!(session.position_ms, 33_000);
    assert_eq!(session.queue.len(), 2);
}

/// Stopping it everywhere is a row of its own in the picker, and it means
/// what it says — and a room with nothing to keep keeps nothing.
#[test]
fn stopping_it_everywhere_leaves_nothing_the_output() {
    let mut house = House::new("alice");
    house.arrive("laptop", true, Kind::Computer);
    playing_on(&mut house, "laptop", 5_000);

    let told = house.say("laptop", Say::Transfer { to: None });
    let session = told.state("laptop").unwrap();
    assert_eq!(session.output, None);
    assert!(!session.playing);

    let mut empty = House::new("carol");
    empty.arrive("laptop", true, Kind::Computer);
    empty.leave("laptop");
    assert!(
        empty
            .hub
            .read_blocking(|h| h.kept("carol").is_none())
            .unwrap(),
        "nothing played and nowhere to play it: worth no disk"
    );
}

/// A room is an account, not a connection: two people's sessions never
/// meet, however many devices each of them has.
#[test]
fn two_accounts_are_two_sessions() {
    let mut house = House::new("alice");
    house.arrive("laptop", true, Kind::Computer);
    playing_on(&mut house, "laptop", 0);

    let told = house.stand("bob", "bob-phone");
    let session = told.state("bob-phone").expect("bob is told his own");
    assert_eq!(session.output, None);
    assert!(session.queue.is_empty());
    assert!(session.devices.is_empty());
    assert!(
        told.state("laptop").is_none(),
        "and alice is told nothing about bob"
    );
}

/// A frame this build cannot read is dropped, not fatal to the room.
#[test]
fn a_sentence_nobody_can_read_changes_nothing() {
    let mut house = House::new("alice");
    house.arrive("laptop", true, Kind::Computer);
    house
        .hub
        .say(house.conns["laptop"], b"\xffnonsense".to_vec())
        .unwrap();
    assert!(house.settle().0.is_empty());
    assert_eq!(house.session().devices.len(), 1);
}
