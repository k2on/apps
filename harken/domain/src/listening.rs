//! One account, one thing playing, however many devices are watching it.
//!
//! A person with a phone, a laptop and a browser tab has **one** listening
//! session, not three. Any of them can say what should happen; exactly one
//! of them is making the sound; and moving the sound from one to another is
//! a sentence in this protocol rather than a track started again somewhere
//! else.
//!
//! **None of this is in the log, and none of it ever will be.** The log is
//! permanent and totally ordered: every peer replays every entry, forever.
//! An afternoon of listening is thousands of pauses, seeks and skips, and
//! not one of them is worth replaying tomorrow. So it lives in a live room
//! (`ark::live`), in the server's memory, and the domain's procedures are
//! still the only `apply`.
//!
//! **It rides the sync socket.** A second socket is a second thing to
//! authenticate, reconnect and keep alive, and a second answer to "am I
//! online" that disagrees with the first at the worst moment.
//!
//! **It is here, in the domain crate, because that is the only vocabulary
//! the server and the clients already share.** It is not the domain:
//! nothing below writes a row, and it is plain Rust, not the vocabulary.
//!
//! **Canonical CBOR, the engine's own encoding** ([`ark::canon`]): every
//! frame is a [`Value`] — a struct with a `"t"` tag for each variant, as the
//! IR's own nodes are — so one frame has one byte string everywhere, and
//! the decoder refuses anything that is not canonical.

use std::collections::BTreeMap;

use ark::value::Value;

/// One device, as everything here names it: whatever names one output
/// stably. For a client it is the login's session id, which is "one login
/// on one device"; for a speaker in the kitchen it is the entity that names
/// it (`media_player.kitchen`), which is still the same speaker tomorrow. It
/// is a live room's `who`, so no device ever says its own id.
pub type DeviceId = String;

/// A frame that does not decode: what was wrong, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Malformed(pub String);

impl std::fmt::Display for Malformed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "a listening frame that does not decode: {}", self.0)
    }
}

impl std::error::Error for Malformed {}

type Decoded<T> = Result<T, Malformed>;

fn bad<T>(what: impl Into<String>) -> Decoded<T> {
    Err(Malformed(what.into()))
}

fn record(tag: Option<&str>, fields: Vec<(&str, Value)>) -> Value {
    let mut m: BTreeMap<String, Value> = fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    if let Some(t) = tag {
        m.insert("t".into(), Value::text(t));
    }
    Value::from(m)
}

fn fields(v: &Value) -> Decoded<&BTreeMap<String, Value>> {
    match v {
        Value::Struct(m) => Ok(m),
        other => bad(format!("expected a struct, got {other:?}")),
    }
}

fn get<'a>(m: &'a BTreeMap<String, Value>, k: &str) -> Decoded<&'a Value> {
    m.get(k).map_or_else(|| bad(format!("no field {k:?}")), Ok)
}

fn text(m: &BTreeMap<String, Value>, k: &str) -> Decoded<String> {
    match get(m, k)? {
        Value::Text(t) => Ok(t.to_string()),
        other => bad(format!("{k}: expected text, got {other:?}")),
    }
}

fn int(m: &BTreeMap<String, Value>, k: &str) -> Decoded<i64> {
    match get(m, k)? {
        Value::Int(n) => Ok(*n),
        other => bad(format!("{k}: expected an int, got {other:?}")),
    }
}

fn index(m: &BTreeMap<String, Value>, k: &str) -> Decoded<u32> {
    u32::try_from(int(m, k)?).or_else(|_| bad(format!("{k}: not an index")))
}

fn boolean(m: &BTreeMap<String, Value>, k: &str) -> Decoded<bool> {
    match get(m, k)? {
        Value::Bool(b) => Ok(*b),
        other => bad(format!("{k}: expected a bool, got {other:?}")),
    }
}

fn opt_text(m: &BTreeMap<String, Value>, k: &str) -> Decoded<Option<String>> {
    match get(m, k)? {
        Value::Null => Ok(None),
        Value::Text(t) => Ok(Some(t.to_string())),
        other => bad(format!("{k}: expected text or null, got {other:?}")),
    }
}

fn opt(v: &Option<String>) -> Value {
    v.as_ref().map_or(Value::Null, |t| Value::text(t.clone()))
}

