//! The library router: what the scanner authors, and what every peer reads.
//!
//! harken's `functions.rs`, for the tables of the library: every playable
//! thing (`media`), what only a song has (`song`), and the rows a track
//! belongs to — `album`, `person`, `work`, `movement`, `recording`,
//! `credit` — which are all keyed by something derived from what they *are*
//! rather than by a minted id, because an entry is handed exactly one fresh
//! id and two peers naming "BWV 988" offline must land on one row. The keys
//! are the helpers below, and they are permanent the way `apply` is.
use ark::authoring::*;

use crate::schema::*;

pub struct AddSong {
    pub title: Text,
    pub artist: Text,
    pub album: Text,
    pub duration_ms: Int,
    pub file: Text,
    pub track: Int,
    pub part: Text,
    pub catalogue: Text,
    pub performer: Text,
    pub bpm: Int,
    pub album_art: Text,
    pub artist_art: Text,
    pub disc: Int,
    pub work_title: Text,
    pub movement_no: Int,
}
impl Input for AddSong {
    fn schema() -> Object<Self> {
        object()
            .field("title", text().trim().min(1).why("a song needs a title"))
            .field("artist", text().trim())
            .field("album", text().trim())
            .field("duration_ms", int())
            .field("file", text().trim())
            .field("track", int())
            .field("part", text().trim())
            .field("catalogue", text().trim())
            .field("performer", text().trim())
            .field("bpm", int())
            .field("album_art", text().trim())
            .field("artist_art", text().trim())
            .field("disc", int())
            .field("work_title", text().trim())
            .field("movement_no", int())
    }
}

pub struct DescribeWork {
    pub id: Text,
    pub opus: Text,
    pub key_sig: Text,
    pub form: Text,
    pub period: Text,
    pub composed: Int,
    pub art: Text,
}
impl Input for DescribeWork {
    fn schema() -> Object<Self> {
        object()
            .field("id", text())
            .field("opus", text().trim())
            .field("key_sig", text().trim())
            .field("form", text().trim())
            .field("period", text().trim())
            .field("composed", int())
            .field("art", text().trim())
    }
}

pub struct DescribeRecording {
    pub id: Text,
    pub recorded: Int,
    pub venue: Text,
    pub label: Text,
    pub licence: Text,
    pub art: Text,
}
impl Input for DescribeRecording {
    fn schema() -> Object<Self> {
        object()
            .field("id", text())
            .field("recorded", int())
            .field("venue", text().trim())
            .field("label", text().trim())
            .field("licence", text().trim())
            .field("art", text().trim())
    }
}

pub struct DescribePerson {
    pub name: Text,
    pub sort_name: Text,
    pub born: Int,
    pub died: Int,
    pub art: Text,
}
impl Input for DescribePerson {
    fn schema() -> Object<Self> {
        object()
            .field("name", text().trim().min(1).why("a person needs a name"))
            .field("sort_name", text().trim())
            .field("born", int())
            .field("died", int())
            .field("art", text().trim())
    }
}

pub struct CreditRecording {
    pub recording_id: Text,
    pub person_name: Text,
    pub role: Text,
    pub instrument: Text,
    pub pos: Int,
}
impl Input for CreditRecording {
    fn schema() -> Object<Self> {
        object()
            .field("recording_id", text())
            .field("person_name", text().trim().min(1).why("a credit needs somebody to credit"))
            .field("role", text().trim())
            .field("instrument", text().trim())
            .field("pos", int())
    }
}

pub struct RemoveMedia {
    pub id: Id<Media>,
}
impl Input for RemoveMedia {
    fn schema() -> Object<Self> {
        object().field("id", id::<Media>())
    }
}

pub struct Library {
    pub playlist_id: Id<Playlist>,
}
impl Input for Library {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>())
    }
}

pub struct AlbumInput {
    pub playlist_id: Id<Playlist>,
    pub name: Text,
}
impl Input for AlbumInput {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>()).field("name", text())
    }
}

pub struct Artist {
    pub playlist_id: Id<Playlist>,
    pub name: Text,
}
impl Input for Artist {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>()).field("name", text())
    }
}

pub struct Works {
    pub composer: Text,
}
impl Input for Works {
    fn schema() -> Object<Self> {
        object().field("composer", text())
    }
}

pub struct WorkInput {
    pub id: Text,
}
impl Input for WorkInput {
    fn schema() -> Object<Self> {
        object().field("id", text())
    }
}

pub struct Recordings {
    pub work_id: Text,
}
impl Input for Recordings {
    fn schema() -> Object<Self> {
        object().field("work_id", text())
    }
}

pub struct Credits {
    pub recording_id: Text,
}
impl Input for Credits {
    fn schema() -> Object<Self> {
        object().field("recording_id", text())
    }
}

