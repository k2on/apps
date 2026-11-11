//! The Linux desktop's player (feature `desktop`): the sound decoded by
//! symphonia and played on the sound card by rodio (cpal, so ALSA), on a
//! thread of its own; a stream (`http:`, `https:`) read by range requests,
//! so a seek is a seek; and MPRIS (`org.mpris.MediaPlayer2`) on the session
//! bus, which the desktop's media keys, its now-playing widgets and
//! `playerctl` reach: each of their asks a [`MediaEvent::Remote`].
//!
//! With no sound card the player says so (`Failed`) and goes on silent;
//! with no session bus there is no MPRIS, and nothing else changes. A
//! desktop browses no tree: [`MediaPlayer::set_children`] is kept nowhere.

use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::{BrowseItem, Controls, MediaEvent, MediaListener, MediaPlayer, NowPlaying, RemoteCommand};

/// How often a playing sound says where it is.
const TICK: Duration = Duration::from_millis(250);

enum Cmd {
    Load(String, i64),
    Play,
    Pause,
    Seek(i64),
    Rate(f32),
    Volume(f32),
    Stop,
}

/// What MPRIS reads, and where events go.
#[derive(Default)]
struct State {
    listener: Option<Arc<dyn MediaListener>>,
    info: Option<NowPlaying>,
    controls: Controls,
    /// "Playing", "Paused" or "Stopped", as MPRIS says it.
    status: &'static str,
    position_ms: i64,
    duration_ms: i64,
    rate: f32,
    volume: f32,
    loaded: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    bus: Mutex<Option<zbus::blocking::Connection>>,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn tell(&self, e: MediaEvent) {
        let l = self.state().listener.clone();
        if let Some(l) = l {
            l.heard(e)
        }
    }
}

/// The desktop's player, and its MPRIS name.
pub struct Desktop {
    to: Mutex<Sender<Cmd>>,
    shared: Arc<Shared>,
}

impl Desktop {
    /// The player, on the bus as `org.mpris.MediaPlayer2.<name>` (with
    /// this process's id, so two windows are two players), `identity` what
    /// a widget calls it ("Harken") and `desktop_entry` its `.desktop`
    /// file's name.
    pub fn new(name: &str, identity: &str, desktop_entry: &str) -> Desktop {
        let shared = Arc::new(Shared::default());
        {
            let mut s = shared.state();
            s.status = "Stopped";
            s.rate = 1.0;
            s.volume = 1.0;
        }
        let (to, from) = mpsc::channel();
        let sh = shared.clone();
        let _ = std::thread::Builder::new().name("ark-media-player".into()).spawn(move || sound(from, sh));
        let bus = mpris::serve(name, identity, desktop_entry, shared.clone());
        *shared.bus.lock().unwrap_or_else(|e| e.into_inner()) = bus;
        Desktop { to: Mutex::new(to), shared }
    }

    fn send(&self, c: Cmd) {
        let _ = self.to.lock().unwrap_or_else(|e| e.into_inner()).send(c);
    }
}

impl MediaPlayer for Desktop {
    fn load(&self, url: String, at_ms: i64) {
        self.send(Cmd::Load(url, at_ms))
    }
    fn play(&self) {
        self.send(Cmd::Play)
    }
    fn pause(&self) {
        self.send(Cmd::Pause)
    }
    fn seek(&self, at_ms: i64) {
        self.send(Cmd::Seek(at_ms))
    }
    fn set_rate(&self, rate: f32) {
        self.send(Cmd::Rate(rate))
    }
    fn set_volume(&self, volume: f32) {
        self.send(Cmd::Volume(volume))
    }
    fn stop(&self) {
        self.send(Cmd::Stop)
    }
    fn set_now_playing(&self, info: Option<NowPlaying>) {
        self.shared.state().info = info;
        mpris::changed(&self.shared, &["Metadata", "PlaybackStatus", "Rate"]);
    }
    fn set_controls(&self, controls: Controls) {
        self.shared.state().controls = controls;
        mpris::changed(&self.shared, &["CanGoNext", "CanGoPrevious", "CanSeek"]);
    }
    fn set_children(&self, _parent: String, _items: Vec<BrowseItem>) {}
    fn set_listener(&self, listener: Option<Arc<dyn MediaListener>>) {
        self.shared.state().listener = listener
    }
}

