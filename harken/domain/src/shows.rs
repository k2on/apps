//! The shows: podcasts and sermons. A show is a feed the server follows or
//! a folder of files under its media root, and each episode of it is a
//! `media` row — its kind the show's, its creator the show's title — beside
//! an `episode` row of what only an episode has, as a song is a `media` row
//! beside a `song`. So a playlist, the queue, a play and `/media` take an
//! episode as they take a song, and the music's lists leave it out.
//!
//! The server authors both, as the library: [`add_show`] for the show,
//! [`add_episode`] for each episode — found in the folder, or read off the
//! feed and its file still to come (`downloads.rs`).
//!
//! Which feeds it follows is the log's too: a [`Subscription`] each,
//! added from a client ([`subscribe`]), and the server reads every one
//! and says what it read ([`feed_read`]). [`fetch_older`] asks for more of
//! a feed's older episodes than it keeps, and [`unsubscribe`] takes the
//! feed, its show and its episodes out of the library.

use ark::prelude::*;

use crate::browse::Plan;
use crate::keys;
use crate::library::{is_library, library_entry, playlist_pos, take_out};
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

/// How many of a feed's newest episodes are downloaded when it is first
/// followed.
pub const KEEP: i64 = 20;

/// Whether a feed's URL is one the server can read: http or https. A
/// `&String`, as a refinement's check is handed the field.
#[allow(clippy::ptr_arg)]
pub fn is_feed(url: &String) -> bool {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"));
    rest.is_some_and(|r| !r.is_empty() && !r.contains(char::is_whitespace))
}

/// Follow a feed: the server reads it and downloads its newest [`KEEP`]
/// episodes. Said again, it is the same feed, its kind as said now and
/// the rest as it was.
#[mutator("library.subscribe")]
pub fn subscribe(ctx: &Ctx, db: &mut Db<Harken>, input: Subscribe) -> Result<(), Refusal> {
    is_library(ctx)?;
    let was = db.subscription.get(&input.url);
    let next = match was {
        Some(s) => Subscription { kind: input.kind, ..s },
        None => Subscription {
            url: input.url,
            kind: input.kind,
            keep: KEEP,
            added_ms: ctx.now,
            user_id: ctx.user().to_string(),
            ..Subscription::default()
        },
    };
    db.subscription.put(next)
}

/// Follow a feed no more: it, its show and every episode of it out of the
/// library, and off every playlist holding one.
#[mutator("library.unsubscribe")]
pub fn unsubscribe(ctx: &Ctx, db: &mut Db<Harken>, input: Unsubscribe) -> Result<(), Refusal> {
    is_library(ctx)?;
    if !db.subscription.has(&input.url) {
        return Err(Refusal(format!("not following {}", input.url)));
    }
    db.subscription.delete(input.url.clone())?;
    for show in db.show.scan().into_iter().filter(|s| s.feed == input.url) {
        for e in db.episode.by(Episode::SHOW_ID).from(&show.id).all() {
            take_out(db, e.media_id)?;
        }
        db.show.delete(show.id)?;
    }
    Ok(())
}

/// More of a followed feed's older episodes: `more` past what it keeps,
/// or 0 for every one. One that keeps every one already writes nothing.
#[mutator("library.fetch_older")]
pub fn fetch_older(ctx: &Ctx, db: &mut Db<Harken>, input: FetchOlder) -> Result<(), Refusal> {
    is_library(ctx)?;
    let Some(was) = db.subscription.get(&input.url) else {
        return Err(Refusal(format!("not following {}", input.url)));
    };
    let keep = match (was.keep, input.more) {
        (0, _) | (_, 0) => 0,
        (keep, more) => keep + more,
    };
    db.subscription.put(Subscription { keep, ..was })
}

/// What the server read of a followed feed: its show's title and how
/// many episodes it has, or why it could not be read — a title and a
/// count kept from the last read that had them. A feed not followed is
/// refused, so a read that finishes after an unsubscribe adds nothing.
#[mutator("library.feed_read")]
pub fn feed_read(ctx: &Ctx, db: &mut Db<Harken>, input: FeedRead) -> Result<(), Refusal> {
    is_library(ctx)?;
    let Some(was) = db.subscription.get(&input.url) else {
        return Err(Refusal(format!("not following {}", input.url)));
    };
    let read = input.error.is_empty();
    db.subscription.put(Subscription {
        title: if read { input.title } else { was.title.clone() },
        items: if read { input.items.max(0) } else { was.items },
        error: input.error,
        ..was
    })
}

/// Every followed feed, by when it was added, with its show where a read
/// has made it and how many of its episodes are in the library.
#[view("library.subscriptions")]
pub fn subscriptions(_: &Ctx, _: &Db<Harken>, _: ()) -> Result<Plan<SubscriptionsEntry>, Refusal> {
    Ok(scan::<Harken, Subscription>().sort(|s| (s.added_ms, s.url.clone())).map_with(|db, s| {
        let show = db.show.scan().into_iter().find(|show| show.feed == s.url);
        let episodes = show.as_ref().map_or(0, |show| db.episode.by(Episode::SHOW_ID).from(&show.id).count());
        SubscriptionsEntry {
            added_ms: s.added_ms,
            episodes: episodes as i64,
            error: s.error.clone(),
            items: s.items,
            keep: s.keep,
            kind: s.kind.clone(),
            show_id: show.map(|show| show.id).unwrap_or_default(),
            title: s.title.clone(),
            url: s.url.clone(),
        }
    }))
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

/// Shows written down as one JSON document: a show's fields and its
/// episodes', as a feed says them. What the demo's library has beside its
/// catalogue (`harken-iced`'s `shows.json`).
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct Shelf {
    pub shows: Vec<ShelfShow>,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct ShelfShow {
    pub kind: String,
    pub title: String,
    pub author: String,
    pub description: String,
    pub art: String,
    pub feed: String,
    pub link: String,
    pub episodes: Vec<ShelfEpisode>,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct ShelfEpisode {
    pub title: String,
    pub guid: String,
    pub published_ms: i64,
    pub duration_ms: i64,
    /// Where it plays from: a URL, or a path under the media root.
    pub file: String,
    pub description: String,
    pub art: String,
    pub season: i64,
    pub number: i64,
}

impl Shelf {
    pub fn parse(json: &str) -> Result<Shelf, String> {
        serde_json::from_str(json).map_err(|e| format!("the shows: {e}"))
    }

    /// Every show and then its episodes, as the mutations that put them in
    /// the library: in the order they run.
    pub fn calls(&self) -> Vec<(AddShow, Vec<AddEpisode>)> {
        self.shows
            .iter()
            .map(|s| {
                let id = keys::show_key(&s.kind, &s.title);
                let show = AddShow {
                    kind: s.kind.clone(),
                    title: s.title.clone(),
                    author: s.author.clone(),
                    description: s.description.clone(),
                    art: s.art.clone(),
                    feed: s.feed.clone(),
                    link: s.link.clone(),
                };
                let episodes = s
                    .episodes
                    .iter()
                    .map(|e| AddEpisode {
                        show: id.clone(),
                        title: e.title.clone(),
                        duration_ms: e.duration_ms,
                        file: e.file.clone(),
                        guid: e.guid.clone(),
                        published_ms: e.published_ms,
                        description: e.description.clone(),
                        source: e.file.clone(),
                        art: e.art.clone(),
                        season: e.season,
                        number: e.number,
                    })
                    .collect();
                (show, episodes)
            })
            .collect()
    }
}