pub struct RecordingInput {
    pub playlist_id: Id<Playlist>,
    pub id: Text,
}
impl Input for RecordingInput {
    fn schema() -> Object<Self> {
        object().field("playlist_id", id::<Playlist>()).field("id", text())
    }
}

/// Something playable — a song today, an episode or a sermon later — as a
/// list renders it, and where it sits on the playlist the list was read
/// against, if it is on it (which playlist that is, is the client's choice).
pub struct LibraryEntry {
    pub added_ms: Int,
    pub creator: Text,
    pub duration_ms: Int,
    pub file: Text,
    pub id: Id<Media>,
    pub kind: Text,
    pub playlist_pos: Opt<Int>,
    pub pos: Int,
    pub title: Text,
    pub user_id: Text,
}
impl Record for LibraryEntry {
    fn fields() -> Fields<Self> {
        fields()
            .field("added_ms", int())
            .field("creator", text())
            .field("duration_ms", int())
            .field("file", text())
            .field("id", id::<Media>())
            .field("kind", text())
            .field("playlist_pos", opt(int()))
            .field("pos", int())
            .field("title", text())
            .field("user_id", text())
    }
}

/// A record, as the albums page lists it: the name, whoever made its first
/// track, how many tracks are on it, and its cover (empty when nobody said,
/// and a client draws the square derived from the name).
pub struct AlbumsEntry {
    pub art: Text,
    pub creator: Text,
    pub name: Text,
    pub tracks: Int,
}
impl Record for AlbumsEntry {
    fn fields() -> Fields<Self> {
        fields().field("art", text()).field("creator", text()).field("name", text()).field("tracks", int())
    }
}

/// Whoever made something, as the artists page lists them: `media.creator`,
/// so kind-neutral, with the picture `person` has for the name.
pub struct ArtistsEntry {
    pub art: Text,
    pub name: Text,
    pub tracks: Int,
}
impl Record for ArtistsEntry {
    fn fields() -> Fields<Self> {
        fields().field("art", text()).field("name", text()).field("tracks", int())
    }
}

/// What is true of a track as a *song*, for a table drawing those columns
/// beside the kind-neutral library row: joined in memory by `media_id`.
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
impl Record for TrackDetailsEntry {
    fn fields() -> Fields<Self> {
        fields()
            .field("album", text())
            .field("bpm", int())
            .field("catalogue", text())
            .field("licence", text())
            .field("media_id", id::<Media>())
            .field("part", text())
            .field("performer", text())
            .field("track", int())
    }
}

/// Somebody some work is by, as the composers index lists them.
pub struct ComposersEntry {
    pub art: Text,
    pub born: Int,
    pub died: Int,
    pub name: Text,
    pub sort_name: Text,
    pub tracks: Int,
    pub works: Int,
}
impl Record for ComposersEntry {
    fn fields() -> Fields<Self> {
        fields()
            .field("art", text())
            .field("born", int())
            .field("died", int())
            .field("name", text())
            .field("sort_name", text())
            .field("tracks", int())
            .field("works", int())
    }
}

/// A composition, as a composer's page lists them and a work's page heads.
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
impl Record for WorkSummary {
    fn fields() -> Fields<Self> {
        fields()
            .field("art", text())
            .field("catalogue", text())
            .field("composer", text())
            .field("form", text())
            .field("id", text())
            .field("period", text())
            .field("recordings", int())
            .field("title", text())
            .field("tracks", int())
    }
}

/// One performance, as a work's page lists them: told apart by who played it.
pub struct RecordingsEntry {
    pub art: Text,
    pub id: Text,
    pub label: Text,
    pub licence: Text,
    pub performers: Text,
    pub recorded: Int,
    pub tracks: Int,
}
impl Record for RecordingsEntry {
    fn fields() -> Fields<Self> {
        fields()
            .field("art", text())
            .field("id", text())
            .field("label", text())
            .field("licence", text())
            .field("performers", text())
            .field("recorded", int())
            .field("tracks", int())
    }
}

/// One person on a recording, and what they did on it.
pub struct CreditsEntry {
    pub instrument: Text,
    pub name: Text,
    pub pos: Int,
    pub role: Text,
}
impl Record for CreditsEntry {
    fn fields() -> Fields<Self> {
        fields().field("instrument", text()).field("name", text()).field("pos", int()).field("role", text())
    }
}

/// The work an entry names. An entry written before `work_title` existed
/// says there is one anyway when it carries a catalogue number (nobody
/// catalogues a track) or a part (a division *of a work*, by the column's
/// definition), and then the work's name is the album's.
pub fn work_title(work_title: Text, album: Text, catalogue: Text, part: Text) -> Text {
    helper(
        "work_title",
        (("work_title", work_title), ("album", album), ("catalogue", catalogue), ("part", part)),
        |work_title: Text, album: Text, catalogue: Text, part: Text| {
            pick(work_title.is_empty().and(catalogue.is_empty().not().or(part.is_empty().not())), album, work_title)
        },
    )
}

