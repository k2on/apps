//! The library's keys: what a row of the library is keyed by, derived from
//! what it is rather than minted, so that two peers naming "BWV 988" offline
//! land on one row. Permanent: a key once written is a row's for good.

/// White space off both ends: Unicode's `White_Space`, as `str::trim` reads
/// it.
pub fn trim(s: &str) -> &str {
    s.trim()
}

/// A character lowercased by its simple mapping: one character for one.
pub fn lower_char(c: char) -> char {
    let mut l = c.to_lowercase();
    match (l.next(), l.next()) {
        (Some(x), None) => x,
        // U+0130, whose full mapping is two: its simple mapping is `i`.
        _ => 'i',
    }
}

/// Text lowercased, character by character ([`lower_char`]).
pub fn lower(s: &str) -> String {
    s.chars().map(lower_char).collect()
}

/// FNV-1a, 64 bits, over the bytes.
pub fn fnv1a64(bytes: &[u8]) -> i64 {
    bytes
        .iter()
        .fold(0xcbf29ce484222325u64, |h, b| (h ^ (*b as u64)).wrapping_mul(0x00000100000001b3)) as i64
}

/// An id as text: its sixteen bytes in hex, dashed as a UUID is.
pub fn text_of_id(b: &[u8; 16]) -> String {
    let h = ark::hash::hex(b);
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

/// The work an entry names: an entry without `work_title` names one anyway
/// when it carries a catalogue number or a part, and then it is the album's.
pub fn work_title(work_title: &str, album: &str, catalogue: &str, part: &str) -> String {
    if work_title.is_empty() && (!catalogue.is_empty() || !part.is_empty()) {
        album.into()
    } else {
        work_title.into()
    }
}

/// One name, reduced to something that can stand in a key: letters and
/// digits lowercased, anything else a dash, none leading or trailing.
/// Non-ASCII letters are kept. A run of separators is a run of dashes, as
/// the language that first spelt it could not see two characters at once.
pub fn slug(name: &str) -> String {
    let spaced: String = name.chars().map(|c| if c.is_alphanumeric() { lower_char(c) } else { ' ' }).collect();
    trim(&spaced).replace(' ', "-")
}

/// …and for a name with no letters at all, `x` and its hash: "!!!" is a
/// band, and every such name in one key would be one band.
pub fn key_part(name: &str) -> String {
    let slug = slug(name);
    if slug.is_empty() {
        format!("x{}", fnv1a64(trim(name).as_bytes()))
    } else {
        slug
    }
}

/// The folder a file is in: its path up to its last slash, empty for none.
pub fn folder_of(file: &str) -> &str {
    file.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// The key of an album: the folder it is in, and its name. Two recordings
/// of one title, in two folders, are two albums; the tracks of one folder
/// named one album are one, whoever plays each.
pub fn album_id(folder: &str, album: &str) -> String {
    format!("{folder}/{}", key_part(album))
}

/// The key of a work: its composer, and its catalogue number or its title.
pub fn work_key(composer: &str, catalogue: &str, title: &str) -> String {
    let of = if catalogue.is_empty() { title } else { catalogue };
    format!("{}/{}", key_part(composer), key_part(of))
}

/// The key of the work a track is of, or none: a work needs a title and a
/// composer.
pub fn work_id(composer: &str, catalogue: &str, title: &str) -> Option<String> {
    (!title.is_empty() && !composer.is_empty()).then(|| work_key(composer, catalogue, title))
}

/// The key of a movement: its work, and where it sits in it.
pub fn movement_key(work_id: &str, no: i64) -> String {
    format!("{work_id}#{no}")
}

/// The key of a recording: what it is of, and by whom.
pub fn recording_key(of: &str, who: &str) -> String {
    format!("{of}@{}", key_part(who))
}

/// The key of the recording a track is part of; where there is no work, the
/// release and the title make a pop track its own performance.
pub fn recording_id(work: Option<&str>, album: &str, title: &str, artist: &str, performer: &str) -> String {
    let of = match work {
        Some(w) => w.to_string(),
        None => format!("{}/{}", key_part(album), key_part(title)),
    };
    recording_key(&of, if performer.is_empty() { artist } else { performer })
}

/// Who the one credit a track carries names: the performer, the artist of a
/// track of no work, and nobody for a classical track naming no performer.
pub fn credited_as(work: Option<&str>, artist: &str, performer: &str) -> String {
    match (performer.is_empty(), work) {
        (false, _) => performer.into(),
        (true, Some(_)) => String::new(),
        (true, None) => artist.into(),
    }
}

/// The key of the movement a track is, or none when it is of no work.
pub fn movement_id(work: Option<&str>, movement_no: i64, track: i64) -> Option<String> {
    work.map(|key| movement_key(key, if movement_no == 0 { track } else { movement_no }))
}

/// The key of a show: its kind and its title, so `#show/podcast/full-time-nix`
/// reads as what it is, and a feed and a folder of one title are one show.
pub fn show_key(kind: &str, title: &str) -> String {
    format!("{kind}/{}", key_part(title))
}
