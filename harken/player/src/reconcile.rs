//! The reconciler (§1.9): one device's player made to agree with the
//! session, and what happened to it said — the same rules for a phone and a
//! speaker, alone and joined.
//!
//! **Joined**, every look runs, in order: what the device can do (`equip`,
//! once a connection), the rule for coming back (sounding elsewhere, its
//! own sound stops; nothing sounding and it was playing on its own, its
//! queue becomes the session — a `start`), its volume and its group, and
//! then either **the output** — its player loaded with the session's queue
//! (the whole of it, so that it plays on through it when the socket goes),
//! moved where the session is, played or paused as it is, and what it did by
//! itself said — or **silent**, let go of once. **Alone**, nothing is said
//! but the plays, which are the log's.
//!
//! A play is authored once a track has played thirty seconds or half its
//! length, once per load, by the device making the sound; short of that the
//! player is asked to wake the reconciler when it will have.

use ark::db::{Durable, Volatile};
use ark::format::Encode;
use ark_peer::{Disk, Instance};
use harken_domain::host::{counts_after, UNSAID_VOLUME};
use harken_domain::{Active, Clock, Output, Present, Track};

use crate::memory::*;
use crate::{Event, Place, Say, Sound, View, Vol, DRIFT_MS, REPORT_MS, SYNC_MS};

/// One device's reconciler: its mind, and its play counter.
pub struct Reconciler {
    me: String,
    minds: Instance,
    /// None where plays are not this player's to author.
    counts: Option<Instance>,
}

impl Reconciler {
    /// The reconciler of the device `me`, counting its plays on `counts`
    /// (none: it counts none).
    pub fn new(me: &str, counts: Option<Box<dyn Disk>>) -> Reconciler {
        Reconciler {
            me: me.into(),
            minds: Instance::ephemeral::<Minds>(),
            counts: counts.map(Instance::open::<Counts>),
        }
    }

    /// What it keeps now.
    pub fn mind(&self) -> Mind {
        let me = self.me.clone();
        self.minds
            .read::<Minds, _>(|db| db.mind.get(me.clone()))
            .unwrap_or_else(|| Mind::new(&me))
    }

    /// Make the player agree with the session as `v` reads it, and say what
    /// happened to it.
    pub fn reconcile(&mut self, v: &View, sound: &mut dyn Sound, say: &mut dyn Say) {
        let mut m = self.mind();
        let before = m.clone();
        self.look(&mut m, v, sound, say);
        if m != before {
            let _ = self.minds.write::<Minds, Volatile, _>(|db| db.mind.put(m));
        }
    }

    fn look(&mut self, m: &mut Mind, v: &View, sound: &mut dyn Sound, say: &mut dyn Say) {
        let heard = sound.heard();
        let Some(mine) = v.listening.device(v.me).filter(|_| v.joined) else {
            // Its own session. Whether it was playing on its own is what
            // matters when it comes back; and a play is the log's, so it
            // counts here too.
            m.alone = if heard.playing { Alone::Playing } else { Alone::Quiet };
            if let Some((at, id)) = heard.at.and_then(|at| Some((at, *heard.queue.get(at)?))) {
                let here = Here { at: at as i64, track: id };
                self.count(sound, here, None, &heard.from, heard.place, heard.playing, say);
            }
            return;
        };
        let s = v.listening;
        let equipped = self.equip(m, v, mine, sound, say);
        if m.met != v.epoch {
            // The first sight of the session on this connection: the rule
            // for coming back.
            m.met = v.epoch;
            if m.alone == Alone::Playing && !heard.queue.is_empty() {
                m.alone = Alone::Quiet;
                if !(s.sounding() && !s.outputs(v.me)) {
                    let tracks = sound.tracks();
                    let at = heard.at.unwrap_or(0).min(tracks.len().saturating_sub(1));
                    let start = harken_domain::Start {
                        at: at as i64,
                        from: heard.from.clone(),
                        position: heard.place.ms().unwrap_or(0).max(0),
                        queue: tracks,
                    };
                    let _ = say.call("start", start.to_bytes(), None, "it came back playing to a quiet session");
                    // Its own sound goes on: the session it has just made
                    // says what it is doing.
                    m.expect = Expect::At { at: at as i64 };
                    return;
                }
                // Somebody else is playing: a controller of it now, and its
                // own sound stops below.
            }
        }
        if !equipped {
            self.volume(m, mine, sound, say);
            self.group(m, v, mine, sound, say);
        }
        match &s.session {
            Some(a) if s.outputs(v.me) => {
                m.role = Role::Output;
                self.drive(m, v, a, sound, say);
            }
            _ => {
                // Not the output: silent. Losing the sound is not something
                // a device does; it is told, and this is where it notices.
                if heard.playing || m.role == Role::Output {
                    let follows = mine.follows.is_some() && mine.follows.as_deref() == s.leader();
                    sound.let_go(follows);
                }
                m.role = Role::Silent;
                m.handoff = Handoff::None;
            }
        }
    }

