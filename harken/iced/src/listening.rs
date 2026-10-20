//! This device's end of the account's listening session: a reconciler.
//!
//! The session is the domain's live state (`harken/domain/listening.ark`):
//! three volatile tables the server's second machine holds (§1.9), one
//! session per account, on the sync socket — what [`crate::client::Replica::listening`]
//! reads is the server's confirmed state with this device's own calls
//! previewed on top. **The state is the
//! instruction**: nothing here is told what to do. Every button on every
//! device is a transition ([`Remote::call`]); the device the session names
//! as its output reads the session and makes its player agree
//! ([`Remote::reconcile`]); every other device is silent and draws it.
//!
//! What the output says back is only what happens to it, each a transition
//! of its own: `started` once it has a hand-off loaded, `advance` when a
//! track ends, `stalled` when the sound stops by itself, and
//! `report_position` when its player has come apart from the clock by more
//! than [`read::SYNC_MS`] — an element that waited on the network, or one
//! that started a moment after `started` was said — because the output is
//! right about where it is, and every bar should draw what is heard. There
//! is no heartbeat: a playing clock carries the server's time it started
//! from, so every device counts the position for itself
//! ([`Remote::position_ms`], against [`crate::client::Replica::server_now`] — this
//! device's own clock: an entry carries the clock of whoever authored it).
//! The player is sent where the session is only when it is the *session*
//! that moved — a seek, a skip, a hand-off — which is told apart from the
//! player drifting by whether the clock is still the one it last saw.
//!
//! **A device unlinked is its own session.** While the socket is down, or
//! before this device stands in the session on a new one (its `on_join`
//! has landed), the player is this
//! device's alone and nothing is called — a call made then would be said on
//! the next connection, long after it meant anything. Coming back, the
//! decided rule: if the session is sounding somewhere else, this device's
//! own sound stops and it is a controller of the device playing; if nothing
//! is sounding and this device was playing on its own, what it was doing
//! becomes the session — a `start`, which hands it the sound.
//!
//! **A device is a login.** `ark-auth` says a session is "one login on one
//! device", so the device id is the login's session id: what a volatile
//! `Hello` declares with an empty id, which the server fills in, and what
//! the session's devices key it by.
//!
//! **The desktop has no audio device** (`Player::AUDIBLE` is false), so it
//! declares itself one that cannot play: it is never the output, and every
//! one of its buttons is a remote control.
//!
//! **The volume is the session's too.** A device whose player has a volume
//! it can set says so once a connection (`equip`, with the level its player
//! is at), and from then on its player follows its own entry of the
//! session — whoever moved it, from whichever device — as the output
//! follows the clock. A change nobody in the session made (an extension,
//! the platform) is said back as `report_volume`. The desktop sounds
//! nothing and says nothing, so it has no volume to draw.
//!
//! **A play counts once**, in the log: the output authors `record_play`
//! once a track it loaded has played thirty seconds or half its length,
//! whichever is first ([`read::counts_after`]). Offline it is pending like
//! any intent and reaches the server with the device.

use crate::client::Replica as Peer;
use crate::domain::host as read;
use crate::rows::Id;
// What the rest of this program asks *this* module about.
pub use crate::domain::host::{Kind, Listening};
pub use crate::domain::{Active, Clock, Output, Present as Device, Track};

/// How far the player may be from where the session counts it before one
/// is put right — the bound every output is held to, the bridge's speakers
/// too.
const SYNC_MS: i64 = read::SYNC_MS;

/// How often, at most, the output says where its player is while it stays
/// apart from the clock: once is enough when it lands, and a player that
/// keeps coming apart — a stream that keeps stalling — is said again a
/// second later rather than on every update.
const REPORT_MS: f64 = 1_000.0;

/// How long the output's player may sit paused while the session says it is
/// playing before the output says it has stalled: long enough for a stream
/// to start after `play()`, which in a browser is asynchronous.
const STALL_MS: f64 = 2_500.0;

/// What sounds, as the reconciler sees it: the App's player, or a test's.
pub trait Sound {
    /// The id of the track loaded, if any.
    fn loaded(&self) -> Option<Id>;
    fn playing(&self) -> bool;
    /// Asked to play and waiting for data: where it is cannot be compared
    /// with where it should be.
    fn buffering(&self) -> bool {
        false
    }
    fn position_ms(&self) -> i64;
    fn ended(&self) -> bool;
    /// Load this track, at this point, playing or not.
    fn load(&mut self, track: &Track, position_ms: i64, playing: bool);
    fn resume(&mut self);
    fn pause(&mut self);
    fn seek(&mut self, position_ms: i64);
    /// The level it sounds at, 0 to 100, and whether it is muted: `None`
    /// where it has no volume anybody can set.
    fn volume(&mut self) -> Option<(i64, bool)>;
    fn set_volume(&mut self, level: i64, muted: bool);
}

/// Every number this end of the session holds that the peer does not, for
/// the debug screen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    /// Transitions called, and refused by the preview or the server.
    pub calls: u64,
    pub refused: u64,
    /// `record_play`s authored.
    pub plays: u64,
    /// `start`s said coming back with something of its own playing.
    pub came_back_playing: u64,
    pub last_refusal: Option<String>,
}

