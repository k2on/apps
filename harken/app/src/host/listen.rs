//! This device's own player, driven by the host (ui/docs/app.md, "harken's
//! Port"): the domain's reconciler (`harken_domain::player::Reconciler`)
//! making this device's sound — the media player module, through its port
//! ([`MediaSound`]) — agree with the account's listening session as the
//! views hold it, and what happened to it said as changes through the place:
//! the session's transitions as live calls, a play counted and where an
//! episode has got to as the log's. A press is the host's too: a live call
//! while this device stands in the session, done to its own sound while it
//! is alone.
//!
//! A device with no media player (a test's place, a server's) draws the
//! session and presses it all the same; it only makes no sound.

use ark::format::Encode;
use ark_app::host::{Cx, Session};
use ark_app::modules::media_player::MediaPlayer;
use harken_domain::host::Listening;
use harken_domain::player::{Change, Level, Loaded, Press, Reconciler, Say, Sound, View};
use harken_domain::{HarkenChange, HarkenView, LibraryEntry, Track};

use super::Harken;
use crate::player::{live, Playing};
use crate::sound::{remote_press, MediaSound};
use crate::words;

/// The reconciler, this device's sound, and what the player was last told
/// of it.
pub struct Listen {
    core: Reconciler,
    sound: Option<MediaSound>,
    /// What plays alone, as the player last drew it.
    pub own: Option<Playing>,
    /// The clock the sound is timed by, which the player's timeline counts
    /// on from (the demo's day is not it).
    clock: ark_app::host::Clock,
}

/// Every song, as the host reads a track a screen named.
pub fn songs() -> HarkenView {
    HarkenView::Library { playlist: [0; 16] }
}

/// The reconciler's answers, sent through the place.
struct Says<'c, 'a> {
    cx: &'c mut Cx<'a, Harken>,
}

impl Say for Says<'_, '_> {
    fn call(&mut self, transition: &'static str, input: Vec<u8>, _as_of: Option<i64>, _why: &str) -> Result<(), String> {
        let change = live(transition, &input).ok_or_else(|| format!("no transition {transition}"))?;
        self.cx.place.send(change)
    }

    fn play(&mut self, play: harken_domain::RecordPlay) -> Result<(), String> {
        self.cx.place.send(HarkenChange::RecordPlay(play))
    }

    /// Of an episode only: a song keeps no place, and an entry that writes
    /// nothing is still one every peer is sent.
    fn progress(&mut self, place: harken_domain::ReportProgress) -> Result<(), String> {
        let episode = self
            .cx
            .rows::<Vec<u8>>(&songs())
            .iter()
            .filter_map(|b| LibraryEntry::from_bytes(b).ok())
            .any(|e| e.id == place.media_id && e.kind == "episode");
        match episode {
            true => self.cx.place.send(HarkenChange::ReportProgress(place)),
            false => Err("not an episode".into()),
        }
    }
}

impl Listen {
    /// Over the session just opened: the place's media player, if it has
    /// one, playing the tracks of its server.
    pub fn open(session: &Session, cx: &mut Cx<'_, Harken>) -> Listen {
        let port = cx.place.modules().get::<dyn MediaPlayer>();
        let waker = cx.place.waker();
        let clock = cx.place.clock();
        let timed = clock.clone();
        let sound = port.map(|p| {
            // A step asked for later comes at the first position the player
            // says once it is due (`MediaSound`): the host keeps no timer.
            MediaSound::new(
                p,
                &session.server,
                move |ms| {
                    if ms == 0 {
                        waker()
                    }
                },
                move || timed(),
            )
        });
        Listen {
            core: Reconciler::new(&session.device, Some(Loaded::Nothing)),
            sound,
            own: None,
            clock,
        }
    }

    /// The account's session as the views hold it.
    fn listening(cx: &Cx<'_, Harken>) -> Listening {
        let rows = |k: HarkenView| cx.rows::<Vec<u8>>(&k);
        Listening::read(&rows(HarkenView::Session), &rows(HarkenView::Devices), &rows(HarkenView::Scrubbing))
    }