    /// What the device can do, said once a connection where the session
    /// does not know it yet. Whether it was said.
    fn equip(&mut self, m: &mut Mind, v: &View, mine: &Present, sound: &mut dyn Sound, say: &mut dyn Say) -> bool {
        let Some(caps) = sound.caps() else { return false };
        if (mine.can_volume, &mine.family) == (caps.can_volume, &caps.family) {
            return false;
        }
        let said = Equipped::Said {
            epoch: v.epoch,
            can_volume: caps.can_volume,
            family: caps.family.clone(),
        };
        if m.equipped == said {
            return false;
        }
        let now = sound.volume().unwrap_or(Vol {
            level: UNSAID_VOLUME,
            muted: false,
        });
        let input = harken_domain::Equip {
            can_volume: caps.can_volume,
            family: caps.family,
            muted: now.muted,
            volume: now.level.clamp(0, 100),
        };
        if say.call("equip", input.to_bytes(), None, "what it says it can do").is_err() {
            return false;
        }
        m.equipped = said;
        m.volume = Tuned::Settled { wanted: now, seen: now };
        true
    }

    /// The player's volume at its entry in the session, whoever moved it
    /// there — and the player moved by anything else, said. Moved is read
    /// against what the player read after this reconciler's own last write,
    /// so its own write is never said back: the session asks for 30, a
    /// phone's stream takes its nearest step and reads 27, and 27 is where it
    /// was left, not a hand.
    fn volume(&mut self, m: &mut Mind, mine: &Present, sound: &mut dyn Sound, say: &mut dyn Say) {
        if !mine.can_volume {
            return;
        }
        let Some(now) = sound.volume() else { return };
        let want = Vol {
            level: mine.volume,
            muted: mine.muted,
        };
        let report = |say: &mut dyn Say, v: Vol| {
            let input = harken_domain::ReportVolume {
                level: v.level.clamp(0, 100),
                muted: v.muted,
            };
            let _ = say.call("report_volume", input.to_bytes(), None, "its volume was moved by hand");
        };
        m.volume = match m.volume {
            // The session moved: the player is told.
            Tuned::Told { wanted } | Tuned::Settled { wanted, .. } if wanted != want => {
                sound.set_volume(want);
                match sound.volume() {
                    Some(seen) => Tuned::Settled { wanted: want, seen },
                    None => Tuned::Told { wanted: want },
                }
            }
            // The player moved, and not by this: buttons, a panel, a knob.
            Tuned::Settled { seen, .. } if now != seen => {
                if now != want {
                    report(say, now);
                }
                Tuned::Settled { wanted: now, seen: now }
            }
            Tuned::Told { wanted } => Tuned::Settled { wanted, seen: now },
            Tuned::Untold => {
                if now != want {
                    report(say, now);
                }
                Tuned::Settled { wanted: now, seen: now }
            }
            settled => settled,
        };
    }

