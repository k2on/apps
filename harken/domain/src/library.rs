//! The library router: what the scanner authors, and what every peer reads
//! (`harken/domain/src/library.rs`, function for function).
//!
//! Every playable thing (`media`), what only a song has (`song`), and the
//! rows a track belongs to — `album`, `person`, `work`, `movement`,
//! `recording`, `credit` — keyed by something derived from what they *are*
//! rather than by a minted id, because two peers naming "BWV 988" offline
//! must land on one row. The keys are the helpers below, permanent.
//!
//! Every mutation here is the library's to make: `is_library` runs first
//! and refuses an author without the `library` role. The queries are
//! everybody's.
//!
//! Where v4 said `insert` it meant "unless a row has this key" and `upsert`
//! "this row, whatever was there": v5's `Insert` refuses an existing key,
//! so each is spelled with the lookup it is. v4's list `fold`s are
//! `Std::Sum`, a sort, or a `split_once`; [`slug`] is the one that cannot
//! be: it does not collapse a run of separators, since nothing in v5's
//! language sees two neighbouring characters (a key with one is not v4's).

use ark::author::{Id, Input, Int, Node, Text};

use crate::schema::*;

#[derive(Input, Node, Clone, Debug, PartialEq, Eq, Default)]
pub struct AddSong {
    #[ark(trim, non_empty, why = "a song needs a title")]
    pub title: Text,
    #[ark(trim)]
    pub artist: Text,
    #[ark(trim)]
    pub album: Text,
    pub duration_ms: Int,
    #[ark(trim)]
    pub file: Text,
    pub track: Int,
    #[ark(trim)]
    pub part: Text,
    #[ark(trim)]
    pub catalogue: Text,
    #[ark(trim)]
    pub performer: Text,
    pub bpm: Int,
    #[ark(trim)]
    pub album_art: Text,
    #[ark(trim)]
    pub artist_art: Text,
    pub disc: Int,
    #[ark(trim)]
    pub work_title: Text,
    pub movement_no: Int,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq, Default)]
pub struct DescribeWork {
    pub id: Text,
    #[ark(trim)]
    pub opus: Text,
    #[ark(trim)]
    pub key_sig: Text,
    #[ark(trim)]
    pub form: Text,
    #[ark(trim)]
    pub period: Text,
    pub composed: Int,
    #[ark(trim)]
    pub art: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq, Default)]
pub struct DescribeRecording {
    pub id: Text,
    pub recorded: Int,
    #[ark(trim)]
    pub venue: Text,
    #[ark(trim)]
    pub label: Text,
    #[ark(trim)]
    pub licence: Text,
    #[ark(trim)]
    pub art: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq, Default)]
pub struct DescribePerson {
    #[ark(trim, non_empty, why = "a person needs a name")]
    pub name: Text,
    #[ark(trim)]
    pub sort_name: Text,
    pub born: Int,
    pub died: Int,
    #[ark(trim)]
    pub art: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq, Eq, Default)]