/// This device's end of the account's listening session.
#[derive(Default)]
pub struct Remote {
    /// This device: the login's session id.
    me: String,
    /// What the picker draws for it.
    name: String,
    /// The connection whose snapshot this device has met: below the peer's
    /// epoch, it is still its own session.
    met: u64,
    /// The player was playing while this device was its own session.
    alone_playing: bool,
    /// The hand-off `started` has been said for: the track it was loaded on.
    started_for: Option<(i64, Id)>,
    /// The track end `advance` has been said for.
    advanced_for: Option<(i64, Id)>,
    /// Every load of a track is a play; this one's number, and the last
    /// number counted.
    play: u64,
    counted: u64,
    /// When this device last asked its player to play, by its own clock.
    asked: Option<f64>,
    /// The volume its player was last given, or last found at: what tells a
    /// change the session asked for from one made behind its back.
    applied: Option<(i64, bool)>,
    /// The track and the clock the player was last reconciled against: a
    /// difference with the clock still this one is the player's, and the
    /// session is told; with another, the session moved, and the player is.
    seen: Option<(i64, Id, Clock)>,
    /// When this device last said where its player is, by its own clock.
    reported: Option<f64>,
    stats: Stats,
}

/// The device this build declares at its volatile `Hello`: its login's
/// session (an empty id), a computer, and whether it can make a sound at all.
pub fn device() -> Vec<u8> {
    crate::client::device(crate::Player::AUDIBLE, &device_name())
}

/// A track's id, as the queue carries it.
pub fn track_id(t: &Track) -> Id {
    t.id.0
}

impl Remote {
    pub fn new() -> Remote {
        Remote {
            name: device_name(),
            ..Remote::default()
        }
    }

    /// This device is a login, so a new login is a new device. Called at
    /// sign-in and whenever the login changes.
    pub fn open(&mut self, device: &str) {
        *self = Remote {
            me: device.to_string(),
            name: std::mem::take(&mut self.name),
            stats: std::mem::take(&mut self.stats),
            ..Remote::default()
        };
    }

    /// Stop. A peer that has signed out has no session to be part of.
    pub fn close(&mut self) {
        self.open("");
    }

    /// This device, as the picker names it.
    pub fn me(&self) -> &str {
        &self.me
    }

    /// What this device calls itself.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// The session, when there is one to be part of: linked, and the
    /// server's snapshot met on this connection. `None` is this device being
    /// its own session.
    pub fn session(&self, peer: &Peer) -> Option<Listening> {
        self.joined(peer).then(|| crate::client::listening(peer))
    }

    /// Whether this device is part of the session now: linked, as the login
    /// it was opened for, and standing in it.
    pub fn joined(&self, peer: &Peer) -> bool {
        peer.linked && self.me == peer.me().session && crate::client::present(peer)
    }

    /// Whether this device is the one making the sound.
    pub fn outputs_here(&self, s: &Listening) -> bool {
        s.outputs(&self.me)
    }

    /// Whether the sound belongs somewhere else — here or not — which is
    /// when this device is silent and its bar draws the session.
    ///
    /// Not the negation of [`Remote::outputs_here`]: a session with *no*
    /// output is neither, and there a press plays here.
    pub fn elsewhere(&self, s: &Listening) -> bool {
        s.output().and_then(Output::id).is_some_and(|id| id != self.me)
    }

    /// How far into the track, counted on from the session's clock against
    /// the server's — and standing still while the sound is on its way to
    /// another device ([`Active::position`]): what a scrubber drawn from any
    /// device shows.
    pub fn position_ms(a: &Active) -> i64 {
        a.position(crate::client::now_ms())
    }

    /// Call a transition of the session. A refusal — the preview's, here —
    /// is said back and counted; the server's arrive later
    /// ([`Remote::refusals`]).
    pub fn call(&mut self, peer: &mut Peer, transition: &str, input: Vec<u8>) -> Result<(), String> {
        self.stats.calls += 1;
        peer.call(transition, input).map_err(|why| {
            self.stats.refused += 1;
            self.stats.last_refusal = Some(format!("{transition}: {why}"));
            why
        })
    }

    /// What the server refused of this device's calls since the last ask.
    pub fn refusals(&mut self, peer: &mut Peer) -> Vec<String> {
        let out = peer.take_live_refusals();
        if let Some(last) = out.last() {
            self.stats.refused += out.len() as u64;
            self.stats.last_refusal = Some(last.clone());
        }
        out
    }

    /// Make this device agree with the session, and say what happened to
    /// it. Called after every update, whatever it was.
    ///
    /// `queue` is the App's own — what this device plays through when it is
    /// its own session — and `at` where its player is in it; while this
    /// device is the output, the session's queue is copied into it, so that
    /// going offline mid-track leaves it playing what it was. `from` is
    /// where a `start` it says was taken from. `clock` is this device's own
    /// milliseconds, for how long its player has sat paused.
    pub fn reconcile(&mut self, peer: &mut Peer, sound: &mut impl Sound, queue: &mut Vec<Track>, at: usize, from: &str, clock: f64) {
        if !self.joined(peer) {
            // Its own session. Whether it was playing on its own is all that
            // matters when it comes back.
            self.alone_playing = sound.playing();
            return;
        }
        let Some(s) = self.session(peer) else { return };
        if self.met != peer.link.epoch {
            // The first sight of the session on this connection: what this
            // device can do, said, and the rule for coming back.
            self.met = peer.link.epoch;
            self.equip(peer, sound, &s);
            if self.alone_playing && !queue.is_empty() {
                self.alone_playing = false;
                if s.sounding() && !self.outputs_here(&s) {
                    // Somebody else is playing: this device is a controller
                    // of it now, and its own sound stops below.
                } else {
                    let position = sound.position_ms();
                    let at = at.min(queue.len() - 1);
                    if self.call(peer, "start", input::start(queue, at, position, from)).is_ok() {
                        self.stats.came_back_playing += 1;
                    }
                    // Its own sound goes on: the session it has just made
                    // says what it is already doing.
                    return;
                }
            }
        }
        let s = self.session(peer).unwrap_or(s);
        self.follow_volume(peer, sound, &s);
        match &s.session {
            Some(a) if self.outputs_here(&s) => self.drive(peer, sound, queue, a, clock),
            // Not the output: silent. Losing the sound is not something this
            // device does, it is told, and this is the one place that can
            // notice.
            _ => {
                if sound.playing() {
                    sound.pause();
                }
                self.asked = None;
            }
        }
    }

