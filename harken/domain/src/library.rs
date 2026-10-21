//! The library: what the scanner authors, and what every peer reads.
//!
//! Every playable thing (`media`), what only a song has (`song`), and the
//! rows a track belongs to — `album`, `person`, `work`, `movement`,
//! `recording`, `credit` — keyed by something derived from what they are
//! rather than by a minted id, because two peers naming "BWV 988" offline
//! must land on one row. The keys are the helpers of `keys.rs`, and
//! permanent.
//!
//! Every mutation here is the library's to make: [`is_library`] runs first
//! and refuses an author without the `library` role. The queries are
//! everybody's.
//!
//! `insert` refuses an existing key, so "unless a row has this key" and
//! "this row, whatever was there" are each spelt with the read they are.

use ark::prelude::*;

use crate::keys;
use crate::schema::*;
use crate::types::*;
use crate::LIBRARY;

// * What the queries share

/// A media row as a list renders it, and where it sits on the playlist the
/// list was read against, if it is on it.
pub fn library_entry(media: &Media, playlist_pos: Option<i64>) -> LibraryEntry {
    LibraryEntry {
        added_ms: media.added_ms,
        creator: media.creator.clone(),
        duration_ms: media.duration_ms,
        file: media.file.clone(),
        id: media.id,
        kind: media.kind.clone(),
        playlist_pos,
        pos: media.pos,
        title: media.title.clone(),
        user_id: media.user_id.clone(),
    }
}

/// The credits worth drawing: the real ones (`pos` from 1) where there are
/// any, which displace the lumped string a track carried at `pos` 0.
pub fn credited(credits: &[Credit]) -> Vec<Credit> {
    let real = credits.iter().any(|c| c.pos > 0);
    credits.iter().filter(|c| c.pos > 0 || !real).cloned().collect()
}

/// Names, as one line: "Hermann Scherchen, London Symphony Orchestra".
pub fn joined(names: &[String]) -> String {
    names.join(", ")
}

/// Who played one recording: its credits in billing order, the composer
/// left out.
pub fn performers(credits: &[Credit], recording_id: &str) -> String {
    let theirs: Vec<Credit> = credits
        .iter()
        .filter(|c| c.recording_id == recording_id && c.role != "composer")
        .cloned()
        .collect();
    let mut billed = credited(&theirs);
    billed.sort_by(|a, b| (a.pos, &a.person_name).cmp(&(b.pos, &b.person_name)));
    joined(&billed.into_iter().map(|c| c.person_name).collect::<Vec<_>>())
}

/// A recording's credits.
pub fn credits_of(db: &Db<Harken>, recording_id: &str) -> Vec<Credit> {
    db.credit.by(Credit::KEY).from(recording_id).all()
}

/// A work, with how much of it the library holds.
pub fn work_summary(work: &Work, recordings: i64, tracks: i64) -> WorkSummary {
    WorkSummary {
        art: work.art.clone(),
        catalogue: work.catalogue.clone(),
        composer: work.composer.clone(),
        form: work.form.clone(),
        id: work.id.clone(),
        period: work.period.clone(),
        recordings,
        title: work.title.clone(),
        tracks,
    }
}

/// How many songs are movements of a work.
pub fn work_tracks(db: &Db<Harken>, work_id: &str) -> i64 {
    let songs = db.song.scan();
    let movements = db.movement.scan().into_iter().filter(|m| m.work_id == work_id);
    movements
        .map(|m| songs.iter().filter(|s| s.movement_id.as_ref() == Some(&m.id)).count() as i64)
        .sum()
}

/// A work, with how many recordings of it there are and how many tracks are
/// movements of it.
pub fn summary(db: &Db<Harken>, work: &Work) -> WorkSummary {
    let recordings = db.recording.scan().iter().filter(|r| r.work_id.as_ref() == Some(&work.id)).count() as i64;
    work_summary(work, recordings, work_tracks(db, &work.id))
}

/// The library's role, or a refusal: what every library mutation runs
/// first.
pub fn is_library(ctx: &Ctx) -> Result<(), Refusal> {
    if !ctx.has_role(LIBRARY) {
        return Err(Refusal::new("only the library writes the library"));
    }
    Ok(())
}