pub struct CreditRecording {
    pub recording_id: Text,
    #[ark(trim, non_empty, why = "a credit needs somebody to credit")]
    pub person_name: Text,
    #[ark(trim)]
    pub role: Text,
    #[ark(trim)]
    pub instrument: Text,
    pub pos: Int,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct RemoveMedia {
    pub id: Id<Media>,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct Library {
    pub playlist_id: Id<Playlist>,
}

/// `search`: the playlist each row's position is read against, and what to
/// look for.
#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct Search {
    pub playlist_id: Id<Playlist>,
    pub needle: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct AlbumInput {
    pub playlist_id: Id<Playlist>,
    pub name: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct Artist {
    pub playlist_id: Id<Playlist>,
    pub name: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct Works {
    pub composer: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct WorkInput {
    pub id: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct Recordings {
    pub work_id: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct Credits {
    pub recording_id: Text,
}

#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct RecordingInput {
    pub playlist_id: Id<Playlist>,
    pub id: Text,
}

/// Something playable as a list renders it, and where it sits on the
/// playlist the list was read against, if it is on it.
#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct LibraryEntry {
    pub added_ms: Int,
    pub creator: Text,
    pub duration_ms: Int,
    pub file: Text,
    pub id: Id<Media>,
    pub kind: Text,
    pub playlist_pos: Option<Int>,
    pub pos: Int,
    pub title: Text,
    pub user_id: Text,
}

/// A record, as the albums page lists it.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct AlbumsEntry {
    pub art: Text,
    pub creator: Text,
    pub name: Text,
    pub tracks: Int,
}

/// Whoever made something, as the artists page lists them.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct ArtistsEntry {
    pub art: Text,
    pub name: Text,
    pub tracks: Int,
}

/// What is true of a track as a song.
#[derive(Input, Node, Clone, Debug, PartialEq)]
pub struct TrackDetailsEntry {
    pub album: Text,
    pub bpm: Int,
    pub catalogue: Text,
    pub licence: Text,
    pub media_id: Id<Media>,
    pub part: Text,
    pub performer: Text,
    pub track: Int,
}

/// Somebody some work is by, as the composers index lists them.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct ComposersEntry {
    pub art: Text,
    pub born: Int,
    pub died: Int,
    pub name: Text,
    pub sort_name: Text,
    pub tracks: Int,
    pub works: Int,
}

/// A composition, as a composer's page lists them and a work's page heads.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct WorkSummary {
    pub art: Text,
    pub catalogue: Text,
    pub composer: Text,
    pub form: Text,
    pub id: Text,
    pub period: Text,
    pub recordings: Int,
    pub title: Text,
    pub tracks: Int,
}

/// One performance, as a work's page lists them.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct RecordingsEntry {
    pub art: Text,
    pub id: Text,
    pub label: Text,
    pub licence: Text,
    pub performers: Text,
    pub recorded: Int,
    pub tracks: Int,
}

/// One person on a recording, and what they did on it.
#[derive(Input, Node, Clone, Debug, PartialEq, Eq)]
pub struct CreditsEntry {
    pub instrument: Text,
    pub name: Text,
    pub pos: Int,
    pub role: Text,
}

// * The keys

/// The work an entry names: an entry without `work_title` names one anyway
/// when it carries a catalogue number or a part, and then it is the album's.
#[ark::helper("library.work_title")]
pub fn work_title(work_title: Text, album: Text, catalogue: Text, part: Text) -> Text {
    if work_title.is_empty() && (!catalogue.is_empty() || !part.is_empty()) {
        album
    } else {
        work_title
    }
}

/// One name, reduced to something that can stand in a key: letters and
/// digits lowercased, anything else a dash, none leading or trailing.
/// Non-ASCII letters are kept. (v4 made a run of separators one dash.)
#[ark::helper("library.slug")]
pub fn slug(text: Text) -> Text {
    let spaced = concat(text.chars().map(|c| if c.is_alnum() { c.lower() } else { " " })).trim();
    concat(spaced.chars().map(|c| if c == " " { "-" } else { c }))
}

/// …and for a name with no letters at all, `x` and its hash: "!!!" is a
/// band, and every such name in one key would be one band.
#[ark::helper("library.key_part")]
pub fn key_part(text: Text) -> Text {
    let s = slug(text);
    if s.is_empty() {
        concat(vec!["x", text.trim().fnv1a64().to_text()])
    } else {
        s
    }
}

/// The key of a work: its composer, and its catalogue number or its title.
#[ark::helper("library.work_key")]
pub fn work_key(composer: Text, catalogue: Text, title: Text) -> Text {
    concat(vec![
        key_part(composer),
        "/",
        key_part(if catalogue.is_empty() { title } else { catalogue }),
    ])
}

/// The key of the work a track is of, or none: a work needs a title and a
/// composer.
#[ark::helper("library.work_id")]
pub fn work_id(artist: Text, catalogue: Text, work_title: Text) -> Option<Text> {
    if work_title.is_empty() || artist.is_empty() {
        None
    } else {
        Some(work_key(artist, catalogue, work_title))
    }
}

