//! A replica, and the lists a window draws from it — every one of them an
//! `ark_client::View`, held open and told what moved.
//!
//! One of these exists for as long as the window does — signed in or not.
//! Whoever is signed in is the replica's business (`ark_client::Peer`
//! authors as them, or as nobody); what is drawn is this.
//!
//! **Nothing here reads a list again.** Every query is a plan and every plan
//! is maintained (`docs/plan-v4.md` §1.5), so the peer holds a small set of
//! open views keyed by query and arguments — the library, the sidebar's four
//! lists, the song details the table joins in, whatever the page shows, and
//! the playlists the picker asked about — and [`Peer::refresh`] hands each
//! of them the changes of one settle. What comes back is spliced into the
//! rows the screens hold, or taken whole on a reset (a rebase, §1.6; a
//! middleware outcome that moved, §1.7). A change costs each view the
//! entries it touched, so a playlist toggle no longer has to be told apart
//! from a song arriving to keep the sidebar cheap: the albums view is not
//! touched by a toggle and costs two index probes for it.
use std::collections::HashMap;

use ark_client::ark::eval::Ctx;
use ark_client::{args, Args, Changes, Id, Patch, Update, Value};
use harken_domain::view::Item;

use crate::places::{Place, Source};
use crate::rows::{self, Album, Artist, Composer, Playlist, Recording, TrackDetail, Work};

/// One line of the sidebar: somewhere the cursor can be and something it can
/// open.
#[derive(Debug, Clone)]
pub struct Choice {
    pub source: Source,
    pub label: String,
    /// The tally drawn on the right, where there is one to draw.
    pub count: Option<i64>,
}

/// No playlist: what the library is read against before there is one.
const NONE: Id = [0; 16];

/// One query held open: its name and arguments — the key a page's views are
/// kept or replaced by — and the engine's view of it.
struct Open {
    name: &'static str,
    args: Args,
    /// The view, or — while its middleware refuses (somebody else's
    /// playlist, nobody signed in) — who it refused, which is no rows.
    view: Result<ark_client::View, Ctx>,
}

impl Open {
    /// Hydrate once. A refusal is no rows rather than an error: a page that
    /// is not yours is an empty page, as the query's answer was.
    fn hydrate(client: &ark_client::Peer, name: &'static str, args: Args) -> Open {
        let view = client.view(name, args.clone()).map_err(|_| client.ctx().clone());
        Open { name, args, view }
    }

    fn is(&self, name: &str, args: &Args) -> bool {
        self.name == name && self.args == *args
    }

    /// The list as the view holds it.
    fn values(&self) -> &[Value] {
        match &self.view {
            Ok(v) => v.rows(),
            Err(_) => &[],
        }
    }

    /// Push one settle's changes through. A view that faults is read again
    /// whole, which is what a reset is; one refused at open is asked again
    /// when anything moved or somebody else is signed in — the query it
    /// stands for would have been.
    fn update(&mut self, client: &ark_client::Peer, changes: &Changes) -> Update {
        match &mut self.view {
            Ok(v) => v.update(client, changes).unwrap_or(Update::Reset),
            Err(who) => {
                let quiet = matches!(changes, Changes::Applied(chs) if chs.is_empty());
                if quiet && client.ctx() == who {
                    return Update::Unchanged;
                }
                *self = Open::hydrate(client, self.name, self.args.clone());
                match self.view {
                    Ok(_) => Update::Reset,
                    Err(_) => Update::Unchanged,
                }
            }
        }
    }

    /// Hydrate into `rows`, decoded by `read`.
    fn fill<T>(&self, rows: &mut Vec<T>, read: fn(&Value) -> T) {
        *rows = self.values().iter().map(read).collect();
    }

    /// [`Open::update`], onto the rows a screen holds: what moved, if
    /// anything did.
    fn refresh<T>(&mut self, client: &ark_client::Peer, changes: &Changes, rows: &mut Vec<T>, read: fn(&Value) -> T) -> Option<Moved> {
        match self.update(client, changes) {
            Update::Unchanged => None,
            Update::Patched(patches) => {
                splice(rows, &patches, read);
                Some(Moved::Patched(patches.len()))
            }
            Update::Reset => {
                self.fill(rows, read);
                Some(Moved::Reset)
            }
        }
    }
}

