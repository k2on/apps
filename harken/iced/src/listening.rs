//! This device's end of the account's listening session: a reconciler.
//!
//! The session is the domain's live state (`harken_domain::listening`),
//! held by the server, one per account, on the sync socket — what
//! [`ark_client::Peer::live`] hands back is the server's confirmed state
//! with this device's own calls replayed on top. **The state is the
//! instruction**: nothing here is told what to do. Every button on every
//! device is a transition ([`Remote::call`]); the device the session names
//! as its output reads the session and makes its player agree
//! ([`Remote::reconcile`]); every other device is silent and draws it.
//!
//! What the output says back is only what happens to it, each a transition
//! of its own: `started` once it has a hand-off loaded, `advance` when a
//! track ends, `stalled` when the sound stops by itself. There is no
//! heartbeat: a playing clock carries the server's time it started from, so
//! every device counts the position for itself ([`Remote::position_ms`],
//! against [`ark_client::Peer::server_now`]).
//!
//! **A device unlinked is its own session.** While the socket is down, or
//! before the server's snapshot has come on a new one, the player is this
//! device's alone and nothing is called — a call made then would be said on
//! the next connection, long after it meant anything. Coming back, the
//! decided rule: if the session is sounding somewhere else, this device's
//! own sound stops and it is a controller of the device playing; if nothing
//! is sounding and this device was playing on its own, what it was doing
//! becomes the session — a `start`, which hands it the sound.
//!
//! **A device is a login.** `ark-auth` says a session is "one login on one
//! device", so the device id is the login's session id: what a `hello`
//! declares with an empty id, and what the session's `devices` key it by.
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

use ark_client::ark::live::state::DeviceDecl;
use ark_client::{Id, Peer};
use harken_domain::listening::read::{self, STATE};
// What the rest of this program asks *this* module about.
pub use harken_domain::listening::read::{input, Active, Clock, Device, Kind, Listening, Output, Track};

/// How far the player may be from where the session counts it before the
/// output seeks to agree: a seek on another device moves the clock by more
/// than this, and a player's own jitter never does.
const DRIFT_MS: i64 = 2_000;

/// How long the output's player may sit paused while the session says it is
/// playing before the output says it has stalled: long enough for a stream
/// to start after `play()`, which in a browser is asynchronous.
const STALL_MS: f64 = 2_500.0;

/// What sounds, as the reconciler sees it: the App's player, or a test's.
pub trait Sound {
    /// The id of the track loaded, if any.
    fn loaded(&self) -> Option<Id>;
    fn playing(&self) -> bool;
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
    met: i64,
    /// The player was playing while this device was its own session.
    alone_playing: bool,
    /// The hand-off `started` has been said for: the track it was loaded on.
    started_for: Option<(usize, Id)>,
    /// The track end `advance` has been said for.
    advanced_for: Option<(usize, Id)>,
    /// Every load of a track is a play; this one's number, and the last
    /// number counted.
    play: u64,
    counted: u64,
    /// When this device last asked its player to play, by its own clock.
    asked: Option<f64>,
    /// The volume its player was last given, or last found at: what tells a
    /// change the session asked for from one made behind its back.
    applied: Option<(i64, bool)>,
    stats: Stats,
}

