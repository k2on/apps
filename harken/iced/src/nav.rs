//! Everything on a page that a click opens, as the keyboard reaches it.
//!
//! A page's main list — its tracks, its cards, its works — is what the
//! cursor walked from the start, numbered `0..` as it always was. Around it
//! a page now draws more that a click opens: a header's names, a Play
//! button, a work's heading between its movements, the performer under a
//! track, a shelf of records, the people on a recording. **Every one of
//! those is a cell too**, numbered after the main list and placed where it
//! is drawn ([`vim::Page`]), so `j`, `k`, `h` and `l` reach whatever the
//! pointer can, and `<Enter>` does what its click does.
//!
//! The view asks [`Layout::cell`] which number a thing has, to draw the
//! cursor on it and to say where the pointer is; the keyboard asks
//! [`Layout::message`] what opening it means. One description of the page
//! for both, so they cannot drift apart.
use arkui::vim::{self, Page, Section};

use crate::places::Source;
use crate::rows::TrackDetail;
use crate::{App, Message};

/// A thing on a page that is not its main list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// The line over a header's title: whose it is.
    Over,
    /// The `n`th name in a header's accent line: who played it.
    By(usize),
    /// A header's Play button.
    Play,
    /// What a page sets apart — a work's most complete recording, a
    /// composer's most recorded work — and its Play.
    Featured,
    FeaturedPlay,
    /// A composer's "See All" under their best known works.
    SeeAll,
    /// The composer, and the work, of the heading over the run that starts
    /// at row `n`.
    Composer(usize),
    Work(usize),
    /// The `k`th name under row `n`.
    Performer(usize, usize),
    /// The `n`th record on a shelf.
    Album(usize),
    /// The `n`th face on a recording or an album.
    Person(usize),
    /// A shows page's +, which opens the search for a show to follow — or,
    /// open, shuts it.
    Add,
    /// The search box.
    Query,
    /// The Unfollow of the `n`th feed the library follows
    /// (`Peer::subscriptions`).
    Unfollow(usize),
    /// A followed show's older episodes fetched: twenty more (`false`), or
    /// every one.
    Older(bool),
}

/// A page, as the keyboard walks it.
#[derive(Debug, Default)]
pub struct Layout {
    pub page: Page,
    /// What each cell past the main list opens: `marks[k]` is cell
    /// `page.main + k`.
    pub marks: Vec<(Mark, Message)>,
}

impl Layout {
    /// The number `mark` has on this page, if it is on it.
    pub fn cell(&self, mark: Mark) -> Option<usize> {
        self.marks.iter().position(|(m, _)| *m == mark).map(|k| self.page.main + k)
    }

    /// What opening cell `at` does, when it is not one of the main list's.
    pub fn message(&self, at: usize) -> Option<&Message> {
        at.checked_sub(self.page.main).and_then(|k| self.marks.get(k)).map(|(_, m)| m)
    }

    /// Give `mark` the next number, opening `go`.
    fn mark(&mut self, mark: Mark, go: Message) -> usize {
        self.marks.push((mark, go));
        self.page.main + self.marks.len() - 1
    }

    /// A block of cells, `columns` across, where it is drawn.
    fn block(&mut self, ids: Vec<usize>, columns: usize) {
        if !ids.is_empty() {
            self.page.sections.push(Section { ids, columns });
        }
    }

    /// One cell on a line of its own.
    fn line(&mut self, mark: Mark, go: Option<Source>) {
        if let Some(s) = go {
            let id = self.mark(mark, Message::Select(s));
            self.block(vec![id], 1);
        }
    }
}

/// The names in a credit line, as it is drawn: "Hogwood, Academy of
/// Ancient Music" is two people, each a link.
pub fn names(line: &str) -> Vec<&str> {
    line.split(", ").map(str::trim).filter(|n| !n.is_empty()).collect()
}