/// What one refresh did to one list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moved {
    /// This many patches, spliced.
    Patched(usize),
    /// Taken whole.
    Reset,
}

/// The patches, in order, onto the decoded list: `ark_client::splice` with
/// the decoding said rather than implied, because a row here is a type of
/// the domain's ([`Item`]) or of this crate's, and `Value` converts to
/// neither by `Into`.
fn splice<T>(rows: &mut Vec<T>, patches: &[Patch], read: fn(&Value) -> T) {
    for p in patches {
        match p {
            Patch::Insert { at, node } => {
                let at = (*at).min(rows.len());
                rows.insert(at, read(node));
            }
            Patch::Remove { at } => {
                if *at < rows.len() {
                    rows.remove(*at);
                }
            }
            Patch::Update { at, node } => {
                if *at < rows.len() {
                    rows[*at] = read(node);
                }
            }
        }
    }
}

/// The views the peer holds open. The first six live as long as the window;
/// the page's are whichever its source needs, opened when the source changes
/// and dropped with it; the picker's is the last track it was asked about.
struct Views {
    library: Open,
    playlists: Open,
    albums: Open,
    artists: Open,
    composers: Open,
    details: Open,
    /// The page's list of tracks (`playlist`, `album`, `artist`,
    /// `recording`).
    shown: Option<Open>,
    /// The page's works (`works`, or one `work` for a header).
    works: Option<Open>,
    /// The page's performances (`recordings`).
    recordings: Option<Open>,
    /// Which playlists one track is on (`playlists_of`).
    on: Option<Open>,
}