// * The sound, on its own thread

/// What plays: the sound card, and the sink of the sound loaded.
struct Out {
    // Kept for as long as anything plays: dropping it stops the sound card.
    _stream: rodio::OutputStream,
    handle: rodio::OutputStreamHandle,
}

fn sound(from: Receiver<Cmd>, shared: Arc<Shared>) {
    let mut out: Option<Out> = None;
    let mut sink: Option<rodio::Sink> = None;
    let mut playing = false;
    let (mut rate, mut volume) = (1.0f32, 1.0f32);
    let status = |s: &'static str, shared: &Shared| {
        shared.state().status = s;
        mpris::changed(shared, &["PlaybackStatus"]);
    };
    loop {
        let cmd = match from.recv_timeout(TICK) {
            Ok(c) => Some(c),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        match cmd {
            None => {
                let Some(s) = &sink else { continue };
                if !playing {
                    continue;
                }
                if s.empty() {
                    playing = false;
                    sink = None;
                    shared.state().loaded = false;
                    status("Stopped", &shared);
                    shared.tell(MediaEvent::Ended);
                } else {
                    let at_ms = s.get_pos().as_millis() as i64;
                    shared.state().position_ms = at_ms;
                    shared.tell(MediaEvent::Position { at_ms });
                }
            }
            Some(Cmd::Load(url, at_ms)) => {
                if let Some(s) = sink.take() {
                    s.stop()
                }
                playing = false;
                if out.is_none() {
                    out = rodio::OutputStream::try_default().ok().map(|(s, h)| Out { _stream: s, handle: h });
                }
                let Some(o) = &out else {
                    shared.tell(MediaEvent::Failed {
                        why: "this computer has no sound card to play on".into(),
                    });
                    continue;
                };
                match open(&url).and_then(|src| {
                    let s = rodio::Sink::try_new(&o.handle).map_err(|e| e.to_string())?;
                    Ok((s, src))
                }) {
                    Ok((s, src)) => {
                        use rodio::Source;
                        let duration_ms = src.total_duration().map_or(0, |d| d.as_millis() as i64);
                        s.pause();
                        s.set_speed(rate);
                        s.set_volume(volume);
                        s.append(src);
                        if at_ms > 0 {
                            let _ = s.try_seek(Duration::from_millis(at_ms as u64));
                        }
                        sink = Some(s);
                        {
                            let mut st = shared.state();
                            st.loaded = true;
                            st.duration_ms = duration_ms;
                            st.position_ms = at_ms.max(0);
                        }
                        status("Paused", &shared);
                        shared.tell(MediaEvent::Loaded { duration_ms });
                    }
                    Err(why) => shared.tell(MediaEvent::Failed { why }),
                }
            }
            Some(Cmd::Play) => {
                if let Some(s) = &sink {
                    s.play();
                    playing = true;
                    status("Playing", &shared);
                    shared.tell(MediaEvent::Playing);
                }
            }
            Some(Cmd::Pause) => {
                if let Some(s) = &sink {
                    s.pause();
                    playing = false;
                    shared.state().position_ms = s.get_pos().as_millis() as i64;
                    status("Paused", &shared);
                    shared.tell(MediaEvent::Paused);
                }
            }
            Some(Cmd::Seek(at_ms)) => {
                if let Some(s) = &sink {
                    match s.try_seek(Duration::from_millis(at_ms.max(0) as u64)) {
                        Ok(()) => {
                            let at_ms = s.get_pos().as_millis() as i64;
                            shared.state().position_ms = at_ms;
                            mpris::seeked(&shared, at_ms);
                            shared.tell(MediaEvent::Position { at_ms });
                        }
                        Err(e) => shared.tell(MediaEvent::Failed { why: e.to_string() }),
                    }
                }
            }
            Some(Cmd::Rate(r)) => {
                rate = r;
                shared.state().rate = r;
                if let Some(s) = &sink {
                    s.set_speed(r)
                }
                mpris::changed(&shared, &["Rate"]);
            }
            Some(Cmd::Volume(v)) => {
                volume = v.clamp(0.0, 1.0);
                shared.state().volume = volume;
                if let Some(s) = &sink {
                    s.set_volume(volume)
                }
                mpris::changed(&shared, &["Volume"]);
            }
            Some(Cmd::Stop) => {
                if let Some(s) = sink.take() {
                    s.stop()
                }
                playing = false;
                // The sound card let go, as a phone lets its audio session go.
                out = None;
                shared.state().loaded = false;
                status("Stopped", &shared);
            }
        }
    }
}