fn items<T>(m: &BTreeMap<String, Value>, k: &str, each: fn(&Value) -> Decoded<T>) -> Decoded<Vec<T>> {
    match get(m, k)? {
        Value::List(xs) => xs.iter().map(each).collect(),
        other => bad(format!("{k}: expected a list, got {other:?}")),
    }
}

fn tag(m: &BTreeMap<String, Value>) -> Decoded<String> {
    text(m, "t")
}

/// Encode a frame as canonical CBOR: what a live room carries.
pub fn encode(v: &Value) -> Vec<u8> {
    ark::canon::encode(v)
}

/// Decode canonical CBOR into a value; a frame's own `from_value` reads it.
pub fn decode(bytes: &[u8]) -> Decoded<Value> {
    ark::canon::decode(bytes).map_err(|e| Malformed(format!("{e:?}")))
}

/// A track, carried rather than looked up: everything needed to play it,
/// so a device handed the session need not find it in its own replica
/// first. `file` travels for the same reason: the receiving device joins
/// it to *its* server, the way [`url`] does. `id` is the media id's text,
/// kept only so a client can find the row if it happens to have one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub creator: String,
    pub album: String,
    pub duration_ms: i64,
    pub file: String,
}

impl Track {
    pub fn to_value(&self) -> Value {
        record(
            None,
            vec![
                ("id", Value::text(self.id.clone())),
                ("title", Value::text(self.title.clone())),
                ("creator", Value::text(self.creator.clone())),
                ("album", Value::text(self.album.clone())),
                ("duration_ms", Value::int(self.duration_ms)),
                ("file", Value::text(self.file.clone())),
            ],
        )
    }

    pub fn from_value(v: &Value) -> Decoded<Track> {
        let m = fields(v)?;
        Ok(Track {
            id: text(m, "id")?,
            title: text(m, "title")?,
            creator: text(m, "creator")?,
            album: text(m, "album")?,
            duration_ms: int(m, "duration_ms")?,
            file: text(m, "file")?,
        })
    }
}

/// What sort of thing a device is, for the one glyph a picker draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Computer,
    Phone,
    /// Something in the house, reached through Home Assistant.
    Speaker,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Computer => "computer",
            Kind::Phone => "phone",
            Kind::Speaker => "speaker",
        }
    }

    /// A kind this build has never heard of reads as a computer, so an
    /// older client still lists a newer device.
    pub fn parse(s: &str) -> Kind {
        match s {
            "phone" => Kind::Phone,
            "speaker" => Kind::Speaker,
            _ => Kind::Computer,
        }
    }
}

/// Somewhere this account listens. Every device the session knows about,
/// which is not every device that is connected: the one the music belongs
/// to stays in the list after its socket goes (`here: false`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: DeviceId,
    /// What the picker draws. The client says it; nothing here interprets it.
    pub name: String,
    /// Whether it can make a sound at all: a device with none is a remote
    /// control and never the output.
    pub audible: bool,
    /// Whether its socket is open right now.
    pub here: bool,
    pub kind: Kind,
}

impl Device {
    pub fn to_value(&self) -> Value {
        record(
            None,
            vec![
                ("id", Value::text(self.id.clone())),
                ("name", Value::text(self.name.clone())),
                ("audible", Value::bool(self.audible)),
                ("here", Value::bool(self.here)),
                ("kind", Value::text(self.kind.name())),
            ],
        )
    }

    pub fn from_value(v: &Value) -> Decoded<Device> {
        let m = fields(v)?;
        Ok(Device {
            id: text(m, "id")?,
            name: text(m, "name")?,
            audible: boolean(m, "audible")?,
            here: boolean(m, "here")?,
            kind: Kind::parse(&text(m, "kind")?),
        })
    }
}

/// The whole session. Small enough to send entire on every change, so a
/// client that receives the whole truth cannot drift from it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Session {
    /// Which device the sound belongs to. `None` only when nothing has
    /// played yet, or somebody stopped it everywhere. It survives that
    /// device's socket going: a laptop lid closing is not a decision to
    /// move the music.
    pub output: Option<DeviceId>,
    /// A hand-off in flight: the sound has been given to this device and it
    /// has not reported since.
    pub moving: Option<DeviceId>,
    pub devices: Vec<Device>,
    /// What is playing and what comes after it: the snapshot taken when play
    /// was pressed.
    pub queue: Vec<Track>,
    pub at: u32,
    pub playing: bool,
    /// How far into [`Session::now`] the output last said it was. No
    /// timestamp beside it: clocks do not agree, so a client counts from
    /// when *it* received this, and the output resends about once a second.
    pub position_ms: i64,
}