    /// Say what this device can do, once a connection: a player with a
    /// volume says so, at the level it is at — the session's entry for it
    /// starts as whatever `on_join` assumed, which is nothing. A device with
    /// no volume says nothing, and stays that.
    fn equip(&mut self, peer: &mut Peer, sound: &mut impl Sound, s: &Listening) {
        let Some((level, muted)) = sound.volume() else { return };
        if s.device(&self.me).is_none() {
            return;
        }
        self.applied = Some((level, muted));
        let _ = self.call(peer, "equip", input::equip(true, None, level, muted));
    }

    /// The player at this device's own volume in the session — and a volume
    /// the player was moved to behind the session's back, said.
    fn follow_volume(&mut self, peer: &mut Peer, sound: &mut impl Sound, s: &Listening) {
        let Some(me) = s.device(&self.me).filter(|d| d.can_volume) else { return };
        let Some(now) = sound.volume() else { return };
        let want = (me.volume, me.muted);
        if self.applied.is_some_and(|was| was != now) {
            // Moved, and not by this: the platform, an extension, a page.
            self.applied = Some(now);
            if now != want {
                let _ = self.call(peer, "report_volume", input::report_volume(now.0, now.1));
            }
            return;
        }
        if now != want {
            sound.set_volume(want.0, want.1);
        }
        self.applied = Some(want);
    }

    // This device is the output: its player made to agree with the session,
    // and what happened to it said.
    fn drive(&mut self, peer: &mut Peer, sound: &mut impl Sound, queue: &mut Vec<Track>, a: &Active, clock: f64) {
        let now = crate::client::now_ms();
        let track = a.now().clone();
        // Where the session stands: a hand-off to this device stands where
        // the track was until this device says it has started, so the track
        // is loaded where it stopped on the device that had it.
        let want = a.position(now);
        if sound.loaded() != Some(track_id(&track)) || *queue != a.queue {
            *queue = a.queue.clone();
            if sound.loaded() != Some(track_id(&track)) {
                sound.load(&track, want, a.clock.playing());
                self.play += 1;
                self.advanced_for = None;
                self.asked = a.clock.playing().then_some(clock);
            }
        }
        // The hand-off has landed when the track is loaded: said once.
        let moving = matches!(&a.output, Output::Device { moving: true, .. });
        if moving && self.started_for != Some((a.at, track_id(&track))) {
            self.started_for = Some((a.at, track_id(&track)));
            let _ = self.call(peer, "started", input::at(sound.position_ms()));
            return;
        }
        if !moving {
            self.started_for = None;
        }
        // The track ended: said once per end. Once said, an element still
        // sitting at its end — the last track of the queue — is reconciled
        // like any other: played again from the start if somebody presses
        // play.
        if sound.ended() && a.clock.playing() && self.advanced_for != Some((a.at, track_id(&track))) {
            self.advanced_for = Some((a.at, track_id(&track)));
            let _ = self.call(peer, "advance", input::advance(a.at));
            return;
        }
        match (a.clock.playing(), sound.playing()) {
            (true, false) => {
                // Asked to play and not playing: start it, and if it has been
                // asked long enough and still sits there, it has stalled.
                match self.asked {
                    Some(t) if clock - t > STALL_MS => {
                        self.asked = None;
                        let _ = self.call(peer, "stalled", input::at(sound.position_ms()));
                    }
                    Some(_) => {}
                    None => {
                        sound.resume();
                        self.asked = Some(clock);
                    }
                }
            }
            (false, true) => {
                sound.pause();
                self.asked = None;
            }
            (true, true) => self.asked = None,
            (false, false) => {}
        }
        // Somewhere else in the track than the session counts, by more than
        // the bound. If the session moved since the player was last made to
        // agree — a seek on another device — the player goes where it is
        // now; if it is the player that came apart from a clock nobody
        // touched, the player is right about where it is and the session is
        // told (`report_position`), at most once a second, against that
        // clock. Waiting on the network, neither: it is standing still for
        // the stream, and will say so if it comes out of it behind.
        let moved = self.seen != Some((a.at, track_id(&track), a.clock));
        self.seen = Some((a.at, track_id(&track), a.clock));
        let off = sound.position_ms() - want;
        if off.abs() > SYNC_MS && !sound.buffering() {
            match (moved, a.clock, sound.playing()) {
                (false, Clock::Playing { since, .. }, true) => {
                    if self.reported.is_none_or(|t| clock - t >= REPORT_MS) {
                        self.reported = Some(clock);
                        let _ = self.call(peer, "report_position", input::report_position(sound.position_ms(), since));
                    }
                }
                (false, Clock::Playing { .. }, false) => {}
                _ => sound.seek(want),
            }
        }
        // A play, once it has played long enough, once.
        if a.clock.playing() && self.counted != self.play && sound.position_ms() >= read::counts_after(track.duration_ms) {
            self.counted = self.play;
            match peer.mutate("record_play", &input::record_play(&track, &a.from)) {
                Ok(_) => self.stats.plays += 1,
                Err(e) => self.stats.last_refusal = Some(format!("record_play: {e}")),
            }
        }
    }
}