/// The sound at `url`, decoded: a stream by range requests, or a file.
fn open(url: &str) -> Result<rodio::Decoder<Box<dyn ReadSeek>>, String> {
    let r: Box<dyn ReadSeek> = if url.starts_with("http://") || url.starts_with("https://") {
        Box::new(Ranged::open(url)?)
    } else {
        let path = url.strip_prefix("file://").unwrap_or(url);
        Box::new(std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?)
    };
    rodio::Decoder::new(r).map_err(|e| e.to_string())
}

/// What a decoder reads.
pub trait ReadSeek: Read + Seek + Send + Sync {}
impl<T: Read + Seek + Send + Sync> ReadSeek for T {}

/// A stream read by range requests: one open response read on, and a new
/// one from wherever a seek lands.
struct Ranged {
    url: String,
    agent: ureq::Agent,
    len: u64,
    at: u64,
    body: Option<Box<dyn Read + Send + Sync>>,
}

impl Ranged {
    fn open(url: &str) -> Result<Ranged, String> {
        let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).build();
        let mut r = Ranged {
            url: url.into(),
            agent,
            len: 0,
            at: 0,
            body: None,
        };
        r.fetch(0).map_err(|e| format!("{url}: {e}"))?;
        Ok(r)
    }

    /// The body from `at` on; and the whole length, from the first answer.
    fn fetch(&mut self, at: u64) -> io::Result<()> {
        let res = self
            .agent
            .get(&self.url)
            .set("Range", &format!("bytes={at}-"))
            .call()
            .map_err(|e| io::Error::other(e.to_string()))?;
        let total = res
            .header("Content-Range")
            .and_then(|c| c.rsplit('/').next())
            .and_then(|n| n.parse::<u64>().ok())
            .or_else(|| res.header("Content-Length").and_then(|n| n.parse::<u64>().ok()).map(|n| n + at));
        if let Some(t) = total {
            self.len = t;
        }
        // A server that ignored the range sent it all: read up to `at`.
        let mut body: Box<dyn Read + Send + Sync> = Box::new(res.into_reader());
        if at > 0 && res_ignored_range(total, self.len, at) {
            io::copy(&mut (&mut body).take(at), &mut io::sink())?;
        }
        self.body = Some(body);
        self.at = at;
        Ok(())
    }
}

/// Whether an answer to a range from `at` was the whole thing.
fn res_ignored_range(total: Option<u64>, len: u64, at: u64) -> bool {
    total.is_none() && len == 0 && at > 0
}

impl Read for Ranged {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.body.is_none() {
            self.fetch(self.at)?;
        }
        let n = self.body.as_mut().map_or(Ok(0), |b| b.read(buf))?;
        self.at += n as u64;
        Ok(n)
    }
}

impl Seek for Ranged {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let at = match to {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::Current(d) => self.at as i64 + d,
            SeekFrom::End(d) => self.len as i64 + d,
        };
        let at = at.max(0) as u64;
        if at != self.at {
            // Read lazily: symphonia seeks to the end and back to learn the length.
            self.body = None;
            self.at = at;
        }
        Ok(at)
    }
}

// * MPRIS

mod mpris {
    use super::*;
    use zbus::zvariant::{ObjectPath, OwnedValue, Value};

    const PATH: &str = "/org/mpris/MediaPlayer2";
    const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

    /// The bus's two interfaces at the player's path; none without a session bus.
    pub(super) fn serve(name: &str, identity: &str, desktop_entry: &str, shared: Arc<Shared>) -> Option<zbus::blocking::Connection> {
        let well_known = format!("org.mpris.MediaPlayer2.{name}.instance{}", std::process::id());
        zbus::blocking::connection::Builder::session()
            .ok()?
            .name(well_known)
            .ok()?
            .serve_at(
                PATH,
                Root {
                    identity: identity.into(),
                    desktop_entry: desktop_entry.into(),
                },
            )
            .ok()?
            .serve_at(PATH, Player { shared })
            .ok()?
            .build()
            .ok()
    }