impl Session {
    /// What is playing, if anything.
    pub fn now(&self) -> Option<&Track> {
        self.queue.get(self.at as usize)
    }

    /// Whether `device` is the one the sound belongs to.
    pub fn outputs(&self, device: &str) -> bool {
        self.output.as_deref() == Some(device)
    }

    /// The device the sound belongs to, as the picker names it.
    pub fn output_device(&self) -> Option<&Device> {
        let id = self.output.as_deref()?;
        self.devices.iter().find(|d| d.id == id)
    }

    /// Whether the device the sound belongs to is actually there: the
    /// difference between sending a transport button and obeying it here.
    pub fn output_here(&self) -> bool {
        self.output_device().is_some_and(|d| d.here)
    }

    /// Whether `device` is waiting to take the sound.
    pub fn moving_to(&self, device: &str) -> bool {
        self.moving.as_deref() == Some(device)
    }

    pub fn device(&self, id: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn to_value(&self) -> Value {
        record(
            None,
            vec![
                ("output", opt(&self.output)),
                ("moving", opt(&self.moving)),
                ("devices", Value::list(self.devices.iter().map(Device::to_value).collect())),
                ("queue", Value::list(self.queue.iter().map(Track::to_value).collect())),
                ("at", Value::int(self.at.into())),
                ("playing", Value::bool(self.playing)),
                ("position_ms", Value::int(self.position_ms)),
            ],
        )
    }

    pub fn from_value(v: &Value) -> Decoded<Session> {
        let m = fields(v)?;
        Ok(Session {
            output: opt_text(m, "output")?,
            moving: opt_text(m, "moving")?,
            devices: items(m, "devices", Device::from_value)?,
            queue: items(m, "queue", Track::from_value)?,
            at: index(m, "at")?,
            playing: boolean(m, "playing")?,
            position_ms: int(m, "position_ms")?,
        })
    }
}

/// Something to be done, wherever the sound is coming from. Play and pause
/// are two verbs rather than one toggle: the device asking is not the
/// device doing, so "the other one of whatever you are" means nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Play,
    Pause,
    Next,
    Previous,
    Seek {
        position_ms: i64,
    },
    /// Take this session: this queue, from here, at this point, playing or
    /// not. Somebody pressing a track and the session moving to another
    /// device are the same sentence with different numbers in it.
    Start {
        queue: Vec<Track>,
        at: u32,
        position_ms: i64,
        playing: bool,
    },
}

impl Command {
    pub fn to_value(&self) -> Value {
        match self {
            Command::Play => record(Some("play"), vec![]),
            Command::Pause => record(Some("pause"), vec![]),
            Command::Next => record(Some("next"), vec![]),
            Command::Previous => record(Some("previous"), vec![]),
            Command::Seek { position_ms } => record(Some("seek"), vec![("position_ms", Value::int(*position_ms))]),
            Command::Start {
                queue,
                at,
                position_ms,
                playing,
            } => record(
                Some("start"),
                vec![
                    ("queue", Value::list(queue.iter().map(Track::to_value).collect())),
                    ("at", Value::int((*at).into())),
                    ("position_ms", Value::int(*position_ms)),
                    ("playing", Value::bool(*playing)),
                ],
            ),
        }
    }

    pub fn from_value(v: &Value) -> Decoded<Command> {
        let m = fields(v)?;
        Ok(match tag(m)?.as_str() {
            "play" => Command::Play,
            "pause" => Command::Pause,
            "next" => Command::Next,
            "previous" => Command::Previous,
            "seek" => Command::Seek {
                position_ms: int(m, "position_ms")?,
            },
            "start" => Command::Start {
                queue: items(m, "queue", Track::from_value)?,
                at: index(m, "at")?,
                position_ms: int(m, "position_ms")?,
                playing: boolean(m, "playing")?,
            },
            other => return bad(format!("no command {other:?}")),
        })
    }
}

/// What a device says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Say {
    /// The first frame: what to call this device and what it can do. No
    /// token and no id: the socket this rides on is already authenticated.
    Here { name: String, audible: bool, kind: Kind },
    /// I am the output, and this is what I am doing. Sent when something
    /// changes and about once a second while playing.
    Report {
        queue: Vec<Track>,
        at: u32,
        playing: bool,
        position_ms: i64,
    },
    /// Do this — here, or wherever the sound actually is.
    Do { command: Command },
    /// Move the sound to `to`, or stop it everywhere with `None`.
    Transfer { to: Option<DeviceId> },
}