/// The key of a movement: its work, and where it sits in it.
#[ark::helper("library.movement_key")]
pub fn movement_key(work_id: Text, no: Int) -> Text {
    concat(vec![work_id, "#", no.to_text()])
}

/// The key of a recording: what it is of, and by whom.
#[ark::helper("library.recording_key")]
pub fn recording_key(of: Text, who: Text) -> Text {
    concat(vec![of, "@", key_part(who)])
}

/// The key of the recording a track is part of; where there is no work,
/// the release and the title make a pop track its own performance.
#[ark::helper("library.recording_id")]
pub fn recording_id(work_id: Option<Text>, album: Text, title: Text, artist: Text, performer: Text) -> Text {
    recording_key(
        work_id.unwrap_or(concat(vec![key_part(album), "/", key_part(title)])),
        if performer.is_empty() { artist } else { performer },
    )
}

/// Who the one credit a track carries names: the performer, the artist of
/// a track of no work, and nobody for a classical track naming no performer.
#[ark::helper("library.credited_as")]
pub fn credited_as(work_id: Option<Text>, artist: Text, performer: Text) -> Text {
    if performer.is_empty() {
        if work_id.is_some() {
            ""
        } else {
            artist
        }
    } else {
        performer
    }
}

/// The key of the movement a track is, or none when it is of no work.
#[ark::helper("library.movement_id")]
pub fn movement_id(work_id: Option<Text>, movement_no: Int, track: Int) -> Option<Text> {
    work_id.map(|w| movement_key(w, if movement_no == 0 { track } else { movement_no }))
}

// * Reading

/// A media row as a list renders it.
#[ark::helper("library.library_entry")]
pub fn library_entry(media: Media, playlist_pos: Option<Int>) -> LibraryEntry {
    LibraryEntry {
        added_ms: media.added_ms,
        creator: media.creator,
        duration_ms: media.duration_ms,
        file: media.file,
        id: media.id,
        kind: media.kind,
        playlist_pos,
        pos: media.pos,
        title: media.title,
        user_id: media.user_id,
    }
}

/// The credits worth drawing: the real ones (`pos` from 1) where there are
/// any, which displace the lumped string a track carried at `pos` 0.
#[ark::helper("library.credited")]
pub fn credited(credits: Vec<Credit>) -> Vec<Credit> {
    credits.filter(|c| c.pos > 0 || credits.filter(|d| d.pos > 0).is_empty())
}

/// Names, as one line: "Hermann Scherchen, London Symphony Orchestra".
#[ark::helper("library.joined")]
pub fn joined(names: Vec<Text>) -> Text {
    concat(names.map(|n| concat(vec![", ", n]))).split_once(", ").map_or("", |p| p.1)
}

/// Who played one recording: its credits in billing order, the composer
/// left out.
#[ark::helper("library.performers")]
pub fn performers(credits: Vec<Credit>, recording_id: Text) -> Text {
    joined(
        credited(credits.filter(|c| c.recording_id == recording_id && c.role != "composer"))
            .sort(|c| (c.pos, c.person_name))
            .map(|c| c.person_name),
    )
}

/// A work, with how much of it the library holds.
#[ark::helper("library.work_summary")]
pub fn work_summary(work: Work, recordings: Int, tracks: Int) -> WorkSummary {
    WorkSummary {
        art: work.art,
        catalogue: work.catalogue,
        composer: work.composer,
        form: work.form,
        id: work.id,
        period: work.period,
        recordings,
        title: work.title,
        tracks,
    }
}

/// The sum of some counts.
#[ark::helper("library.total")]
pub fn total(counts: Vec<Int>) -> Int {
    counts.sum()
}

/// How many songs are movements of a work.
#[ark::helper("library.work_tracks")]
pub fn work_tracks(db: Harken, work_id: Text) -> Int {
    total(
        db.movement
            .filter(|m| m.work_id == work_id)
            .map(|m| db.song.filter(|s| s.movement_id == Some(m.id)).len()),
    )
}

