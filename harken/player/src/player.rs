//! A device's player over its own peer (§1.9): the [`Reconciler`] run
//! against the session the peer holds — the server's, with the device's
//! own calls previewed on top — its answers called as the device's live
//! transitions and its plays authored in the log; a [`Press`] called the
//! same way; and **one view** of what plays, which a phone draws whether it
//! is part of the session or its own (`active`: the session's, or, alone,
//! its own player's as if it were one).
//!
//! **A device unlinked is its own session**: nothing is called while its
//! socket is down or before it stands in the session on a new one (its
//! `on_join` has landed). Plays are the exception, because they are the
//! log's: offline, one is a pending intent like any other.

use ark_peer::{now_ms, Disk, Peer};
use harken_domain::host::Listening;
use harken_domain::{Active, Clock, Output};
use serde::Serialize;
use serde_json::{json, Value};

use crate::screens::{self, Alone, Ask, Devices, Line, NowPlaying};
use crate::{Press, Reconciler, Say, Sound, View, NOT_PART};

/// Every number a device's player holds that its peer does not, for the
/// diagnostics.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Stats {
    /// Transitions called, and refused by the preview or the server.
    pub calls: u64,
    pub refused: u64,
    /// `record_play`s authored.
    pub plays: u64,
    /// `start`s said coming back with something of its own playing.
    pub came_back: u64,
    pub last_refusal: Option<String>,
}

/// One device's player.
pub struct Player {
    me: String,
    core: Reconciler,
    pub stats: Stats,
    /// The last refusal the server sent this device's calls.
    refused: Option<String>,
}

/// The reconciler's answers, called through the peer and counted.
struct Calls<'a> {
    peer: &'a mut Peer,
    stats: &'a mut Stats,
}

impl Calls<'_> {
    fn call(&mut self, transition: &str, input: Vec<u8>) -> Result<(), String> {
        self.stats.calls += 1;
        let out = self.peer.call(&format!("listening.{transition}"), input);
        if let Err(why) = &out {
            self.stats.refused += 1;
            self.stats.last_refusal = Some(format!("{transition}: {why}"));
        }
        out
    }
}

impl Say for Calls<'_> {
    fn call(&mut self, transition: &'static str, input: Vec<u8>, _as_of: Option<i64>, _why: &str) -> Result<(), String> {
        let out = Calls::call(self, transition, input);
        // The reconciler's only `start` is the rule for coming back.
        if out.is_ok() && transition == "start" {
            self.stats.came_back += 1;
        }
        out
    }

    fn play(&mut self, play: harken_domain::RecordPlay) -> Result<(), String> {
        match self.peer.mutate("listening.record_play", &play) {
            Ok(_) => {
                self.stats.plays += 1;
                Ok(())
            }
            Err(why) => {
                self.stats.last_refusal = Some(format!("record_play: {why}"));
                Err(why)
            }
        }
    }
}

impl Player {
    /// The player of the device `me` (its login's session id, which is what
    /// its `hello` declared), its plays counted on `counts`.
    pub fn new(me: &str, counts: Option<Box<dyn Disk>>) -> Player {
        Player {
            me: me.into(),
            core: Reconciler::new(me, counts),
            stats: Stats::default(),
            refused: None,
        }
    }

    pub fn me(&self) -> &str {
        &self.me
    }

    /// The account's session as the peer holds it, linked or not.
    pub fn listening(p: &Peer) -> Listening {
        match &p.live {
            Some(live) => Listening::of(&live.store, &p.me().actor),
            None => Listening::default(),
        }
    }

    /// Whether the device is part of the session now: linked, as itself,
    /// and standing in it.
    pub fn joined(&self, p: &Peer, s: &Listening) -> bool {
        p.status().linked && p.me().session == self.me && s.device(&self.me).is_some()
    }

    /// Make the player agree with the session, and say what happened to it;
    /// what the server refused since, taken.
    pub fn step(&mut self, p: &mut Peer, sound: &mut dyn Sound) {
        let s = Player::listening(p);
        let v = View {
            me: &self.me,
            joined: self.joined(p, &s),
            epoch: p.status().epoch as i64,
            listening: &s,
            now: now_ms(),
        };
        self.core.reconcile(
            &v,
            sound,
            &mut Calls {
                peer: p,
                stats: &mut self.stats,
            },
        );
        self.refusals(p);
    }

    /// What the server refused of this device's calls since the last ask.
    pub fn refusals(&mut self, p: &mut Peer) -> Vec<String> {
        let out = p.take_live_refusals();
        if let Some(last) = out.last() {
            self.stats.refused += out.len() as u64;
            self.stats.last_refusal = Some(last.clone());
            self.refused = Some(last.clone());
        }
        out
    }

