//! One listening session per account, as a live room.
//!
//! `/sync` carries the log and this rides beside it on the same socket: see
//! [`harken_domain::listening`] for why none of what follows is ever written
//! to the log, and `ark_server::live` for how a channel that is not the log
//! works. What is here is the other half — the rules, as a state machine
//! over values.
//!
//! [`Desk`] owns no socket, which is the engine's own shape for the engine's
//! own reason: every hook takes what happened and posts what falls out, so
//! who ends up with the sound, what a button does when the speaker is
//! unplugged, and what a closing laptop means are all tested against a hub
//! with no network under it.
//!
//! Four rules decide everything:
//!
//! - **Exactly one device is the output**, and only it makes a sound. Every
//!   other device of that account draws what it is told.
//! - **The output survives its socket.** A laptop lid closing, a phone going
//!   to sleep and a tab being reloaded are not decisions to move the music.
//!   The sound still belongs to that device; it is simply not answering, and
//!   the session says so rather than handing itself to whoever asks next.
//! - **A command goes to the output, not to whoever asked.** That is the
//!   feature: pressing pause on a phone pauses the laptop.
//! - **…unless the output cannot be reached, and the asker can make a
//!   sound.** Then the asker takes it — because somebody pressed play and
//!   there is nothing else in the house that can answer. This is the only way
//!   a device takes the sound without being picked, and it needs a press: a
//!   device that merely arrives takes nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::Sender;

use ark::value::Value;
use ark_server::{Live, Peer, Post};
use harken_domain::listening::{Command, Device, DeviceId, Hear, Kind, Say, Session, Track};

/// A room: one account, by its id.
pub type Room = String;

/// What a bridge standing in rooms needs to know: which rooms have somebody
/// listening in them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Watch {
    /// Somebody is listening as this account. Offer it whatever stands in
    /// every room.
    Open(Room),
    /// Nobody is, any more. Take those back out, so the room can empty, be
    /// written down and be let go — a standing peer keeps a room open like
    /// any other, so a bridge standing in every room would mean no room was
    /// ever empty.
    Shut(Room),
}

/// Every account's session: one of these per server, handed to the hub as
/// its live machine. Rooms are keyed the way `ark::live` keys them — by
/// account.
#[derive(Default)]
pub struct Desk {
    rooms: BTreeMap<Room, Session>,
    /// Which rooms have been announced as open, so that a second device
    /// arriving is not a second announcement.
    listening: BTreeSet<Room>,
    /// Told whenever that changes. A channel rather than a call,
    /// deliberately: whatever answers this will call back into the hub to
    /// stand a device, and the hub is the thread this runs on.
    watchers: Vec<Sender<Watch>>,
}

impl Desk {
    pub fn new() -> Desk {
        Desk::default()
    }

    /// Be told when a room opens and when it closes. Register before the desk
    /// is handed to the hub: after that it belongs to the hub's thread, and a
    /// room opened in the gap is a room the house never hears about.
    pub fn watch(&mut self, tx: Sender<Watch>) {
        for room in &self.listening {
            if tx.send(Watch::Open(room.clone())).is_err() {
                return;
            }
        }
        self.watchers.push(tx);
    }

    fn tell_watchers(&mut self, news: Watch) {
        self.watchers.retain(|w| w.send(news.clone()).is_ok());
    }

    /// Whether anybody is *listening* here, as opposed to standing here. A
    /// speaker is in the session when somebody is listening, not the other
    /// way round.
    fn anyone_listening(session: &Session) -> bool {
        session
            .devices
            .iter()
            .any(|d| d.here && d.kind != Kind::Speaker)
    }

    /// Say whether the room is open, if that has changed since last time.
    fn reconsider(&mut self, room: &str) {
        let open = self.rooms.get(room).is_some_and(Self::anyone_listening);
        if open && self.listening.insert(room.to_string()) {
            self.tell_watchers(Watch::Open(room.to_string()));
        } else if !open && self.listening.remove(room) {
            self.tell_watchers(Watch::Shut(room.to_string()));
        }
    }
}