/// A playlist's entry for a media row, if it is on it.
pub fn playlist_pos(db: &Db<Harken>, playlist_id: Id<Playlist>, media_id: Id<Media>) -> Option<i64> {
    db.playlist_item.get((playlist_id, media_id)).map(|i| i.pos)
}

/// The movement a song is, if it is one.
pub fn movement_of(db: &Db<Harken>, song: &Song) -> Option<Movement> {
    song.movement_id.as_ref().and_then(|id| db.movement.get(id))
}

/// Whether a text holds the needle, both lowercased.
pub fn has(text: &str, needle: &str) -> bool {
    needle.is_empty() || keys::lower(text).contains(&keys::lower(needle))
}

// * The mutations

/// Put a song in the library, and everything it belongs to as rows. An
/// upsert by `file`: a file the library has is the song it is, said again,
/// so the media row keeps its id, place, when and by whom it was added; the
/// same file with nothing new writes no row. What the song was part of and
/// no longer is — a recording, a movement, a work — goes when nothing else
/// is part of it.
#[mutator("library.add_song")]
pub fn add_song(ctx: &Ctx, db: &mut Db<Harken>, input: AddSong) -> Result<(), Refusal> {
    is_library(ctx)?;
    let (now, user) = (ctx.now, ctx.user().to_string());
    let was = db.media.by(Media::FILE).from(&input.file).first().filter(|m| !m.file.is_empty());
    let id = match &was {
        Some(m) => m.id,
        None => ctx.new_id(),
    };
    let newest = db.media.by(Media::POS).whole().last();
    db.media.put(Media {
        id,
        kind: was.as_ref().map_or("song".into(), |m| m.kind.clone()),
        title: input.title.clone(),
        creator: input.artist.clone(),
        duration_ms: input.duration_ms,
        file: input.file.clone(),
        pos: was.as_ref().map_or(newest.map_or(0, |m| m.pos) + 1, |m| m.pos),
        added_ms: was.as_ref().map_or(now, |m| m.added_ms),
        user_id: was.as_ref().map_or(user.clone(), |m| m.user_id.clone()),
    })?;
    if !input.album.is_empty() {
        let album = db.album.get(&input.album);
        let stale = album.as_ref().is_none_or(|a| !input.album_art.is_empty() && a.art != input.album_art);
        if stale {
            db.album.put(Album {
                name: input.album.clone(),
                label: album.as_ref().map_or(String::new(), |a| a.label.clone()),
                released: album.as_ref().map_or(0, |a| a.released),
                art: input.album_art.clone(),
                added_ms: now,
                user_id: user.clone(),
            })?;
        }
    }
    if !input.artist.is_empty() {
        let person = db.person.get(&input.artist);
        let stale = person.as_ref().is_none_or(|p| !input.artist_art.is_empty() && p.art != input.artist_art);
        if stale {
            db.person.put(Person {
                name: input.artist.clone(),
                sort_name: person.as_ref().map_or(String::new(), |p| p.sort_name.clone()),
                born: person.as_ref().map_or(0, |p| p.born),
                died: person.as_ref().map_or(0, |p| p.died),
                art: input.artist_art.clone(),
                added_ms: person.as_ref().map_or(now, |p| p.added_ms),
                user_id: person.as_ref().map_or(user.clone(), |p| p.user_id.clone()),
            })?;
        }
    }
    let title = keys::work_title(&input.work_title, &input.album, &input.catalogue, &input.part);
    let work_of = keys::work_id(&input.artist, &input.catalogue, &title);
    if work_of.is_some() {
        let key = keys::work_key(&input.artist, &input.catalogue, &title);
        let no = if input.movement_no == 0 { input.track } else { input.movement_no };
        let movement_key = keys::movement_key(&key, no);
        let work = db.work.get(&key);
        db.work.put(Work {
            id: key.clone(),
            composer: input.artist.clone(),
            title,
            catalogue: input.catalogue.clone(),
            opus: work.as_ref().map_or(String::new(), |w| w.opus.clone()),
            key_sig: work.as_ref().map_or(String::new(), |w| w.key_sig.clone()),
            form: work.as_ref().map_or(String::new(), |w| w.form.clone()),
            period: work.as_ref().map_or(String::new(), |w| w.period.clone()),
            composed: work.as_ref().map_or(0, |w| w.composed),
            art: work.as_ref().map_or(String::new(), |w| w.art.clone()),
            added_ms: work.as_ref().map_or(now, |w| w.added_ms),
            user_id: work.as_ref().map_or(user.clone(), |w| w.user_id.clone()),
        })?;
        let movement = db.movement.get(&movement_key);
        db.movement.put(Movement {
            id: movement_key,
            work_id: key,
            no,
            title: input.title.clone(),
            part: input.part.clone(),
            added_ms: movement.as_ref().map_or(now, |m| m.added_ms),
            user_id: movement.as_ref().map_or(user.clone(), |m| m.user_id.clone()),
        })?;
    }
    let work_of = work_of.as_deref();
    let recording_id = keys::recording_id(work_of, &input.album, &input.title, &input.artist, &input.performer);
    let credited = keys::credited_as(work_of, &input.artist, &input.performer);
    if !db.recording.has(&recording_id) {
        db.recording.insert(Recording {
            id: recording_id.clone(),
            work_id: work_of.map(String::from),
            recorded: 0,
            venue: String::new(),
            label: String::new(),
            licence: String::new(),
            art: String::new(),
            added_ms: now,
            user_id: user.clone(),
        })?;
    }
    if !credited.is_empty() {
        if !db.person.has(&credited) {
            db.person.insert(Person {
                name: credited.clone(),
                added_ms: now,
                user_id: user.clone(),
                ..Person::default()
            })?;
        }
        let role = if work_of.is_some() { "performer" } else { "artist" };
        if !db.credit.has((recording_id.clone(), credited.clone(), role.to_string())) {
            db.credit.insert(Credit {
                recording_id: recording_id.clone(),
                person_name: credited,
                role: role.into(),
                instrument: String::new(),
                pos: 0,
                added_ms: now,
                user_id: user.clone(),
            })?;
        }
    }
    let movement_id = keys::movement_id(work_of, input.movement_no, input.track);
    let previous = db.song.get(id);
    db.song.put(Song {
        media_id: id,
        album_name: (!input.album.is_empty()).then(|| input.album.clone()),
        disc: if input.disc > 0 { input.disc.min(99) } else { 1 },
        track: input.track.clamp(0, 999),
        recording_id: recording_id.clone(),
        movement_id: movement_id.clone(),
        bpm: if (20..=300).contains(&input.bpm) { input.bpm } else { 0 },
    })?;
    let left = previous.as_ref().map(|p| p.recording_id.clone()).filter(|was| *was != recording_id);
    if let Some(left) = left {
        if !db.song.scan().iter().any(|s| s.recording_id == left) {
            for c in db.credit.by(Credit::KEY).from(&left).all() {
                db.credit.remove(&c)?;
            }
            db.recording.delete(&left)?;
        }
    }
    let moved = previous.and_then(|p| p.movement_id).filter(|was| Some(was) != movement_id.as_ref());
    if let Some(moved) = moved {
        if !db.song.scan().iter().any(|s| s.movement_id.as_ref() == Some(&moved)) {
            let of = db.movement.get(&moved).map(|m| m.work_id).unwrap_or_default();
            db.movement.delete(&moved)?;
            let movements = db.movement.scan().iter().any(|m| m.work_id == of);
            let recordings = db.recording.scan().iter().any(|r| r.work_id.as_ref() == Some(&of));
            if !movements && !recordings {
                db.work.delete(&of)?;
            }
        }
    }
    Ok(())
}