/// What this device is called in somebody else's picker.
///
/// No more than the build, deliberately: more than this would want a
/// user-agent string, which is a guess dressed as a fact. The picker marks
/// which row is you anyway.
fn device_name() -> String {
    if cfg!(target_arch = "wasm32") {
        "Browser".to_string()
    } else {
        "Desktop".to_string()
    }
}

/// Each transition's input, as its bytes: the domain's own input types.
pub mod input {
    use crate::domain::Id;
    use crate::domain::*;
    use ark::ir::Node;

    /// A transition that takes nothing.
    pub fn none() -> Vec<u8> {
        ().to_bytes()
    }

    pub fn start(queue: &[Track], at: usize, position: i64, from: &str) -> Vec<u8> {
        Start {
            at: at as i64,
            from: from.into(),
            position,
            queue: queue.to_vec(),
        }
        .to_bytes()
    }

    pub fn seek(ms: i64) -> Vec<u8> {
        Seek { ms }.to_bytes()
    }

    pub fn skip(by: i64) -> Vec<u8> {
        Skip { by }.to_bytes()
    }

    pub fn jump(index: usize) -> Vec<u8> {
        Jump { index: index as i64 }.to_bytes()
    }

    pub fn remove(index: usize) -> Vec<u8> {
        Remove { index: index as i64 }.to_bytes()
    }

    pub fn move_track(from: usize, to: usize) -> Vec<u8> {
        Move {
            from: from as i64,
            to: to as i64,
        }
        .to_bytes()
    }

    pub fn enqueue(tracks: &[Track], next: bool) -> Vec<u8> {
        Enqueue {
            next,
            tracks: tracks.to_vec(),
        }
        .to_bytes()
    }

    pub fn transfer(to: &str) -> Vec<u8> {
        Transfer { to: to.into() }.to_bytes()
    }

    pub fn at(at_ms: i64) -> Vec<u8> {
        At { at_ms }.to_bytes()
    }

    pub fn advance(from_track: i64) -> Vec<u8> {
        Advance { from_track }.to_bytes()
    }

    pub fn report_position(at_ms: i64, since: i64) -> Vec<u8> {
        Report { at_ms, since }.to_bytes()
    }

    pub fn equip(can_volume: bool, family: Option<&str>, volume: i64, muted: bool) -> Vec<u8> {
        Equip {
            can_volume,
            family: family.map(|f| f.into()),
            muted,
            volume,
        }
        .to_bytes()
    }

    pub fn set_volume(device: &str, level: i64) -> Vec<u8> {
        SetVolume {
            device: device.into(),
            level,
        }
        .to_bytes()
    }

    pub fn nudge_volume(device: &str, delta: i64) -> Vec<u8> {
        NudgeVolume {
            delta,
            device: device.into(),
        }
        .to_bytes()
    }

    pub fn set_mute(device: &str, muted: bool) -> Vec<u8> {
        SetMute {
            device: device.into(),
            muted,
        }
        .to_bytes()
    }

    pub fn report_volume(level: i64, muted: bool) -> Vec<u8> {
        ReportVolume { level, muted }.to_bytes()
    }

    pub fn member(device: &str) -> Vec<u8> {
        Member { device: device.into() }.to_bytes()
    }

    pub fn set_group_volume(level: i64) -> Vec<u8> {
        GroupVolume { level }.to_bytes()
    }

    pub fn nudge_group_volume(delta: i64) -> Vec<u8> {
        GroupNudge { delta }.to_bytes()
    }

    pub fn set_group_mute(muted: bool) -> Vec<u8> {
        GroupMute { muted }.to_bytes()
    }

    pub fn scrub(at_ms: Option<i64>) -> Vec<u8> {
        Scrub { at_ms }.to_bytes()
    }