/// One name, reduced to something that can stand in a key: letters and
/// digits survive lowercased, every run of anything else is one dash, and
/// none leads or trails. Non-ASCII letters are kept rather than folded:
/// "Dvořák" and "Dvorak" are two keys, as they are two `person` names.
pub fn slug(text: Text) -> Text {
    helper("slug", ("text", text), |text: Text| {
        concat(text.chars().map(|x| pick(x.is_alnum(), x.lower(), " ")))
            .trim()
            .chars()
            .fold("", |acc: Text, x| {
                pick(
                    x.eq(" ").and(acc.chars().last().map_or(false, |x_2| x_2.eq("-"))),
                    acc,
                    concat(list([acc, pick(x.eq(" "), "-", x)])),
                )
            })
    })
}

/// …and the same for a name with no letters in it at all: "!!!" is a band,
/// and every such name in one key would be one band.
pub fn key_part(text: Text) -> Text {
    helper("key_part", ("text", text), |text: Text| {
        pick(slug(text).is_empty(), concat(list(["x".into(), text.trim().fnv1a64().to_text()])), slug(text))
    })
}

/// The key of a work: its composer, and the name that survives translation
/// — the catalogue number where there is one. The composer is in it because
/// catalogue numbers are per composer: `Op. 23` belongs to everybody.
pub fn work_key(composer: Text, catalogue: Text, title: Text) -> Text {
    helper("work_key", (("composer", composer), ("catalogue", catalogue), ("title", title)), |composer: Text, catalogue: Text, title: Text| {
        concat(list([key_part(composer), "/".into(), key_part(pick(catalogue.is_empty(), title, catalogue))]))
    })
}

/// The key of the work a track is of, or none: a work needs a title and a
/// composer, and `media.creator` is the composer for this repertoire.
pub fn work_id(artist: Text, catalogue: Text, work_title: Text) -> Opt<Text> {
    helper(
        "work_id",
        (("artist", artist), ("catalogue", catalogue), ("work_title", work_title)),
        |artist: Text, catalogue: Text, work_title: Text| {
            pick(
                work_title.is_empty().or(artist.is_empty()),
                none::<Text>(),
                some(work_key(artist, catalogue, work_title)),
            )
        },
    )
}

/// The key of a movement: its work, and where it sits in it.
pub fn movement_key(work_id: Text, no: Int) -> Text {
    helper("movement_key", (("work_id", work_id), ("no", no)), |work_id: Text, no: Int| {
        concat(list([work_id, "#".into(), no.to_text()]))
    })
}

/// The key of a recording: what it is a recording *of*, and by whom.
pub fn recording_key(of: Text, who: Text) -> Text {
    helper("recording_key", (("of", of), ("who", who)), |of: Text, who: Text| concat(list([of, "@".into(), key_part(who)])))
}

/// The key of the recording a track is part of. Every track has one: where
/// there is no work, the release and the title make a pop track its own
/// performance, so `credit` is the one answer to "who played this".
pub fn recording_id(work_id: Opt<Text>, album: Text, title: Text, artist: Text, performer: Text) -> Text {
    helper(
        "recording_id",
        (("work_id", work_id), ("album", album), ("title", title), ("artist", artist), ("performer", performer)),
        |work_id: Opt<Text>, album: Text, title: Text, artist: Text, performer: Text| {
            recording_key(
                work_id.unwrap_or(concat(list([key_part(album), "/".into(), key_part(title)]))),
                pick(performer.is_empty(), artist, performer),
            )
        },
    )
}

/// Who the one credit a track carries names: the performer when it says
/// one, the artist of a track of no work, and nobody for a classical track
/// that names no performer — which is more honest than crediting the
/// composer with the performance.
pub fn credited_as(work_id: Opt<Text>, artist: Text, performer: Text) -> Text {
    helper(
        "credited_as",
        (("work_id", work_id), ("artist", artist), ("performer", performer)),
        |work_id: Opt<Text>, artist: Text, performer: Text| pick(performer.is_empty(), pick::<Text>(work_id.is_some(), "", artist), performer),
    )
}

/// The key of the movement a track is, or none when it is of no work. Where
/// it sits in the work is the track number when the entry did not say.
pub fn movement_id(work_id: Opt<Text>, movement_no: Int, track: Int) -> Opt<Text> {
    helper(
        "movement_id",
        (("work_id", work_id), ("movement_no", movement_no), ("track", track)),
        |work_id: Opt<Text>, movement_no: Int, track: Int| work_id.map(|x| movement_key(x, pick(movement_no.eq(0), track, movement_no))),
    )
}

