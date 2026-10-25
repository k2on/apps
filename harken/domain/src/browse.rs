//! The library as people browse it: the views a library screen reads, each
//! everybody's.

use ark::prelude::*;

use crate::library::{credited, credits_of, has, library_entry, movement_of, performers, playlist_pos, summary, work_tracks};
use crate::schema::*;
use crate::types::*;

/// A plan over harken's tables.
pub type Plan<T> = ark::plan::Plan<Harken, T>;

/// The whole library in the order things were added, each with where it
/// sits on one playlist.
#[view("library.library")]
pub fn library(_: &Ctx, _: &Db<Harken>, input: Library) -> Result<Plan<LibraryEntry>, Refusal> {
    let on = input.playlist_id;
    Ok(scan::<Harken, Media>()
        .sort(|m| m.pos)
        .map_with(move |db, m| library_entry(m, playlist_pos(db, on, m.id))))
}

/// The library's rows whose title or creator holds the needle, case
/// folded, in library order.
#[view("library.search")]
pub fn search(_: &Ctx, _: &Db<Harken>, input: Search) -> Result<Plan<LibraryEntry>, Refusal> {
    let (on, needle) = (input.playlist_id, input.needle);
    Ok(scan::<Harken, Media>()
        .filter(move |m| has(&m.title, &needle) || has(&m.creator, &needle))
        .sort(|m| m.pos)
        .map_with(move |db, m| library_entry(m, playlist_pos(db, on, m.id))))
}

/// Every album some song is on, by name, with whoever made its first track
/// (in the song table's order) and how many tracks it has.
#[view("library.albums")]
pub fn albums(_: &Ctx, _: &Db<Harken>, _: ()) -> Result<Plan<AlbumsEntry>, Refusal> {
    Ok(scan::<Harken, Album>()
        .map_with(|db, a| {
            let creators: Vec<String> = db
                .song
                .scan()
                .into_iter()
                .filter(|s| s.album_name.as_ref() == Some(&a.name))
                .map(|s| db.media.get(s.media_id).map(|m| m.creator).unwrap_or_default())
                .collect();
            (a.clone(), creators)
        })
        .filter(|(_, creators)| !creators.is_empty())
        .map(|(a, creators)| AlbumsEntry {
            art: a.art.clone(),
            creator: creators[0].clone(),
            name: a.name.clone(),
            tracks: creators.len() as i64,
        }))
}

/// Everyone who made something in the library, by name, with the picture
/// `person` has for the name.
#[view("library.artists")]
pub fn artists(_: &Ctx, _: &Db<Harken>, _: ()) -> Result<Plan<ArtistsEntry>, Refusal> {
    Ok(scan::<Harken, Media>().group(
        |m| m.creator.clone(),
        |db, name, tracks| {
            let p = db.person.get(name).unwrap_or_default();
            ArtistsEntry {
                art: p.art,
                banner: p.banner,
                name: name.clone(),
                tracks: tracks.len() as i64,
            }
        },
    ))
}

/// Every song, with the columns only a song has.
#[view("library.track_details")]
pub fn track_details(_: &Ctx, _: &Db<Harken>, _: ()) -> Result<Plan<TrackDetailsEntry>, Refusal> {
    Ok(scan::<Harken, Song>().map_with(|db, song| {
        let movement = movement_of(db, song);
        let work = movement.as_ref().and_then(|m| db.work.get(&m.work_id));
        let recording = db.recording.get(&song.recording_id);
        TrackDetailsEntry {
            album: song.album_name.clone().unwrap_or_default(),
            bpm: song.bpm,
            catalogue: work.map(|w| w.catalogue).unwrap_or_default(),
            licence: recording.map(|r| r.licence).unwrap_or_default(),
            media_id: song.media_id,
            part: movement.map(|m| m.part).unwrap_or_default(),
            performer: performers(&credits_of(db, &song.recording_id), &song.recording_id),
            track: song.track,
        }
    }))
}

/// One album's tracks in the order the work goes: part, then track (an
/// untagged one, 0, last in its part), then title, then the key.
#[view("library.album")]
pub fn album(_: &Ctx, _: &Db<Harken>, input: AlbumInput) -> Result<Plan<LibraryEntry>, Refusal> {
    let (on, name) = (input.playlist_id, input.name);
    Ok(scan::<Harken, Song>()
        .filter_with(move |db, s| s.album_name.as_ref() == Some(&name) && db.media.has(s.media_id))
        .sort_with(|db, s| {
            let part = movement_of(db, s).map(|m| m.part).unwrap_or_default();
            let title = db.media.get(s.media_id).map(|m| m.title).unwrap_or_default();
            (part, s.track == 0, s.track, title)
        })
        .map_with(move |db, s| library_entry(&db.media.get(s.media_id).expect("a song's media"), playlist_pos(db, on, s.media_id))))
}