/// Say more about a work. A work exists because a song named it, so an
/// unknown one is refused; every field is fill-if-given.
#[mutator("library.describe_work")]
pub fn describe_work(ctx: &Ctx, db: &mut Db<Harken>, input: DescribeWork) -> Result<(), Refusal> {
    is_library(ctx)?;
    let Some(work) = db.work.get(&input.id) else {
        return Err(Refusal(format!("no work {}; a work exists because a song named it", input.id)));
    };
    let or = |given: &str, was: &str| if given.is_empty() { was.to_string() } else { given.to_string() };
    db.work.update(Work {
        opus: or(&input.opus, &work.opus),
        key_sig: or(&input.key_sig, &work.key_sig),
        form: or(&input.form, &work.form),
        period: or(&input.period, &work.period),
        composed: if input.composed == 0 { work.composed } else { input.composed },
        art: or(&input.art, &work.art),
        added_ms: ctx.now,
        ..work
    })
}

/// Say more about a performance; refuses an unknown one.
#[mutator("library.describe_recording")]
pub fn describe_recording(ctx: &Ctx, db: &mut Db<Harken>, input: DescribeRecording) -> Result<(), Refusal> {
    is_library(ctx)?;
    let Some(r) = db.recording.get(&input.id) else {
        return Err(Refusal(format!("no recording {}; one exists because a song is part of it", input.id)));
    };
    let or = |given: &str, was: &str| if given.is_empty() { was.to_string() } else { given.to_string() };
    db.recording.update(Recording {
        recorded: if input.recorded == 0 { r.recorded } else { input.recorded },
        venue: or(&input.venue, &r.venue),
        label: or(&input.label, &r.label),
        licence: or(&input.licence, &r.licence),
        art: or(&input.art, &r.art),
        added_ms: ctx.now,
        ..r
    })
}

