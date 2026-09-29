//! What the domain's queries answer, as plain Rust a screen can hold.
//!
//! A query answers an `ark::value::Value` — a list of structs — and a view
//! function wants names, not lookups. Each type here is one query's row, read
//! leniently: a field the domain has not got yet reads as empty rather than
//! as a panic in the middle of a frame. The library row itself is the
//! domain's own [`harken_domain::view::Item`].
use ark_client::{Id, Value};

/// A struct's field, or `Null` when it has none.
fn get(v: &Value, k: &str) -> Value {
    match v {
        Value::Struct(m) => m.get(k).cloned().unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

pub(crate) fn text(v: &Value, k: &str) -> String {
    match get(v, k) {
        Value::Text(t) => t,
        _ => String::new(),
    }
}

pub(crate) fn int(v: &Value, k: &str) -> i64 {
    match get(v, k) {
        Value::Int(n) => n,
        _ => 0,
    }
}

pub(crate) fn id(v: &Value, k: &str) -> Id {
    match get(v, k) {
        Value::Id(i) => i,
        _ => [0; 16],
    }
}

/// The rows of a list, each read by `f`; nothing when it is not a list.
pub fn list<T>(v: &Value, f: impl Fn(&Value) -> T) -> Vec<T> {
    match v {
        Value::List(xs) => xs.iter().map(f).collect(),
        _ => Vec::new(),
    }
}

/// A playlist of the caller's (`playlists`, `playlists_of`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    pub id: Id,
    pub name: String,
}

impl Playlist {
    pub fn from_value(v: &Value) -> Playlist {
        Playlist {
            id: id(v, "id"),
            name: text(v, "name"),
        }
    }
}

/// A record, as the albums page lists it (`albums`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Album {
    pub name: String,
    /// Whoever made its first track.
    pub creator: String,
    pub tracks: i64,
    /// `media.file`'s spelling; empty is the derived square.
    pub art: String,
}

impl Album {
    pub fn from_value(v: &Value) -> Album {
        Album {
            name: text(v, "name"),
            creator: text(v, "creator"),
            tracks: int(v, "tracks"),
            art: text(v, "art"),
        }
    }
}

/// Whoever made something, as the artists page lists them (`artists`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artist {
    pub name: String,
    pub tracks: i64,
    pub art: String,
}

impl Artist {
    pub fn from_value(v: &Value) -> Artist {
        Artist {
            name: text(v, "name"),
            tracks: int(v, "tracks"),
            art: text(v, "art"),
        }
    }
}

/// Somebody some work is by (`composers`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composer {
    pub name: String,
    pub sort_name: String,
    pub born: i64,
    pub died: i64,
    pub works: i64,
    pub tracks: i64,
    pub art: String,
}

impl Composer {
    pub fn from_value(v: &Value) -> Composer {
        Composer {
            name: text(v, "name"),
            sort_name: text(v, "sort_name"),
            born: int(v, "born"),
            died: int(v, "died"),
            works: int(v, "works"),
            tracks: int(v, "tracks"),
            art: text(v, "art"),
        }
    }
}

/// A composition (`works`, `work`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    /// The derived key, `johann-sebastian-bach/bwv-988`.
    pub id: String,
    pub title: String,
    pub catalogue: String,
    pub composer: String,
    pub form: String,
    pub period: String,
    pub recordings: i64,
    pub tracks: i64,
    pub art: String,
}

impl Work {
    pub fn from_value(v: &Value) -> Work {
        Work {
            id: text(v, "id"),
            title: text(v, "title"),
            catalogue: text(v, "catalogue"),
            composer: text(v, "composer"),
            form: text(v, "form"),
            period: text(v, "period"),
            recordings: int(v, "recordings"),
            tracks: int(v, "tracks"),
            art: text(v, "art"),
        }
    }
}

/// One performance of a work (`recordings`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recording {
    /// The derived key: the work's, `@`, and who played it.
    pub id: String,
    pub performers: String,
    pub recorded: i64,
    pub label: String,
    pub licence: String,
    pub tracks: i64,
    pub art: String,
}

impl Recording {
    pub fn from_value(v: &Value) -> Recording {
        Recording {
            id: text(v, "id"),
            performers: text(v, "performers"),
            recorded: int(v, "recorded"),
            label: text(v, "label"),
            licence: text(v, "licence"),
            tracks: int(v, "tracks"),
            art: text(v, "art"),
        }
    }
}

/// One person on a recording (`credits`). No page draws credits yet — the
/// recording's `performers` is what a header says — so only the tests read
/// them.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credit {
    pub name: String,
    pub role: String,
    pub instrument: String,
    pub pos: i64,
}

#[cfg(test)]
impl Credit {
    pub fn from_value(v: &Value) -> Credit {
        Credit {
            name: text(v, "name"),
            role: text(v, "role"),
            instrument: text(v, "instrument"),
            pos: int(v, "pos"),
        }
    }
}

/// What is true of a track as a *song* (`track_details`): joined to the
/// kind-neutral library row in memory, by `media_id`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrackDetail {
    pub media_id: Id,
    pub album: String,
    /// Where it sits on the release; 0 is "nobody said".
    pub track: i64,
    /// The division of the work it belongs to.
    pub part: String,
    pub catalogue: String,
    pub performer: String,
    pub licence: String,
    pub bpm: i64,
}

impl TrackDetail {
    pub fn from_value(v: &Value) -> TrackDetail {
        TrackDetail {
            media_id: id(v, "media_id"),
            album: text(v, "album"),
            track: int(v, "track"),
            part: text(v, "part"),
            catalogue: text(v, "catalogue"),
            performer: text(v, "performer"),
            licence: text(v, "licence"),
            bpm: int(v, "bpm"),
        }
    }
}

/// An id as the hyphenated lowercase text a queue carries between devices:
/// a [`harken_domain::listening::Track`]'s `id` crosses three languages, and
/// a sixteen-byte array is a Rust type.
pub fn id_text(id: &Id) -> String {
    let h = ark_client::ark::value::hex(id);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// …and back. A queue handed over by a device whose library does not overlap
/// this one's may carry anything: that is `None`, which matches no row and
/// highlights nothing, and the track still plays from the file it brought.
pub fn parse_id(s: &str) -> Option<Id> {
    let hex: Vec<u8> = s.bytes().filter(|b| *b != b'-').collect();
    if hex.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, pair) in hex.chunks(2).enumerate() {
        let s = std::str::from_utf8(pair).ok()?;
        out[i] = u8::from_str_radix(s, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An id survives the trip through a queue's text, and text that is not
    /// one is nothing rather than a zero id that might match a row.
    ///
    /// Falsified by dropping the hyphens from `id_text`: the length check in
    /// the literal comparison fails.
    #[test]
    fn an_id_survives_a_queue() {
        let id: Id = [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(id_text(&id), "12345678-9abc-def0-0102-030405060708");
        assert_eq!(parse_id(&id_text(&id)), Some(id));
        assert_eq!(parse_id("not an id"), None);
        assert_eq!(parse_id(""), None);
    }

    /// A row with a field missing reads as empty rather than panicking in a
    /// frame. Falsified by reading with `Value::field`: it panics.
    #[test]
    fn a_missing_field_is_empty() {
        let v = Value::Struct([("name".to_string(), Value::text("Water Music"))].into_iter().collect());
        let a = Album::from_value(&v);
        assert_eq!((a.name.as_str(), a.creator.as_str(), a.tracks), ("Water Music", "", 0));
    }
}
