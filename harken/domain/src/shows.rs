//! The shows: podcasts and sermons. A show is a feed the server follows or
//! a folder of files under its media root, and each episode of it is a
//! `media` row — its kind the show's, its creator the show's title — beside
//! an `episode` row of what only an episode has, as a song is a `media` row
//! beside a `song`. So a playlist, the queue, a play and `/media` take an
//! episode as they take a song, and the music's lists leave it out.
//!
//! The server authors both, as the library: [`add_show`] for the show,
//! [`add_episode`] for each episode once its file is here — downloaded
//! from the feed, or found in the folder.

use ark::prelude::*;

use crate::browse::Plan;
use crate::keys;
use crate::library::{is_library, library_entry, playlist_pos};
use crate::schema::*;
use crate::types::*;

/// What a show can be.
pub const KINDS: &[&str] = &["podcast", "sermon"];

/// Whether a kind is a show's. A `&String`, as a refinement's check is
/// handed the field.
#[allow(clippy::ptr_arg)]
pub fn is_kind(kind: &String) -> bool {
    KINDS.contains(&kind.as_str())
}

/// Put a show in the library, or say more about it: keyed by its kind and
/// its title, every field fill-if-given, and one that says nothing new
/// writes nothing (a feed says it again at every refresh).
#[mutator("library.add_show")]
pub fn add_show(ctx: &Ctx, db: &mut Db<Harken>, input: AddShow) -> Result<(), Refusal> {
    is_library(ctx)?;
    let id = keys::show_key(&input.kind, &input.title);
    let was = db.show.get(&id);
    let or = |given: &str, was: Option<&String>| match given.is_empty() {
        true => was.cloned().unwrap_or_default(),
        false => given.to_string(),
    };
    let next = Show {
        id: id.clone(),
        kind: input.kind,
        title: input.title,
        author: or(&input.author, was.as_ref().map(|s| &s.author)),
        description: or(&input.description, was.as_ref().map(|s| &s.description)),
        art: or(&input.art, was.as_ref().map(|s| &s.art)),
        feed: or(&input.feed, was.as_ref().map(|s| &s.feed)),
        link: or(&input.link, was.as_ref().map(|s| &s.link)),
        added_ms: was.as_ref().map_or(ctx.now, |s| s.added_ms),
        user_id: was.as_ref().map_or(ctx.user().to_string(), |s| s.user_id.clone()),
    };
    db.show.put(next)
}

/// Put an episode in the library, its file here. An upsert by `file`, as
/// `add_song` is: a file the library has keeps its id, place, when and by
/// whom it was added, and the same file with nothing new writes no row. An
/// episode of a show the library has not got is refused.
#[mutator("library.add_episode")]
pub fn add_episode(ctx: &Ctx, db: &mut Db<Harken>, input: AddEpisode) -> Result<(), Refusal> {
    is_library(ctx)?;
    let Some(show) = db.show.get(&input.show) else {
        return Err(Refusal(format!("no show {}; add the show first", input.show)));
    };
    let (now, user) = (ctx.now, ctx.user().to_string());
    let was = db.media.by(Media::FILE).from(&input.file).first().filter(|m| !m.file.is_empty());
    let id = match &was {
        Some(m) => m.id,
        None => ctx.new_id(),
    };
    let newest = db.media.by(Media::POS).whole().last();
    db.media.put(Media {
        id,
        kind: show.kind.clone(),
        title: input.title,
        creator: show.title.clone(),
        duration_ms: input.duration_ms.max(0),
        file: input.file,
        pos: was.as_ref().map_or(newest.map_or(0, |m| m.pos) + 1, |m| m.pos),
        added_ms: was.as_ref().map_or(now, |m| m.added_ms),
        user_id: was.as_ref().map_or(user, |m| m.user_id.clone()),
    })?;
    db.episode.put(Episode {
        media_id: id,
        show_id: show.id,
        guid: input.guid,
        published_ms: input.published_ms.max(0),
        description: input.description,
        source: input.source,
        art: input.art,
        season: input.season.max(0),
        number: input.number.max(0),
    })
}

/// Every show, by title, with how many episodes it has here and when the
/// newest went out.
#[view("library.shows")]
pub fn shows(_: &Ctx, _: &Db<Harken>, _: ()) -> Result<Plan<ShowsEntry>, Refusal> {
    Ok(scan::<Harken, Show>()
        .map_with(|db, s| {
            let episodes: Vec<Episode> = db.episode.by(Episode::SHOW_ID).from(&s.id).all();
            let here: Vec<&Episode> = episodes.iter().filter(|e| db.media.has(e.media_id)).collect();
            ShowsEntry {
                art: s.art.clone(),
                author: s.author.clone(),
                description: s.description.clone(),
                episodes: here.len() as i64,
                feed: s.feed.clone(),
                id: s.id.clone(),
                kind: s.kind.clone(),
                link: s.link.clone(),
                newest_ms: here.iter().map(|e| e.published_ms).max().unwrap_or(0),
                title: s.title.clone(),
            }
        })
        .sort(|s| (keys::lower(&s.title), s.id.clone())))
}

/// One show's episodes, the newest first: by when each went out, then by
/// when it was added.
#[view("library.show")]
pub fn show(_: &Ctx, _: &Db<Harken>, input: ShowInput) -> Result<Plan<LibraryEntry>, Refusal> {
    let (on, id) = (input.playlist_id, input.id);
    Ok(scan::<Harken, Episode>()
        .filter_with(move |db, e| e.show_id == id && db.media.has(e.media_id))
        .sort_with(|db, e| {
            let pos = db.media.get(e.media_id).map_or(0, |m| m.pos);
            (Desc(e.published_ms), Desc(pos))
        })
        .map_with(move |db, e| library_entry(&db.media.get(e.media_id).expect("an episode's media"), playlist_pos(db, on, e.media_id))))
}

/// Every episode, with the columns only an episode has.
#[view("library.episode_details")]
pub fn episode_details(_: &Ctx, _: &Db<Harken>, _: ()) -> Result<Plan<EpisodeDetailsEntry>, Refusal> {
    Ok(scan::<Harken, Episode>().map_with(|db, e| EpisodeDetailsEntry {
        art: e.art.clone(),
        description: e.description.clone(),
        media_id: e.media_id,
        number: e.number,
        published_ms: e.published_ms,
        season: e.season,
        show: db.show.get(&e.show_id).map(|s| s.title).unwrap_or_default(),
        show_id: e.show_id.clone(),
    }))
}