/// Works, each with how many recordings of it there are and how many
/// tracks are movements of it.
#[ark::helper("library.summaries")]
pub fn summaries(db: Harken, works: Vec<Work>) -> Vec<WorkSummary> {
    works.map(|w| work_summary(w, db.recording.filter(|r| r.work_id == Some(w.id)).len(), work_tracks(db, w.id)))
}

/// The library's role, or a refusal: what every library mutation runs first.
#[ark::guard("library.is_library")]
pub fn is_library(ctx: Ctx) {
    if !ctx.has_role("library") {
        refuse!("only the library writes the library");
    }
}

/// A playlist's entry for a media row, if it is on it.
#[ark::helper("library.playlist_pos")]
pub fn playlist_pos(db: Harken, playlist_id: Id<Playlist>, media_id: Id<Media>) -> Option<Int> {
    db.playlist_item.get((playlist_id, media_id)).map(|i| i.pos)
}

/// The movement a song is, if it is one.
#[ark::helper("library.movement_of")]
pub fn movement_of(db: Harken, song: Song) -> Option<Movement> {
    match song.movement_id {
        Some(m) => db.movement.get(m),
        None => None,
    }
}

/// Whether a text holds the needle, both folded by the pinned `lower`.
#[ark::helper("library.has")]
pub fn has(text: Text, needle: Text) -> bool {
    needle.is_empty() || text.lower().split_once(needle.lower()).is_some()
}

// * The mutations