/// The device this build declares at `hello`: its login's session (an empty
/// id), a computer, and whether it can make a sound at all.
pub fn device() -> DeviceDecl {
    DeviceDecl::new("", Kind::Computer.name(), device_name(), crate::Player::AUDIBLE)
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
        if !self.joined(peer) {
            return None;
        }
        peer.live(STATE).and_then(|v| Listening::from_value(&v))
    }

    /// Whether this device is part of the session now.
    pub fn joined(&self, peer: &Peer) -> bool {
        peer.linked() && self.me == peer.ctx().session && peer.live_status(STATE).is_some_and(|s| s.synced)
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
    /// the server's: what a scrubber drawn from any device shows.
    pub fn position_ms(peer: &Peer, a: &Active) -> i64 {
        a.clock.position(peer.server_now())
    }

    /// Call a transition of the session. A refusal — the preview's, here —
    /// is said back and counted; the server's arrive later
    /// ([`Remote::refusals`]).
    pub fn call(&mut self, peer: &mut Peer, transition: &str, input: ark_client::Args) -> Result<(), String> {
        self.stats.calls += 1;
        let throttled = transition == "scrub";
        let called = match throttled {
            true => peer.call_throttled(STATE, transition, input),
            false => peer.call(STATE, transition, input),
        };
        called.map(|_| ()).map_err(|e| {
            let why = e.to_string();
            self.stats.refused += 1;
            self.stats.last_refusal = Some(format!("{transition}: {why}"));
            why
        })
    }

    /// What the server refused of this device's calls since the last ask.
    pub fn refusals(&mut self, peer: &mut Peer) -> Vec<String> {
        let out: Vec<String> = peer.take_live_refusals().into_iter().map(|r| r.reason).collect();
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
        if self.met != peer.epoch() {
            // The first sight of the session on this connection: what this
            // device can do, said, and the rule for coming back.
            self.met = peer.epoch();
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
        let now = peer.server_now();
        let track = a.now().clone();
        let want = a.clock.position(now);
        if sound.loaded() != Some(track.id) || *queue != a.queue {
            *queue = a.queue.clone();
            if sound.loaded() != Some(track.id) {
                sound.load(&track, want, a.clock.playing());
                self.play += 1;
                self.advanced_for = None;
                self.asked = a.clock.playing().then_some(clock);
            }
        }
        // The hand-off has landed when the track is loaded: said once.
        let moving = matches!(&a.output, Output::Device { moving: true, .. });
        if moving && self.started_for != Some((a.at, track.id)) {
            self.started_for = Some((a.at, track.id));
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
        if sound.ended() && a.clock.playing() && self.advanced_for != Some((a.at, track.id)) {
            self.advanced_for = Some((a.at, track.id));
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
        // Somewhere else in the track than the session counts: a seek made
        // on another device.
        if (sound.position_ms() - want).abs() > DRIFT_MS {
            sound.seek(want);
        }
        // A play, once it has played long enough, once.
        if a.clock.playing() && self.counted != self.play && sound.position_ms() >= read::counts_after(track.duration_ms) {
            self.counted = self.play;
            let args = input::record_play(&track, &a.from);
            match peer.mutate("record_play", args) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use ark_client::ark::live::state::{Desk, Runner, Said, Who};
    use ark_client::ark::protocol::{ClientMsg, ServerMsg};
    use ark_client::{Domain, Options};

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
            self.loaded = Some((track.id, position_ms));
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
            id,
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

    /// A server in this test — the engine's own sans-io desk — and the
    /// devices on it: each a real `Peer`, its frames carried by hand.
    struct Net {
        desk: Desk,
        peers: Vec<(Peer, i64)>,
        now: i64,
    }

    fn runner() -> Runner {
        let m = harken_domain::module();
        let mut r = Runner::of_module(m.build());
        r.hold(m.lives());
        r
    }

    impl Net {
        fn new() -> Net {
            Net {
                desk: Desk::new(runner()),
                peers: vec![],
                now: ark_client::autos::now_ms(),
            }
        }

        /// A device of alice's, linked: `can_play` as its build can.
        fn device(&mut self, session: &str, can_play: bool) -> usize {
            let opts = Options::server("alice", session, Some("t".into())).with_device(DeviceDecl::new("", "computer", session, can_play));
            let mut p = Peer::open_memory(Domain::new(&harken_domain::module()), opts).unwrap();
            let conn = self.peers.len() as i64 + 1;
            p.connected();
            self.peers.push((p, conn));
            let k = self.peers.len() - 1;
            self.carry();
            k
        }

        fn drop_link(&mut self, k: usize) {
            let (p, c) = &mut self.peers[k];
            p.disconnected();
            let out = self.desk.part(*c, self.now);
            self.deliver(out);
        }

        fn relink(&mut self, k: usize) {
            self.peers[k].0.connected();
            self.carry();
        }

        fn deliver(&mut self, out: ark_client::ark::live::state::Out) {
            for (c, said) in out {
                if let Some((p, _)) = self.peers.iter_mut().find(|(_, pc)| *pc == c) {
                    if p.linked() {
                        p.recv(match said {
                            Said::Applied(a) => ServerMsg::Applied(a),
                            Said::Refused(r) => ServerMsg::Refused(r),
                            Said::Snapshot(s) => ServerMsg::LiveSnapshot(s),
                        });
                    }
                }
            }
        }

        /// What every linked peer has to say, said, until nobody has more.
        fn carry(&mut self) {
            for _ in 0..8 {
                let mut out = vec![];
                for (p, c) in self.peers.iter_mut() {
                    if !p.linked() {
                        continue;
                    }
                    for m in p.take_outgoing() {
                        match m {
                            ClientMsg::Hello { device: Some(d), .. } => out.extend(self.desk.hello(
                                *c,
                                Who {
                                    user: p.ctx().user.clone(),
                                    session: p.ctx().session.clone(),
                                    roles: Default::default(),
                                },
                                d,
                                self.now,
                            )),
                            ClientMsg::Call(call) => out.extend(self.desk.call(*c, call, self.now)),
                            _ => {}
                        }
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
        assert_eq!(sb.loaded(), Some(queue()[0].id), "the browser loaded what it pressed");
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
        assert!(sl.playing && sl.loaded() == Some(queue()[0].id));
        let s = rl.session(net.peer(l)).unwrap();
        assert!(s.outputs("laptop") && !s.moving_to("laptop"), "the laptop said it had started");
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
        assert_eq!(sb.loaded(), Some(queue()[1].id));
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
        let recent = net.peer(b).query("recently_played", &ark_client::Args::new()).unwrap();
        assert_eq!(recent.as_list().len(), 1, "the play is in this device's view of the log at once");
        assert_eq!(recent.as_list()[0].field("from"), ark_client::Value::text("album/Water Music"));
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