/// The session, as the frame every device of the room is told.
fn state(session: &Session) -> Vec<u8> {
    Hear::State {
        session: session.clone(),
    }
    .encode()
}

fn told(command: Command) -> Vec<u8> {
    Hear::Do { command }.encode()
}

/// What survives an empty room, and therefore a restart: a session with
/// nothing playing, no hand-off in flight, and at most the one device the
/// sound belongs to — because "your kitchen speaker, which is not answering"
/// is worth drawing and an id with no name is not. Every other device is a
/// fact about now, and every one of them would come back `here: false`.
///
/// Written as a session's own canonical CBOR under a tag of its own, so a
/// snapshot this build cannot read is simply not woken from.
pub fn kept(session: &Session) -> Option<Vec<u8>> {
    // A room with nothing playing and nowhere for it to play is worth no disk
    // at all — and saying so deletes the row rather than leaving a stale one.
    if session.output.is_none() && session.queue.is_empty() {
        return None;
    }
    let keep = Session {
        output: session.output.clone(),
        moving: None,
        devices: session.output_device().cloned().into_iter().collect(),
        queue: session.queue.clone(),
        at: session.at,
        playing: false,
        position_ms: session.position_ms,
    };
    let mut m = BTreeMap::new();
    m.insert("t".to_string(), Value::text("kept"));
    m.insert("session".to_string(), keep.to_value());
    Some(ark::canon::encode(&Value::Struct(m)))
}

/// Yesterday's session, from the disk: paused, and with every device away.
/// What was true is where it was playing, never that it is playing.
pub fn woken(bytes: &[u8]) -> Option<Session> {
    let Value::Struct(m) = ark::canon::decode(bytes).ok()? else {
        return None;
    };
    if m.get("t") != Some(&Value::text("kept")) {
        return None;
    }
    let mut s = Session::from_value(m.get("session")?).ok()?;
    for d in &mut s.devices {
        d.here = false;
    }
    s.moving = None;
    s.playing = false;
    Some(s)
}

impl Live for Desk {
    fn open(&mut self, room: &str, kept: Option<&[u8]>) {
        let session = kept.and_then(woken).unwrap_or_default();
        self.rooms.insert(room.to_string(), session);
    }

    /// A connection joined. Nothing is claimed and nothing is announced: the
    /// device has not said what it is yet, and — the rule this file exists
    /// for — *arriving* is not how a device gets the sound.
    ///
    /// It is told the session at once, though, so a tab that has just opened
    /// draws what the house is doing rather than an empty bar it will fill in
    /// a moment.
    fn join(&mut self, peer: &Peer, post: &mut Post<'_>) {
        let session = self.rooms.entry(peer.room.clone()).or_default();
        // A reconnect: the same device, which may well still be the output.
        let mut came_back = false;
        if let Some(device) = session.devices.iter_mut().find(|d| d.id == peer.who) {
            came_back = !device.here;
            device.here = true;
        }
        if came_back {
            // Everyone else sees it answering again.
            post.tell_room(state(session));
        } else {
            post.tell_conn(peer.conn, state(session));
        }
        self.reconsider(&peer.room);
    }

    fn say(&mut self, peer: &Peer, frame: &[u8], post: &mut Post<'_>) {
        // A sentence this build cannot read is a newer client's: nothing to
        // do with it, and nothing to break over.
        let Ok(say) = Say::decode(frame) else {
            return;
        };
        match say {
            Say::Here {
                name,
                audible,
                kind,
            } => self.here(peer, name, audible, kind, post),
            Say::Report {
                queue,
                at,
                playing,
                position_ms,
            } => self.report(peer, queue, at, playing, position_ms, post),
            Say::Do { command } => self.command(peer, command, post),
            Say::Transfer { to } => self.transfer(peer, to, post),
        }
    }