/// Put a song in the library, and everything it belongs to as rows. An
/// upsert by `file`: a file the library has is the song it is, said again,
/// so the media row keeps its id, place, when and by whom it was added;
/// the same file with nothing new writes no row. What the song was part of
/// and no longer is — a recording, a movement, a work — goes when nothing
/// else is part of it.
#[ark::mutator("library.add_song")]
pub fn add_song(ctx: Ctx, db: Harken, input: AddSong) {
    is_library(ctx);
    let was = db.media.filter(|m| m.file == input.file).first().filter(|m| !m.file.is_empty());
    let id = was.map_or(ctx.new_id::<Media>(), |m| m.id);
    let newest = db.media.sort(|m| Desc(m.pos)).first();
    let media = Media {
        id,
        kind: was.map_or("song", |m| m.kind),
        title: input.title,
        creator: input.artist,
        duration_ms: input.duration_ms,
        file: input.file,
        pos: was.map_or(newest.map_or(0, |m| m.pos) + 1, |m| m.pos),
        added_ms: was.map_or(ctx.now, |m| m.added_ms),
        user_id: was.map_or(ctx.user, |m| m.user_id),
    };
    if db.media.get(id).is_some() {
        db.media.update(id, |_| media);
    } else {
        db.media.insert(media);
    }
    // No row yet makes one; a picture replaces a picture; nothing (or the
    // same picture) replaces nothing.
    if !input.album.is_empty() {
        let album = db.album.get(input.album);
        if album.map_or(true, |a| !input.album_art.is_empty() && a.art != input.album_art) {
            let row = Album {
                name: input.album,
                label: album.map_or("", |a| a.label),
                released: album.map_or(0, |a| a.released),
                art: input.album_art,
                added_ms: ctx.now,
                user_id: ctx.user,
            };
            if album.is_some() {
                db.album.update(input.album, |_| row);
            } else {
                db.album.insert(row);
            }
        }
    }
    if !input.artist.is_empty() {
        let person = db.person.get(input.artist);
        if person.map_or(true, |p| !input.artist_art.is_empty() && p.art != input.artist_art) {
            let row = Person {
                name: input.artist,
                sort_name: person.map_or("", |p| p.sort_name),
                born: person.map_or(0, |p| p.born),
                died: person.map_or(0, |p| p.died),
                art: input.artist_art,
                added_ms: person.map_or(ctx.now, |p| p.added_ms),
                user_id: person.map_or(ctx.user, |p| p.user_id),
            };
            if person.is_some() {
                db.person.update(input.artist, |_| row);
            } else {
                db.person.insert(row);
            }
        }
    }
    let work_named = work_title(input.work_title, input.album, input.catalogue, input.part);
    let work_of = work_id(input.artist, input.catalogue, work_named);
    if work_of.is_some() {
        let key = work_key(input.artist, input.catalogue, work_named);
        let no = if input.movement_no == 0 { input.track } else { input.movement_no };
        let movement_ref = movement_key(key, no);
        let work = db.work.get(key);
        let row = Work {
            id: key,
            composer: input.artist,
            title: work_named,
            catalogue: input.catalogue,
            opus: work.map_or("", |w| w.opus),
            key_sig: work.map_or("", |w| w.key_sig),
            form: work.map_or("", |w| w.form),
            period: work.map_or("", |w| w.period),
            composed: work.map_or(0, |w| w.composed),
            art: work.map_or("", |w| w.art),
            added_ms: work.map_or(ctx.now, |w| w.added_ms),
            user_id: work.map_or(ctx.user, |w| w.user_id),
        };
        if work.is_some() {
            db.work.update(key, |_| row);
        } else {
            db.work.insert(row);
        }
        let movement = db.movement.get(movement_ref);
        let row = Movement {
            id: movement_ref,
            work_id: key,
            no,
            title: input.title,
            part: input.part,
            added_ms: movement.map_or(ctx.now, |m| m.added_ms),
            user_id: movement.map_or(ctx.user, |m| m.user_id),
        };
        if movement.is_some() {
            db.movement.update(movement_ref, |_| row);
        } else {
            db.movement.insert(row);
        }
    }
    let recording = recording_id(work_of, input.album, input.title, input.artist, input.performer);
    let credited = credited_as(work_of, input.artist, input.performer);
    if db.recording.get(recording).is_none() {
        db.recording.insert(Recording {
            id: recording,
            work_id: work_of,
            recorded: 0,
            venue: "",
            label: "",
            licence: "",
            art: "",
            added_ms: ctx.now,
            user_id: ctx.user,
        });
    }
    // The fallback credit: `pos` 0 marks the lumped string standing in
    // until `credit_recording` says who is who.
    if !credited.is_empty() {
        if db.person.get(credited).is_none() {
            db.person.insert(Person {
                name: credited,
                sort_name: "",
                born: 0,
                died: 0,
                art: "",
                added_ms: ctx.now,
                user_id: ctx.user,
            });
        }
        let role = if work_of.is_some() { "performer" } else { "artist" };
        if db.credit.get((recording, credited, role)).is_none() {
            db.credit.insert(Credit {
                recording_id: recording,
                person_name: credited,
                role,
                instrument: "",
                pos: 0,
                added_ms: ctx.now,
                user_id: ctx.user,
            });
        }
    }
    let movement = movement_id(work_of, input.movement_no, input.track);
    let song = db.song.get(id);
    let row = Song {
        media_id: id,
        album_name: if input.album.is_empty() { None } else { Some(input.album) },
        disc: if input.disc > 0 { input.disc.min(99) } else { 1 },
        track: input.track.clamp(0, 999),
        recording_id: recording,
        movement_id: movement,
        bpm: if input.bpm >= 20 && input.bpm <= 300 { input.bpm } else { 0 },
    };
    if song.is_some() {
        db.song.update(id, |_| row);
    } else {
        db.song.insert(row);
    }
    // The recording the song has left, with its credits, when no song is
    // part of it any more; before the movement, since a recording names the
    // work the movement's emptying may take.
    let left = song.map(|s| s.recording_id).filter(|r| r != recording);
    if left.is_some() && db.song.filter(|s| s.recording_id == left.unwrap()).is_empty() {
        db.credit
            .filter(|c| c.recording_id == left.unwrap())
            .map(|c| db.credit.delete((c.recording_id, c.person_name, c.role)));
        db.recording.delete(left.unwrap());
    }
    // The movement it has left, when no song is it; and that movement's
    // work, when no movement and no recording is of it.
    let moved = song.map_or(None, |s| s.movement_id).filter(|m| Some(m) != movement);
    if moved.is_some() && db.song.filter(|s| s.movement_id == moved).is_empty() {
        let of = db.movement.get(moved.unwrap()).map_or("", |m| m.work_id);
        db.movement.delete(moved.unwrap());
        if db.movement.filter(|m| m.work_id == of).is_empty() && db.recording.filter(|r| r.work_id == Some(of)).is_empty() {
            db.work.delete(of);
        }
    }
}