    /// A press, as a transition — or, its own session, [`NOT_PART`]: the
    /// caller does it to the player. `local` is that player, whose queue a
    /// play presses when the session has none. None when it was taken.
    pub fn press(&mut self, p: &mut Peer, press: &Press, local: Option<&dyn Sound>) -> Option<String> {
        let s = Player::listening(p);
        if !self.joined(p, &s) {
            return Some(NOT_PART.into());
        }
        let local = local.map(|l| (l.tracks(), l.heard().at.unwrap_or(0), l.heard().from));
        let (transition, input) = match press.transition(&s, &self.me, local) {
            Ok(Some(call)) => call,
            Ok(None) => return None,
            Err(why) => return Some(why.into()),
        };
        Calls {
            peer: p,
            stats: &mut self.stats,
        }
        .call(transition, input)
        .err()
    }

    /// What plays, as the device draws it: `{me, joined, linked, epoch,
    /// listening, active, stats, refused}` — `listening` the session while
    /// joined, and `active` what plays, the session's or, alone, the
    /// player's own.
    pub fn view(&self, p: &Peer, sound: Option<&dyn Sound>) -> Value {
        let s = Player::listening(p);
        let joined = self.joined(p, &s);
        let st = p.status();
        let active = match (joined, sound) {
            (true, _) => s.session.clone(),
            (false, Some(sound)) => self.own(sound),
            (false, None) => None,
        };
        let listening = joined.then(|| json!({ "devices": s.devices, "scrub": s.scrub, "session": s.session }));
        json!({
            "me": self.me,
            "joined": joined,
            "linked": st.linked,
            "epoch": st.epoch,
            "listening": listening,
            "active": active,
            "stats": self.stats,
            "refused": self.refused,
        })
    }

    /// The player screen's row (`screens::now_playing`): `here` what this
    /// phone is called, `server` what a cover's path is joined to — each
    /// album's cover read from the log as the peer holds it.
    pub fn now_playing(&self, p: &Peer, sound: Option<&dyn Sound>, here: &str, server: &str) -> NowPlaying {
        let s = Player::listening(p);
        let joined = self.joined(p, &s);
        let active = match joined {
            true => s.session.clone(),
            false => sound.and_then(|sound| self.own(sound)),
        };
        let buffering = sound.is_some_and(|s| matches!(s.heard().place, crate::Place::Believed(_)));
        let cover = |album: &str| {
            let art = ark::db::read::<harken_domain::schema::Harken, _>(&p.log.store, |db| db.album.get(album).map(|a| a.art));
            art.filter(|a| !a.is_empty())
                .map(|a| harken_domain::host::url(server, &a))
                .unwrap_or_default()
        };
        screens::now_playing(
            &s,
            &self.me,
            joined,
            active.as_ref(),
            (sound.and_then(|s| s.volume()), buffering),
            here,
            &cover,
        )
    }

    /// The device picker's rows, while joined; none alone.
    pub fn picker(&self, p: &Peer) -> Devices {
        let s = Player::listening(p);
        match self.joined(p, &s) {
            true => screens::picker(&s, &self.me),
            false => Devices::default(),
        }
    }

    /// The diagnostics' listening numbers.
    pub fn numbers(&self, p: &Peer) -> Vec<Line> {
        let s = Player::listening(p);
        let joined = self.joined(p, &s);
        let st = p.status();
        let held = joined.then_some(&s);
        screens::numbers(
            held,
            &self.me,
            joined,
            st.linked,
            st.epoch as i64,
            &self.stats,
            self.refused.as_deref(),
            now_ms(),
        )
    }

    /// A screen's ask: its presses while joined, the first refusal said;
    /// alone, a level put on the player's stream, or [`NOT_PART`] for what
    /// the caller does to the player itself.
    pub fn ask(&mut self, p: &mut Peer, ask: &Ask, sound: Option<&mut dyn Sound>) -> Option<String> {
        let s = Player::listening(p);
        if !self.joined(p, &s) {
            return match ask.alone(sound.as_ref().and_then(|s| s.volume())) {
                Ok(Some(Alone::Stream(v))) => {
                    if let Some(sound) = sound {
                        sound.set_volume(v);
                    }
                    None
                }
                Ok(Some(Alone::Player)) => Some(NOT_PART.into()),
                Ok(None) => None,
                Err(why) => Some(why.into()),
            };
        }
        let presses = match ask.presses(&s, &self.me) {
            Ok(ps) => ps,
            Err(why) => return Some(why.into()),
        };
        let mut why = None;
        for press in presses {
            let local = sound.as_deref().map(|s| s as &dyn Sound);
            why = why.or(self.press(p, &press, local));
        }
        why
    }

    /// The player's own queue as a session would hold it, this device its
    /// output: what a device alone draws.
    fn own(&self, sound: &dyn Sound) -> Option<Active> {
        let h = sound.heard();
        let queue = sound.tracks();
        let at = h.at?.min(queue.len().checked_sub(1)?);
        let ms = h.place.ms().unwrap_or(0).max(0);
        Some(Active {
            at: at as i64,
            clock: match h.playing {
                true => Clock::Playing { at_ms: ms, since: now_ms() },
                false => Clock::Paused { at_ms: ms },
            },
            from: h.from,
            output: Output::Device {
                id: self.me.clone(),
                moving: false,
            },
            queue,
        })
    }
}