    /// A connection closed. The device is marked away and the sound stays
    /// where it was — which is the whole of the fix for music jumping back to
    /// whichever device happened to be looking.
    fn part(&mut self, peer: &Peer, post: &mut Post<'_>) {
        // The same device on another connection — a tab reloaded before its
        // old socket was noticed gone — is still here, and still itself.
        if post.here(&peer.who) {
            return;
        }
        let Some(session) = self.rooms.get_mut(&peer.room) else {
            return;
        };
        if session.moving_to(&peer.who) {
            // It was given the sound and never took it.
            session.moving = None;
        }
        if session.outputs(&peer.who) {
            // It still owns the sound. What it cannot be is *playing*: there
            // is nothing on the other end of that socket to be making one.
            session.playing = false;
            if let Some(device) = session.devices.iter_mut().find(|d| d.id == peer.who) {
                device.here = false;
            }
        } else {
            // Anything else that has gone is simply gone. Keeping it would
            // fill the picker with every tab anyone ever opened.
            session.devices.retain(|d| d.id != peer.who);
        }
        post.tell_room(state(session));
        post.keep();
        self.reconsider(&peer.room);
    }

    fn snapshot(&mut self, room: &str) -> Option<Vec<u8>> {
        kept(self.rooms.get(room)?)
    }

    fn close(&mut self, room: &str) {
        self.rooms.remove(room);
        if self.listening.remove(room) {
            self.tell_watchers(Watch::Shut(room.to_string()));
        }
    }
}

impl Desk {
    /// A device says what it is. This is where it enters the picker, and
    /// where a reconnecting output is recognised as the device that still
    /// owns the sound.
    fn here(&mut self, peer: &Peer, name: String, audible: bool, kind: Kind, post: &mut Post<'_>) {
        let Some(session) = self.rooms.get_mut(&peer.room) else {
            return;
        };
        let device = Device {
            id: peer.who.clone(),
            name,
            audible,
            here: true,
            kind,
        };
        session.devices.retain(|d| d.id != device.id);
        session.devices.push(device);
        session
            .devices
            .sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        post.tell_room(state(session));
        post.keep();
        self.reconsider(&peer.room);
    }

    /// The output says what it is doing. From anyone else it is ignored: a
    /// device that is not making the sound cannot be right about it.
    fn report(
        &mut self,
        peer: &Peer,
        queue: Vec<Track>,
        at: u32,
        playing: bool,
        position_ms: i64,
        post: &mut Post<'_>,
    ) {
        let Some(session) = self.rooms.get_mut(&peer.room) else {
            return;
        };
        if !session.outputs(&peer.who) {
            return;
        }
        let was = session.clone();
        // It has taken the hand-off. A report is the only evidence of that
        // there can be, because taking it is exactly "started playing".
        if session.moving_to(&peer.who) {
            session.moving = None;
        }
        let structural = session.queue != queue || session.at != at || session.playing != playing;
        session.queue = queue;
        session.at = at;
        session.playing = playing;
        session.position_ms = position_ms;
        // A report a second means a broadcast a second, and the position it
        // carries is what the other devices' scrubbers are waiting for. What
        // is not worth sending is a report that changed nothing at all, which
        // is what a paused output sends.
        if was != *session {
            post.tell_room(state(session));
        }
        // A position that moved is not worth a disk write; a different track
        // is. This is the difference `Post::keep` exists to let an app draw.
        if structural {
            post.keep();
        }
    }

    /// Somebody asks for something.
    fn command(&mut self, peer: &Peer, command: Command, post: &mut Post<'_>) {
        let Some(session) = self.rooms.get_mut(&peer.room) else {
            return;
        };

        // The ordinary case, and the feature: the sound is somewhere that is
        // answering, so that is where the button goes.
        if let Some(output) = session.output.clone() {
            if post.here(&output) {
                post.tell(&output, told(command));
                return;
            }
        }

        // Nothing is answering where the sound belongs. Only a press that
        // means "make a sound" moves it — a pause or a seek aimed at a device
        // that has gone is a press with nothing to do, and answering it by
        // seizing the sound would be a device assuming control it was never
        // given.
        let wants_sound = matches!(
            command,
            Command::Play | Command::Next | Command::Previous | Command::Start { .. }
        );
        let can = session
            .device(&peer.who)
            .is_some_and(|d| d.audible && d.here);
        if !wants_sound || !can {
            // Told rather than ignored: the bar that asked is the bar that has
            // to show the press went nowhere.
            post.tell_room(state(session));
            return;
        }

        session.output = Some(peer.who.clone());
        session.moving = None;
        match command {
            // It brought its own queue; nothing here knows better.
            Command::Start { .. } => post.tell(&peer.who, told(command)),
            other => {
                // Hand it the session first, because it may never have had
                // one — a phone that has been watching a speaker all evening
                // holds no queue of its own. Then the press it actually made,
                // so `Next` still means next.
                post.tell(
                    &peer.who,
                    told(Command::Start {
                        queue: session.queue.clone(),
                        at: session.at,
                        position_ms: session.position_ms,
                        playing: true,
                    }),
                );
                if !matches!(other, Command::Play) {
                    post.tell(&peer.who, told(other));
                }
            }
        }
        post.tell_room(state(session));
        post.keep();
    }