    /// Whether this device is part of the session: linked, as itself, and
    /// standing in it.
    fn joined(s: &Session, l: &Listening) -> bool {
        s.linked == Some(true) && !s.device.is_empty() && l.device(&s.device).is_some()
    }

    /// The sound made to agree with the session, what the system's
    /// controls asked pressed, and what plays alone worked out for the
    /// player.
    pub fn step(&mut self, cx: &mut Cx<'_, Harken>) {
        let Some(sound) = self.sound.as_mut() else { return };
        let remote = sound.take_remote();
        let at = sound.heard().place.ms().unwrap_or(0);
        for p in remote.into_iter().filter_map(|c| remote_press(c, at)) {
            self.press(p, cx);
        }
        let Some(sound) = self.sound.as_mut() else { return };
        let s = cx.place.session();
        let l = Listen::listening(cx);
        // One clock for the whole timeline: the sound's, which the
        // session's clocks and what plays alone are read against (a demo's
        // place has a fixed day of its own for its library).
        let v = View {
            me: &s.device,
            joined: Listen::joined(&s, &l),
            epoch: s.epoch,
            listening: &l,
            now: (self.clock)(),
        };
        self.core.reconcile(&v, sound, &mut Says { cx });
        self.own = match v.joined {
            true => None,
            false => own(&s.server, sound, (self.clock)()),
        };
    }

    /// A press: a live call while this device stands in the session, done
    /// to its own sound while it is alone. Why not, where it was neither.
    pub fn press(&mut self, p: Press, cx: &mut Cx<'_, Harken>) -> Option<String> {
        let s = cx.place.session();
        let l = Listen::listening(cx);
        if Listen::joined(&s, &l) {
            let local = self.sound.as_ref().map(|o| (o.tracks(), o.heard().at.unwrap_or(0), o.heard().from));
            return match p.transition(&l, &s.device, local) {
                Ok(Some((t, input))) => live(t, &input).map_or(Some(format!("no transition {t}")), |c| cx.place.send(c).err()),
                Ok(None) => None,
                Err(why) => Some(why.into()),
            };
        }
        match self.sound.as_mut().map(|o| o.alone(&p)) {
            Some(true) => None,
            Some(false) => Some(harken_domain::player::NOT_PART.into()),
            None => Some("nothing here plays".into()),
        }
    }

    /// A volume press: the output's group while this device stands in the
    /// session, its own sound alone.
    pub fn loud(&self, change: Change, cx: &Cx<'_, Harken>) -> Press {
        let s = cx.place.session();
        let level = match Listen::joined(&s, &Listen::listening(cx)) {
            true => Level::Group,
            false => Level::Here,
        };
        Press::Loud { level, change }
    }

    /// The tracks a screen named (each its id's hex), as the library has
    /// them; one it has not is left out.
    pub fn tracks(ids: &[String], cx: &Cx<'_, Harken>) -> Vec<Track> {
        let all: Vec<LibraryEntry> = cx
            .rows::<Vec<u8>>(&songs())
            .iter()
            .filter_map(|b| LibraryEntry::from_bytes(b).ok())
            .collect();
        ids.iter()
            .filter_map(|id| all.iter().find(|e| e.id.hex() == *id))
            .map(|e| Track {
                album: String::new(),
                creator: e.creator.clone(),
                duration_ms: e.duration_ms,
                file: e.file.clone(),
                id: e.id,
                title: e.title.clone(),
            })
            .collect()
    }
}

/// What this device's sound plays on its own, as the player draws it.
fn own(server: &str, sound: &MediaSound, now: i64) -> Option<Playing> {
    let h = sound.heard();
    let queue = sound.tracks();
    let at = h.at?;
    let t = queue.get(at)?;
    Some(Playing {
        key: t.id.hex(),
        title: t.title.clone(),
        under: t.creator.clone(),
        art: words::art_url(server, &t.file),
        seed: t.title.clone(),
        at_ms: h.place.ms().unwrap_or(0).max(0),
        since_ms: now,
        playing: h.playing,
        length_ms: t.duration_ms,
        can_back: true,
        can_forward: at + 1 < queue.len(),
        volume: sound.volume().map_or(100, |v| v.level),
        muted: sound.volume().is_some_and(|v| v.muted),
    })
}