/// Say more about a work. A work exists because a song named it, so an
/// unknown one is refused; every field is fill-if-given.
#[ark::mutator("library.describe_work")]
pub fn describe_work(ctx: Ctx, db: Harken, input: DescribeWork) {
    is_library(ctx);
    if db.work.get(input.id).is_none() {
        refuse!(concat(vec!["no work ", input.id, "; a work exists because a song named it"]));
    }
    db.work.update(input.id, |w| Work {
        opus: if input.opus.is_empty() { w.opus } else { input.opus },
        key_sig: if input.key_sig.is_empty() { w.key_sig } else { input.key_sig },
        form: if input.form.is_empty() { w.form } else { input.form },
        period: if input.period.is_empty() { w.period } else { input.period },
        composed: if input.composed == 0 { w.composed } else { input.composed },
        art: if input.art.is_empty() { w.art } else { input.art },
        added_ms: ctx.now,
        ..w
    });
}

/// Say more about a performance; refuses an unknown one.
#[ark::mutator("library.describe_recording")]
pub fn describe_recording(ctx: Ctx, db: Harken, input: DescribeRecording) {
    is_library(ctx);
    if db.recording.get(input.id).is_none() {
        refuse!(concat(vec!["no recording ", input.id, "; one exists because a song is part of it"]));
    }
    db.recording.update(input.id, |r| Recording {
        recorded: if input.recorded == 0 { r.recorded } else { input.recorded },
        venue: if input.venue.is_empty() { r.venue } else { input.venue },
        label: if input.label.is_empty() { r.label } else { input.label },
        licence: if input.licence.is_empty() { r.licence } else { input.licence },
        art: if input.art.is_empty() { r.art } else { input.art },
        added_ms: ctx.now,
        ..r
    });
}

/// Say more about somebody, making the row when it is not there.
#[ark::mutator("library.describe_person")]
pub fn describe_person(ctx: Ctx, db: Harken, input: DescribePerson) {
    is_library(ctx);
    let person = db.person.get(input.name);
    let row = Person {
        name: input.name,
        sort_name: if input.sort_name.is_empty() {
            person.map_or("", |p| p.sort_name)
        } else {
            input.sort_name
        },
        born: if input.born == 0 { person.map_or(0, |p| p.born) } else { input.born },
        died: if input.died == 0 { person.map_or(0, |p| p.died) } else { input.died },
        art: if input.art.is_empty() { person.map_or("", |p| p.art) } else { input.art },
        added_ms: person.map_or(ctx.now, |p| p.added_ms),
        user_id: person.map_or(ctx.user, |p| p.user_id),
    };
    if person.is_some() {
        db.person.update(input.name, |_| row);
    } else {
        db.person.insert(row);
    }
}

/// Credit somebody on a recording, properly: keyed by the three, so a
/// person may hold two roles and a second entry is a correction. A real
/// credit is at least 1; 0 is the lumped fallback's.
#[ark::mutator("library.credit_recording")]
pub fn credit_recording(ctx: Ctx, db: Harken, input: CreditRecording) {
    is_library(ctx);
    if db.recording.get(input.recording_id).is_none() {
        refuse!(concat(vec!["no recording ", input.recording_id, " to credit anybody on"]));
    }
    if db.person.get(input.person_name).is_none() {
        db.person.insert(Person {
            name: input.person_name,
            sort_name: "",
            born: 0,
            died: 0,
            art: "",
            added_ms: ctx.now,
            user_id: ctx.user,
        });
    }
    let role = if input.role.is_empty() { "artist" } else { input.role };
    let credit = db.credit.get((input.recording_id, input.person_name, role));
    let row = Credit {
        recording_id: input.recording_id,
        person_name: input.person_name,
        role,
        instrument: if input.instrument.is_empty() {
            credit.map_or("", |c| c.instrument)
        } else {
            input.instrument
        },
        pos: if input.pos == 0 {
            credit.map_or(0, |c| c.pos).max(1)
        } else {
            input.pos.max(1)
        },
        added_ms: ctx.now,
        user_id: ctx.user,
    };
    if credit.is_some() {
        db.credit.update((input.recording_id, input.person_name, role), |_| row);
    } else {
        db.credit.insert(row);
    }
}