    /// `props` of the player changed: said to whoever watches.
    pub(super) fn changed(shared: &Shared, props: &[&str]) {
        let Some(bus) = shared.bus.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
            return;
        };
        let p = Player::read(shared);
        let mut out: HashMap<String, OwnedValue> = HashMap::new();
        for k in props {
            let v = match *k {
                "PlaybackStatus" => owned(Value::from(p.status)),
                "Metadata" => owned(Value::from(p.metadata.clone())),
                "Rate" => owned(Value::from(p.rate)),
                "Volume" => owned(Value::from(p.volume)),
                "CanGoNext" => owned(Value::from(p.controls.has_next)),
                "CanGoPrevious" => owned(Value::from(p.controls.has_previous)),
                "CanSeek" => owned(Value::from(p.controls.can_seek)),
                _ => None,
            };
            if let Some(v) = v {
                out.insert((*k).into(), v);
            }
        }
        let _ = bus.emit_signal(
            None::<()>,
            PATH,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            &(PLAYER, out, Vec::<String>::new()),
        );
    }

    /// The position moved by more than playing moves it.
    pub(super) fn seeked(shared: &Shared, at_ms: i64) {
        let Some(bus) = shared.bus.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
            return;
        };
        let _ = bus.emit_signal(None::<()>, PATH, PLAYER, "Seeked", &(at_ms * 1000));
    }

    fn owned(v: Value<'_>) -> Option<OwnedValue> {
        v.try_to_owned().ok()
    }

    struct Root {
        identity: String,
        desktop_entry: String,
    }

    #[zbus::interface(name = "org.mpris.MediaPlayer2")]
    impl Root {
        fn raise(&self) {}
        fn quit(&self) {}
        #[zbus(property)]
        fn can_quit(&self) -> bool {
            false
        }
        #[zbus(property)]
        fn can_raise(&self) -> bool {
            false
        }
        #[zbus(property)]
        fn has_track_list(&self) -> bool {
            false
        }
        #[zbus(property)]
        fn identity(&self) -> String {
            self.identity.clone()
        }
        #[zbus(property)]
        fn desktop_entry(&self) -> String {
            self.desktop_entry.clone()
        }
        #[zbus(property)]
        fn supported_uri_schemes(&self) -> Vec<String> {
            vec![]
        }
        #[zbus(property)]
        fn supported_mime_types(&self) -> Vec<String> {
            vec![]
        }
    }

    struct Player {
        shared: Arc<Shared>,
    }

    /// The player's properties at one moment.
    struct Read {
        status: &'static str,
        metadata: HashMap<String, Value<'static>>,
        rate: f64,
        volume: f64,
        position_us: i64,
        controls: Controls,
        loaded: bool,
    }

    impl Player {
        fn read(shared: &Shared) -> Read {
            let s = shared.state();
            let mut metadata = HashMap::new();
            if let Some(i) = &s.info {
                if let Ok(id) = ObjectPath::try_from("/org/ark/media/now") {
                    metadata.insert("mpris:trackid".into(), Value::from(id.into_owned()));
                }
                metadata.insert("xesam:title".into(), Value::from(i.title.clone()));
                metadata.insert("xesam:artist".into(), Value::from(vec![i.artist.clone()]));
                metadata.insert("xesam:album".into(), Value::from(i.album.clone()));
                let d = if s.loaded && s.duration_ms > 0 { s.duration_ms } else { i.duration_ms };
                metadata.insert("mpris:length".into(), Value::from(d * 1000));
                if let Some(a) = &i.artwork {
                    metadata.insert("mpris:artUrl".into(), Value::from(a.clone()));
                }
            }
            // A sound made elsewhere is shown as the app says it.
            let (status, position_ms, rate) = match (&s.info, s.loaded) {
                (Some(i), false) => (if i.rate > 0.0 { "Playing" } else { "Paused" }, i.position_ms, i.rate.max(1.0)),
                (None, false) => ("Stopped", 0, s.rate),
                _ => (s.status, s.position_ms, s.rate),
            };
            Read {
                status,
                metadata,
                rate: rate as f64,
                volume: s.volume as f64,
                position_us: position_ms * 1000,
                controls: s.controls,
                loaded: s.loaded,
            }
        }

        fn ask(&self, command: RemoteCommand) {
            self.shared.tell(MediaEvent::Remote { command })
        }
    }

    #[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
    impl Player {
        fn next(&self) {
            self.ask(RemoteCommand::Next)
        }
        fn previous(&self) {
            self.ask(RemoteCommand::Previous)
        }
        fn pause(&self) {
            self.ask(RemoteCommand::Pause)
        }
        fn play_pause(&self) {
            self.ask(RemoteCommand::Toggle)
        }
        fn stop(&self) {
            self.ask(RemoteCommand::Stop)
        }
        fn play(&self) {
            self.ask(RemoteCommand::Play)
        }
        /// By `offset` microseconds, forward or back.
        fn seek(&self, offset: i64) {
            let secs = (offset.unsigned_abs() / 1_000_000) as u32;
            match offset >= 0 {
                true => self.ask(RemoteCommand::SkipForward { seconds: secs }),
                false => self.ask(RemoteCommand::SkipBack { seconds: secs }),
            }
        }
        fn set_position(&self, _track: ObjectPath<'_>, position: i64) {
            self.ask(RemoteCommand::SeekTo { at_ms: position / 1000 })
        }
        fn open_uri(&self, _uri: String) {}

        #[zbus(property)]
        fn playback_status(&self) -> String {
            Player::read(&self.shared).status.into()
        }
        #[zbus(property)]
        fn rate(&self) -> f64 {
            Player::read(&self.shared).rate
        }
        #[zbus(property)]
        fn set_rate(&self, rate: f64) {
            self.ask(RemoteCommand::Rate { rate: rate as f32 })
        }
        #[zbus(property)]
        fn metadata(&self) -> HashMap<String, OwnedValue> {
            Player::read(&self.shared)
                .metadata
                .into_iter()
                .filter_map(|(k, v)| owned(v).map(|v| (k, v)))
                .collect()
        }
        #[zbus(property)]
        fn volume(&self) -> f64 {
            Player::read(&self.shared).volume
        }
        #[zbus(property)]
        fn position(&self) -> i64 {
            Player::read(&self.shared).position_us
        }
        #[zbus(property)]
        fn minimum_rate(&self) -> f64 {
            0.5
        }
        #[zbus(property)]
        fn maximum_rate(&self) -> f64 {
            if Player::read(&self.shared).controls.can_rate {
                3.0
            } else {
                1.0
            }
        }
        #[zbus(property)]
        fn can_go_next(&self) -> bool {
            Player::read(&self.shared).controls.has_next
        }
        #[zbus(property)]
        fn can_go_previous(&self) -> bool {
            Player::read(&self.shared).controls.has_previous
        }
        #[zbus(property)]
        fn can_play(&self) -> bool {
            let r = Player::read(&self.shared);
            r.loaded || !r.metadata.is_empty()
        }
        #[zbus(property)]
        fn can_pause(&self) -> bool {
            self.can_play()
        }
        #[zbus(property)]
        fn can_seek(&self) -> bool {
            Player::read(&self.shared).controls.can_seek
        }
        #[zbus(property)]
        fn can_control(&self) -> bool {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Log(Mutex<Vec<MediaEvent>>);
    impl MediaListener for Log {
        fn heard(&self, e: MediaEvent) {
            self.0.lock().unwrap().push(e)
        }
    }

    #[test]
    fn a_file_that_is_not_there_fails_and_says_so() {
        let d = Desktop::new("test", "Test", "test");
        let log = Arc::new(Log(Mutex::new(vec![])));
        d.set_listener(Some(log.clone()));
        d.load("file:///nowhere/at/all.mp3".into(), 0);
        let mut heard = vec![];
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(50));
            heard = log.0.lock().unwrap().clone();
            if !heard.is_empty() {
                break;
            }
        }
        // No sound card here (a container, CI) and no file are both a failure, said.
        assert!(matches!(heard.as_slice(), [MediaEvent::Failed { .. }]), "{heard:?}");
    }
}