impl Say {
    pub fn to_value(&self) -> Value {
        match self {
            Say::Here { name, audible, kind } => record(
                Some("here"),
                vec![
                    ("name", Value::text(name.clone())),
                    ("audible", Value::bool(*audible)),
                    ("kind", Value::text(kind.name())),
                ],
            ),
            Say::Report {
                queue,
                at,
                playing,
                position_ms,
            } => record(
                Some("report"),
                vec![
                    ("queue", Value::list(queue.iter().map(Track::to_value).collect())),
                    ("at", Value::int((*at).into())),
                    ("playing", Value::bool(*playing)),
                    ("position_ms", Value::int(*position_ms)),
                ],
            ),
            Say::Do { command } => record(Some("do"), vec![("command", command.to_value())]),
            Say::Transfer { to } => record(Some("transfer"), vec![("to", opt(to))]),
        }
    }

    pub fn from_value(v: &Value) -> Decoded<Say> {
        let m = fields(v)?;
        Ok(match tag(m)?.as_str() {
            "here" => Say::Here {
                name: text(m, "name")?,
                audible: boolean(m, "audible")?,
                kind: Kind::parse(&text(m, "kind")?),
            },
            "report" => Say::Report {
                queue: items(m, "queue", Track::from_value)?,
                at: index(m, "at")?,
                playing: boolean(m, "playing")?,
                position_ms: int(m, "position_ms")?,
            },
            "do" => Say::Do {
                command: Command::from_value(get(m, "command")?)?,
            },
            "transfer" => Say::Transfer { to: opt_text(m, "to")? },
            other => return bad(format!("nothing is said as {other:?}")),
        })
    }

    /// The frame's bytes.
    pub fn encode(&self) -> Vec<u8> {
        encode(&self.to_value())
    }

    pub fn decode(bytes: &[u8]) -> Decoded<Say> {
        Say::from_value(&decode(bytes)?)
    }
}

/// What the server says back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hear {
    /// The session, entire, to every device of this account.
    State { session: Session },
    /// You are the output. Do this.
    Do { command: Command },
}

impl Hear {
    pub fn to_value(&self) -> Value {
        match self {
            Hear::State { session } => record(Some("state"), vec![("session", session.to_value())]),
            Hear::Do { command } => record(Some("do"), vec![("command", command.to_value())]),
        }
    }

    pub fn from_value(v: &Value) -> Decoded<Hear> {
        let m = fields(v)?;
        Ok(match tag(m)?.as_str() {
            "state" => Hear::State {
                session: Session::from_value(get(m, "session")?)?,
            },
            "do" => Hear::Do {
                command: Command::from_value(get(m, "command")?)?,
            },
            other => return bad(format!("nothing is heard as {other:?}")),
        })
    }

    /// The frame's bytes.
    pub fn encode(&self) -> Vec<u8> {
        encode(&self.to_value())
    }

    pub fn decode(bytes: &[u8]) -> Decoded<Hear> {
        Hear::from_value(&decode(bytes)?)
    }
}

/// Where a [`Track::file`] is, from where a server is. The column is a
/// *path*, and every device joins it to its own server — which is what
/// makes a hand-off work between a phone and a speaker that only knows a
/// LAN address. An absolute URL is already an answer and passes through.
pub fn url(base: &str, file: &str) -> String {
    if file.is_empty() || file.starts_with("http://") || file.starts_with("https://") {
        return file.to_string();
    }
    let mut out = format!("{}/media", base.trim_end_matches('/'));
    for part in file.split('/') {
        out.push('/');
        encode_segment(part, &mut out);
    }
    out
}