/// A media row as a list renders it, read against one playlist's entries.
pub fn library_entry(media: Media, items: List<PlaylistItem>) -> LibraryEntry {
    helper("library_entry", (("media", media), ("items", items)), |media: Media, items: List<PlaylistItem>| LibraryEntry {
        added_ms: media.added_ms,
        creator: media.creator,
        duration_ms: media.duration_ms,
        file: media.file,
        id: media.id,
        kind: media.kind,
        playlist_pos: items.filter(|row| row.media_id.eq(media.id)).first().map(|row| row.pos),
        pos: media.pos,
        title: media.title,
        user_id: media.user_id,
    })
}

/// The credits worth drawing: the real ones (`pos` from 1) where a recording
/// has any, which displace the lumped string a track carried at `pos` 0.
pub fn credited(credits: List<Credit>) -> List<Credit> {
    helper("credited", ("credits", credits), |credits: List<Credit>| {
        credits.filter(|row| row.pos.gt(0).or(credits.any(|row_2| row_2.pos.gt(0)).not()))
    })
}

/// Names, as one line: "Hermann Scherchen, London Symphony Orchestra".
pub fn joined(names: List<Text>) -> Text {
    helper("joined", ("names", names), |names: List<Text>| {
        names.fold("", |acc: Text, x| pick(acc.is_empty(), x, concat(list([acc, ", ".into(), x]))))
    })
}

/// Who played one recording, as the one string a column draws: its credits
/// in billing order, the composer (who is on the work) left out.
pub fn performers(credits: List<Credit>, recording_id: Text) -> Text {
    helper("performers", (("credits", credits), ("recording_id", recording_id)), |credits: List<Credit>, recording_id: Text| {
        joined(
            credited(credits.filter(|row| row.recording_id.eq(recording_id).and(row.role.ne("composer"))))
                .sort_by(|row| row.person_name)
                .sort_by(|row| row.pos)
                .map(|row| row.person_name),
        )
    })
}

/// How many of these songs are movements among these.
pub fn tracks_on(songs: List<Song>, movements: List<Movement>) -> Int {
    helper("tracks_on", (("songs", songs), ("movements", movements)), |songs: List<Song>, movements: List<Movement>| {
        songs
            .filter(|row| row.movement_id.map_or(false, |x| movements.any(|row_2| row_2.id.eq(x))))
            .len()
    })
}

/// A work, with how much of it the library holds.
pub fn work_summary(work: Work, recordings: List<Recording>, songs: List<Song>, movements: List<Movement>) -> WorkSummary {
    helper(
        "work_summary",
        (("work", work), ("recordings", recordings), ("songs", songs), ("movements", movements)),
        |work: Work, recordings: List<Recording>, songs: List<Song>, movements: List<Movement>| WorkSummary {
            art: work.art,
            catalogue: work.catalogue,
            composer: work.composer,
            form: work.form,
            id: work.id,
            period: work.period,
            recordings: recordings.filter(|row| row.work_id.eq(some(work.id))).len(),
            title: work.title,
            tracks: tracks_on(songs, movements.filter(|row| row.work_id.eq(work.id))),
        },
    )
}