    /// `record_play`'s input: the track as the session carried it.
    pub fn record_play(track: &Track, from: &str) -> RecordPlay {
        RecordPlay {
            track: Id::new(track.id.0),
            title: track.title.clone(),
            creator: track.creator.clone(),
            album: track.album.clone(),
            duration_ms: track.duration_ms.max(0),
            file: track.file.clone(),
            from: from.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::domain::Id as Key;
    use ark::peer::Who;
    use ark_peer::{Config, ConnId, Live, Peer as Hub};

    /// A player that does what it is told, and a clock it is told about.
    #[derive(Default)]
    struct Fake {
        loaded: Option<(Id, i64)>,
        playing: bool,
        at_ms: i64,
        ended: bool,
        loads: usize,
        /// Its volume, where it has one.
        volume: Option<(i64, bool)>,
    }

    impl Sound for Fake {
        fn loaded(&self) -> Option<Id> {
            self.loaded.map(|(i, _)| i)
        }
        fn playing(&self) -> bool {
            self.playing
        }
        fn position_ms(&self) -> i64 {
            self.at_ms
        }
        fn ended(&self) -> bool {
            self.ended
        }
        fn load(&mut self, track: &Track, position_ms: i64, playing: bool) {
            self.loaded = Some((track_id(track), position_ms));
            self.at_ms = position_ms;
            self.playing = playing;
            self.ended = false;
            self.loads += 1;
        }
        fn resume(&mut self) {
            self.playing = true;
            self.ended = false;
        }
        fn pause(&mut self) {
            self.playing = false;
        }
        fn seek(&mut self, position_ms: i64) {
            self.at_ms = position_ms;
        }
        fn volume(&mut self) -> Option<(i64, bool)> {
            self.volume
        }
        fn set_volume(&mut self, level: i64, muted: bool) {
            if self.volume.is_some() {
                self.volume = Some((level, muted));
            }
        }
    }

    fn track(t: &str, n: u8) -> Track {
        let mut id = [0u8; 16];
        id[15] = n;
        Track {
            id: Key::new(id),
            title: t.into(),
            creator: "Bach".into(),
            album: "Water Music".into(),
            duration_ms: 100_000,
            file: format!("music/{t}.mp3"),
        }
    }

    fn queue() -> Vec<Track> {
        vec![track("a", 1), track("b", 2)]
    }

    /// A server in this test — the hub (`ark-server`), sans-io, holding
    /// the session in its volatile machine — and the devices on it: each a
    /// real replica, its frames carried by hand.
    struct Net {
        hub: Hub,
        peers: Vec<(Peer, Who, ConnId)>,
    }

    impl Net {
        fn new() -> Net {
            let d = crate::client::domain();
            let m = ark_peer::Module::of(d);
            let mut c = Config::new(m.clone());
            let get = |n: &str| Some(d.get(&format!("listening.{n}")));
            c.live = Some(Live {
                on_join: get("on_join"),
                on_leave: get("on_leave"),
                on_restore: get("on_restore"),
                arrive: Arc::new(crate::domain::host::arrive),
                keep: Some(64),
                ..Live::new(m)
            });
            Net {
                hub: Hub::serve(c, Box::new(ark_peer::Memory::default()), crate::client::now_ms()),
                peers: vec![],
            }
        }

        /// A device of alice's, linked: `can_play` as its build can.
        fn device(&mut self, session: &str, can_play: bool) -> usize {
            let who = Who {
                actor: "alice".into(),
                session: session.into(),
                roles: vec![],
            };
            let said = crate::client::device(can_play, session);
            let p = crate::client::memory_as(who.clone(), Some("t"), said);
            self.peers.push((p, who, 0));
            let k = self.peers.len() - 1;
            self.relink(k);
            k
        }

        fn drop_link(&mut self, k: usize) {
            let (p, _, c) = &mut self.peers[k];
            p.disconnected();
            let out = self.hub.leave(*c, crate::client::now_ms());
            self.deliver(out);
        }

        fn relink(&mut self, k: usize) {
            let c = self.hub.accept(self.peers[k].1.clone());
            self.peers[k].2 = c;
            self.peers[k].0.connected();
            self.carry();
        }

        fn deliver(&mut self, out: Vec<(ConnId, Vec<u8>)>) {
            for (c, b) in out {
                if let Some((p, _, _)) = self.peers.iter_mut().find(|(p, _, pc)| *pc == c && p.linked) {
                    p.receive(b);
                }
            }
        }

        /// What every linked peer has to say, said, until nobody has more.
        fn carry(&mut self) {
            for _ in 0..16 {
                let mut out = vec![];
                for (p, _, c) in self.peers.iter_mut() {
                    if !p.linked {
                        continue;
                    }
                    p.tick(crate::client::now_ms() + 1_000);
                    for f in p.take_frames() {
                        out.extend(self.hub.frame(*c, f, crate::client::now_ms()));
                    }
                }
                if out.is_empty() {
                    return;
                }
                self.deliver(out);
            }
        }

        fn peer(&mut self, k: usize) -> &mut Peer {
            &mut self.peers[k].0
        }
    }

    /// The one rule this device lives by: the output's player is made to
    /// agree with the session, and every other device is silent. A press on
    /// the desktop — a remote, which cannot play — pauses the browser's
    /// player, through nothing but the session.
    ///
    /// Falsified by a reconcile that left a device that is not the output
    /// alone: the browser went on playing after the sound moved.
    #[test]
    fn the_output_obeys_the_session_and_nobody_else_makes_a_sound() {
        let mut net = Net::new();
        let b = net.device("browser", true);
        let d = net.device("desktop", false);
        let (mut rb, mut rd) = (Remote::new(), Remote::new());
        rb.open("browser");
        rd.open("desktop");
        let (mut sb, mut sd) = (Fake::default(), Fake::default());
        let (mut qb, mut qd) = (vec![], vec![]);

        rb.call(net.peer(b), "start", input::start(&queue(), 0, 0, "album/Water Music")).unwrap();
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 0.0);
        assert_eq!(sb.loaded(), Some(track_id(&queue()[0])), "the browser loaded what it pressed");
        assert!(sb.playing);
        assert_eq!(qb, queue(), "and holds the session's queue as its own");

        rd.reconcile(net.peer(d), &mut sd, &mut qd, 0, "", 0.0);
        assert!(!sd.playing && sd.loaded().is_none(), "the desktop makes no sound");
        let s = rd.session(net.peer(d)).unwrap();
        assert!(rd.elsewhere(&s) && !rd.outputs_here(&s));

        rd.call(net.peer(d), "pause", input::none()).unwrap();
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 1.0);
        assert!(!sb.playing, "a pause pressed on the desktop is a pause in the browser");

        rd.call(net.peer(d), "seek", input::seek(40_000)).unwrap();
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 2.0);
        assert_eq!(sb.at_ms, 40_000, "and a seek is a seek");