/// One artist's tracks, in library order.
#[view("library.artist")]
pub fn artist(_: &Ctx, _: &Db<Harken>, input: Artist) -> Result<Plan<LibraryEntry>, Refusal> {
    let (on, name) = (input.playlist_id, input.name);
    Ok(scan::<Harken, Media>()
        .filter(move |m| m.creator == name)
        .sort(|m| m.pos)
        .map_with(move |db, m| library_entry(m, playlist_pos(db, on, m.id))))
}

/// Everyone some work here is by, with how many tracks are movements of
/// their works and how many works.
#[view("library.composers")]
pub fn composers(_: &Ctx, _: &Db<Harken>, _: ()) -> Result<Plan<ComposersEntry>, Refusal> {
    Ok(scan::<Harken, Person>()
        .map_with(|db, p| {
            let works = db.work.scan().into_iter().filter(|w| w.composer == p.name);
            (p.clone(), works.map(|w| work_tracks(db, &w.id)).collect::<Vec<i64>>())
        })
        .filter(|(_, works)| !works.is_empty())
        .map(|(p, works)| ComposersEntry {
            art: p.art.clone(),
            banner: p.banner.clone(),
            born: p.born,
            died: p.died,
            name: p.name.clone(),
            sort_name: if p.sort_name.is_empty() { p.name.clone() } else { p.sort_name.clone() },
            tracks: works.iter().sum(),
            works: works.len() as i64,
        }))
}

/// One composer's works, by catalogue and then title.
#[view("library.works")]
pub fn works(_: &Ctx, _: &Db<Harken>, input: Works) -> Result<Plan<WorkSummary>, Refusal> {
    Ok(scan::<Harken, Work>()
        .filter(move |w| w.composer == input.composer)
        .sort(|w| (w.catalogue.clone(), w.title.clone()))
        .map_with(summary))
}

/// One work by its key: none or one.
#[view("library.work")]
pub fn work(_: &Ctx, _: &Db<Harken>, input: WorkInput) -> Result<Plan<WorkSummary>, Refusal> {
    Ok(scan::<Harken, Work>().filter(move |w| w.id == input.id).map_with(summary))
}

/// Every performance of one work: the most complete first, then the
/// oldest, then the key.
#[view("library.recordings")]
pub fn recordings(_: &Ctx, _: &Db<Harken>, input: Recordings) -> Result<Plan<RecordingsEntry>, Refusal> {
    Ok(scan::<Harken, Recording>()
        .filter(move |r| r.work_id.as_ref() == Some(&input.work_id))
        .map_with(|db, r| (r.clone(), db.song.scan().iter().filter(|s| s.recording_id == r.id).count() as i64))
        .sort(|(r, tracks)| (-tracks, r.recorded))
        .map_with(|db, (r, tracks)| RecordingsEntry {
            art: r.art.clone(),
            id: r.id.clone(),
            label: r.label.clone(),
            licence: r.licence.clone(),
            performers: performers(&credits_of(db, &r.id), &r.id),
            recorded: r.recorded,
            tracks: *tracks,
        }))
}

/// Who is on one recording, the roles kept, in billing order: the real
/// credits where there are any.
#[view("library.credits")]
pub fn credits(_: &Ctx, _: &Db<Harken>, input: Credits) -> Result<Plan<CreditsEntry>, Refusal> {
    Ok(scan::<Harken, Credit>()
        .filter(move |c| c.recording_id == input.recording_id)
        .filter_with(|db, c| credited(&credits_of(db, &c.recording_id)).contains(c))
        .sort(|c| (c.pos, c.person_name.clone()))
        .map(|c| CreditsEntry {
            instrument: c.instrument.clone(),
            name: c.person_name.clone(),
            pos: c.pos,
            role: c.role.clone(),
        }))
}

/// One performance's tracks in the order the work goes: by movement number,
/// a track of no movement after the rest.
#[view("library.recording")]
pub fn recording(_: &Ctx, _: &Db<Harken>, input: RecordingInput) -> Result<Plan<LibraryEntry>, Refusal> {
    let (on, id) = (input.playlist_id, input.id);
    Ok(scan::<Harken, Song>()
        .filter_with(move |db, s| s.recording_id == id && db.media.has(s.media_id))
        .sort_with(|db, s| {
            let no = movement_of(db, s).map_or(0, |m| m.no);
            let title = db.media.get(s.media_id).map(|m| m.title).unwrap_or_default();
            (s.movement_id.is_none(), no, title)
        })
        .map_with(move |db, s| library_entry(&db.media.get(s.media_id).expect("a song's media"), playlist_pos(db, on, s.media_id))))
}