/// The runs of a list of tracks, as `[start, end)`: one per work in a row
/// where the page heads its works, or the whole list as one.
pub fn runs(details: &[TrackDetail], headings: bool) -> Vec<(usize, usize)> {
    let of = |d: &TrackDetail| (d.composer.clone(), d.work.clone());
    let mut out = vec![];
    let mut at = 0;
    while at < details.len() {
        let end = match headings {
            true => (at..details.len())
                .find(|&j| of(&details[j]) != of(&details[at]))
                .unwrap_or(details.len()),
            false => details.len(),
        };
        out.push((at, end));
        at = end;
    }
    out
}

impl App {
    /// The page a person's name opens, whatever they did: one page a
    /// person has. `None` for nobody this library has.
    pub fn person(&self, name: &str) -> Option<Source> {
        let p = &self.peer;
        if name.is_empty() {
            return None;
        }
        let played = || p.details.values().any(|d| names(&d.performer).contains(&name));
        let known = p.composers().iter().any(|c| c.name == name) || p.artists().iter().any(|a| a.name == name);
        (known || played()).then(|| Source::Artist(name.to_string()))
    }

    /// Who is under a track, as the page draws them: whoever played it, or
    /// — on a record of several people's songs — whose this one is.
    pub fn under(&self, creator: &str, title_creator: &str, d: &TrackDetail) -> Vec<String> {
        match d.performer.is_empty() {
            false => names(&d.performer).into_iter().map(str::to_string).collect(),
            true if title_creator != creator && !title_creator.is_empty() => vec![title_creator.to_string()],
            true => vec![],
        }
    }

    /// A page's tracks, run by run: each heading's composer and work, then
    /// each row beside the names under it.
    fn tracks(&self, l: &mut Layout, creator: &str, headings: bool) {
        let p = &self.peer;
        let rows = p.rows();
        let details: Vec<_> = rows.iter().map(|i| p.detail_of(i.id)).collect();
        for (start, end) in runs(&details, headings) {
            let d = &details[start];
            if headings && !d.work.is_empty() {
                let mut ids = vec![];
                if let Some(s) = self.person(&d.composer) {
                    ids.push(l.mark(Mark::Composer(start), Message::Select(s)));
                }
                if !d.work_id.is_empty() {
                    ids.push(l.mark(Mark::Work(start), Message::Select(Source::Work(d.work_id.clone(), d.work.clone()))));
                }
                let n = ids.len();
                l.block(ids, n);
            }
            // Each row and the names under it that open somebody, the rows
            // as wide as the widest: a shorter one names its last cell
            // again, which `l` does not step onto.
            let under: Vec<Vec<usize>> = (start..end)
                .map(|at| {
                    self.under(creator, &rows[at].creator, &details[at])
                        .iter()
                        .enumerate()
                        .filter_map(|(k, n)| self.person(n).map(|s| l.mark(Mark::Performer(at, k), Message::Select(s))))
                        .collect()
                })
                .collect();
            let columns = 1 + under.iter().map(Vec::len).max().unwrap_or(0);
            let mut ids = vec![];
            for (at, names) in (start..end).zip(under) {
                ids.push(at);
                ids.extend(&names);
                let last = *ids.last().unwrap_or(&at);
                ids.extend(std::iter::repeat_n(last, columns - 1 - names.len()));
            }
            l.block(ids, columns);
        }
    }

    /// The names in a header's accent line, side by side.
    fn by(&self, l: &mut Layout, line: &str) {
        let ids = names(line)
            .iter()
            .enumerate()
            .filter_map(|(k, n)| self.person(n).map(|s| l.mark(Mark::By(k), Message::Select(s))))
            .collect::<Vec<_>>();
        let n = ids.len();
        l.block(ids, n);
    }

    /// A header's Play button, for the page's first track.
    fn play(&self, l: &mut Layout) {
        if let Some(first) = self.peer.rows().first() {
            let id = l.mark(Mark::Play, Message::PlayItem(first.id));
            l.block(vec![id], 1);
        }
    }

