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
use ark_app::modules::media_player::{MediaControls, MediaPlayer, NowPlaying};
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
    /// Its queue, as the player lists it.
    pub queue: Vec<crate::player::QueueRow>,
    /// The songs' covers and albums, as the last step read them.
    covers: words::Covers,
    /// The clock the sound is timed by, which the player's timeline counts
    /// on from (the demo's day is not it).
    clock: ark_app::host::Clock,
}

/// How far an episode's skips move, as the system's controls offer them.
const JOG_BACK_S: u32 = 15;
const JOG_FORWARD_S: u32 = 30;

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
        let mut l = Listen {
            core: Reconciler::new(&session.device, Some(Loaded::Nothing)),
            sound: None,
            own: None,
            queue: vec![],
            covers: words::Covers::default(),
            clock: cx.place.clock(),
        };
        l.attach(&session.url, cx);
        l
    }

    /// The place's media player, once it has one: at the session's open,
    /// or (a phone whose screen registers its player after the session
    /// opened) the first step after.
    fn attach(&mut self, server: &str, cx: &mut Cx<'_, Harken>) {
        if self.sound.is_some() {
            return;
        }
        let Some(port) = cx.place.modules().get::<dyn MediaPlayer>() else {
            return;
        };
        let waker = cx.place.waker();
        let timed = cx.place.clock();
        // A step asked for later comes at the first position the player
        // says once it is due (`MediaSound`): the host keeps no timer.
        self.sound = Some(MediaSound::new(
            port,
            server,
            move |ms| {
                if ms == 0 {
                    waker()
                }
            },
            move || timed(),
        ));
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
        let server = cx.place.session().url;
        self.attach(&server, cx);
        let Some(sound) = self.sound.as_mut() else { return };
        let remote = sound.take_remote();
        let at = sound.heard().place.ms().unwrap_or(0);
        for p in remote.into_iter().filter_map(|c| remote_press(c, at)) {
            self.press(p, cx);
        }
        let Some(sound) = self.sound.as_mut() else { return };
        let s = cx.place.session();
        let l = Listen::listening(cx);
        let [details, albums] = &crate::words::Covers::VIEWS;
        let typed = |k: &HarkenView| cx.rows::<Vec<u8>>(k);
        let details: Vec<harken_domain::TrackDetailsEntry> = typed(details).iter().filter_map(|b| Encode::from_bytes(b).ok()).collect();
        let albums: Vec<harken_domain::AlbumsEntry> = typed(albums).iter().filter_map(|b| Encode::from_bytes(b).ok()).collect();
        let covers = crate::words::Covers::of(&s.url, &details, &albums);
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
        self.queue = match v.joined {
            true => vec![],
            false => crate::player::queue_rows(&covers, &sound.tracks(), sound.heard().at.map_or(-1, |a| a as i64)),
        };
        self.own = match v.joined {
            true => None,
            false => own(&covers, sound, (self.clock)()),
        };
        self.covers = covers;
    }

    /// What plays (`now`, as the player draws it, the session's or this
    /// device's own; none for nothing), shown to the system — the lock
    /// screen, the notification, the browser's and the desktop's media
    /// controls, a headset, a watch — with the controls it offers: a
    /// track's next and previous, an episode's skips of seconds, the
    /// scrubber where it has a length. Told only when it changed (`MediaSound::show`).
    pub fn show(&self, now: Option<&Playing>, cx: &Cx<'_, Harken>) {
        let Some(sound) = self.sound.as_ref() else { return };
        let Some(p) = now else {
            return sound.show(None, MediaControls::default());
        };
        let episode = cx
            .rows::<Vec<u8>>(&songs())
            .iter()
            .filter_map(|b| LibraryEntry::from_bytes(b).ok())
            .any(|e| e.id.hex() == p.key && e.kind == "episode");
        let album = self.covers.album(&p.key);
        let at_ms = match p.playing {
            true => p.at_ms + ((self.clock)() - p.since_ms).max(0),
            false => p.at_ms,
        };
        let info = NowPlaying {
            title: p.title.clone(),
            artist: p.under.clone(),
            // With no cover, or none to be had, the derived square the
            // screens draw for it.
            placeholder: Some(if album.is_empty() { p.title.clone() } else { album.clone() }),
            album,
            artwork: (!p.art.is_empty()).then(|| p.art.clone()),
            duration_ms: p.length_ms,
            position_ms: match p.length_ms > 0 {
                true => at_ms.clamp(0, p.length_ms),
                false => at_ms.max(0),
            },
            rate: if p.playing { 1.0 } else { 0.0 },
        };
        let controls = MediaControls {
            has_next: p.can_forward,
            has_previous: p.can_back,
            skip_forward_s: if episode { JOG_FORWARD_S } else { 0 },
            skip_back_s: if episode { JOG_BACK_S } else { 0 },
            can_seek: p.length_ms > 0,
            can_rate: false,
            can_add: true,
        };
        sound.show(Some(info), controls);
    }

    /// What `old` held of this device carried into this one, made anew
    /// over the same place (the browser's back and forward,
    /// `HostedApp::keep`): its sound and the queue it plays, its
    /// reconciler with them, so what plays plays on with its controls.
    /// The sound this one made in their place is let go first, so the
    /// player is heard by the one kept.
    pub fn keep(&mut self, old: &mut Listen) {
        std::mem::swap(self, old);
        old.sound = None;
        if let Some(s) = &self.sound {
            s.listen_again();
        }
    }

    /// A press: a live call while this device stands in the session, done
    /// to its own sound while it is alone. Why not, where it was neither.
    pub fn press(&mut self, p: Press, cx: &mut Cx<'_, Harken>) -> Option<String> {
        let s = cx.place.session();
        self.attach(&s.url, cx);
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

    /// What a car or a watch asked since the last step.
    pub fn asks(&self) -> Vec<ark_app::modules::media_player::MediaEvent> {
        self.sound.as_ref().map(|o| o.take_asks()).unwrap_or_default()
    }

    /// A branch of the browse tree answered.
    pub fn children(&self, parent: String, items: Vec<ark_app::modules::media_player::BrowseItem>) {
        if let Some(o) = &self.sound {
            o.port().set_children(parent, items)
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
fn own(covers: &words::Covers, sound: &MediaSound, now: i64) -> Option<Playing> {
    let h = sound.heard();
    let queue = sound.tracks();
    let at = h.at?;
    let t = queue.get(at)?;
    Some(Playing {
        key: t.id.hex(),
        title: t.title.clone(),
        under: t.creator.clone(),
        art: covers.track(&t.id.hex()),
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