/// Take something out of the library, and off every playlist holding it.
#[ark::mutator("library.remove_media")]
pub fn remove_media(ctx: Ctx, db: Harken, input: RemoveMedia) {
    is_library(ctx);
    db.playlist_item
        .filter(|i| i.media_id == input.id)
        .map(|i| db.playlist_item.delete((i.playlist_id, i.media_id)));
    db.song.delete(input.id);
    db.media.delete(input.id);
}

// * The queries

/// The whole library in the order things were added, each with where it
/// sits on one playlist.
#[ark::query("library.library")]
pub fn library(db: Harken, input: Library) -> Vec<LibraryEntry> {
    db.media
        .sort(|m| m.pos)
        .map(|m| library_entry(m, playlist_pos(db, input.playlist_id, m.id)))
}

/// The library's rows whose title or creator holds the needle, case folded,
/// in library order.
#[ark::query("library.search")]
pub fn search(db: Harken, input: Search) -> Vec<LibraryEntry> {
    db.media
        .filter(|m| has(m.title, input.needle) || has(m.creator, input.needle))
        .sort(|m| m.pos)
        .map(|m| library_entry(m, playlist_pos(db, input.playlist_id, m.id)))
}

/// Every album some song is on, by name, with whoever made its first track
/// (in the song table's order) and how many tracks it has.
#[ark::query("library.albums")]
pub fn albums(db: Harken) -> Vec<AlbumsEntry> {
    db.album
        .map(|a| {
            (
                a,
                db.song
                    .filter(|s| s.album_name == Some(a.name))
                    .map(|s| db.media.get(s.media_id).map_or("", |m| m.creator)),
            )
        })
        .filter(|g| g.1.len() > 0)
        .map(|g| AlbumsEntry {
            art: g.0.art,
            creator: g.1.first().unwrap_or(""),
            name: g.0.name,
            tracks: g.1.len(),
        })
}

/// Everyone who made something in the library, by name, with the picture
/// `person` has for the name.
#[ark::query("library.artists")]
pub fn artists(db: Harken) -> Vec<ArtistsEntry> {
    db.media
        .group(
            |m| m.creator,
            |c, ms| ArtistsEntry {
                art: db.person.get(c).map_or("", |p| p.art),
                name: c,
                tracks: ms.len(),
            },
        )
        .map(|g| g.1)
}

/// Every song, with the columns only a song has.
#[ark::query("library.track_details")]
pub fn track_details(db: Harken) -> Vec<TrackDetailsEntry> {
    db.song.map(|s| {
        let movement = movement_of(db, s);
        let work = match movement {
            Some(m) => db.work.get(m.work_id),
            None => None,
        };
        let recording = db.recording.get(s.recording_id);
        TrackDetailsEntry {
            album: s.album_name.unwrap_or(""),
            bpm: s.bpm,
            catalogue: work.map_or("", |w| w.catalogue),
            licence: recording.map_or("", |r| r.licence),
            media_id: s.media_id,
            part: movement.map_or("", |m| m.part),
            performer: performers(db.credit.filter(|c| c.recording_id == s.recording_id), s.recording_id),
            track: s.track,
        }
    })
}