    /// The device's group, from its leader's side: who follows it in the
    /// session against who plays with it in the house — the session moved
    /// is told to the house (`join`, and `unjoin` of what the session has in
    /// no group at all), the house moved is said (`report_group`).
    fn group(&mut self, m: &mut Mind, v: &View, mine: &Present, sound: &mut dyn Sound, say: &mut dyn Say) {
        let s = v.listening;
        if mine.family.is_none() {
            return;
        }
        let Some(f) = sound.followers() else { return };
        let mut session: Vec<String> = s.followers(v.me).iter().map(|d| d.id.clone()).collect();
        let mut house: Vec<String> = match f.leads {
            true => f.members.into_iter().filter(|x| x != v.me && s.device(x).is_some()).collect(),
            false => vec![],
        };
        session.sort();
        house.sort();
        if session == house {
            m.group = Grouped::Told { members: session };
            return;
        }
        match &m.group {
            Grouped::Told { members } if *members != session => {
                let join: Vec<String> = session.iter().filter(|x| !house.contains(x)).cloned().collect();
                if !join.is_empty() {
                    sound.join(&join);
                }
                // Let go of only what the session has in no group at all: a
                // member another leader has taken is that leader's to join.
                for x in house.iter().filter(|x| !session.contains(x)) {
                    if s.device(x).is_some_and(|d| d.follows.is_none() && s.followers(x).is_empty()) {
                        sound.unjoin(x);
                    }
                }
                m.group = Grouped::Told { members: session };
            }
            _ if f.leads => {
                let input = harken_domain::ReportGroup { members: house.clone() };
                let _ = say.call("report_group", input.to_bytes(), None, "it was grouped by hand");
                m.group = Grouped::Told { members: house };
            }
            _ => {}
        }
    }

    /// The device is the output: its player made to agree with the session,
    /// and what happened to it said.
    fn drive(&mut self, m: &mut Mind, v: &View, a: &Active, sound: &mut dyn Sound, say: &mut dyn Say) {
        let event = sound.take();
        if let Some(Event::Lost { why }) = &event {
            let _ = say.call("release", ().to_bytes(), None, why);
            m.handoff = Handoff::None;
            return;
        }
        let at = (a.at.max(0) as usize).min(a.queue.len().saturating_sub(1));
        let track = a.now();
        let here = Here {
            at: at as i64,
            track: track.id,
        };
        let want = a.position(v.now);
        let playing = a.clock.playing();
        let mut heard = sound.heard();
        if heard.queue.len() != a.queue.len() || heard.queue.iter().zip(&a.queue).any(|(h, t)| *h != t.id) {
            // Another queue: the session's, loaded as the player's own, so
            // that going offline mid-track leaves it playing what it was.
            sound.load(&a.queue, at, want, playing, &a.from);
            m.expect = Expect::At { at: here.at };
            m.handoff = Handoff::None;
            heard = sound.heard();
        } else if heard.at != Some(at) {
            match (m.expect, heard.at) {
                // The move it said has not landed yet: waited for, not fought.
                _ if m.handoff == (Handoff::Advanced { here }) => return,
                // The session is where the player was put: the player moved
                // by itself — the track ended, or a hand moved it.
                (Expect::At { at: e }, Some(i)) if e == here.at => {
                    m.handoff = Handoff::Advanced { here };
                    if i == at + 1 && playing {
                        let input = harken_domain::Advance { from_track: here.at };
                        let _ = say.call("advance", input.to_bytes(), None, "the track ended");
                    } else {
                        let input = harken_domain::Jump { index: i as i64 };
                        let _ = say.call("jump", input.to_bytes(), None, "it was moved by hand");
                    }
                    return;
                }
                // Somebody moved the session: a skip, a jump, a track pressed.
                _ => {
                    sound.jump(at, want);
                    m.expect = Expect::At { at: here.at };
                    if matches!(m.handoff, Handoff::Advanced { .. }) {
                        m.handoff = Handoff::None;
                    }
                    heard = sound.heard();
                }
            }
        } else {
            m.expect = Expect::At { at: here.at };
        }
        // The hand-off has landed when the player has it: said once, where
        // it measurably is.
        let moving = matches!(a.output, Output::Device { moving: true, .. });
        if moving && heard.ready && m.handoff != (Handoff::Started { here }) {
            m.handoff = Handoff::Started { here };
            let (ms, as_of) = match heard.place {
                Place::Measured { ms, at } => (ms, playing.then_some(at)),
                p => (p.ms().unwrap_or(want), None),
            };
            let _ = say.call("started", harken_domain::At { at_ms: ms.max(0) }.to_bytes(), as_of, "it has the hand-off");
            return;
        }
        if !moving && matches!(m.handoff, Handoff::Started { .. }) {
            m.handoff = Handoff::None;
        }
        // The last track ended: said once. Once said, a player still at its
        // end is reconciled like any other.
        if heard.ended && playing && m.handoff != (Handoff::Advanced { here }) {
            m.handoff = Handoff::Advanced { here };
            let _ = say.call(
                "advance",
                harken_domain::Advance { from_track: here.at }.to_bytes(),
                None,
                "the queue ended",
            );
            return;
        }
        match event {
            // It stopped by itself: the session pauses there.
            Some(Event::Stalled { at_ms }) if playing => {
                let _ = say.call(
                    "stalled",
                    harken_domain::At { at_ms: at_ms.max(0) }.to_bytes(),
                    None,
                    "it stopped by itself, or by hand",
                );
                return;
            }
            Some(Event::Resumed) if !playing => {
                let _ = say.call("play", ().to_bytes(), None, "it was resumed by hand");
                return;
            }
            _ => {}
        }
        match (playing, heard.playing) {
            (true, false) => sound.resume(),
            (false, true) => sound.pause(),
            _ => {}
        }
        // Somewhere else in the track than the session counts, by more than
        // the bound. If the session moved since the player was last made to
        // agree — a seek on another device — the player goes where it is
        // now; if it is the player that came apart from a clock nobody
        // touched, the player is right about where it is, and the session is
        // told: as the player's own `seek` past [`DRIFT_MS`], else
        // `report_position`, at most once a [`REPORT_MS`].
        let against = Against::Clock { here, clock: a.clock };
        let moved = m.against != against;
        m.against = against;
        match heard.place {
            p if moved => {
                if p.ms().is_none_or(|ms| (ms - want).abs() > SYNC_MS) {
                    sound.seek(want);
                }
            }
            Place::Measured { ms, at: when } if heard.playing => {
                let Clock::Playing { since, .. } = a.clock else { return };
                let off = ms - a.clock.position(when);
                if off.abs() > DRIFT_MS {
                    let _ = say.call(
                        "seek",
                        harken_domain::Seek { ms: ms.max(0) }.to_bytes(),
                        Some(when),
                        "it is somewhere else in the track",
                    );
                } else if off.abs() > SYNC_MS && !matches!(m.reported, Reported::At { ms: last } if v.now - last < REPORT_MS) {
                    m.reported = Reported::At { ms: v.now };
                    let input = harken_domain::Report { at_ms: ms.max(0), since };
                    let _ = say.call("report_position", input.to_bytes(), Some(when), "it is not where the session counts it");
                }
            }
            _ => {}
        }
        self.count(sound, here, Some(track), &a.from, heard.place, heard.playing, say);
    }