/// Say more about somebody, making the row when it is not there.
#[mutator("library.describe_person")]
pub fn describe_person(ctx: &Ctx, db: &mut Db<Harken>, input: DescribePerson) -> Result<(), Refusal> {
    is_library(ctx)?;
    let p = db.person.get(&input.name);
    let text = |given: &str, was: Option<&String>| {
        if given.is_empty() {
            was.cloned().unwrap_or_default()
        } else {
            given.to_string()
        }
    };
    let int = |given: i64, was: Option<i64>| if given == 0 { was.unwrap_or(0) } else { given };
    db.person.put(Person {
        name: input.name.clone(),
        sort_name: text(&input.sort_name, p.as_ref().map(|p| &p.sort_name)),
        born: int(input.born, p.as_ref().map(|p| p.born)),
        died: int(input.died, p.as_ref().map(|p| p.died)),
        art: text(&input.art, p.as_ref().map(|p| &p.art)),
        added_ms: p.as_ref().map_or(ctx.now, |p| p.added_ms),
        user_id: p.as_ref().map_or(ctx.user().to_string(), |p| p.user_id.clone()),
    })
}

/// Credit somebody on a recording, properly: keyed by the three, so a
/// person may hold two roles and a second entry is a correction. A real
/// credit is at least 1; 0 is the lumped fallback's.
#[mutator("library.credit_recording")]
pub fn credit_recording(ctx: &Ctx, db: &mut Db<Harken>, input: CreditRecording) -> Result<(), Refusal> {
    is_library(ctx)?;
    if !db.recording.has(&input.recording_id) {
        return Err(Refusal(format!("no recording {} to credit anybody on", input.recording_id)));
    }
    let (now, user) = (ctx.now, ctx.user().to_string());
    if !db.person.has(&input.person_name) {
        db.person.insert(Person {
            name: input.person_name.clone(),
            added_ms: now,
            user_id: user.clone(),
            ..Person::default()
        })?;
    }
    let role = if input.role.is_empty() {
        "artist".to_string()
    } else {
        input.role.clone()
    };
    let credit = db.credit.get((input.recording_id.clone(), input.person_name.clone(), role.clone()));
    db.credit.put(Credit {
        recording_id: input.recording_id,
        person_name: input.person_name,
        role,
        instrument: if input.instrument.is_empty() {
            credit.as_ref().map(|c| c.instrument.clone()).unwrap_or_default()
        } else {
            input.instrument
        },
        pos: if input.pos == 0 {
            credit.as_ref().map_or(0, |c| c.pos).max(1)
        } else {
            input.pos.max(1)
        },
        added_ms: now,
        user_id: user,
    })
}

/// Take something out of the library, and off every playlist holding it.
#[mutator("library.remove_media")]
pub fn remove_media(ctx: &Ctx, db: &mut Db<Harken>, input: RemoveMedia) -> Result<(), Refusal> {
    is_library(ctx)?;
    for item in db.playlist_item.scan().into_iter().filter(|i| i.media_id == input.id) {
        db.playlist_item.remove(&item)?;
    }
    db.song.delete(input.id)?;
    db.media.delete(input.id)
}