/// Percent-encode one path segment: everything outside RFC 3986's
/// unreserved set, which covers spaces, `&`, the silently destructive `#`
/// and every non-ASCII byte.
fn encode_segment(part: &str, out: &mut String) {
    for b in part.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(title: &str) -> Track {
        Track {
            id: String::new(),
            title: title.into(),
            creator: "Bach".into(),
            album: String::new(),
            duration_ms: 1000,
            file: "music/a.mp3".into(),
        }
    }

    trait Bytes {
        fn to_value_bytes(&self) -> Vec<u8>;
    }
    impl Bytes for Value {
        fn to_value_bytes(&self) -> Vec<u8> {
            encode(self)
        }
    }

    fn device(id: &str, here: bool) -> Device {
        Device {
            id: id.into(),
            name: id.into(),
            audible: true,
            here,
            kind: Kind::Computer,
        }
    }

    /// Every frame survives the wire the engine carries it on, and its
    /// bytes are canonical: decoding and encoding again is the same bytes.
    #[test]
    fn the_frames_survive_canonical_cbor() {
        let says = [
            Say::Here {
                name: "Phone".into(),
                audible: true,
                kind: Kind::Phone,
            },
            Say::Report {
                queue: vec![track("Air"), track("Gigue")],
                at: 1,
                playing: true,
                position_ms: 12_345,
            },
            Say::Do {
                command: Command::Start {
                    queue: vec![track("Air")],
                    at: 0,
                    position_ms: 0,
                    playing: false,
                },
            },
            Say::Do { command: Command::Previous },
            Say::Transfer { to: None },
            Say::Transfer {
                to: Some("media_player.kitchen".into()),
            },
        ];
        for say in says {
            let bytes = say.encode();
            assert!(ark::canon::round_trip(&bytes));
            assert_eq!(Say::decode(&bytes).unwrap(), say);
        }
        let hears = [
            Hear::Do {
                command: Command::Seek { position_ms: 4200 },
            },
            Hear::State {
                session: Session {
                    output: Some("d".into()),
                    moving: Some("k".into()),
                    devices: vec![device("d", true), device("k", false)],
                    queue: vec![track("Air"), track("Gigue")],
                    at: 1,
                    playing: true,
                    position_ms: 12,
                },
            },
            Hear::State { session: Session::default() },
        ];
        for hear in hears {
            let bytes = hear.encode();
            assert!(ark::canon::round_trip(&bytes));
            assert_eq!(Hear::decode(&bytes).unwrap(), hear);
        }
        // A frame that is not one is said to be not one, not guessed at.
        assert!(Say::decode(&Hear::State { session: Session::default() }.encode()).is_err());
        assert!(Say::decode(&record(Some("do"), vec![("command", record(Some("rewind"), vec![]))]).to_value_bytes()).is_err());
        assert!(Say::decode(b"\xff").is_err());
        assert!(Hear::from_value(&record(Some("state"), vec![])).is_err());
    }

    /// A path becomes a URL against whichever server is asking, and a URL
    /// is already an answer.
    #[test]
    fn a_file_is_joined_to_the_server_that_is_asking() {
        assert_eq!(
            url("http://10.0.0.2:8787", "music/Bach/air.flac"),
            "http://10.0.0.2:8787/media/music/Bach/air.flac"
        );
        assert_eq!(
            url("https://harken.example.com/", "music/a.mp3"),
            "https://harken.example.com/media/music/a.mp3",
            "a trailing slash is not a second one"
        );
        assert_eq!(
            url("http://h", "music/Boléro & co/no #1.mp3"),
            "http://h/media/music/Bol%C3%A9ro%20%26%20co/no%20%231.mp3"
        );
        assert_eq!(
            url("http://h", "https://upload.wikimedia.org/x.mp3"),
            "https://upload.wikimedia.org/x.mp3"
        );
        assert_eq!(url("http://h", ""), "", "nothing to stream is not a URL");
    }

    /// The derived facts, and the one that decides whether a button is an
    /// instruction or a message.
    #[test]
    fn the_session_says_what_is_playing_and_whether_it_can_be_reached() {
        let mut session = Session {
            queue: vec![track("Air"), track("Gigue")],
            at: 1,
            ..Session::default()
        };
        assert_eq!(session.now().map(|t| t.title.as_str()), Some("Gigue"));
        assert!(!session.outputs("d"), "nobody has it yet");

        session.devices = vec![device("d", true)];
        session.output = Some("d".into());
        assert!(session.outputs("d"));
        assert!(session.output_here());

        // The device the sound belongs to has gone, and it still belongs to it.
        session.devices = vec![device("d", false)];
        assert!(session.outputs("d"), "still its sound");
        assert!(!session.output_here(), "and still nowhere to send a button");

        session.moving = Some("k".into());
        assert!(session.moving_to("k"));
        session.at = 9;
        assert!(session.now().is_none(), "past the end is nothing playing");
    }
}