/// A page slot, brought to what the source wants: kept when it is already
/// that query with those arguments, opened when it is not, dropped (and its
/// rows cleared) when the page wants nothing there.
fn want<T>(
    client: &ark_client::Peer,
    slot: &mut Option<Open>,
    wanted: Option<(&'static str, Args)>,
    rows: &mut Vec<T>,
    read: fn(&Value) -> T,
) -> bool {
    match wanted {
        None => {
            let had = slot.take().is_some();
            rows.clear();
            had
        }
        Some((name, a)) if slot.as_ref().is_some_and(|o| o.is(name, &a)) => false,
        Some((name, a)) => {
            let open = Open::hydrate(client, name, a);
            open.fill(rows, read);
            *slot = Some(open);
            true
        }
    }
}

/// A page slot's settle, when the page has one there.
fn settle<T>(client: &ark_client::Peer, slot: &mut Option<Open>, changes: &Changes, rows: &mut Vec<T>, read: fn(&Value) -> T) -> Option<Moved> {
    slot.as_mut().and_then(|o| o.refresh(client, changes, rows, read))
}

/// A replica, and the lists a window draws from it.
pub struct Peer {
    pub client: ark_client::Peer,
    views: Views,
    /// What `view` draws of the library: decoded once per change, not per
    /// frame.
    pub items: Vec<Item>,
    /// The playlist the library is read against, which is what gives every
    /// row its `on_playlist`. The domain has no favorites of its own — a
    /// playlist named Favorites is just the first one. Zero when there is
    /// none, which is what a peer nobody is signed in on has.
    pub playlist: Id,
    /// The caller's playlists, in their order: the sidebar's last lines, and
    /// the picker's rows.
    playlists: Vec<Playlist>,
    /// What the sidebar picked, and the list that answers it.
    ///
    /// `Source::Library` is `items` itself. Every other list of tracks is the
    /// page's own view, opened when the source changes.
    pub source: Source,
    pub shown: Vec<Item>,
    /// Which album each track is on, and the rest of what only a song has: a
    /// map beside the list, because the library row is kind-neutral. Joined
    /// in memory while drawing.
    pub details: HashMap<Id, TrackDetail>,
    /// `track_details` in the view's order: what a patch's position means,
    /// so the map can be moved by the same patches.
    detail_rows: Vec<TrackDetail>,
    /// The sidebar, flat, in the order it is drawn. Headings are derived
    /// while drawing rather than stored, so "line 9" means the same to the
    /// keyboard and to the eye.
    pub choices: Vec<Choice>,
    /// The index pages' contents — and what a `#album/…` link resolves
    /// against, now that no sidebar line carries one.
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub composers: Vec<Composer>,
    /// What the current page shows when it is not a list of tracks: one
    /// composer's works, or one work's recordings (and that work's own row,
    /// for its header).
    pub works: Vec<Work>,
    pub recordings: Vec<Recording>,
    /// `playlists_of` for the track the picker was last opened on.
    on: Vec<Playlist>,
    /// What the last [`Peer::refresh`] moved, by query: the one place to
    /// see that a change cost the lists it touched and no others.
    pub moved: Vec<(&'static str, Moved)>,
    /// Bumped whenever a list that carries covers moved: the window asks for
    /// covers when this moves, which is the *data* changing — not the wire,
    /// which was wrong both ways (the demo has no wire; a real client's first
    /// library comes out of its own replica).
    pub art_gen: u64,
}

impl Peer {
    /// Every list the window starts on, hydrated once; everything after this
    /// is maintained.
    pub fn open(client: ark_client::Peer) -> Peer {
        let playlists = Open::hydrate(&client, "playlists", args([]));
        let mut rows = Vec::new();
        playlists.fill(&mut rows, Playlist::from_value);
        let playlist = rows.first().map_or(NONE, |p: &Playlist| p.id);
        let views = Views {
            library: Open::hydrate(&client, "library", args([("playlist_id", Value::Id(playlist))])),
            playlists,
            albums: Open::hydrate(&client, "albums", args([])),
            artists: Open::hydrate(&client, "artists", args([])),
            composers: Open::hydrate(&client, "composers", args([])),
            details: Open::hydrate(&client, "track_details", args([])),
            shown: None,
            works: None,
            recordings: None,
            on: None,
        };
        let mut peer = Peer {
            client,
            views,
            items: Vec::new(),
            playlist,
            playlists: rows,
            source: Source::Library,
            shown: Vec::new(),
            details: HashMap::new(),
            detail_rows: Vec::new(),
            choices: Vec::new(),
            albums: Vec::new(),
            artists: Vec::new(),
            composers: Vec::new(),
            works: Vec::new(),
            recordings: Vec::new(),
            on: Vec::new(),
            moved: Vec::new(),
            art_gen: 1,
        };
        let v = &peer.views;
        v.library.fill(&mut peer.items, Item::from_value);
        v.albums.fill(&mut peer.albums, Album::from_value);
        v.artists.fill(&mut peer.artists, Artist::from_value);
        v.composers.fill(&mut peer.composers, Composer::from_value);
        v.details.fill(&mut peer.detail_rows, TrackDetail::from_value);
        peer.details = peer.detail_rows.iter().map(|t| (t.media_id, t.clone())).collect();
        // What the hydrate already read is not news.
        let _ = peer.client.take_changes();
        peer.choose();
        peer
    }

    /// Make sure there is a playlist to read the library against: make
    /// "Favorites" if this replica has none.
    ///
    /// **When** is the window's question, not this one's (`App::default_playlist`):
    /// signed in, not before the log has been heard, because a later playlist
    /// of the same name comes back renamed — "Favorites (1)" — and every
    /// device making its own on its first run would leave a person one per
    /// device. A refusal is not worth a status line: it only means there is
    /// no playlist yet.
    pub fn ensure_playlist(&mut self) {
        if self.playlists.is_empty() && self.client.mutate("create_playlist", args([("name", Value::text("Favorites"))])).is_ok() {
            self.refresh();
        }
    }

    /// Hand what this peer did alone to the server at `url`
    /// (`ark_client::Peer::join`, `docs/plan-alone.md` §4), in place: the
    /// replica rolls back to its fork and re-queues its local history, and
    /// every open list is told that as changes — patched, not rebuilt, so
    /// the library does not blink. With no login the history stays
    /// nobody's until a sign-in makes it the signer's.
    pub fn join(&mut self, url: &str, login: Option<ark_client::Login>) -> Result<(), ark_client::Error> {
        self.client.join(url, login)?;
        self.refresh();
        Ok(())
    }

    /// Bring every list up to date with whatever just happened. `true` when
    /// any of them moved.
    ///
    /// Nothing is read again here: each open view is handed the settle's
    /// changes and answers with what it moved. A rebase is changes too: the
    /// inverse of what it undid, what landed, what it re-applied
    /// (`docs/plan-perf.md` R2). `Rebuilt` is a store replaced whole — a
    /// snapshot adopted from the server, below its horizon or past its head
    /// (R6) — and resets every view, which is the one thing no list of
    /// changes describes (§1.6). The views are asked even when
    /// nothing changed, because a view also answers to who is signed in
    /// (§1.7); that costs a comparison each.
    pub fn refresh(&mut self) -> bool {
        let changes = self.client.take_changes();
        let (c, v) = (&self.client, &mut self.views);
        let mut moved = Vec::new();
        let mut note = |name: &'static str, m: Option<Moved>| match m {
            Some(m) => {
                moved.push((name, m));
                true
            }
            None => false,
        };
        let lists = note("playlists", v.playlists.refresh(c, &changes, &mut self.playlists, Playlist::from_value));
        let library = note("library", v.library.refresh(c, &changes, &mut self.items, Item::from_value));
        let albums = note("albums", v.albums.refresh(c, &changes, &mut self.albums, Album::from_value));
        let artists = note("artists", v.artists.refresh(c, &changes, &mut self.artists, Artist::from_value));
        let composers = note("composers", v.composers.refresh(c, &changes, &mut self.composers, Composer::from_value));
        let details = match v.details.update(c, &changes) {
            Update::Unchanged => None,
            Update::Patched(patches) => {
                patch_details(&mut self.detail_rows, &mut self.details, &patches);
                Some(Moved::Patched(patches.len()))
            }
            Update::Reset => {
                v.details.fill(&mut self.detail_rows, TrackDetail::from_value);
                self.details = self.detail_rows.iter().map(|t| (t.media_id, t.clone())).collect();
                Some(Moved::Reset)
            }
        };
        let details = note("track_details", details);
        let name = |slot: &Option<Open>| slot.as_ref().map_or("", |o| o.name);
        let shown = note(name(&v.shown), settle(c, &mut v.shown, &changes, &mut self.shown, Item::from_value));
        let works = note(name(&v.works), settle(c, &mut v.works, &changes, &mut self.works, Work::from_value));
        let recordings = note(
            name(&v.recordings),
            settle(c, &mut v.recordings, &changes, &mut self.recordings, Recording::from_value),
        );
        let on = note(name(&v.on), settle(c, &mut v.on, &changes, &mut self.on, Playlist::from_value));
        self.moved = moved;

        // The playlist the library is read against can stop existing, and
        // routinely does: the default this peer made on its first run is
        // dropped on the rebase when another device's turns out to have been
        // first. Re-pointing costs a hydrate of the library and of a page
        // read against it, so only when it moved.
        let first = self.playlists.first().map_or(NONE, |p| p.id);
        let repointed = !self.playlists.iter().any(|p| p.id == self.playlist) && self.playlist != first;
        if repointed {
            self.playlist = first;
            self.views.library = Open::hydrate(&self.client, "library", args([("playlist_id", Value::Id(first))]));
            self.views.library.fill(&mut self.items, Item::from_value);
            self.open_page();
        }
        if lists || library || albums || artists || composers || repointed {
            self.choose();
        }
        if albums || artists || composers || works {
            self.art_gen = self.art_gen.wrapping_add(1);
        }
        if recordings {
            self.label_recording();
        }
        lists || library || albums || artists || composers || details || shown || works || recordings || on || repointed
    }

    /// The sidebar, from the lists it counts. Grouping is the domain's, so
    /// every client folds the library the same way.
    fn choose(&mut self) {
        // Music, then the lists somebody made. **A line is drawn only when it
        // has rows behind it**, which is what lets one schema serve every
        // genre: a pop library has no works, so no Composers line. Songs is
        // unconditional because it is the library.
        let mut choices = vec![Choice {
            source: Source::Library,
            label: "Songs".into(),
            count: Some(self.items.len() as i64),
        }];
        for (source, label, count) in [
            (Source::Albums, "Albums", self.albums.len()),
            (Source::Artists, "Artists", self.artists.len()),
            (Source::Composers, "Composers", self.composers.len()),
        ] {
            if count > 0 {
                choices.push(Choice {
                    source,
                    label: label.into(),
                    count: Some(count as i64),
                });
            }
        }
        choices.extend(self.playlists.iter().map(|p| Choice {
            source: Source::Playlist(p.id, p.name.clone()),
            label: p.name.clone(),
            count: None,
        }));
        self.choices = choices;
    }

    /// The caller's playlists, as the sidebar holds them.
    pub fn playlists(&self) -> &[Playlist] {
        &self.playlists
    }

    /// Which of the caller's playlists a track is on: a view kept open on
    /// the last track asked about, so the picker reopened on the same track —
    /// after it made a playlist — reads nothing.
    pub fn playlists_of(&mut self, media: Id) -> Vec<Playlist> {
        let wanted = Some(("playlists_of", args([("media_id", Value::Id(media))])));
        want(&self.client, &mut self.views.on, wanted, &mut self.on, Playlist::from_value);
        self.on.clone()
    }

    /// Run a query once and read its rows; a refusal (a guard, nobody signed
    /// in) is no rows. For what is not a list on screen: resolving a link,
    /// and the tests, which hold the maintained lists to what the query
    /// answers.
    pub(crate) fn ask<T>(&self, name: &str, a: Args, f: impl Fn(&Value) -> T) -> Vec<T> {
        match self.client.query(name, &a) {
            Ok(v) => rows::list(&v, f),
            Err(_) => Vec::new(),
        }
    }

    pub fn work(&self, id: &str) -> Vec<Work> {
        self.ask("work", args([("id", Value::text(id))]), Work::from_value)
    }

    /// What a page's query answers, read once. The page holds views; these
    /// are what the tests compare them with.
    #[cfg(test)]
    pub fn works_of(&self, composer: &str) -> Vec<Work> {
        self.ask("works", args([("composer", Value::text(composer))]), Work::from_value)
    }

    #[cfg(test)]
    pub fn recordings_of(&self, work: &str) -> Vec<Recording> {
        self.ask("recordings", args([("work_id", Value::text(work))]), Recording::from_value)
    }

    #[cfg(test)]
    pub fn credits(&self, recording: &str) -> Vec<rows::Credit> {
        self.ask("credits", args([("recording_id", Value::text(recording))]), rows::Credit::from_value)
    }

    #[cfg(test)]
    pub fn album_rows(&self, name: &str) -> Vec<Item> {
        self.ask(
            "album",
            args([("playlist_id", Value::Id(self.playlist)), ("name", Value::text(name))]),
            Item::from_value,
        )
    }

    #[cfg(test)]
    pub fn track_details(&self) -> Vec<TrackDetail> {
        self.ask("track_details", args([]), TrackDetail::from_value)
    }

    /// What is true of a track as a song, or nothing if its kind has none.
    pub fn detail_of(&self, id: Id) -> TrackDetail {
        self.details.get(&id).cloned().unwrap_or(TrackDetail {
            media_id: id,
            ..TrackDetail::default()
        })
    }

    /// Open the views the page needs for whatever the sidebar picked, and
    /// drop the last page's. A view the new page shares with the old one —
    /// the same query with the same arguments, a work's recordings from its
    /// work page to one of them — is kept rather than read again.
    pub fn open_page(&mut self) {
        let text = |s: &str| Value::text(s);
        let on = |key: &str, value: &str| args([("playlist_id", Value::Id(self.playlist)), (key, text(value))]);
        let (shown, works, recordings): (Option<(&'static str, Args)>, _, _) = match &self.source {
            Source::Library | Source::Albums | Source::Artists | Source::Composers => (None, None, None),
            Source::Works(composer) => (None, Some(("works", args([("composer", text(composer))]))), None),
            // Both halves: the work itself, for the header, and its
            // performances. This page may have been reached without opening a
            // composer at all — a link somebody sent — so the row is read by
            // key rather than looked up in a composer's list.
            Source::Work(id, _) => (
                None,
                Some(("work", args([("id", text(id))]))),
                Some(("recordings", args([("work_id", text(id))]))),
            ),
            Source::Playlist(id, _) => (Some(("playlist", args([("playlist_id", Value::Id(*id))]))), None, None),
            Source::Album(name) => (Some(("album", on("name", name))), None, None),
            Source::Artist(name) => (Some(("artist", on("name", name))), None, None),
            // The one list ordered by the *work* rather than the release, and
            // every recording of that work, so the header can find this one —
            // the work's key is everything before the `@`.
            Source::Recording(id, _) => match id.split_once('@') {
                Some((work, _)) => (
                    Some(("recording", on("id", id))),
                    Some(("work", args([("id", text(work))]))),
                    Some(("recordings", args([("work_id", text(work))]))),
                ),
                None => (Some(("recording", on("id", id))), None, None),
            },
        };
        let (c, v) = (&self.client, &mut self.views);
        want(c, &mut v.shown, shown, &mut self.shown, Item::from_value);
        let works = want(c, &mut v.works, works, &mut self.works, Work::from_value);
        want(c, &mut v.recordings, recordings, &mut self.recordings, Recording::from_value);
        if works {
            self.art_gen = self.art_gen.wrapping_add(1);
        }
        self.label_recording();
    }

    /// A link arriving cold carries a recording's key and no label: the row
    /// knows who played it, and this is where the two meet.
    fn label_recording(&mut self) {
        if let Source::Recording(id, who) = &mut self.source {
            if who.is_empty() {
                if let Some(take) = self.recordings.iter().find(|r| r.id == *id) {
                    who.clone_from(&take.performers);
                }
            }
        }
    }

    /// A work's title by its key, or `None` if this peer has no such work. A
    /// read rather than a lookup in `works`, which is one composer's: a
    /// `#work/…` link can arrive before any composer has been opened.
    pub fn work_named(&self, id: &str) -> Option<String> {
        self.work(id).into_iter().next().map(|w| w.title)
    }

    /// The source a place names, against what this peer actually has. A link
    /// to a playlist since renamed, or to an album not received yet, lands on
    /// the library — a page, rather than a heading with nothing under it.
    pub fn source_of(&self, place: &Place) -> Source {
        match place {
            Place::Album(name) => match self.albums.iter().any(|a| a.name == *name) {
                true => Source::Album(name.clone()),
                false => Source::Library,
            },
            Place::Artist(name) => match self.artists.iter().any(|a| a.name == *name) {
                true => Source::Artist(name.clone()),
                false => Source::Library,
            },
            Place::Composer(name) => match self.composers.iter().any(|c| c.name == *name) {
                true => Source::Works(name.clone()),
                false => Source::Library,
            },
            // By *querying*, not against a list this peer holds: a link
            // arriving cold has no page yet, and the key is enough to ask with.
            Place::Work(id) => match self.work_named(id) {
                Some(title) => Source::Work(id.clone(), title),
                None => Source::Library,
            },
            Place::Recording(id) => match id.split_once('@').and_then(|(work, _)| self.work_named(work)) {
                Some(_) => Source::Recording(id.clone(), String::new()),
                None => Source::Library,
            },
            _ => self
                .choices
                .iter()
                .find(|c| c.source.place() == *place)
                .map(|c| c.source.clone())
                .unwrap_or(Source::Library),
        }
    }

    /// What the table is showing, whichever side it came from.
    pub fn rows(&self) -> &[Item] {
        match self.source {
            Source::Library => &self.items,
            _ => &self.shown,
        }
    }

    /// Every album this artist made something on: `Album::creator` is whoever
    /// made its first track, the same fact an artist page selects on.
    pub fn albums_by(&self, artist: &str) -> Vec<&Album> {
        self.albums.iter().filter(|a| a.creator == artist).collect()
    }
}

/// The patches onto `track_details` as a list and as the map the table
/// reads: a track's detail is found by its media id while drawing, and a
/// patch names a position — so the list is kept beside the map to say which
/// row a position is.
fn patch_details(rows: &mut Vec<TrackDetail>, map: &mut HashMap<Id, TrackDetail>, patches: &[Patch]) {
    for p in patches {
        match p {
            Patch::Insert { at, node } => {
                let d = TrackDetail::from_value(node);
                map.insert(d.media_id, d.clone());
                rows.insert((*at).min(rows.len()), d);
            }
            Patch::Remove { at } => {
                if *at < rows.len() {
                    let d = rows.remove(*at);
                    map.remove(&d.media_id);
                }
            }
            Patch::Update { at, node } => {
                if *at < rows.len() {
                    let d = TrackDetail::from_value(node);
                    map.remove(&rows[*at].media_id);
                    map.insert(d.media_id, d.clone());
                    rows[*at] = d;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_client::{Domain, Options};

    fn song(title: &str, artist: &str, album: &str, file: &str) -> Args {
        let t = |s: &str| Value::text(s);
        args([
            ("title", t(title)),
            ("artist", t(artist)),
            ("album", t(album)),
            ("duration_ms", Value::Int(1000)),
            ("file", t(file)),
            ("track", Value::Int(0)),
            ("part", t("")),
            ("catalogue", t("")),
            ("performer", t("")),
            ("bpm", Value::Int(0)),
            ("album_art", t("")),
            ("artist_art", t("")),
            ("disc", Value::Int(0)),
            ("work_title", t("")),
            ("movement_no", Value::Int(0)),
        ])
    }

    /// The maintained list is the query, after every step — one change, a
    /// batch of them, and a playlist toggle that moves `playlist_pos`.
    ///
    /// Falsified by dropping the `Update` arm of `splice`: the toggle — which
    /// arrives as an update to the row it moved — leaves the old
    /// `playlist_pos`. A batch is pushed against the final store, once per
    /// touched entry (`docs/plan-v4.md` §1.5).
    #[test]
    fn the_maintained_library_is_the_query() {
        let domain = Domain::new(&harken_domain::module());
        let client = ark_client::Peer::open_memory(domain, Options::alone("me")).unwrap();
        let mut peer = Peer::open(client);
        peer.ensure_playlist();
        assert_ne!(peer.playlist, [0; 16], "Favorites was made");
        let read = |p: &Peer| p.ask("library", args([("playlist_id", Value::Id(p.playlist))]), Item::from_value);

        peer.client.mutate("add_song", song("Air", "Bach", "Suites", "a")).unwrap();
        assert!(peer.refresh());
        assert_eq!(peer.items, read(&peer));

        for (t, f) in [("Glue", "b"), ("Opal", "c"), ("Gosh", "d")] {
            peer.client.mutate("add_song", song(t, "X", "Y", f)).unwrap();
        }
        peer.refresh();
        assert_eq!(peer.items, read(&peer), "a batch, each change against its own store");

        let id = peer.items[2].id;
        peer.client
            .mutate(
                "add_to_playlist",
                args([("playlist_id", Value::Id(peer.playlist)), ("media_id", Value::Id(id))]),
            )
            .unwrap();
        peer.refresh();
        assert_eq!(peer.items, read(&peer));
        assert!(peer.items[2].on_playlist());
        assert!(!peer.refresh(), "nothing moved, nothing read");

        // A song and its place on the playlist in one batch: the song's entry
        // is built once, against the final store, already carrying its place.
        peer.client.mutate("add_song", song("Late", "Z", "", "e")).unwrap();
        let late = peer.client.query("library", &args([("playlist_id", Value::Id(peer.playlist))])).unwrap();
        let late = rows::list(&late, Item::from_value).last().unwrap().id;
        peer.client
            .mutate(
                "add_to_playlist",
                args([("playlist_id", Value::Id(peer.playlist)), ("media_id", Value::Id(late))]),
            )
            .unwrap();
        peer.refresh();
        assert_eq!(peer.items, read(&peer), "a song and its playlist entry in one batch");
    }

    /// `docs/plan-alone.md` §4: a window alone joins a server in place. The
    /// replica is opened alone as nobody, as the desktop opens it, a
    /// playlist and songs are made and one put on it, and the join hands
    /// all of it to an in-process hub: no open list is taken whole through
    /// the join — as nobody to nobody, the rebase undoes and re-applies the
    /// same rows, and the lists are told nothing moved — and each is the
    /// query after it; a
    /// sign-in then makes it the signer's, and once the hub has taken it
    /// all, the lists are patched again and still the query.
    ///
    /// Falsified by `ark_client::Peer::join` replacing the replica whole
    /// (`Replica::open` at the fork with the history pending, instead of
    /// `Replica::fork_back`): every list is `Reset`.
    #[test]
    fn a_window_alone_joins_a_server_in_place() {
        use ark_client::ark::live::Silent;
        use ark_client::ark::peer::Authority;
        use ark_client::ark::protocol::{open_access, trusting, Server};
        let domain = Domain::new(&harken_domain::module());
        let client = ark_client::Peer::open_memory(domain.clone(), Options::alone_as_nobody()).unwrap();
        let mut peer = Peer::open(client);
        peer.ensure_playlist();
        for (t, f) in [("Air", "a"), ("Glue", "b"), ("Opal", "c")] {
            peer.client.mutate("add_song", song(t, "Bach", "Suites", f)).unwrap();
        }
        peer.refresh();
        let id = peer.items[1].id;
        peer.client
            .mutate(
                "add_to_playlist",
                args([("playlist_id", Value::Id(peer.playlist)), ("media_id", Value::Id(id))]),
            )
            .unwrap();
        peer.refresh();
        assert_eq!(peer.client.status().link, "alone");
        let read = |p: &Peer| p.ask("library", args([("playlist_id", Value::Id(p.playlist))]), Item::from_value);
        let before = peer.items.clone();

        peer.join("ws://hub/sync", None).unwrap();
        assert!(
            peer.moved.iter().all(|(_, m)| matches!(m, Moved::Patched(_))),
            "patched, not reset: {:?}",
            peer.moved
        );
        assert_eq!(peer.items, before, "the library does not blink");
        assert_eq!(peer.items, read(&peer));
        assert_eq!(peer.client.pending_len(), 5);

        let mut a = Authority::new(domain.module().schema.clone(), domain.closures().clone());
        a.hold(domain.native_list());
        let mut hub = Server::open(trusting(), open_access(), Silent, a);
        peer.client.sign_in("alice", "dev", Some("alice".into())).unwrap();
        peer.refresh();
        peer.client.connected();
        loop {
            let up = peer.client.take_outgoing();
            for m in up.iter().cloned() {
                hub.recv(1, m);
            }
            let down = hub.take_outgoing();
            if up.is_empty() && down.is_empty() {
                break;
            }
            for (_, m) in down {
                peer.client.recv(m);
            }
        }
        peer.refresh();
        assert!(peer.moved.iter().all(|(_, m)| matches!(m, Moved::Patched(_))), "{:?}", peer.moved);
        assert_eq!(hub.authority.log.head_seq(), 5);
        assert_eq!(peer.client.pending_len(), 0);
        assert_eq!(peer.items, read(&peer));
        assert!(peer.items[1].on_playlist());
    }

    /// The playlist the library is read against can go — here a rebase
    /// takes back the one this peer made, with another still pending — and
    /// the library is re-pointed at the first that is left: a view with new
    /// arguments, hydrated, and an album page read against the old one
    /// reopened against the new. The rest of the sidebar is patched, not
    /// re-read.
    ///
    /// Falsified by re-pointing only a peer that had no playlist
    /// (`self.playlist == NONE &&` in `refresh`): `playlist` stays on the one
    /// that went and "re-pointed at what is left" fails; and by not reopening
    /// the page on a re-point: "…and so is the page" fails.
    #[test]
    fn a_playlist_that_goes_repoints_the_library() {
        use ark_client::ark::protocol::ServerMsg;
        let domain = Domain::new(&harken_domain::module());
        let client = ark_client::Peer::open_memory(domain, Options::dev("me")).unwrap();
        let mut peer = Peer::open(client);
        peer.client.mutate("add_song", song("Air", "Bach", "Suites", "a")).unwrap();
        let made = peer.client.mutate("create_playlist", args([("name", Value::text("Mine"))])).unwrap();
        peer.client.mutate("create_playlist", args([("name", Value::text("Other"))])).unwrap();
        peer.refresh();
        let [mine, other] = [0, 1].map(|i| peer.playlists()[i].id);
        assert_eq!(peer.playlist, mine, "the first is read against");
        peer.client
            .mutate(
                "add_to_playlist",
                args([("playlist_id", Value::Id(other)), ("media_id", Value::Id(peer.items[0].id))]),
            )
            .unwrap();
        peer.refresh();
        assert!(!peer.items[0].on_playlist(), "on Other, not on Mine");
        peer.source = Source::Album("Suites".into());
        peer.open_page();

        peer.client.recv(ServerMsg::Reject {
            id: made,
            reason: "no".into(),
        });
        peer.refresh();
        assert_eq!(peer.playlist, other, "re-pointed at what is left");
        assert!(peer.items[0].on_playlist(), "the library is read against Other now");
        assert_eq!(
            peer.items,
            peer.ask("library", args([("playlist_id", Value::Id(other))]), Item::from_value)
        );
        assert!(peer.shown[0].on_playlist(), "…and so is the page");
        let lines: Vec<&str> = peer.choices.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(lines, ["Songs", "Albums", "Artists", "Other"]);
    }
}