pub fn library() -> Router<Harken> {
    let library = router::<Harken>("library");
    library.routes((
        // Put a song in the library, and everything it belongs to as rows:
        // the album, the artist, the work and movement it is of, the
        // recording it is part of and the one credit its lumped performer
        // string can honestly give. The same file twice is the first entry's
        // (a rescan is free); an empty file is no path and never collides.
        library.input::<AddSong>().mutation("add_song", |ctx, db, input| {
            let media = db.media.filter(Media::file.eq(input.file)).first();
            unless(input.file.is_empty().not().and(media.is_some()), || {
                let media_2 = db.media.order_by(Media::pos.desc()).first();
                db.media.insert(Media {
                    id: ctx.new_id("id"),
                    kind: "song".into(),
                    title: input.title,
                    creator: input.artist,
                    duration_ms: input.duration_ms,
                    file: input.file,
                    pos: media_2.map_or(0, |row| row.pos).add(1),
                    added_ms: ctx.now("added_ms"),
                    user_id: ctx.user,
                });
                // art_to_write: no row yet makes one; a picture replaces a
                // picture; nothing (or the same picture) replaces nothing.
                when(input.album.is_empty().not(), || {
                    let album = db.album.get((input.album,));
                    when(album.map_or(true, |row| input.album_art.is_empty().not().and(row.art.ne(input.album_art))), || {
                        db.album.upsert(Album {
                            name: input.album,
                            label: album.map_or("", |row| row.label),
                            released: album.map_or(0, |row| row.released),
                            art: input.album_art,
                            added_ms: ctx.now("added_ms"),
                            user_id: ctx.user,
                        })
                    })
                });
                when(input.artist.is_empty().not(), || {
                    let person = db.person.get((input.artist,));
                    when(person.map_or(true, |row| input.artist_art.is_empty().not().and(row.art.ne(input.artist_art))), || {
                        db.person.upsert(Person {
                            name: input.artist,
                            sort_name: person.map_or("", |row| row.sort_name),
                            born: person.map_or(0, |row| row.born),
                            died: person.map_or(0, |row| row.died),
                            art: input.artist_art,
                            added_ms: person.map_or(ctx.now("added_ms"), |row| row.added_ms),
                            user_id: person.map_or(ctx.user, |row| row.user_id),
                        })
                    })
                });
                when(
                    work_id(input.artist, input.catalogue, work_title(input.work_title, input.album, input.catalogue, input.part)).is_some(),
                    || {
                        let work = db.work.get((work_key(
                            input.artist,
                            input.catalogue,
                            work_title(input.work_title, input.album, input.catalogue, input.part),
                        ),));
                        db.work.upsert(Work {
                            id: work_key(input.artist, input.catalogue, work_title(input.work_title, input.album, input.catalogue, input.part)),
                            composer: input.artist,
                            title: work_title(input.work_title, input.album, input.catalogue, input.part),
                            catalogue: input.catalogue,
                            opus: work.map_or("", |row| row.opus),
                            key_sig: work.map_or("", |row| row.key_sig),
                            form: work.map_or("", |row| row.form),
                            period: work.map_or("", |row| row.period),
                            composed: work.map_or(0, |row| row.composed),
                            art: work.map_or("", |row| row.art),
                            added_ms: work.map_or(ctx.now("added_ms"), |row| row.added_ms),
                            user_id: work.map_or(ctx.user, |row| row.user_id),
                        });
                        let movement = db.movement.get((movement_key(
                            work_key(input.artist, input.catalogue, work_title(input.work_title, input.album, input.catalogue, input.part)),
                            pick(input.movement_no.eq(0), input.track, input.movement_no),
                        ),));
                        db.movement.upsert(Movement {
                            id: movement_key(
                                work_key(input.artist, input.catalogue, work_title(input.work_title, input.album, input.catalogue, input.part)),
                                pick(input.movement_no.eq(0), input.track, input.movement_no),
                            ),
                            work_id: work_key(input.artist, input.catalogue, work_title(input.work_title, input.album, input.catalogue, input.part)),
                            no: pick(input.movement_no.eq(0), input.track, input.movement_no),
                            title: input.title,
                            part: input.part,
                            added_ms: movement.map_or(ctx.now("added_ms"), |row| row.added_ms),
                            user_id: movement.map_or(ctx.user, |row| row.user_id),
                        })
                    },
                );
                db.recording.insert(Recording {
                    id: recording_id(
                        work_id(input.artist, input.catalogue, work_title(input.work_title, input.album, input.catalogue, input.part)),
                        input.album,
                        input.title,
                        input.artist,
                        input.performer,
                    ),
                    work_id: work_id(input.artist, input.catalogue, work_title(input.work_title, input.album, input.catalogue, input.part)),
                    recorded: 0.into(),
                    venue: "".into(),
                    label: "".into(),
                    licence: "".into(),
                    art: "".into(),
                    added_ms: ctx.now("added_ms"),
                    user_id: ctx.user,
                });
                // The fallback credit: `pos` 0 is what marks it as the lumped
                // string standing in until `credit_recording` says who is who.
                when(
                    credited_as(
                        work_id(input.artist, input.catalogue, work_title(input.work_title, input.album, input.catalogue, input.part)),
                        input.artist,
                        input.performer,
                    )
                    .is_empty()
                    .not(),
                    || {
                        db.person.insert(Person {
                            name: credited_as(
                                work_id(input.artist, input.catalogue, work_title(input.work_title, input.album, input.catalogue, input.part)),
                                input.artist,
                                input.performer,
                            ),
                            sort_name: "".into(),
                            born: 0.into(),
                            died: 0.into(),
                            art: "".into(),
                            added_ms: ctx.now("added_ms"),
                            user_id: ctx.user,
                        });
                        db.credit.insert(Credit {
                            recording_id: recording_id(
                                work_id(input.artist, input.catalogue, work_title(input.work_title, input.album, input.catalogue, input.part)),
                                input.album,
                                input.title,
                                input.artist,
                                input.performer,
                            ),
                            person_name: credited_as(
                                work_id(input.artist, input.catalogue, work_title(input.work_title, input.album, input.catalogue, input.part)),
                                input.artist,
                                input.performer,
                            ),
                            role: pick(
                                work_id(input.artist, input.catalogue, work_title(input.work_title, input.album, input.catalogue, input.part)).is_some(),
                                "performer",
                                "artist",
                            ),
                            instrument: "".into(),
                            pos: 0.into(),
                            added_ms: ctx.now("added_ms"),
                            user_id: ctx.user,
                        })
                    },
                );
                db.song.insert(Song {
                    media_id: ctx.new_id("id"),
                    album_name: pick(input.album.is_empty(), none::<Text>(), some(input.album)),
                    disc: pick(input.disc.gt(0), input.disc.min(99), 1),
                    track: input.track.clamp(0, 999),
                    recording_id: recording_id(
                        work_id(input.artist, input.catalogue, work_title(input.work_title, input.album, input.catalogue, input.part)),
                        input.album,
                        input.title,
                        input.artist,
                        input.performer,
                    ),
                    movement_id: movement_id(
                        work_id(input.artist, input.catalogue, work_title(input.work_title, input.album, input.catalogue, input.part)),
                        input.movement_no,
                        input.track,
                    ),
                    bpm: pick(input.bpm.ge(20).and(input.bpm.le(300)), input.bpm, 0),
                })
            })
        }),
        // Say more about a work than the track that created it could. A work
        // exists because a song named it, so an unknown one is refused; every
        // field is fill-if-given, so an empty one leaves what is there.
        library.input::<DescribeWork>().mutation("describe_work", |ctx, db, input| {
            let work = db.work.exists((input.id,));
            unless(work, || {
                refuse(concat(list(["no work ".into(), input.id, "; a work exists because a song named it".into()])))
            });
            db.work.update((input.id,), |row| Work {
                id: row.id,
                composer: row.composer,
                title: row.title,
                catalogue: row.catalogue,
                opus: pick(input.opus.is_empty(), row.opus, input.opus),
                key_sig: pick(input.key_sig.is_empty(), row.key_sig, input.key_sig),
                form: pick(input.form.is_empty(), row.form, input.form),
                period: pick(input.period.is_empty(), row.period, input.period),
                composed: pick(input.composed.eq(0), row.composed, input.composed),
                art: pick(input.art.is_empty(), row.art, input.art),
                added_ms: ctx.now("added_ms"),
                user_id: row.user_id,
            })
        }),
        // Say more about a performance; refuses an unknown one, as above.
        library.input::<DescribeRecording>().mutation("describe_recording", |ctx, db, input| {
            let recording = db.recording.exists((input.id,));
            unless(recording, || {
                refuse(concat(list(["no recording ".into(), input.id, "; one exists because a song is part of it".into()])))
            });
            db.recording.update((input.id,), |row| Recording {
                id: row.id,
                work_id: row.work_id,
                recorded: pick(input.recorded.eq(0), row.recorded, input.recorded),
                venue: pick(input.venue.is_empty(), row.venue, input.venue),
                label: pick(input.label.is_empty(), row.label, input.label),
                licence: pick(input.licence.is_empty(), row.licence, input.licence),
                art: pick(input.art.is_empty(), row.art, input.art),
                added_ms: ctx.now("added_ms"),
                user_id: row.user_id,
            })
        }),
        // Say more about somebody. This one makes the row when it is not
        // there: a person needs nothing but a name.
        library.input::<DescribePerson>().mutation("describe_person", |ctx, db, input| {
            let person = db.person.get((input.name,));
            db.person.upsert(Person {
                name: input.name,
                sort_name: pick(input.sort_name.is_empty(), person.map_or("", |row| row.sort_name), input.sort_name),
                born: pick(input.born.eq(0), person.map_or(0, |row| row.born), input.born),
                died: pick(input.died.eq(0), person.map_or(0, |row| row.died), input.died),
                art: pick(input.art.is_empty(), person.map_or("", |row| row.art), input.art),
                added_ms: person.map_or(ctx.now("added_ms"), |row| row.added_ms),
                user_id: person.map_or(ctx.user, |row| row.user_id),
            })
        }),
        // Credit somebody on a recording, properly: keyed by the three, so a
        // person may hold two roles and a second entry is a correction. A
        // real credit is at least 1; 0 is the lumped fallback's.
        library.input::<CreditRecording>().mutation("credit_recording", |ctx, db, input| {
            let recording = db.recording.exists((input.recording_id,));
            unless(recording, || {
                refuse(concat(list(["no recording ".into(), input.recording_id, " to credit anybody on".into()])))
            });
            db.person.insert(Person {
                name: input.person_name,
                sort_name: "".into(),
                born: 0.into(),
                died: 0.into(),
                art: "".into(),
                added_ms: ctx.now("added_ms"),
                user_id: ctx.user,
            });
            let credit = db.credit.get((input.recording_id, input.person_name, pick(input.role.is_empty(), "artist", input.role)));
            db.credit.upsert(Credit {
                recording_id: input.recording_id,
                person_name: input.person_name,
                role: pick(input.role.is_empty(), "artist", input.role),
                instrument: pick(input.instrument.is_empty(), credit.map_or("", |row| row.instrument), input.instrument),
                pos: pick(input.pos.eq(0), credit.map_or(0, |row| row.pos).max(1), input.pos.max(1)),
                added_ms: ctx.now("added_ms"),
                user_id: ctx.user,
            })
        }),
        // Take something out of the library, and off every playlist holding it.
        library.input::<RemoveMedia>().mutation("remove_media", |_ctx, db, input| {
            let playlist_item = db.playlist_item.filter(PlaylistItem::media_id.eq(input.id)).all();
            for_each(playlist_item, |row| db.playlist_item.delete((row.playlist_id, row.media_id)));
            db.song.delete((input.id,));
            db.media.delete((input.id,))
        }),
    ));
    // TEMPORARY: routes_tuple! stops at 12.
    library.routes((
        // The whole library, in the order things were added — every kind, one
        // list — read against a playlist. The one a client maintains
        // (`crate::view`).
        library.input::<Library>().query("library", |_ctx, db, input| {
            let playlist_item = db.playlist_item.filter(PlaylistItem::playlist_id.eq(input.playlist_id)).all();
            db.media.order_by(Media::pos.asc()).all().map(|row| library_entry(row, playlist_item))
        }),
        // Every album some song is on, by name.
        library.query("albums", |_ctx, db, _input: ()| {
            let song = db.song.all();
            let media = db.media.all();
            db.album
                .all()
                .map(|row| AlbumsEntry {
                    art: row.art,
                    creator: song
                        .filter(|row_2| row_2.album_name.eq(some(row.name)))
                        .first()
                        .map_or("", |row_2| media.filter(|row_3| row_3.id.eq(row_2.media_id)).first().map_or("", |row_3| row_3.creator)),
                    name: row.name,
                    tracks: song.filter(|row_2| row_2.album_name.eq(some(row.name))).len(),
                })
                .filter(|row| row.tracks.gt(0))
        }),
        // Everyone who made something in the library, by name.
        library.query("artists", |_ctx, db, _input: ()| {
            let person = db.person.all();
            let media = db.media.order_by(Media::creator.asc()).all();
            media
                .filter(|row| media.filter(|row_2| row_2.creator.eq(row.creator)).first().map_or(false, |row_2| row_2.id.eq(row.id)))
                .map(|row| ArtistsEntry {
                    art: person.filter(|row_2| row_2.name.eq(row.creator)).first().map_or("", |row_2| row_2.art),
                    name: row.creator,
                    tracks: media.filter(|row_2| row_2.creator.eq(row.creator)).len(),
                })
        }),
        // Every song, with the columns only a song has.
        library.query("track_details", |_ctx, db, _input: ()| {
            let work = db.work.all();
            let movement = db.movement.all();
            let credit = db.credit.all();
            let recording = db.recording.all();
            db.song.all().map(|row| TrackDetailsEntry {
                album: row.album_name.unwrap_or(""),
                bpm: row.bpm,
                catalogue: row.movement_id.map_or("", |x| {
                    movement
                        .filter(|row_2| row_2.id.eq(x))
                        .first()
                        .map_or("", |row_2| work.filter(|row_3| row_3.id.eq(row_2.work_id)).first().map_or("", |row_3| row_3.catalogue))
                }),
                licence: recording.filter(|row_2| row_2.id.eq(row.recording_id)).first().map_or("", |row_2| row_2.licence),
                media_id: row.media_id,
                part: row.movement_id.map_or("", |x| movement.filter(|row_2| row_2.id.eq(x)).first().map_or("", |row_2| row_2.part)),
                performer: performers(credit, row.recording_id),
                track: row.track,
            })
        }),
        // One album's tracks in the order the work goes: part, then track
        // (an untagged one, 0, last in its part), then title, then id — as
        // stable sorts, the last key first.
        library.input::<AlbumInput>().query("album", |_ctx, db, input| {
            let playlist_item = db.playlist_item.filter(PlaylistItem::playlist_id.eq(input.playlist_id)).all();
            let movement = db.movement.all();
            let song = db.song.filter(Song::album_name.eq(some(input.name))).all();
            db.media
                .all()
                .filter(|row| song.any(|row_2| row_2.media_id.eq(row.id)))
                .sort_by(|row| row.title)
                .sort_by(|row| song.filter(|row_2| row_2.media_id.eq(row.id)).first().map_or(0, |row_2| row_2.track))
                .sort_by(|row| song.filter(|row_2| row_2.media_id.eq(row.id)).first().map_or(false, |row_2| row_2.track.eq(0)))
                .sort_by(|row| {
                    song.filter(|row_2| row_2.media_id.eq(row.id)).first().map_or("", |row_2| {
                        row_2
                            .movement_id
                            .map_or("", |x| movement.filter(|row_3| row_3.id.eq(x)).first().map_or("", |row_3| row_3.part))
                    })
                })
                .map(|row| library_entry(row, playlist_item))
        }),
        // One artist's tracks, in library order.
        library.input::<Artist>().query("artist", |_ctx, db, input| {
            let playlist_item = db.playlist_item.filter(PlaylistItem::playlist_id.eq(input.playlist_id)).all();
            db.media
                .filter(Media::creator.eq(input.name))
                .order_by(Media::pos.asc())
                .all()
                .map(|row| library_entry(row, playlist_item))
        }),
        // Everyone some work here is by: the composers index.
        library.query("composers", |_ctx, db, _input: ()| {
            let work = db.work.all();
            let movement = db.movement.all();
            let song = db.song.all();
            db.person
                .all()
                .filter(|row| work.any(|row_2| row_2.composer.eq(row.name)))
                .map(|row| ComposersEntry {
                    art: row.art,
                    born: row.born,
                    died: row.died,
                    name: row.name,
                    sort_name: pick(row.sort_name.is_empty(), row.name, row.sort_name),
                    tracks: tracks_on(
                        song,
                        movement.filter(|row_2| work.any(|row_3| row_3.id.eq(row_2.work_id).and(row_3.composer.eq(row.name)))),
                    ),
                    works: work.filter(|row_2| row_2.composer.eq(row.name)).len(),
                })
        }),
        // One composer's works, by catalogue and then title.
        library.input::<Works>().query("works", |_ctx, db, input| {
            let recording = db.recording.all();
            let song = db.song.all();
            let movement = db.movement.all();
            db.work
                .filter(Work::composer.eq(input.composer))
                .order_by((Work::catalogue.asc(), Work::title.asc()))
                .all()
                .map(|row| work_summary(row, recording, song, movement))
        }),
        // One work by its key: none or one, for a page opened from a link.
        library.input::<WorkInput>().query("work", |_ctx, db, input| {
            let recording = db.recording.all();
            let song = db.song.all();
            let movement = db.movement.all();
            db.work
                .filter(Work::id.eq(input.id))
                .all()
                .map(|row| work_summary(row, recording, song, movement))
        }),
        // Every performance of one work: the most complete first, then the
        // oldest, then the key.
        library.input::<Recordings>().query("recordings", |_ctx, db, input| {
            let song = db.song.all();
            let credit = db.credit.all();
            db.recording
                .filter(Recording::work_id.eq(some(input.work_id)))
                .all()
                .map(|row| RecordingsEntry {
                    art: row.art,
                    id: row.id,
                    label: row.label,
                    licence: row.licence,
                    performers: performers(credit, row.id),
                    recorded: row.recorded,
                    tracks: song.filter(|row_2| row_2.recording_id.eq(row.id)).len(),
                })
                .sort_by(|row| row.recorded)
                .sort_by(|row| row.tracks.neg())
        }),
        // Who is on one recording, with the roles kept, in billing order.
        library.input::<Credits>().query("credits", |_ctx, db, input| {
            let credit = db
                .credit
                .filter(Credit::recording_id.eq(input.recording_id))
                .order_by((Credit::pos.asc(), Credit::person_name.asc()))
                .all();
            credited(credit).map(|row| CreditsEntry {
                instrument: row.instrument,
                name: row.person_name,
                pos: row.pos,
                role: row.role,
            })
        }),
        // One performance's tracks in the order the work goes: by movement
        // number, not by track number (a compilation's track 9 may be a
        // sonata's first movement); a track of no movement after the rest.
        library.input::<RecordingInput>().query("recording", |_ctx, db, input| {
            let playlist_item = db.playlist_item.filter(PlaylistItem::playlist_id.eq(input.playlist_id)).all();
            let movement = db.movement.all();
            let song = db.song.filter(Song::recording_id.eq(input.id)).all();
            db.media
                .all()
                .filter(|row| song.any(|row_2| row_2.media_id.eq(row.id)))
                .sort_by(|row| row.title)
                .sort_by(|row| {
                    song.filter(|row_2| row_2.media_id.eq(row.id)).first().map_or(0, |row_2| {
                        row_2
                            .movement_id
                            .map_or(0, |x| movement.filter(|row_3| row_3.id.eq(x)).first().map_or(0, |row_3| row_3.no))
                    })
                })
                .sort_by(|row| song.filter(|row_2| row_2.media_id.eq(row.id)).first().map_or(true, |row_2| row_2.movement_id.is_none()))
                .map(|row| library_entry(row, playlist_item))
        }),
    ))
}