/// One album's tracks in the order the work goes: part, then track (an
/// untagged one, 0, last in its part), then title, then the key.
#[ark::query("library.album")]
pub fn album(db: Harken, input: AlbumInput) -> Vec<LibraryEntry> {
    db.song
        .filter(|s| s.album_name == Some(input.name) && db.media.get(s.media_id).is_some())
        .sort(|s| {
            (
                movement_of(db, s).map_or("", |m| m.part),
                s.track == 0,
                s.track,
                db.media.get(s.media_id).map_or("", |m| m.title),
            )
        })
        .map(|s| library_entry(db.media.get(s.media_id).unwrap(), playlist_pos(db, input.playlist_id, s.media_id)))
}

/// One artist's tracks, in library order.
#[ark::query("library.artist")]
pub fn artist(db: Harken, input: Artist) -> Vec<LibraryEntry> {
    db.media
        .filter(|m| m.creator == input.name)
        .sort(|m| m.pos)
        .map(|m| library_entry(m, playlist_pos(db, input.playlist_id, m.id)))
}

/// Everyone some work here is by, with how many tracks are movements of
/// their works and how many works.
#[ark::query("library.composers")]
pub fn composers(db: Harken) -> Vec<ComposersEntry> {
    db.person
        .map(|p| (p, db.work.filter(|w| w.composer == p.name).map(|w| work_tracks(db, w.id))))
        .filter(|g| g.1.len() > 0)
        .map(|g| ComposersEntry {
            art: g.0.art,
            born: g.0.born,
            died: g.0.died,
            name: g.0.name,
            sort_name: if g.0.sort_name.is_empty() { g.0.name } else { g.0.sort_name },
            tracks: total(g.1),
            works: g.1.len(),
        })
}

/// One composer's works, by catalogue and then title.
#[ark::query("library.works")]
pub fn works(db: Harken, input: Works) -> Vec<WorkSummary> {
    summaries(db, db.work.filter(|w| w.composer == input.composer).sort(|w| (w.catalogue, w.title)))
}

/// One work by its key: none or one.
#[ark::query("library.work")]
pub fn work(db: Harken, input: WorkInput) -> Vec<WorkSummary> {
    summaries(db, db.work.filter(|w| w.id == input.id))
}

/// Every performance of one work: the most complete first, then the
/// oldest, then the key.
#[ark::query("library.recordings")]
pub fn recordings(db: Harken, input: Recordings) -> Vec<RecordingsEntry> {
    db.recording
        .filter(|r| r.work_id == Some(input.work_id))
        .map(|r| (r, db.song.filter(|s| s.recording_id == r.id).len()))
        .sort(|g| (-g.1, g.0.recorded))
        .map(|g| RecordingsEntry {
            art: g.0.art,
            id: g.0.id,
            label: g.0.label,
            licence: g.0.licence,
            performers: performers(db.credit.filter(|c| c.recording_id == g.0.id), g.0.id),
            recorded: g.0.recorded,
            tracks: g.1,
        })
}

/// Who is on one recording, the roles kept, in billing order: the real
/// credits where there are any.
#[ark::query("library.credits")]
pub fn credits(db: Harken, input: Credits) -> Vec<CreditsEntry> {
    let all = db.credit.filter(|c| c.recording_id == input.recording_id);
    credited(all).sort(|c| (c.pos, c.person_name)).map(|c| CreditsEntry {
        instrument: c.instrument,
        name: c.person_name,
        pos: c.pos,
        role: c.role,
    })
}

/// One performance's tracks in the order the work goes: by movement number,
/// a track of no movement after the rest.
#[ark::query("library.recording")]
pub fn recording(db: Harken, input: RecordingInput) -> Vec<LibraryEntry> {
    db.song
        .filter(|s| s.recording_id == input.id && db.media.get(s.media_id).is_some())
        .sort(|s| {
            (
                s.movement_id.is_none(),
                movement_of(db, s).map_or(0, |m| m.no),
                db.media.get(s.media_id).map_or("", |m| m.title),
            )
        })
        .map(|s| library_entry(db.media.get(s.media_id).unwrap(), playlist_pos(db, input.playlist_id, s.media_id)))
}