    /// A play, once it has played long enough, once per load of a track —
    /// and, short of that, a wake at the moment it will have. `track` is
    /// the one at `here`, read from the player when not given.
    #[allow(clippy::too_many_arguments)]
    fn count(&mut self, sound: &mut dyn Sound, here: Here, track: Option<&Track>, from: &str, place: Place, playing: bool, say: &mut dyn Say) {
        let Some(counts) = &mut self.counts else { return };
        let me = self.me.clone();
        let was = counts
            .read::<Counts, _>(|db| db.count.get(me.clone()))
            .map_or(Loaded::Nothing, |c| c.loaded);
        let mut loaded = match was {
            Loaded::Track { here: h, counted } if h == here => Loaded::Track { here, counted },
            _ => Loaded::Track { here, counted: false },
        };
        if let (Loaded::Track { counted: false, .. }, true, Some(ms)) = (loaded, playing, place.ms()) {
            let fetched;
            let track = match track {
                Some(t) => t,
                None => {
                    fetched = sound.tracks();
                    match fetched.get(here.at as usize).filter(|t| t.id == here.track) {
                        Some(t) => t,
                        None => return,
                    }
                }
            };
            let due = counts_after(track.duration_ms) - ms;
            if due > 0 {
                sound.wake_in(due);
            } else {
                loaded = Loaded::Track { here, counted: true };
                let _ = say.play(harken_domain::RecordPlay {
                    track: track.id,
                    title: track.title.clone(),
                    creator: track.creator.clone(),
                    album: track.album.clone(),
                    duration_ms: track.duration_ms.max(0),
                    file: track.file.clone(),
                    from: from.into(),
                });
            }
        }
        if loaded != was {
            let _ = counts.write::<Counts, Durable, _>(|db| db.count.put(Count { device: me, loaded }));
        }
    }
}