    /// Records as a shelf, `columns` across.
    fn shelf<'a>(&self, l: &mut Layout, albums: impl Iterator<Item = &'a crate::rows::Album>, columns: usize) {
        let ids = albums
            .enumerate()
            .map(|(i, a)| l.mark(Mark::Album(i), Message::Select(Source::Album(a.name.clone()))))
            .collect();
        l.block(ids, columns);
    }

    /// People as a row of faces.
    fn faces(&self, l: &mut Layout, people: &[(String, String)]) {
        let ids = people
            .iter()
            .enumerate()
            .filter_map(|(i, (n, _))| self.person(n).map(|s| l.mark(Mark::Person(i), Message::Select(s))))
            .collect::<Vec<_>>();
        let n = ids.len();
        l.block(ids, n);
    }

    /// Who is on the album or the recording the page is: the composers
    /// first, then everybody credited, each once with their roles.
    pub fn people(&self, composers: &[String]) -> Vec<(String, String)> {
        let mut people: Vec<(String, String)> = composers.iter().map(|c| (c.clone(), "Composer".to_string())).collect();
        for c in self.peer.on_recording() {
            let mut role = c.role.clone();
            if !c.instrument.is_empty() && c.instrument != c.role {
                role = match role.is_empty() {
                    true => c.instrument.clone(),
                    false => format!("{role}, {}", c.instrument),
                };
            }
            match people.iter_mut().find(|(n, _)| *n == c.name) {
                Some((_, r)) => *r = format!("{r}, {role}"),
                None => people.push((c.name.clone(), role)),
            }
        }
        people
    }

    /// The composers of the page's tracks, each once, in the order their
    /// works come.
    pub fn composers_here(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for i in self.peer.rows() {
            let c = self.peer.detail_of(i.id).composer;
            if !c.is_empty() && !out.contains(&c) {
                out.push(c);
            }
        }
        out
    }

    /// The records a recording's tracks are on, each once.
    pub fn albums_here(&self) -> Vec<&crate::rows::Album> {
        let p = &self.peer;
        let mut on: Vec<&crate::rows::Album> = vec![];
        for i in p.rows() {
            let named = p.detail_of(i.id).album;
            if let Some(a) = p.albums().iter().find(|a| a.name == named) {
                if !on.iter().any(|o| o.name == a.name) {
                    on.push(a);
                }
            }
        }
        on
    }

    /// The page the tracks pane shows, as the keyboard walks it — the one
    /// description the view draws its cursor from.
    pub fn layout(&self) -> Layout {
        let p = &self.peer;
        let grid = self.main_grid();
        let mut l = Layout {
            page: Page {
                sections: vec![],
                main: grid.cells,
            },
            marks: vec![],
        };
        let detailed = !(self.help || self.debug);
        match &p.source {
            Source::Album(name) if detailed => {
                let album = p.albums().iter().find(|a| a.name == *name);
                let creator = album.map(|a| a.creator.clone()).unwrap_or_default();
                self.by(&mut l, &self.album_by(name));
                self.play(&mut l);
                self.tracks(&mut l, &creator, true);
                let people = self.people(&self.composers_here());
                self.faces(&mut l, &people);
            }
            // One person's page: a composer's works, or anybody else's songs.
            Source::Artist(name) if detailed => {
                if self.banner_of(name).is_none() {
                    self.play(&mut l);
                }
                let works = !p.works().is_empty();
                if let Some(best) = self.best_known() {
                    let open = Message::Select(Source::Work(best.id.clone(), best.title.clone()));
                    self.featured(&mut l, open, Message::PlayWork(best.id.clone()));
                }
                if works {
                    l.page.sections.push(Section::run(0, grid.cells, grid.columns));
                    if self.listed_works().len() < p.works().len() {
                        let id = l.mark(Mark::SeeAll, Message::AllWorks(true));
                        l.block(vec![id], 1);
                    }
                }
                self.shelf(&mut l, p.albums_by(name).into_iter(), self.columns());
                if !works {
                    self.tracks(&mut l, name, true);
                }
            }
            Source::Recording(_, who) if detailed => {
                let composer = p.works().first().map(|w| w.composer.clone()).unwrap_or_default();
                l.line(Mark::Over, self.person(&composer));
                self.by(&mut l, who);
                self.play(&mut l);
                self.tracks(&mut l, "", false);
                self.shelf(&mut l, self.albums_here().into_iter(), usize::MAX);
                let composers: Vec<String> = (!composer.is_empty()).then_some(composer).into_iter().collect();
                let people = self.people(&composers);
                self.faces(&mut l, &people);
            }
            // A show's page is a playlist's: its Play, over its episodes —
            // and, followed, its older ones fetched and its Unfollow.
            Source::Show(id, _) if detailed => {
                self.play(&mut l);
                if let Some((k, s)) = p
                    .subscriptions()
                    .iter()
                    .enumerate()
                    .find(|(_, s)| !s.show_id.is_empty() && s.show_id == *id)
                {
                    let mut ids = vec![];
                    if s.has_older() {
                        ids.push(l.mark(Mark::Older(false), Message::FetchOlder(s.url.clone(), 20)));
                        if s.items - s.keep > 20 {
                            ids.push(l.mark(Mark::Older(true), Message::FetchOlder(s.url.clone(), 0)));
                        }
                    }
                    ids.push(l.mark(Mark::Unfollow(k), Message::Unfollow(s.url.clone())));
                    let n = ids.len();
                    l.block(ids, n);
                }
                l.page.sections.push(Section::run(0, grid.cells, grid.columns));
            }
            Source::Playlist(..) if detailed => {
                self.play(&mut l);
                l.page.sections.push(Section::run(0, grid.cells, grid.columns));
            }
            // A shows page: its +, then the search it opens — the box over
            // what it found — or the feeds followed that are no show yet,
            // each with its Unfollow, over the shows.
            Source::Podcasts | Source::Sermons if detailed => {
                let id = l.mark(Mark::Add, Message::Adding(self.adding.is_none()));
                l.block(vec![id], 1);
                match self.adding.is_some() {
                    true => {
                        let id = l.mark(Mark::Query, Message::FocusQuery);
                        l.block(vec![id], 1);
                    }
                    false => {
                        for (k, s) in self.unread_feeds() {
                            let id = l.mark(Mark::Unfollow(k), Message::Unfollow(s.url.clone()));
                            l.block(vec![id], 1);
                        }
                    }
                }
                l.page.sections.push(Section::run(0, grid.cells, grid.columns));
            }
            Source::Work(..) => {
                let composer = p.works().first().map(|w| w.composer.clone()).unwrap_or_default();
                l.line(Mark::Over, self.person(&composer));
                if let Some(first) = p.recordings().first() {
                    let open = Message::Select(Source::Recording(first.id.clone(), first.performers.clone()));
                    self.featured(&mut l, open, Message::PlayRecording(first.id.clone()));
                }
                l.page.sections.push(Section::run(0, grid.cells, grid.columns));
                return l;
            }
            _ => {
                l.page.sections.push(Section::run(0, grid.cells, grid.columns));
            }
        }
        l
    }

    /// Who an album's header names in the accent: whoever is credited on
    /// the most of it (`albums`), or whoever made it.
    pub fn album_by(&self, name: &str) -> String {
        let album = self.peer.albums().iter().find(|a| a.name == name);
        match album.filter(|a| !a.performer.is_empty()) {
            Some(a) => a.performer.clone(),
            None => album.map(|a| a.creator.clone()).unwrap_or_default(),
        }
    }

    /// What the page's cursor is on when it is not the main list: a mark
    /// keeps its place when the list before it grows or shrinks, as a
    /// search's results do under its box, and its number does not.
    pub fn cursor_mark(&self) -> Option<Mark> {
        let l = self.layout();
        let at = self.at(crate::Pane::Tracks).checked_sub(l.page.main)?;
        l.marks.get(at).map(|(m, _)| *m)
    }

    /// Put the page's cursor back on `mark`, where it is now.
    pub fn keep_on(&mut self, mark: Option<Mark>) {
        if let Some(cell) = mark.and_then(|m| self.layout().cell(m)) {
            self.cursors[crate::Pane::Tracks as usize] = cell;
        }
    }

    /// The search for a show to follow, when the page is a shows page
    /// with it open.
    pub fn adding_here(&self) -> Option<&crate::Adding> {
        match self.peer.source {
            Source::Podcasts | Source::Sermons => self.adding.as_ref(),
            _ => None,
        }
    }

    /// The kind of show a shows page lists.
    pub fn shows_kind(&self) -> &'static str {
        match self.peer.source {
            Source::Sermons => "sermon",
            _ => "podcast",
        }
    }

    /// The feeds of the page's kind the library follows that are no show
    /// yet — not read, or a read that failed — each with where it is in
    /// `Peer::subscriptions`.
    pub fn unread_feeds(&self) -> Vec<(usize, &crate::rows::Subscription)> {
        let p = &self.peer;
        let kind = self.shows_kind();
        p.subscriptions()
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind == kind && !p.shows().iter().any(|show| !s.show_id.is_empty() && show.id == s.show_id))
            .collect()
    }

    /// What a page sets apart, side by side: opening it, and its Play.
    fn featured(&self, l: &mut Layout, open: Message, play: Message) {
        let ids = vec![l.mark(Mark::Featured, open), l.mark(Mark::FeaturedPlay, play)];
        l.block(ids, 2);
    }

    /// A composer's most recorded work, when one is: a tie is no answer to
    /// "which is best known".
    pub fn best_known(&self) -> Option<&crate::rows::Work> {
        match crate::known(self.peer.works())[..] {
            [best, next, ..] if best.recordings > next.recordings => Some(best),
            _ => None,
        }
    }

    /// A person's wide picture, whichever list has it.
    pub fn banner_of(&self, name: &str) -> Option<&str> {
        let p = &self.peer;
        let artist = p.artists().iter().find(|a| a.name == name).map(|a| a.banner.as_str());
        let composer = p.composers().iter().find(|c| c.name == name).map(|c| c.banner.as_str());
        artist.filter(|b| !b.is_empty()).or(composer.filter(|b| !b.is_empty()))
    }

    /// The page's main list as a grid: what the cursor walked before there
    /// was anything else to reach.
    pub fn main_grid(&self) -> vim::Grid {
        let p = &self.peer;
        match &p.source {
            crate::places::Source::Albums => crate::SHELF.grid(p.albums().len(), self.pane_width()),
            Source::Artists => crate::SHELF.grid(p.artists().len(), self.pane_width()),
            Source::Composers => crate::SHELF.grid(p.composers().len(), self.pane_width()),
            Source::Podcasts | Source::Sermons if self.adding.is_some() => vim::Grid::column(self.adding.as_ref().map_or(0, |a| a.shows().len())),
            Source::Podcasts => crate::SHELF.grid(p.shows_of("podcast").len(), self.pane_width()),
            Source::Sermons => crate::SHELF.grid(p.shows_of("sermon").len(), self.pane_width()),
            Source::Artist(_) if !p.works().is_empty() => vim::Grid {
                cells: self.listed_works().len(),
                columns: self.listing_columns(),
            },
            // Every performance, under the one set apart.
            Source::Work(..) => vim::Grid {
                cells: p.recordings().len(),
                columns: self.listing_columns(),
            },
            _ => vim::Grid::column(p.rows().len()),
        }
    }
}