        // A second player that can play takes the sound by transfer; the
        // browser goes quiet.
        let l = net.device("laptop", true);
        let mut rl = Remote::new();
        rl.open("laptop");
        let (mut sl, mut ql) = (Fake::default(), vec![]);
        rd.call(net.peer(d), "play", input::none()).unwrap();
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 2.5);
        assert!(sb.playing, "a play pressed on the desktop plays the browser");
        rd.call(net.peer(d), "transfer", input::transfer("laptop")).unwrap();
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 3.0);
        rl.reconcile(net.peer(l), &mut sl, &mut ql, 0, "", 3.0);
        net.carry();
        assert!(!sb.playing, "the browser lost the sound and is silent");
        assert!(sl.playing && sl.loaded() == Some(track_id(&queue()[0])));
        let s = rl.session(net.peer(l)).unwrap();
        assert!(s.outputs("laptop") && !s.moving_to("laptop"), "the laptop said it had started");
    }

    /// A hand-off between two players is a pause where the track was and the
    /// same point picked up on the other: the browser forty seconds in hands
    /// it to the laptop, and is silent on its next reconcile — not when the
    /// laptop answers; the session stands at forty seconds meanwhile however
    /// long the laptop takes; the laptop loads the track there and says it
    /// started there; and at no point does any device's position go back.
    ///
    /// Falsified by loading the track where the session's clock counts it
    /// rather than where the session stands: the laptop, a second and a half
    /// late to reconcile, loaded at 41 506 where the track had stopped at
    /// 40 000.
    #[test]
    fn a_hand_off_between_two_players_picks_up_where_the_track_stopped() {
        let mut net = Net::new();
        let b = net.device("browser", true);
        let l = net.device("laptop", true);
        let (mut rb, mut rl) = (Remote::new(), Remote::new());
        rb.open("browser");
        rl.open("laptop");
        let (mut sb, mut sl) = (Fake::default(), Fake::default());
        let (mut qb, mut ql) = (vec![], vec![]);
        rb.call(net.peer(b), "start", input::start(&queue(), 0, 40_000, "")).unwrap();
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 0.0);
        assert!(sb.playing);
        let stands = |r: &Remote, p: &mut Peer| {
            let now = crate::client::now_ms();
            r.session(p).and_then(|s| s.session).map(|a| a.position(now)).unwrap()
        };

        rb.call(net.peer(b), "transfer", input::transfer("laptop")).unwrap();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 1.0);
        assert!(!sb.playing, "silent on the reconcile after its own transfer, before anybody answers");
        net.carry();
        let handed = stands(&rb, net.peer(b));
        assert!((40_000..40_500).contains(&handed), "{handed}");
        // The laptop is slow to get to it.
        std::thread::sleep(std::time::Duration::from_millis(1_500));
        assert_eq!(
            stands(&rb, net.peer(b)),
            handed,
            "standing where it was, not counting a sound nobody hears"
        );
        rl.reconcile(net.peer(l), &mut sl, &mut ql, 0, "", 2.0);
        assert_eq!(sl.loaded, Some((track_id(&queue()[0]), handed)), "loaded where the track stopped");
        net.carry();
        let s = rl.session(net.peer(l)).unwrap();
        assert!(s.outputs("laptop") && !s.moving_to("laptop"));
        let a = s.session.unwrap();
        assert!(
            matches!(a.clock, Clock::Playing { at_ms, .. } if at_ms == handed),
            "started there: {:?}",
            a.clock
        );
        assert!(stands(&rl, net.peer(l)) >= handed, "and never back");
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 3.0);
        assert!(!sb.playing, "and the browser stays quiet");
    }

    /// The output is right about where it is. A browser's element that
    /// waited a second on the network is a second behind a clock nobody
    /// touched: the session is told where the element is
    /// (`report_position`), so every bar draws what is heard, and the
    /// element is not sent a second ahead to catch a clock up — which was a
    /// second of the track skipped. Within the bound nothing is said. A seek
    /// on another device is the session moving, and the element goes there.
    ///
    /// Falsified by the old rule, the element always sent where the clock
    /// counts: it was sought to 41 006, skipping what the network had held
    /// up, and the session never heard where it was.
    #[test]
    fn the_session_follows_the_output_and_the_output_follows_a_seek() {
        let mut net = Net::new();
        let b = net.device("browser", true);
        let d = net.device("desktop", false);
        let (mut rb, mut rd) = (Remote::new(), Remote::new());
        rb.open("browser");
        rd.open("desktop");
        let (mut sb, mut qb) = (Fake::default(), vec![]);
        rb.call(net.peer(b), "start", input::start(&queue(), 0, 40_000, "")).unwrap();
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 0.0);
        let loaded = sb.at_ms;
        assert!((40_000..40_100).contains(&loaded) && sb.playing, "{loaded}");
        let counted = |r: &Remote, p: &mut Peer| {
            let now = crate::client::now_ms();
            r.session(p).and_then(|s| s.session).map(|a| a.position(now)).unwrap()
        };

        // A second on the clock, and the element has played none of it.
        std::thread::sleep(std::time::Duration::from_millis(1_000));
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 1_000.0);
        assert_eq!(sb.at_ms, loaded, "not sent ahead to where the clock counts");
        net.carry();
        let at = counted(&rb, net.peer(b));
        assert!((at - loaded).abs() <= SYNC_MS, "the session is where the element is: {at}");
        let calls = rb.stats().calls;
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 1_001.0);
        assert_eq!(rb.stats().calls, calls, "and says it once");

        // Within the bound, nothing either way.
        sb.at_ms = counted(&rb, net.peer(b)) - SYNC_MS / 2;
        let was = sb.at_ms;
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 2_500.0);
        assert_eq!((rb.stats().calls, sb.at_ms), (calls, was));

        // A seek on the desktop is the session moving: the element goes.
        rd.call(net.peer(d), "seek", input::seek(100_000)).unwrap();
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 3_000.0);
        assert!((sb.at_ms - 100_000).abs() <= SYNC_MS, "sought where the session is: {}", sb.at_ms);
        assert_eq!(rb.stats().calls, calls, "and nothing said back");
    }

    /// The output says what happens to it, and only that, each once: a
    /// track that ended is `advance`, and the next one is loaded from the
    /// session it makes; the last one ending leaves the session paused at
    /// its start, said once however often an element still at its end is
    /// reconciled — and pressing play there plays it again.
    ///
    /// Falsified by not remembering which end was said: the last track's
    /// end was said on every reconcile after it.
    #[test]
    fn a_track_that_ends_is_said_once_and_the_next_one_plays() {
        let mut net = Net::new();
        let b = net.device("browser", true);
        let mut rb = Remote::new();
        rb.open("browser");
        let (mut sb, mut qb) = (Fake::default(), vec![]);
        rb.call(net.peer(b), "start", input::start(&queue(), 0, 0, "")).unwrap();
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 0.0);
        sb.ended = true;
        sb.playing = false;
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 1.0);
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 2.0);
        assert_eq!(rb.session(net.peer(b)).unwrap().session.unwrap().at, 1, "one track on");
        assert_eq!(sb.loaded(), Some(track_id(&queue()[1])));
        assert!(sb.playing);

        sb.ended = true;
        sb.playing = false;
        let calls = rb.stats().calls;
        for t in 3..6 {
            rb.reconcile(net.peer(b), &mut sb, &mut qb, 1, "", t as f64);
            net.carry();
        }
        assert_eq!(rb.stats().calls, calls + 1, "the end of the queue, said once");
        let a = rb.session(net.peer(b)).unwrap().session.unwrap();
        assert_eq!((a.at, a.clock), (1, Clock::Paused { at_ms: 0 }));
        assert!(!sb.playing);
        rb.call(net.peer(b), "play", input::none()).unwrap();
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 1, "", 7.0);
        assert!(sb.playing, "and played again from its start");
    }

    /// A play counts once it has played thirty seconds or half the track,
    /// once per play, and only on the device making the sound — offline
    /// too, as a pending intent that reaches the server later.
    ///
    /// Falsified by counting on every reconcile past the threshold: the
    /// second one authored a second play.
    #[test]
    fn a_play_is_recorded_once_by_the_output_alone() {
        let mut net = Net::new();
        let b = net.device("browser", true);
        let d = net.device("desktop", false);
        let (mut rb, mut rd) = (Remote::new(), Remote::new());
        rb.open("browser");
        rd.open("desktop");
        let (mut sb, mut sd) = (Fake::default(), Fake::default());
        let (mut qb, mut qd) = (vec![], vec![]);
        rb.call(net.peer(b), "start", input::start(&queue(), 0, 29_000, "album/Water Music"))
            .unwrap();
        net.carry();
        let pending = net.peer(b).pending_len();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 0.0);
        assert!((29_000..29_500).contains(&sb.at_ms), "loaded where the session counts it: {}", sb.at_ms);
        assert_eq!(
            net.peer(b).pending_len(),
            pending,
            "not yet: half of a hundred seconds is fifty, thirty is first"
        );
        // A second and a half on: the player played on, and the session's
        // clock with it (a seek, here, standing in for the time passing).
        sb.at_ms = 30_500;
        rb.call(net.peer(b), "seek", input::seek(30_500)).unwrap();
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 2.0);
        assert_eq!(net.peer(b).pending_len(), pending + 1, "thirty seconds in: a play");
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 3.0);
        assert_eq!(net.peer(b).pending_len(), pending + 1, "once per play");
        assert_eq!(rb.stats().plays, 1);
        rd.reconcile(net.peer(d), &mut sd, &mut qd, 0, "", 3.0);
        assert_eq!(rd.stats().plays, 0, "a remote control records nothing");
        // Unlinked, the play is an intent like any other: pending, said
        // when the device next reaches the server.
        let recent: Vec<crate::domain::Play> = net.peer(b).query("recently_played", &()).unwrap();
        assert_eq!(recent.len(), 1, "the play is in this device's view of the log at once");
        assert_eq!(recent[0].from, "album/Water Music");
    }

    /// The decided rule for coming back, the first way: something is
    /// playing elsewhere when this device's link returns, so its own sound
    /// — it went on playing while it was away — stops, and it says nothing.
    ///
    /// Falsified by a reconcile that says `start` whenever it comes back
    /// playing: the browser took the sound from the laptop.
    #[test]
    fn coming_back_to_a_session_sounding_elsewhere_stops_this_device() {
        let mut net = Net::new();
        let b = net.device("browser", true);
        let l = net.device("laptop", true);
        let (mut rb, mut rl) = (Remote::new(), Remote::new());
        rb.open("browser");
        rl.open("laptop");
        let (mut sb, mut sl) = (Fake::default(), Fake::default());
        let (mut qb, mut ql) = (vec![], vec![]);
        rb.call(net.peer(b), "start", input::start(&queue(), 0, 0, "")).unwrap();
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 0.0);
        rl.reconcile(net.peer(l), &mut sl, &mut ql, 0, "", 0.0);

        // The browser's link drops; it plays on, on its own. The laptop
        // takes the session's sound meanwhile.
        net.drop_link(b);
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 1.0);
        assert!(sb.playing, "offline, its own session: it plays on");
        rl.call(net.peer(l), "skip", input::skip(1)).unwrap();
        net.carry();
        rl.reconcile(net.peer(l), &mut sl, &mut ql, 0, "", 1.0);
        assert!(sl.playing, "the laptop took the sound: the browser that had it is away");

        net.relink(b);
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 2.0);
        net.carry();
        assert!(!sb.playing, "something is playing elsewhere: this device's own sound stops");
        let s = rb.session(net.peer(b)).unwrap();
        assert!(s.outputs("laptop") && s.sounding(), "and the laptop plays on");
        assert_eq!(rb.stats().came_back_playing, 0);
    }

    /// …and the other way: nothing is sounding when it comes back, so what
    /// it was doing on its own becomes the session — its queue, its track
    /// and where it had got to, on this device.
    ///
    /// Falsified by a reconcile that only ever stops its sound on coming
    /// back: the session stayed paused and away, and the browser went
    /// silent.
    #[test]
    fn coming_back_playing_to_a_quiet_session_makes_it_this_one() {
        let mut net = Net::new();
        let b = net.device("browser", true);
        let mut rb = Remote::new();
        rb.open("browser");
        let (mut sb, mut qb) = (Fake::default(), vec![]);
        rb.call(net.peer(b), "start", input::start(&queue(), 0, 0, "")).unwrap();
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 0.0);

        net.drop_link(b);
        // On its own, it moved to the second track and on into it.
        sb.load(&queue()[1], 12_000, true);
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 1, "playlist/Favorites", 1.0);
        net.relink(b);
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 1, "playlist/Favorites", 2.0);
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 1, "playlist/Favorites", 3.0);
        assert!(sb.playing, "its own sound goes on");
        let s = rb.session(net.peer(b)).unwrap();
        let a = s.session.as_ref().unwrap();
        assert!(s.outputs("browser") && s.sounding());
        assert_eq!((a.at, a.from.as_str()), (1, "playlist/Favorites"));
        assert!(matches!(a.clock, Clock::Playing { at_ms: 12_000, .. }));
        assert_eq!(rb.stats().came_back_playing, 1);
    }

    /// Unlinked, nothing is called: a call made then would be said on the
    /// next connection, long after it meant anything.
    ///
    /// Falsified by `joined` not asking whether the peer is linked: the
    /// offline reconcile drove the player from a stale session.
    #[test]
    fn a_device_unlinked_is_its_own_session() {
        let mut net = Net::new();
        let b = net.device("browser", true);
        let mut rb = Remote::new();
        rb.open("browser");
        let (mut sb, mut qb) = (Fake::default(), vec![]);
        rb.call(net.peer(b), "start", input::start(&queue(), 0, 0, "")).unwrap();
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 0.0);
        net.drop_link(b);
        sb.pause();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 1.0);
        assert!(!sb.playing, "the stale session, which says playing, is not obeyed");
        assert!(rb.session(net.peer(b)).is_none());
    }

    /// A browser's player follows its own volume in the session, whoever
    /// moved it: it says what it can do and where it is once it has joined,
    /// a level set on the desktop is the level it plays at, a mute is a
    /// mute — and a volume the element was moved to behind the session's
    /// back is said back, once, as the browser's own. The desktop, which
    /// sounds nothing, says nothing and has no volume to move.
    ///
    /// Falsified by a `follow_volume` that did not remember what it applied:
    /// the element's own move was taken for the session's old level and put
    /// back to 30, never reported.
    #[test]
    fn the_players_volume_follows_the_session_and_a_change_behind_it_is_said() {
        let mut net = Net::new();
        let b = net.device("browser", true);
        let d = net.device("desktop", false);
        let (mut rb, mut rd) = (Remote::new(), Remote::new());
        rb.open("browser");
        rd.open("desktop");
        let mut sb = Fake {
            volume: Some((70, false)),
            ..Fake::default()
        };
        let mut sd = Fake::default();
        let (mut qb, mut qd) = (vec![], vec![]);
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 0.0);
        rd.reconcile(net.peer(d), &mut sd, &mut qd, 0, "", 0.0);
        net.carry();
        let s = rd.session(net.peer(d)).unwrap();
        let browser = s.device("browser").unwrap();
        assert_eq!(
            (browser.can_volume, browser.volume, browser.muted),
            (true, 70, false),
            "said once it joined"
        );
        assert!(!s.device("desktop").unwrap().can_volume, "the desktop has no volume to say");

        rd.call(net.peer(d), "set_volume", input::set_volume("browser", 30)).unwrap();
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 1.0);
        assert_eq!(sb.volume, Some((30, false)), "a level set on the desktop is the browser's");
        rd.call(net.peer(d), "set_mute", input::set_mute("browser", true)).unwrap();
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 2.0);
        assert_eq!(sb.volume, Some((30, true)), "and a mute is a mute");

        // Something moved the element itself.
        sb.volume = Some((55, false));
        let calls = rb.stats().calls;
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 3.0);
        net.carry();
        rb.reconcile(net.peer(b), &mut sb, &mut qb, 0, "", 4.0);
        assert_eq!(sb.volume, Some((55, false)), "not fought");
        assert_eq!(rb.stats().calls, calls + 1, "said, once");
        let s = rd.session(net.peer(d)).unwrap();
        let browser = s.device("browser").unwrap();
        assert_eq!((browser.volume, browser.muted), (55, false), "and the desktop hears it");
    }
}