    /// Move the sound. `None` stops it everywhere.
    ///
    /// The new output is handed the session as one [`Command::Start`] — the
    /// queue, the place in it, the point in the track and whether it was
    /// playing — because a hand-off is that sentence and nothing else. That
    /// is also what makes picking a speaker halfway through a track resume
    /// rather than restart.
    ///
    /// The old one is told nothing: it learns from the broadcast that it is no
    /// longer the output, and a device that is not the output is silent. One
    /// rule, in one place, rather than a stop command a dropped socket could
    /// lose.
    fn transfer(&mut self, peer: &Peer, to: Option<DeviceId>, post: &mut Post<'_>) {
        let Some(session) = self.rooms.get_mut(&peer.room) else {
            return;
        };
        match to {
            None => {
                session.output = None;
                session.moving = None;
                session.playing = false;
            }
            Some(id) => {
                // Only a device that can be heard, and only one that is here:
                // handing the sound to something that is not answering is the
                // one way to lose it entirely.
                let ready =
                    session.device(&id).is_some_and(|d| d.audible && d.here) && post.here(&id);
                if !ready {
                    post.tell_room(state(session));
                    return;
                }
                if session.outputs(&id) && session.moving.is_none() {
                    return;
                }
                session.output = Some(id.clone());
                // Until it says otherwise it is *connecting*, not playing. A
                // speaker in the house takes a second or two to fetch
                // anything, and a picker that goes straight to "playing"
                // spends that second lying.
                session.moving = Some(id.clone());
                let take = Command::Start {
                    queue: session.queue.clone(),
                    at: session.at,
                    position_ms: session.position_ms,
                    playing: session.playing,
                };
                post.tell(&id, told(take));
            }
        }
        post.tell_room(state(session));
        post.keep();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(t: &str) -> Track {
        Track {
            id: t.into(),
            title: t.into(),
            creator: "Bach".into(),
            album: String::new(),
            duration_ms: 1000,
            file: format!("music/{t}.mp3"),
        }
    }

    /// What is kept is where the music was and what it was — never that it
    /// is playing, never a hand-off, and never anybody but the output.
    #[test]
    fn a_kept_session_wakes_paused_with_only_its_output_away() {
        let device = |id: &str, here| Device {
            id: id.into(),
            name: id.into(),
            audible: true,
            here,
            kind: Kind::Computer,
        };
        let session = Session {
            output: Some("laptop".into()),
            moving: Some("phone".into()),
            devices: vec![device("laptop", true), device("phone", true)],
            queue: vec![track("a"), track("b")],
            at: 1,
            playing: true,
            position_ms: 61_000,
        };
        let bytes = kept(&session).expect("a session with a queue is kept");
        assert!(ark::canon::round_trip(&bytes));
        let back = woken(&bytes).unwrap();
        assert_eq!(back.output.as_deref(), Some("laptop"));
        assert_eq!(back.moving, None);
        assert_eq!(back.devices, vec![device("laptop", false)]);
        assert_eq!(back.queue, session.queue);
        assert_eq!(
            (back.at, back.position_ms, back.playing),
            (1, 61_000, false)
        );

        assert!(
            kept(&Session::default()).is_none(),
            "nothing worth a disk write"
        );
        assert!(
            woken(&Hear::State { session }.encode()).is_none(),
            "not a snapshot"
        );
        assert!(woken(b"\xff").is_none());
    }
}
