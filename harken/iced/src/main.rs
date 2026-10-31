//! The harken library, in iced — on the desktop and in a browser.
//!
//! One client among several: the phones are the others. They all run the
//! same domain — `harken/domain`'s module, its functions run by ArkDB v5's
//! interpreter (`docs/plan-v5.md` §1.4) and called through the bindings
//! `build.rs` generates ([`domain`]) — and none of them contains a line of
//! domain logic.
//!
//!   the server:  harken-server
//!   the desktop: harken-iced --server http://127.0.0.1:8787 --user alice
//!   alone:       harken-iced, with no server — "connect" joins one later
//!   a browser:   the same crate compiled to wasm, served beside the server
//!   the demo:    `--features demo`, a seeded library and no server at all
//!
//! **It works with nobody signed in.** The window opens on this device's
//! replica of the server's log and shows what it has; whatever is done is
//! kept pending here, authored as nobody. Signing in moves that same replica
//! on: the pending work becomes the signer's and is pushed. See `auth.rs`.
//!
//! Every change this window makes has a standing — pending, confirmed, or
//! refused with the sentence every replica reaches — and a refusal is said,
//! naming what was refused (`App::edits`).
//!
//! Most of what a window is lives in `arkui`: the vim keyboard, the table,
//! the menus and pickers and the rules for who has the keyboard, the card
//! grid, the derived square, the picture cache and the address bar. What is
//! here is harken's: its places, its panes, its messages, its screens.

mod auth;
mod client;
mod directory;
mod domain;
mod listening;
mod nav;
mod palette;
mod peer;
mod places;
mod player;
mod rows;
/// The demo's library. Compiled only into the demo, so the client that talks
/// to a real server carries none of it.
#[cfg(feature = "demo")]
mod seed;
mod view;
mod wake;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::time::Duration;

use ark::log::EntryId;
use ark_auth::Login;
use arkui::account::{Account, Who};
use arkui::cards::Shelf;
use arkui::context::{Context, Layer};
use arkui::glyphs;
use arkui::images::{self, Images};
use arkui::menu::{self, Entry, Menu};
use arkui::picker::{Choice, Picked, Picker};
use arkui::route::Router;
use arkui::vim;
use iced::{Point, Size, Subscription, Task};

use crate::domain::{AddToPlaylist, CreatePlaylist, NameVoice, RemoveFromPlaylist};
#[cfg(not(feature = "demo"))]
use client::Replica;
use peer::{key, Peer};
use places::{Focus, Pane, Place, Source};
use player::{Player, Track};
use rows::Id;
// The tab's title and the platform's media controller are the browser's, the
// way the `<audio>` element is; the desktop build has neither.
#[cfg(target_arch = "wasm32")]
use player::Remote;

/// Where a track's bytes are.
///
/// `file` is one of two things, and which is not a mode: the demo's
/// recordings are whole URLs into Wikimedia, and a scanned track's is the path
/// the scanner wrote, relative to the media root — which is what `/media/`
/// serves back. Handing the relative one straight to an `<audio>` element is
/// what this exists to stop: the browser resolves it against the page, drops
/// the `/media/` prefix, and a server with a single-page fallback answers
/// `index.html` and a 200 — "the media resource was not suitable". The rule is
/// the domain's (`crate::domain::host::url`), because three things join
/// it: this, the phone, and a speaker with no client at all.
fn media_url(server: &str, file: &str) -> String {
    crate::domain::host::url(server, file)
}

/// The terms a page of tracks was given on, a line each, for its foot: the
/// licence alone when there is one, and each beside whoever gave it when
/// there are several — `CC BY 3.0 (Busch Chamber Players)`. A page that
/// reserves nothing has none.
fn licences(details: &[rows::TrackDetail]) -> Vec<String> {
    let mut terms: Vec<(&str, Vec<&str>)> = Vec::new();
    for d in details.iter().filter(|d| !d.licence.is_empty()) {
        let at = match terms.iter().position(|(l, _)| *l == d.licence) {
            Some(at) => at,
            None => {
                terms.push((&d.licence, Vec::new()));
                terms.len() - 1
            }
        };
        if !d.performer.is_empty() && !terms[at].1.contains(&d.performer.as_str()) {
            terms[at].1.push(&d.performer);
        }
    }
    let one = terms.len() == 1;
    terms
        .into_iter()
        .map(|(l, who)| match one || who.is_empty() {
            true => l.to_string(),
            false => format!("{l} ({})", who.join(", ")),
        })
        .collect()
}

/// Where a row's menu opens, which is decided by what asked for it: a right
/// click on the pointer, the row's ⋯ (or `m`) on the ⋯ column. The two rules
/// AppKit already has; see `arkui::menu::Anchor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Pointer,
    Dots,
}

/// The dialog a shows page's + opens, to follow a show: the kind it
/// follows as, what its box says, what the directory found for it
/// (`directory`), and where its cursor is among the rows.
#[derive(Debug, Clone)]
pub struct Adding {
    /// `podcast` or `sermon`: the page's.
    pub kind: &'static str,
    pub query: String,
    pub found: Vec<directory::Found>,
    /// What the search came to when it is not a list: searching, nothing
    /// found, why it failed; or why a follow was refused.
    pub said: Option<String>,
    pub at: usize,
    /// How far open it is, and whether it is on its way out
    /// (`arkui::dialog`).
    pub shown: arkui::motion::Shown,
}

impl Adding {
    pub fn new(kind: &'static str) -> Adding {
        Adding {
            kind,
            query: String::new(),
            found: vec![],
            said: None,
            at: 0,
            shown: arkui::dialog::shown(iced::time::Instant::now()),
        }
    }

    /// The shows its rows are: a feed's address is the one show it names;
    /// words, what the directory found for them.
    pub fn shows(&self) -> Vec<directory::Found> {
        match directory::is_feed(&self.query) {
            true => vec![directory::Found {
                title: self.query.trim().to_string(),
                feed: self.query.trim().to_string(),
                ..Default::default()
            }],
            false => self.found.clone(),
        }
    }
}

/// The command box's own box and list, for its keys and its scrolling.
pub const COMMAND_INPUT: &str = "harken-command";
pub const COMMAND_LIST: &str = "harken-command-list";

/// How many of each kind the command box shows.
pub const COMMAND_SONGS: usize = 8;
pub const COMMAND_EACH: usize = 5;

/// The search box's id, for putting the keys in it; and what it found's,
/// for keeping the cursor's row in sight.
pub const QUERY: &str = "harken-show-search";
pub const FOUND: &str = "harken-show-found";

/// How many works a composer's page lists before its "See All".
pub const KNOWN_WORKS: usize = 6;

/// How many older episodes of a followed show the button under its list
/// fetches at a time.
pub const OLDER: i64 = 20;

/// Works best known first: the most recorded, then the most tracks, the
/// catalogue's order between equals.
pub fn known(works: &[rows::Work]) -> Vec<&rows::Work> {
    let mut ranked: Vec<&rows::Work> = works.iter().collect();
    ranked.sort_by_key(|w| (std::cmp::Reverse(w.recordings), std::cmp::Reverse(w.tracks)));
    ranked
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Show a playlist, an album, an artist, a page — or everything.
    Select(Source),
    /// Start this one, and make what is on screen the queue it plays through.
    PlayItem(Id),
    /// Start one performance (its key) from its first movement, the queue
    /// its movements.
    PlayRecording(String),
    /// Start a work (its key): its most complete performance.
    PlayWork(String),
    /// A composer's page: every work, or only the best known.
    AllWorks(bool),
    PlayPause,
    Skip(i32),
    /// Back or on this many seconds in what plays: an episode's skip, in
    /// place of the track before and after.
    Jog(i64),
    /// Dragging the bar's progress, in seconds: where this device's
    /// scrubber is, said to the session as the ephemeral `scrub`…
    Seek(f32),
    /// …and let go: a seek, there.
    SeekDone,
    /// What the connect entry says: the server a window alone would join.
    ConnectUrl(String),
    /// Join that server, in place, and sign in there
    /// (`docs/plan-alone.md` §4).
    Connect,
    /// Whether the page's own origin is a harken server, answered: the
    /// window it opens on — that server's, or alone.
    Started(Option<String>),
    /// The sign-in came back, one way or the other.
    SignedIn(Result<Login, String>),
    SignOut,
    /// Settings' Resync: a full look asked of the server, which is its
    /// catalogue's sync too.
    Resync,
    /// An episode from this far in: one of its chapters.
    PlayFrom(Id, i64),
    /// The episode playing, from this far in: a line of its transcript.
    Cue(i64),
    /// The transcript follows the line being said (`true`: its Follow, and
    /// `f`), or no longer does (`false`: it was scrolled by hand).
    Follow(bool),
    /// Somebody's web page, opened outside the app.
    OpenLink(String),
    /// An episode said played (`true`) or not by the signed-in person.
    MarkPlayed(Id, bool),
    /// A shows page's +: the search for a show to follow, open or shut.
    Adding(bool),
    /// What the search box says: words for the directory, or a feed's
    /// address.
    Query(String),
    /// The typing paused at these words: search, if they are still what
    /// the box says.
    SearchDue(String),
    /// What the directory found for these words, or why it found nothing.
    Searched(String, Result<Vec<directory::Found>, String>),
    /// Put the keys in the search box.
    FocusQuery,
    /// The dialog's cursor onto a row of what it found.
    AddingAt(usize),
    /// A followed show's menu, from the ⋯ in its header: its Unfollow.
    ShowMenu(String),
    /// Follow this feed, as the kind of show the page lists
    /// (`library.subscribe`).
    FollowFeed(String),
    /// Follow a feed no more, by its URL (`library.unsubscribe`).
    Unfollow(String),
    /// More of a followed feed's older episodes: this many more, or 0 for
    /// every one (`library.fetch_older`).
    FetchOlder(String, i64),
    /// The account button at the top right: its menu, whose entries open
    /// Settings and send `SignOut`.
    AccountMenu,
    /// Where the pointer is, in the window: tracked on the root so a menu's
    /// `pin` shares its origin.
    Hover(Point),
    /// Open a row's menu: the ⋯, a right click, or `m`. It lands the cursor on
    /// that row first.
    RowMenu(Id, Anchor),
    CloseMenu,
    /// The playlist picker over the track the menu is about, or the one under
    /// the cursor.
    OpenPicker,
    /// Who the voice saying a line of the transcript is: a picker of the
    /// episode's people, and a name typed.
    NameVoice(usize),
    PickerAt(usize),
    MenuAt(usize),
    MenuActivate,
    PickerActivate,
    PickerName(String),
    PickerCreate,
    ClosePicker,
    /// The pointer moved onto a row or a card. Content panes only: in the
    /// sidebar the cursor *is* the selection, so hovering there would
    /// navigate on the way past.
    HoverAt(usize),
    /// A cover arrived, or did not.
    Cover(images::Loaded),
    /// A banner fetched, for the banners' own cache.
    Banner(images::Loaded),
    /// A key nothing on screen wanted.
    Key(iced::keyboard::Key, iced::keyboard::Modifiers),
    Resized(Size),
    /// The wheel moved mostly sideways: perhaps a swipe back or forward.
    Wheel(iced::mouse::ScrollDelta),
    /// A key the command box answers (`arkui::command`): Ctrl+K, or with
    /// it up `<Esc>` and the arrows, whatever has the keyboard.
    CommandKey(arkui::command::Press),
    /// What is typed in it.
    CommandQuery(String),
    /// The keys onto one of its rows, from the top: the pointer over it.
    CommandAt(usize),
    /// Its list scrolled to here.
    CommandScrolled(f32),
    /// One of its rows chosen: the box closes, and this is done.
    Command(Box<Message>),
    /// A song found by it: played from its album.
    CommandSong(Id),
    /// The queue: open it, or close it — `q`, and the button on the bar.
    OpenQueue,
    CloseQueue,
    /// A row of what plays next pressed, from the top of them: perhaps a
    /// click, perhaps the start of a drag (`arkui::reorder`).
    QueueGrab(usize),
    /// The pointer, while one is held down on the queue.
    QueueDrag(Point),
    /// …and let go.
    QueueDrop,
    /// The cursor onto a row of it, from the top: the track playing, then
    /// what plays after it.
    QueueAt(usize),
    /// Play the track at this place in the queue.
    QueueJump(usize),
    /// Take the track at this place out of the queue.
    QueueRemove(usize),
    /// Move the track at the first place to the second.
    QueueMove(usize, usize),
    /// Put this track after the one playing, or at the end of the queue.
    PlayNext(Id),
    AddToQueue(Id),
    /// Which device is making the sound: open the picker, walk it, pick.
    OpenDevices,
    DeviceAt(usize),
    PickDevice(String),
    CloseDevices,
    /// A volume slider dragged, 0 to 100: the bar's, or a picker row's.
    /// Sent to the session at most every [`client::THROTTLE_MS`], and
    /// the last value always.
    Volume(Level, f32),
    /// …and let go: whatever the throttle held goes now.
    VolumeDone,
    /// Mute it, or unmute it.
    Mute(Level),
    /// Up or down by so much: `+` and `-`.
    Nudge(Level, i64),
    /// A speaker joins the group that is playing, or leaves it.
    Join(String),
    Unjoin(String),
    /// Something said it happened (`docs/plan-reactive.md`): the sync
    /// socket opened, closed or brought a frame — the log, an ack, the
    /// listening session — or, in a browser, the back button, a lock-screen
    /// button, or the `<audio>` element starting, pausing or ending. What it
    /// was is whatever the update after it finds; see [`App::react`].
    Wake,
    /// A deadline this window armed came due: the redial after a close.
    Due,
    /// A clock, running only while something on screen moves by itself: 50
    /// ms while a submenu's dwell counts,
    /// 250 ms while audio plays (the scrubber and the media session) —
    /// [`App::clock`]. At rest there is none.
    Tick,
    /// A wheel over a context window's backdrop, whose whole job is not to
    /// reach the page: a menu is pinned to a window coordinate, and a list
    /// scrolling under it would leave it beside a row it is not about.
    Swallow,
    /// The page was scrolled to this far down (absolute, in pixels).
    PageScrolled(f32),
    /// Put the page's scroll where it was left, or at the top: sent once the
    /// page it is for has been drawn, so it lands on that page's list.
    ScrollPage(f32),
}

/// What a volume control moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Level {
    /// Whatever is playing: the output's group — a group of one is its
    /// leader — or, with no output, this device; alone, this device's
    /// player. The bar's, and `+`, `-` and `M`'s.
    Output,
    /// The output's group, every member in proportion.
    Group,
    /// One device of the session.
    Device(String),
}

/// What a [`Level`] comes to, now: the transitions it calls, or this
/// device's own player while it is its own session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Volumed {
    Group,
    Device(String),
    Local,
}

/// How far `+` and `-` move a volume.
pub const NUDGE: i64 = 5;

/// How a volume control moves what it moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    To(i64),
    By(i64),
    /// Muted the other way from how it is.
    Mute,
}

/// The volume keys: `+` (or `=`, the same key unshifted) up, `-` down,
/// `M` the mute.
pub fn volume_key(c: char, level: Level) -> Option<Message> {
    match c {
        '+' | '=' => Some(Message::Nudge(level, NUDGE)),
        '-' => Some(Message::Nudge(level, -NUDGE)),
        'M' => Some(Message::Mute(level)),
        _ => None,
    }
}

/// How far an episode's skip goes back, and on, in seconds: what a podcast
/// player's do.
pub const JOG_BACK: i64 = 15;
pub const JOG_FORWARD: i64 = 30;

/// A transport button, wherever it was pressed ([`App::press`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    PlayPause,
    Play,
    Pause,
    Skip(i32),
    /// Milliseconds into the track.
    Seek(i64),
    /// The queue as it now stands, from this track, this far in.
    Start(usize, i64),
}

/// This device's player as harken's one reconciler drives it
/// (`harken_player::Sound`): the window's [`Player`], which sounds one track,
/// over the window's queue, which it plays through on its own
/// ([`App::react`] moves it on at a track's end) — the browser's `<audio>`,
/// or on the desktop nothing at all, which never plays.
pub struct Sounding<'a> {
    player: &'a mut Player,
    server: &'a str,
    queue: &'a mut Vec<listening::Track>,
    /// Where the queue was taken from, as whoever set it said.
    from: &'a mut String,
    /// The volume, read once a look: reading it may make the element.
    volume: Option<harken_player::Vol>,
}

impl<'a> Sounding<'a> {
    pub fn new(player: &'a mut Player, server: &'a str, queue: &'a mut Vec<listening::Track>, from: &'a mut String) -> Sounding<'a> {
        let volume = player.volume().map(|(level, muted)| harken_player::Vol { level, muted });
        Sounding {
            player,
            server,
            queue,
            from,
            volume,
        }
    }

    fn ms(&self) -> i64 {
        (self.player.position() * 1000.0) as i64
    }
}

/// Where in `queue` the player's track is: where it was put, or, the queue
/// edited since, where that track is now.
fn loaded_at(player: &Player, queue: &[listening::Track]) -> Option<usize> {
    let id = player.track()?.id;
    match queue.get(player.at()) {
        Some(t) if t.id.0 == id => Some(player.at()),
        _ => queue.iter().position(|t| t.id.0 == id),
    }
}

/// What is playing now ([`App::now`]): a queue, where in it, and its
/// clock, in seconds.
pub struct Now<'a> {
    pub queue: &'a [listening::Track],
    pub at: usize,
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
}

impl<'a> Now<'a> {
    pub fn track(&self) -> &'a listening::Track {
        &self.queue[self.at]
    }
}

/// Put `player` on `at` in `queue`, at this point, playing or not: the one
/// place a queued track becomes a sound. Whether it has a file to stream.
fn play_at(player: &mut Player, server: &str, queue: &[listening::Track], at: usize, position_ms: i64, playing: bool) -> bool {
    let Some(track) = queue.get(at) else { return false };
    player.play(
        Track {
            id: track.id.0,
            title: track.title.clone(),
            ms: track.duration_ms,
        },
        at,
        &media_url(server, &track.file),
    );
    if position_ms > 0 {
        player.seek(position_ms as f64 / 1000.0);
    }
    if !playing {
        player.pause();
    }
    !track.file.is_empty()
}

impl harken_player::Sound for Sounding<'_> {
    fn heard(&self) -> harken_player::Heard {
        use harken_player::Place;
        let at = loaded_at(self.player, self.queue);
        let ms = self.ms();
        harken_player::Heard {
            queue: self.queue.iter().map(|t| t.id).collect(),
            at,
            playing: self.player.asked(),
            ready: at.is_some(),
            place: match self.player.buffering() {
                true => Place::Believed(ms),
                false => Place::Measured { ms, at: client::now_ms() },
            },
            ended: self.player.ended() && at.is_some_and(|i| i + 1 >= self.queue.len()),
            from: self.from.clone(),
        }
    }
    fn take(&mut self) -> Option<harken_player::Event> {
        let at_ms = self.ms();
        self.player.take_stalled().then_some(harken_player::Event::Stalled { at_ms })
    }
    fn tracks(&self) -> Vec<listening::Track> {
        self.queue.clone()
    }
    fn load(&mut self, queue: &[listening::Track], at: usize, position_ms: i64, playing: bool, from: &str) {
        *self.queue = queue.to_vec();
        *self.from = from.into();
        play_at(self.player, self.server, self.queue, at, position_ms, playing);
    }
    fn jump(&mut self, at: usize, position_ms: i64) {
        let playing = self.player.asked();
        play_at(self.player, self.server, self.queue, at, position_ms, playing);
    }
    fn resume(&mut self) {
        self.player.resume();
    }
    fn pause(&mut self) {
        self.player.pause();
    }
    fn seek(&mut self, ms: i64) {
        self.player.seek(ms as f64 / 1000.0);
    }
    fn volume(&self) -> Option<harken_player::Vol> {
        self.volume
    }
    fn set_volume(&mut self, v: harken_player::Vol) {
        self.player.set_volume(v.level, v.muted);
        self.volume = self.player.volume().map(|(level, muted)| harken_player::Vol { level, muted });
    }
}

/// One row of the device picker: a device standing in the session, the
/// one the sound stayed with after its socket went — or, first, the group
/// that is playing, as a whole, when the output is one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PickerRow {
    pub id: String,
    pub name: String,
    /// Here, and able to play: what a click or `<Enter>` can hand the sound
    /// to.
    pub takeable: bool,
    /// Its socket has gone; the sound stays with it.
    pub away: bool,
    /// Its volume, where anybody can move it: a row with none draws no
    /// slider.
    pub volume: Option<i64>,
    pub muted: bool,
    /// Playing what the output plays: the output, or a speaker following it.
    pub playing: bool,
    /// What `J` and its button do: join it to the output's group (`Some(true)`),
    /// take it out (`Some(false)`), or nothing.
    pub join: Option<bool>,
    /// The row for the group as a whole, its volume every member's.
    pub group: bool,
}

impl PickerRow {
    /// What its volume controls move.
    pub fn level(&self) -> Level {
        match self.group {
            true => Level::Group,
            false => Level::Device(self.id.clone()),
        }
    }
}

/// One change this window made, kept so its fate can be said about *it*.
///
/// A refusal arrives as an id and a sentence; "the server refused a change"
/// says nothing about which. So every mutation this window authors is
/// written down with what it was, and `standing` follows each.
#[derive(Debug, Clone)]
pub struct Edit {
    pub id: EntryId,
    /// What it was, as a person would say it: "put Air on Favorites".
    pub what: String,
}

// The geometry, shared by the view that draws and the arithmetic that decides
// where the keyboard lands and where a panel goes. iced lays out after `view`,
// so these are declared rather than measured.
pub const SIDEBAR_WIDTH: f32 = 200.0;
/// The chrome's padding: above the top bar, and at the right-hand end of it
/// and of the play bar.
pub const PAGE_PADDING: f32 = 16.0;
/// The index pages' cards, and every page's shelf of them.
pub const SHELF: Shelf = Shelf::DEFAULT;
/// The page's gutter either side: the shelf's padding, so a list, a header
/// and a row of cards all start at one line.
pub const GUTTER: f32 = SHELF.padding;
/// How tall the play bar is — declared because the device picker sits on it
/// and is placed before anything is laid out.
pub const BAR_HEIGHT: f32 = 64.0;
/// What the device picker draws a row at: a name, and beside it the row's
/// join button, its mute and its volume slider.
pub const DEVICES_WIDTH: f32 = 336.0;
/// Where a menu asked for with `m` opens, down the window: by key there is no
/// pointer, and nothing can say where the cursor's row currently is.
pub const MENU_BY_KEY_Y: f32 = 120.0;
/// Where the account button's bottom is, down the window: the top bar is the
/// page's first line, [`arkui::account::HEIGHT`] tall.
pub const ACCOUNT_BOTTOM: f32 = PAGE_PADDING + arkui::account::HEIGHT;
/// The two scrollables a cursor keeps itself inside of.
pub const SIDEBAR: &str = "sidebar";
pub const TRACKS: &str = "tracks";
/// The queue sheet's list of what plays next, for keeping its cursor on
/// screen.
pub const QUEUE: &str = "queue";
/// A row of the queue let go somewhere new, still settling into its place
/// ([`arkui::reorder::Settle`]); and until the queue says it has moved —
/// the session's answer comes back from the room — the order it moved
/// from, so the list is drawn in the new order in the meantime.
#[derive(Clone, Debug)]
pub struct QueueSettle {
    pub settle: arkui::reorder::Settle,
    pub from: usize,
    pub to: usize,
    pub before: Vec<crate::domain::Id<crate::domain::Media>>,
    pub since: iced::time::Instant,
}

/// How long a move of the queue is drawn ahead of the queue saying so.
pub const QUEUE_PENDING: Duration = Duration::from_secs(2);

/// The last row of the playlist picker, which is a row and not a key.
pub const NEW_PLAYLIST: &str = "New playlist\u{2026}";
/// …and of the voice picker.
pub const SOMEBODY_ELSE: &str = "Somebody else\u{2026}";

/// What a picker's row stands for: a playlist the track is on or off, or
/// who a voice is.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    Playlist(Id),
    Person(String),
}
/// About how tall a transcript's header is, over its lines, which scroll
/// under it; and one of its lines, a line of words and its padding — what
/// keeps the line being said in the middle of the room left.
pub const TRANSCRIPT_HEAD: f32 = 132.0;
pub const TRANSCRIPT_LINE: f32 = 40.0;

pub struct App {
    /// Which pane the cursor is in, and where it is in each: a cursor per
    /// pane, so leaving the sidebar and coming back does not lose your place.
    pub pane: Pane,
    pub cursors: [usize; 2],
    /// Half-typed keys: a count, a `g`, a `/` search.
    pub keys: vim::Keys,
    /// The last accepted `/` search, for `n` and `N`.
    pub search: String,
    /// `?` — a keymap nobody can guess is a keymap nobody uses.
    pub help: bool,
    /// What Settings' Resync last came to, to say under it.
    pub resync: Option<String>,
    /// The command box, Ctrl+K, while it is up; and the one closed, drawn
    /// until it has faded out.
    pub command: Option<arkui::command::Command>,
    pub command_leaving: Option<arkui::command::Command>,
    /// The search for a show to follow, while a shows page has it open.
    pub adding: Option<Adding>,
    /// …and the one closed, drawn until it has faded out.
    pub adding_leaving: Option<Adding>,
    /// `D` — every number the session holds.
    pub debug: bool,
    /// A composer's page lists every work, not only the best known: its
    /// "See All" was pressed. Gone with the page.
    pub all_works: bool,
    /// The transcript keeps its cursor on the line being said, in the
    /// middle of the page — until the cursor is moved or the page scrolled
    /// by hand, and again once the line being said is the cursor's, or
    /// Follow is pressed.
    pub following: bool,
    /// The line being said, as the transcript last followed it.
    pub said: Option<usize>,
    /// A row's menu, and the playlist picker that is its submenu — or the one
    /// `a` opens alone, or who a voice is. arkui holds the rules.
    pub ctx: Context<Message, Pick>,
    /// The track the menu is about. Held by id rather than index: the list
    /// under it can move while it is up.
    pub menu_about: Option<Id>,
    /// …and the track the picker is about.
    pub picker_about: Option<Id>,
    /// …or the episode and the voice in it.
    pub voice_about: Option<(Id, String)>,
    /// The device picker, when it is up; the last row is "nowhere".
    pub devices: Option<usize>,
    /// The queue panel, when it is up: where its cursor is, from the top.
    pub queue_panel: Option<usize>,
    /// The queue's sheet sliding in, in, or sliding out: drawn while this
    /// is `Some` and not gone (`arkui::sheet`).
    pub queue_sheet: Option<arkui::motion::Shown>,
    /// A row of what plays next held down, and perhaps being dragged.
    pub queue_drag: Option<arkui::reorder::Reorder>,
    /// A row let go into a new place, settling into it.
    pub queue_settle: Option<QueueSettle>,
    /// How big the window is, so a panel near an edge opens the other way.
    pub window: Size,
    /// The address bar, reconciled after every update.
    pub router: Router<Place>,
    /// A sideways trackpad gesture under way: back and forward, as a
    /// browser has them ([`arkui::swipe`]).
    pub swipe: arkui::swipe::Swipe,
    /// How far down each page was scrolled when it was left, so back and
    /// forward land where it was, as a browser's do.
    pub scrolls: HashMap<Place, f32>,
    /// Where the page just opened is to be scrolled, once it is drawn.
    pub scroll_due: Option<f32>,
    /// The page being opened is a step back or forward, not a new visit.
    pub travelling: bool,
    /// Where the pointer is.
    pub cursor: Point,
    /// Where the pointer was when a key last moved the cursor, until it
    /// moves: a list the keyboard scrolls slides rows under a pointer that
    /// is standing still, and a row arriving under it is not somebody
    /// pointing at it — taken as a hover, it pulled the cursor back under
    /// the pointer at every step.
    pub keyed: Option<Point>,
    pub player: Player,
    /// What `Skip` moves through: the list as it stood when play was pressed,
    /// as the tracks a hand-off carries — this device's own while it is its
    /// own session, and the session's copied here while it is the output.
    pub queue: Vec<listening::Track>,
    /// Where that queue was taken from: the page it was pressed on.
    pub queued_from: String,
    /// This device's end of the account's listening session.
    pub listening: listening::Remote,
    /// The session as it stood after the last update, when this device is
    /// part of it (`listening::Remote::session`): what the bar, the picker
    /// and `D` draw.
    pub session: Option<listening::Listening>,
    /// Where this device's scrubber is being dragged to, in seconds.
    pub scrubbing: Option<f32>,
    /// A volume slider being dragged, and where to: what it draws until it
    /// is let go.
    pub volume_drag: Option<(Level, f32)>,
    /// …what the throttle is holding back of it, and when it last sent.
    pub volume_held: Option<(Level, i64)>,
    pub volume_sent: u64,
    /// Covers, fetched once and kept.
    pub covers: Images,
    /// The wide pictures a person's page opens with, kept larger than a
    /// cover: they are drawn across the page.
    pub banners: Images,
    /// The `Peer::art_gen` covers were last asked for.
    pub art_seen: u64,
    pub server: String,
    /// A name to offer a dev server, so a desktop given `--user` needs no
    /// browser. Ignored by a real one.
    pub user: Option<String>,
    /// Who is signed in, if anybody. A login whose token is empty was turned
    /// away by the server.
    pub login: Option<Login>,
    /// Where logins are remembered — `None` in a test, which must not write
    /// into the config directory of whoever runs it.
    pub logins: Option<ark_peer::Logins>,
    /// This load has already gone to sign in, or come back from doing so,
    /// and does not go again by itself (`App::sign_in_unasked`).
    pub sign_in_tried: bool,
    /// The page stays on its splash ([`auth::reveal`]): this window is
    /// finding out whether it has a server, trading a sign-in's code, or
    /// going to sign in. What it would draw meanwhile is a player it is
    /// about to leave.
    pub held: bool,
    /// What the connect entry holds, while this window is alone.
    pub connect: String,
    /// What wakes this window: the peer's socket rings it, and in a browser
    /// the page's own events do (`wake.rs`).
    pub bell: arkui::wake::Bell,
    pub peer: Peer,
    /// This window's own changes, for saying which one was refused.
    pub edits: Vec<Edit>,
    /// Refusals, newest last, as `(what, why)`.
    pub refused: Vec<(String, String)>,
    pub note: String,
}

impl App {
    /// A window over `peer`, with everything else at rest.
    pub fn with_peer(peer: Peer, server: String, login: Option<Login>) -> App {
        let bell = arkui::wake::Bell::new();
        peer.client.set_waker(wake::waker(&bell));
        wake::page(&bell);
        App {
            pane: Pane::Tracks,
            cursors: [0; 2],
            keys: vim::Keys::new(),
            search: String::new(),
            help: false,
            resync: None,
            command: None,
            command_leaving: None,
            adding: None,
            adding_leaving: None,
            debug: false,
            all_works: false,
            following: true,
            said: None,
            ctx: Context::new(),
            menu_about: None,
            picker_about: None,
            voice_about: None,
            devices: None,
            queue_panel: None,
            queue_sheet: None,
            queue_drag: None,
            queue_settle: None,
            window: Size::new(860.0, 600.0),
            router: Router::new(),
            swipe: Default::default(),
            scrolls: HashMap::new(),
            scroll_due: None,
            travelling: false,
            keyed: None,
            cursor: Point::ORIGIN,
            player: Player::new(),
            queue: Vec::new(),
            queued_from: String::new(),
            listening: listening::Remote::new(),
            session: None,
            scrubbing: None,
            volume_drag: None,
            volume_held: None,
            volume_sent: 0,
            covers: Images::new("harken/covers"),
            banners: Images::bounded("harken/banners", images::WIDE),
            art_seen: 0,
            server,
            user: None,
            login,
            logins: None,
            sign_in_tried: false,
            held: false,
            connect: String::new(),
            bell,
            peer,
            edits: Vec::new(),
            refused: Vec::new(),
            note: String::new(),
        }
    }

    /// The demo: a seeded replica that is its own authority, and nothing to
    /// sign in to. In memory on both targets, as it always was — a demo that
    /// reloads to its seed is a demo that cannot be broken.
    #[cfg(feature = "demo")]
    pub fn demo() -> App {
        let peer = Peer::open(seed::seeded());
        App::with_peer(peer, String::new(), None)
    }

    /// No server named and none joined: this device's own replica, its own
    /// authority, for real (`docs/plan-alone.md` §4). What is done is kept
    /// here as local history until "connect" hands it to a server.
    #[cfg(not(feature = "demo"))]
    fn alone(user: Option<String>) -> App {
        let (client, note) = match auth::open_alone() {
            Ok(c) => (c, String::new()),
            Err(e) => (
                App::alone_in_memory(),
                format!("the saved library would not open ({e}) \u{2014} working in memory"),
            ),
        };
        let mut peer = Peer::open(client);
        // Alone nobody signs in, so no server welcomes anybody: the replica
        // is the whole truth, and its Favorites is made here.
        peer.ensure_playlist();
        let mut app = App::with_peer(peer, String::new(), None);
        app.logins = Some(auth::logins());
        app.user = user;
        app.connect = auth::DEFAULT_SERVER.into();
        app.note = note;
        app
    }

    /// A replica in memory, alone, as nobody: what a window works on while
    /// the saved one will not open, or before a page knows its server.
    #[cfg(not(feature = "demo"))]
    fn alone_in_memory() -> Replica {
        let mut r = crate::client::memory(client::nobody(), None);
        r.alone();
        r
    }

    /// "Connect": the replica this window has been using alone joins the
    /// server in the entry, in place — the local history pending there, the
    /// lists patched rather than rebuilt — the server is remembered for the
    /// `local` place, and the sign-in that server needs starts. Until it
    /// finishes the history is nobody's and nothing is dialled; the sign-in
    /// makes it the signer's and pushes it, as any work done signed out.
    pub fn connect(&mut self) -> Task<Message> {
        let server = self.connect.trim().trim_end_matches('/').to_string();
        if server.is_empty() {
            self.note = "which server? type its address".into();
            return Task::none();
        }
        self.peer.join(&ark_auth::socket_url(&server));
        if let Err(e) = auth::remember_joined(&server) {
            self.note = format!("connected, but this device will not remember it: {e}");
        }
        let pending = self.peer.client.lock().pending_len();
        self.server = server;
        self.login = self.logins.as_ref().and_then(|l| l.recall(&self.server));
        if let Some(login) = self.login.clone() {
            self.signed_in(login);
            return Task::none();
        }
        self.note = format!(
            "joined {} \u{2014} {pending} change{} waiting for a sign-in",
            self.server,
            if pending == 1 { "" } else { "s" }
        );
        self.start_sign_in()
    }

    fn boot() -> (App, Task<Message>) {
        let (mut app, task) = App::open();
        app.arrive();
        if !app.held {
            auth::reveal();
        }
        (app, task)
    }

    /// A link somebody was sent, read before anything is drawn.
    ///
    /// The address bar is reconciled after every update, and the first update
    /// is whatever iced sends first — a resize, the pointer. Reconciled before
    /// the address bar had been read, `#album/Water%20Music` was overwritten
    /// with `#library` and the link went nowhere; so the window reads it here,
    /// once, as it opens. Seen in a browser, which is the only place this
    /// shows.
    pub fn arrive(&mut self) {
        if let Some(place) = self.router.poll(!self.peer.choices.is_empty()) {
            let wanted = self.peer.source_of(&place);
            if let Some(at) = self.peer.choices.iter().position(|c| c.source == wanted) {
                self.cursors[Pane::Sidebar as usize] = at;
            }
            self.peer.source = wanted;
            self.peer.open_page();
        }
    }

    fn open() -> (App, Task<Message>) {
        #[cfg(feature = "demo")]
        {
            (App::demo(), Task::none())
        }
        #[cfg(not(feature = "demo"))]
        {
            let (named, user) = auth::config();
            match auth::start(named, auth::joined(), auth::origin()) {
                auth::Start::Server(server) => App::open_server(server, user),
                auth::Start::Alone => (App::alone(user), Task::none()),
                // A page with no `?server=`: whether whoever served it is a
                // harken server is one fetch of its `/healthz` away. Until it
                // answers the window is alone in memory, writing nothing.
                auth::Start::Ask(origin) => {
                    let mut app = App::with_peer(Peer::open(App::alone_in_memory()), String::new(), None);
                    app.user = user;
                    app.note = format!("looking for a server at {origin}\u{2026}");
                    app.held = true;
                    let probe = auth::answers(format!("{origin}/healthz"));
                    let task = Task::perform(probe, move |ok| Message::Started(auth::decide(None, None, Some(origin.clone()), |_| ok)));
                    (app, task)
                }
            }
        }
    }

    /// A window over `server`'s replica on this device, signed in as the
    /// login it remembers there, or signed out.
    #[cfg(not(feature = "demo"))]
    fn open_server(server: String, user: Option<String>) -> (App, Task<Message>) {
        {
            let login = auth::logins().recall(&server);
            let (client, note) = match auth::open(&server, login.as_ref()) {
                Ok(c) => (c, String::new()),
                // A replica that will not open is not a reason to show
                // nothing. This window works in memory and says so.
                Err(e) => {
                    let mut c = crate::client::memory(client::nobody(), None);
                    if let Some(l) = &login {
                        c.login(auth::who_of(l), l.token.clone());
                    }
                    (c, format!("the saved library would not open ({e}) — working in memory"))
                }
            };
            // The roles the remembered login was signed in with, which every
            // intent carries: a library write is then refused on this device
            // as the server would (`docs/plan-guards.md` D1), and shown as
            // any refusal is — `auth::open` gave the replica the login's.
            let peer = Peer::open(client);
            peer.client.lock().connect(&ark_auth::socket_url(&server));
            // No Favorites made here: the server makes a person's at their
            // first sign-in (`playlists.welcome`), and a device that made its
            // own before hearing the log would leave them two.
            let mut app = App::with_peer(peer, server, login);
            app.logins = Some(auth::logins());
            app.user = user;
            app.note = note;
            if let Some(login) = &app.login {
                app.listening.open(&login.session);
                return (app, Task::none());
            }
            // Nobody to sign in as is not a state anybody is left in: the
            // page goes to the server's sign-in, or trades the code it came
            // back with, and the desktop hands off to a browser.
            let task = app.sign_in();
            (app, task)
        }
    }

    /// The sign-in a window on a server opens into, with nobody signed in.
    #[cfg(not(feature = "demo"))]
    fn sign_in(&mut self) -> Task<Message> {
        #[cfg(target_arch = "wasm32")]
        let (code, returned) = (ark_auth::web::take_code(), auth::came_back());
        #[cfg(not(target_arch = "wasm32"))]
        let (code, returned) = (None, false);
        self.arrive_signing_in(auth::arrival(code, returned))
    }

    /// What [`auth::arrival`] said, done.
    #[cfg_attr(feature = "demo", allow(dead_code))]
    pub fn arrive_signing_in(&mut self, arrival: auth::Arrival) -> Task<Message> {
        match arrival {
            auth::Arrival::Exchange(code) => {
                (self.sign_in_tried, self.held) = (true, true);
                self.note = "signing in\u{2026}".into();
                let server = self.server.clone();
                #[cfg(target_arch = "wasm32")]
                {
                    Task::perform(async move { ark_auth::web::exchange(&server, &code).await }, Message::SignedIn)
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    off_thread(move || ark_auth::client::exchange(&server, &code))
                }
            }
            auth::Arrival::CameBack => {
                self.sign_in_tried = true;
                self.note = "the sign-in came back without a login \u{2014} reload to try again".into();
                Task::none()
            }
            auth::Arrival::Go => self.sign_in_unasked(),
        }
    }

    /// Go to sign in without being asked — at launch, and when the server
    /// turns the login away — unless this load already has. A sign-in that
    /// came back refused or empty, or a login turned away again straight
    /// after it was made, would otherwise send the window round and round;
    /// the reason is in the status line, and a reload starts afresh.
    pub fn sign_in_unasked(&mut self) -> Task<Message> {
        match self.sign_in_tried {
            true => Task::none(),
            false => self.start_sign_in(),
        }
    }

    /// Send the person to sign in.
    fn start_sign_in(&mut self) -> Task<Message> {
        #[cfg(feature = "demo")]
        {
            Task::none()
        }
        #[cfg(all(not(feature = "demo"), target_arch = "wasm32"))]
        {
            // The page goes away and comes back with a code; `boot` finishes.
            // The mark says it went, so the load it comes back as does not
            // go again if it comes back with nothing.
            (self.sign_in_tried, self.held) = (true, true);
            auth::leaving();
            ark_auth::web::go_sign_in(&self.server, self.user.as_deref());
            Task::none()
        }
        #[cfg(all(not(feature = "demo"), not(target_arch = "wasm32")))]
        {
            self.sign_in_tried = true;
            self.note = "signing in — look for a browser tab if one opened".into();
            let server = self.server.clone();
            let user = self.user.clone();
            off_thread(move || ark_auth::client::login(&server, user.as_deref(), ark_auth::client::open_browser))
        }
    }

    /// Somebody signed in. The replica this window has been using — signed
    /// out, or signed in as them before — moves on as them: whatever was
    /// done before becomes theirs and is pushed, and the link dials.
    pub fn signed_in(&mut self, login: Login) {
        if let Some(logins) = &self.logins {
            logins.remember(&self.server, &login);
        }
        // The roles the sign-in said this login holds, which every intent
        // carries (`docs/plan-guards.md` D1).
        self.peer.client.lock().login(auth::who_of(&login), login.token.clone());
        let pending = self.peer.client.lock().pending_len();
        self.note = match pending {
            0 => format!("signed in as {}", auth::who(&login)),
            n => format!(
                "signed in as {} — {n} change{} from before going up",
                auth::who(&login),
                if n == 1 { "" } else { "s" }
            ),
        };
        self.peer.refresh();
        // A device is a login, so a new login is a new device — which is what
        // makes signing out and back in honestly a different row in somebody
        // else's picker, and a reloaded tab the same one.
        self.listening.open(&login.session);
        self.login = Some(login);
    }

    /// Author a change, and remember what it was. A refusal here is the
    /// domain's own verdict — a guard, a check — and is said at once; one the
    /// server reaches later is matched back to this by its id.
    pub fn author(&mut self, what: String, name: &str, input: &impl ark::format::Encode) -> bool {
        match self.peer.client.lock().mutate(name, input) {
            Ok(id) => {
                self.edits.push(Edit { id, what });
                true
            }
            Err(e) => {
                self.note = format!("could not {what}: {e}");
                self.refused.push((what, e));
                false
            }
        }
    }

    /// Where the cursor is in a pane.
    pub fn at(&self, pane: Pane) -> usize {
        self.cursors[pane as usize]
    }

    /// Which grid the keyboard is in. The one definition — see `Focus`.
    pub fn focus(&self) -> Focus {
        if self.devices.is_some() {
            return Focus::Devices;
        }
        if self.queue_panel.is_some() {
            return Focus::Queue;
        }
        match self.ctx.focus() {
            Some(Layer::Picker) => Focus::Picker,
            Some(Layer::Menu) => Focus::Menu,
            None if self.adding.is_some() => Focus::Adding,
            None => Focus::Pane(self.pane),
        }
    }

    /// Whether this pane has the keyboard, which is the whole of what decides
    /// whether its cursor is drawn: a context window over it has the keys, and
    /// a cursor drawn where the next `j` will not go is a lie.
    pub fn has_keys(&self, pane: Pane) -> bool {
        self.focus() == Focus::Pane(pane)
    }

    /// How wide the page beside the sidebar is: the window less the sidebar,
    /// which is every width `view` lays out before the page — the page runs
    /// to the window's edge, and each page keeps its own gutter inside it.
    pub fn pane_width(&self) -> f32 {
        self.window.width - SIDEBAR_WIDTH
    }

    /// How many cards fit across an index page — the same division the view
    /// does, from the same width, which keeps the cursor under its card.
    pub fn columns(&self) -> usize {
        SHELF.columns(self.pane_width())
    }

    /// How many columns a composer's works are listed in — the view's
    /// division, from the room its gutters leave, the shelf's.
    pub fn listing_columns(&self) -> usize {
        arkui::hero::columns(SHELF.room(self.pane_width()))
    }

    /// What shape a pane is, which is all `vim` needs to know about it: the
    /// sidebar a column, and the page everything on it a click opens
    /// ([`nav`]) — its main list first, as it always was.
    pub fn shape(&self, pane: Pane) -> vim::Page {
        match pane {
            Pane::Sidebar => vim::Page::single(vim::Grid::column(self.peer.choices.len())),
            Pane::Tracks => self.layout().page,
        }
    }

    /// The text `/` searches, for whichever pane has the cursor: what is on
    /// screen, so on an index page the cards.
    pub fn labels(&self, pane: Pane) -> Vec<String> {
        let p = &self.peer;
        match pane {
            Pane::Sidebar => p.choices.iter().map(|c| c.label.clone()).collect(),
            Pane::Tracks => match &p.source {
                Source::Albums => p.albums().iter().map(|a| format!("{} {}", a.name, a.creator)).collect(),
                Source::Artists => p.artists().iter().map(|a| a.name.clone()).collect(),
                Source::Composers => p.composers().iter().map(|c| c.name.clone()).collect(),
                Source::Podcasts => p.shows_of("podcast").iter().map(|s| format!("{} {}", s.title, s.author)).collect(),
                Source::Sermons => p.shows_of("sermon").iter().map(|s| format!("{} {}", s.title, s.author)).collect(),
                Source::Artist(_) if !p.works().is_empty() => self.listed_works().iter().map(|w| format!("{} {}", w.title, w.catalogue)).collect(),
                Source::Work(..) => p.recordings().iter().map(|r| r.performers.clone()).collect(),
                Source::Transcript => self.cues().into_iter().map(|c| c.text).collect(),
                _ => p.rows().iter().map(|i| format!("{} {}", i.title, i.creator)).collect(),
            },
        }
    }

    /// Do what a finished command asked for.
    pub fn act(&mut self, action: vim::Action) -> Task<Message> {
        // Every grid answers a motion the same three ways — step, land, or
        // hand it to whatever is beyond that edge — so `travel` is one call
        // wherever the keyboard is. What is left below is only what an overlay
        // answers *differently* from a pane.
        if let vim::Action::Move(motion) = action {
            return self.travel(motion);
        }
        let focus = self.focus();
        if !matches!(focus, Focus::Pane(_)) {
            return match action {
                // In the device picker the volume keys are the row's, and `J`
                // joins it or takes it out; over anything else they are what
                // is playing's, as the transport is.
                vim::Action::Key(c) if focus == Focus::Devices => match self.device_key(c) {
                    Some(m) => self.update(m),
                    None => Task::none(),
                },
                // In the queue: `x` takes the row out, `J` and `K` move it
                // down and up, `q` closes it.
                vim::Action::Key(c) if focus == Focus::Queue => match self.queue_key(c) {
                    Some(m) => self.update(m),
                    None => Task::none(),
                },
                // In the dialog, `i` is back to typing, as in vim.
                vim::Action::Key('i') if focus == Focus::Adding => self.update(Message::FocusQuery),
                vim::Action::Key(c) => match volume_key(c, Level::Output) {
                    Some(m) => self.update(m),
                    None => Task::none(),
                },
                vim::Action::Activate => match focus {
                    Focus::Menu => self.update(Message::MenuActivate),
                    Focus::Adding => match self.adding.as_ref().and_then(|a| a.shows().get(a.at).cloned()) {
                        Some(found) => self.update(Message::FollowFeed(found.feed)),
                        None => Task::none(),
                    },
                    Focus::Picker => self.update(Message::PickerActivate),
                    Focus::Queue => match self.queue_rows().get(self.queue_panel.unwrap_or(0)) {
                        Some((index, _)) => self.update(Message::QueueJump(*index)),
                        None => Task::none(),
                    },
                    _ => self.pick_device(),
                },
                vim::Action::Cancel => match focus {
                    Focus::Devices => self.update(Message::CloseDevices),
                    Focus::Queue => self.update(Message::CloseQueue),
                    Focus::Adding => self.update(Message::Adding(false)),
                    // A menu and its submenu are one thing, so `<Esc>` takes
                    // both; `a`'s picker has no parent and is only itself.
                    _ => {
                        self.ctx.cancel();
                        Task::none()
                    }
                },
                // The transport should not stop working because a panel is up.
                vim::Action::Toggle => self.update(Message::PlayPause),
                _ => Task::none(),
            };
        }
        match action {
            vim::Action::Move(_) => Task::none(),
            vim::Action::Activate => self.activate(),
            vim::Action::Cycle => {
                self.pane = self.pane.next();
                self.reveal()
            }
            // Back and forward, the way the browser's buttons go: the bar
            // moves, and the next update arrives where it went.
            vim::Action::Jump(by) => {
                self.router.go(by);
                Task::none()
            }
            vim::Action::Toggle => self.update(Message::PlayPause),
            vim::Action::Search(query) => {
                self.search = query;
                // From one before the cursor, so a match on the line you are
                // on is found rather than skipped.
                let from = self.at(self.pane).checked_sub(1);
                self.seek(from, 1)
            }
            vim::Action::Match(delta) => self.seek(Some(self.at(self.pane)), delta),
            vim::Action::Cancel => {
                self.help = false;
                self.debug = false;
                self.note.clear();
                self.narrow(None);
                Task::none()
            }
            // No range is selected here and no sequence is bound.
            vim::Action::Visual(_) | vim::Action::Inner(_) | vim::Action::Bound(_) => Task::none(),
            // Keys this app binds for itself, kept out of the grammar so a
            // convenience cannot collide with a motion by accident.
            vim::Action::Key(c) => match c {
                '?' => {
                    self.help = !self.help;
                    Task::none()
                }
                'D' => {
                    self.debug = !self.debug;
                    Task::none()
                }
                // "Which lists is this on", which is the one question left
                // after the hearts went.
                'a' => self.update(Message::OpenPicker),
                // The same menu the dots and a right click open, on the ⋯
                // column — by key there is no pointer to land on.
                'm' => match self.row_under_cursor() {
                    Some(id) => {
                        self.cursor.y = MENU_BY_KEY_Y;
                        self.update(Message::RowMenu(id, Anchor::Dots))
                    }
                    None => Task::none(),
                },
                'd' => self.update(Message::OpenDevices),
                'q' => self.update(Message::OpenQueue),
                't' => self.update(Message::Select(Source::Transcript)),
                'f' if self.peer.source == Source::Transcript => self.update(Message::Follow(true)),
                '}' => self.update(self.forward()),
                '{' => self.update(self.back()),
                other => match volume_key(other, Level::Output) {
                    Some(m) => self.update(m),
                    None => Task::none(),
                },
            },
        }
    }

    /// Move the cursor of whichever grid has the keyboard, and on a refusal
    /// say what is beyond that edge: a refused step means "not mine".
    pub fn travel(&mut self, motion: vim::Motion) -> Task<Message> {
        match self.focus() {
            // arkui knows what is beyond a menu's edges: the submenu to the
            // right of the entry that owns one, the parent to the left of it.
            Focus::Menu | Focus::Picker => match self.ctx.travel(motion) {
                Some(message) => self.update(message),
                None => Task::none(),
            },
            Focus::Devices => {
                let grid = vim::Grid::column(self.picker_rows().len());
                if let Some(at) = grid.step(self.devices.unwrap_or(0), motion) {
                    self.devices = Some(at);
                }
                Task::none()
            }
            // Up off the first row is back into the box, where it began.
            Focus::Adding => {
                let rows = self.adding.as_ref().map_or(0, |a| a.shows().len());
                let grid = vim::Grid::column(rows);
                let at = self.adding.as_ref().map_or(0, |a| a.at);
                match grid.step(at, motion) {
                    Some(to) => {
                        if let Some(a) = self.adding.as_mut() {
                            a.at = to;
                        }
                        arkui::scroll::reveal(FOUND, grid, to)
                    }
                    None if matches!(motion, vim::Motion::Up(_)) => self.update(Message::FocusQuery),
                    None => Task::none(),
                }
            }
            Focus::Queue => {
                let grid = vim::Grid::column(self.queue_rows().len());
                match grid.step(self.queue_panel.unwrap_or(0), motion) {
                    Some(at) => {
                        self.queue_panel = Some(at);
                        arkui::scroll::reveal(QUEUE, grid, at)
                    }
                    None => Task::none(),
                }
            }
            Focus::Pane(pane) => match self.shape(pane).step(self.at(pane), motion) {
                Some(at) => self.land(pane, at),
                None => match pane.beyond(motion) {
                    Some(next) => {
                        self.pane = next;
                        self.reveal()
                    }
                    None => Task::none(),
                },
            },
        }
    }

    /// Put the cursor down in a pane, and do everything that follows: in the
    /// sidebar the cursor *is* the selection, so landing there shows it.
    pub fn land(&mut self, pane: Pane, at: usize) -> Task<Message> {
        self.cursors[pane as usize] = at;
        if pane == Pane::Sidebar {
            self.show_under_cursor();
        }
        // A transcript's cursor moved by hand is not on the line being
        // said: it stops following it.
        if pane == Pane::Tracks && self.peer.source == Source::Transcript {
            self.following = false;
        }
        self.reveal()
    }

    /// The row the page's cursor is on, whichever list it is in.
    fn row_under_cursor(&self) -> Option<Id> {
        let at = self.at(Pane::Tracks);
        match self.layout().mark_at(at) {
            Some(nav::Mark::Episode(k)) => self.peer.appearances().get(k).map(|i| i.id),
            _ => self.peer.rows().get(at).map(|i| i.id),
        }
    }

    /// Point the page at whatever the sidebar's cursor is on.
    fn show_under_cursor(&mut self) {
        let Some(source) = self.peer.choices.get(self.at(Pane::Sidebar)).map(|c| c.source.clone()) else {
            return;
        };
        if self.peer.source == source {
            return;
        }
        self.peer.source = source;
        // Walking the sidebar is one navigation, not one per row: forty
        // history entries would need forty presses of back to undo a scroll.
        self.router.replace_next();
        self.peer.open_page();
        self.cursors[Pane::Tracks as usize] = 0;
        self.scroll_due = Some(0.0);
    }

    /// The Songs page narrowed to a search, or widened back: the cursor goes
    /// to the top of whatever list that leaves, as a page opened does.
    fn narrow(&mut self, needle: Option<&str>) {
        if self.peer.search(needle) {
            self.cursors[Pane::Tracks as usize] = 0;
        }
    }

    /// Keep the cursor on screen — on a transcript's line, in the middle.
    fn reveal(&self) -> Task<Message> {
        let at = self.at(self.pane);
        let id = match self.pane {
            Pane::Sidebar => SIDEBAR,
            Pane::Tracks if self.peer.source == Source::Transcript && at < self.cues().len() => return self.centre(at),
            Pane::Tracks => TRACKS,
        };
        arkui::scroll::reveal_to(id, self.shape(self.pane).progress(at))
    }

    /// What is said in the episode playing, line by line: none for a song,
    /// or an episode whose feed has no timed transcript.
    pub fn cues(&self) -> Vec<rows::Cue> {
        self.now().map(|n| self.peer.transcript_of(n.track().id.0)).unwrap_or_default()
    }

    /// The line of the transcript being said now, if one has begun.
    pub fn cue_now(&self) -> Option<usize> {
        let ms = (self.now()?.position * 1000.0) as i64;
        rows::cue_at(&self.cues(), ms)
    }

    /// Put a transcript's line in the middle of the room under its header.
    fn centre(&self, at: usize) -> Task<Message> {
        let seen = (self.window.height - BAR_HEIGHT - TRANSCRIPT_HEAD) / TRANSCRIPT_LINE;
        arkui::scroll::centre(TRACKS, at, self.cues().len(), seen)
    }

    /// Keep the transcript on the line being said while it follows it: its
    /// cursor there and the line in the middle, once each time the line
    /// changes — and follow again once the line being said is the one the
    /// cursor was left on.
    fn follow(&mut self) -> Task<Message> {
        if self.peer.source != Source::Transcript {
            return Task::none();
        }
        let now = self.cue_now();
        if now == self.said {
            return Task::none();
        }
        self.said = now;
        let Some(at) = now else { return Task::none() };
        if at == self.at(Pane::Tracks) {
            self.following = true;
        }
        if !self.following {
            return Task::none();
        }
        self.cursors[Pane::Tracks as usize] = at;
        self.centre(at)
    }

    /// Jump to the next label matching the last search, from `from`
    /// (`None` is before the first), wrapping the way vim does.
    fn seek(&mut self, from: Option<usize>, delta: isize) -> Task<Message> {
        if self.search.is_empty() {
            return Task::none();
        }
        let labels = self.labels(self.pane);
        let hit = match from {
            Some(at) => vim::find(&labels, &self.search, at, delta),
            // Before the first: the first match there is, which `find` from 0
            // would skip if it were at 0.
            None => labels.iter().position(|l| l.to_lowercase().contains(&self.search.to_lowercase())),
        };
        match hit {
            Some(at) => self.land(self.pane, at),
            None => {
                self.note = format!("no match for {}", self.search);
                Task::none()
            }
        }
    }

    /// Run the device row the cursor is on. A device that cannot be heard is
    /// not a target, exactly as it is not a click target.
    fn pick_device(&mut self) -> Task<Message> {
        let Some(at) = self.devices else {
            return Task::none();
        };
        match self.picker_rows().get(at) {
            Some(row) if row.takeable => self.update(Message::PickDevice(row.id.clone())),
            _ => Task::none(),
        }
    }

    /// The queue panel's rows, from the top: the track playing and every one
    /// after it, each with its place in the queue — the session's, or, this
    /// device being its own session, its own.
    pub fn queue_rows(&self) -> Vec<(usize, listening::Track)> {
        self.now()
            .map(|n| n.queue.iter().cloned().enumerate().skip(n.at).collect())
            .unwrap_or_default()
    }

    /// What a key does on the queue row under the cursor: `x` takes it out,
    /// `J` moves it one down and `K` one up — only what plays after the
    /// track playing moves, and only among itself — and `q` closes.
    pub fn queue_key(&self, c: char) -> Option<Message> {
        let rows = self.queue_rows();
        let row = self.queue_panel?;
        let (index, _) = rows.get(row)?;
        match c {
            'q' => Some(Message::CloseQueue),
            'x' => Some(Message::QueueRemove(*index)),
            'J' if row >= 1 && row + 1 < rows.len() => Some(Message::QueueMove(*index, index + 1)),
            'K' if row >= 2 => Some(Message::QueueMove(*index, index - 1)),
            _ => None,
        }
    }

    /// A row of the page as a track of the queue: copied rather than referred
    /// to, since a device handed the session may not have that album yet,
    /// and its file the path as the log carries it, which each device joins
    /// to *its* server.
    fn queued(&self, id: Id) -> Option<listening::Track> {
        let i = self.peer.row(id)?;
        Some(listening::Track {
            id: key(i.id),
            title: i.title.clone(),
            creator: i.creator.clone(),
            album: self.peer.detail_of(i.id).album,
            duration_ms: i.duration_ms,
            file: i.file.clone(),
        })
    }

    /// Put a track after the one playing (`next`) or at the end of the queue:
    /// the session's `enqueue`, part of it — and, this device its own session,
    /// its own queue, played at once when nothing is playing, as `enqueue`
    /// starts a session with nothing queued.
    fn enqueue(&mut self, id: Id, next: bool) {
        let Some(track) = self.queued(id) else { return };
        if self.listening.session(&self.peer.client.lock()).is_some() {
            let tracks = vec![track];
            self.call(&match next {
                true => harken_player::Press::Next { tracks },
                false => harken_player::Press::Append { tracks },
            });
            return;
        }
        if self.player.track().is_none() {
            self.queue = vec![track];
            self.start_at(0, 0, true);
            return;
        }
        let at = match next {
            true => self.playing_at() + 1,
            false => self.queue.len(),
        };
        self.queue.insert(at.min(self.queue.len()), track);
    }

    /// The session's transition for an edit of the queue: what `<Enter>`,
    /// `x`, `J` and `K` in the queue panel call, part of a session.
    pub fn queue_call(message: &Message) -> Option<harken_player::Press> {
        use harken_player::Press;
        Some(match *message {
            Message::QueueJump(i) => Press::Jump { index: i as i64 },
            Message::QueueRemove(i) => Press::Remove { index: i as i64 },
            Message::QueueMove(from, to) => Press::Move {
                from: from as i64,
                to: to as i64,
            },
            _ => return None,
        })
    }

    /// An edit of the queue: the session's transition, part of it; this
    /// device's own queue otherwise.
    fn edit_queue(&mut self, message: &Message) {
        if self.listening.session(&self.peer.client.lock()).is_some() {
            if let Some(press) = Self::queue_call(message) {
                self.call(&press);
            }
            return;
        }
        let at = self.playing_at();
        match *message {
            Message::QueueJump(i) => self.start_at(i, 0, true),
            Message::QueueRemove(i) if i < self.queue.len() && i != at => {
                self.queue.remove(i);
            }
            Message::QueueRemove(_) => self.skip(1),
            Message::QueueMove(from, to) if from < self.queue.len() && to < self.queue.len() => {
                let t = self.queue.remove(from);
                self.queue.insert(to, t);
            }
            _ => {}
        }
    }

    /// A key for the command box: Ctrl+K opens it, the keys in its box, or
    /// closes it; `<Esc>` closes it; the arrows walk what it found.
    fn command_key(&mut self, press: arkui::command::Press) -> Task<Message> {
        use arkui::command::Press;
        let now = iced::time::Instant::now();
        match press {
            Press::Toggle if self.command.is_some() => self.close_command(),
            Press::Toggle => {
                // Over everything else, which it closes: one thing up at once.
                self.close_queue();
                self.devices = None;
                self.ctx.cancel();
                self.command = Some(arkui::command::Command::new(now));
                self.command_leaving = None;
                return iced::widget::operation::focus(COMMAND_INPUT);
            }
            Press::Close => self.close_command(),
            Press::Up | Press::Down => {
                let groups = self.command_groups();
                if let Some(c) = self.command.as_mut() {
                    return c.step(press == Press::Down, &groups, COMMAND_LIST);
                }
            }
        }
        Task::none()
    }

    /// The command box closed: it fades out as it came.
    fn close_command(&mut self) {
        if let Some(mut c) = self.command.take() {
            c.shown.hide(iced::time::Instant::now());
            self.command_leaving = Some(c);
        }
    }

    /// What the command box finds for what is typed in it: the library's
    /// songs, albums, artists and composers whose names have it in them —
    /// those that start with it first — a few of each. A song is played
    /// from its album; the rest open their page. Nothing typed, nothing
    /// found.
    pub fn command_groups(&self) -> Vec<arkui::command::Group<Message>> {
        use arkui::command::{Group, Item};
        let Some(query) = self
            .command
            .as_ref()
            .or(self.command_leaving.as_ref())
            .map(|c| c.query.trim().to_lowercase())
        else {
            return vec![];
        };
        if query.is_empty() {
            return vec![];
        }
        let has = |s: &str| s.to_lowercase().contains(&query);
        let starts = |s: &str| !s.to_lowercase().starts_with(&query);
        let chosen = |m: Message| Message::Command(Box::new(m));
        let mut songs: Vec<_> = self
            .peer
            .items()
            .iter()
            .filter(|i| i.kind == "song" && (has(&i.title) || has(&i.creator)))
            .collect();
        songs.sort_by_key(|i| (starts(&i.title), i.title.to_lowercase()));
        let mut albums: Vec<_> = self.peer.albums().iter().filter(|a| has(&a.name) || has(&a.creator)).collect();
        albums.sort_by_key(|a| (starts(&a.name), a.name.to_lowercase()));
        let mut artists: Vec<_> = self.peer.artists().iter().filter(|a| has(&a.name)).collect();
        artists.sort_by_key(|a| (starts(&a.name), a.name.to_lowercase()));
        let mut composers: Vec<_> = self.peer.composers().iter().filter(|c| has(&c.name)).collect();
        composers.sort_by_key(|c| (starts(&c.name), c.name.to_lowercase()));
        vec![
            Group {
                heading: "Songs".into(),
                items: songs
                    .into_iter()
                    .take(COMMAND_SONGS)
                    .map(|i| Item {
                        glyph: arkui::glyphs::NOTE,
                        label: i.title.clone(),
                        detail: Some(i.creator.clone()),
                        choose: chosen(Message::CommandSong(i.id)),
                    })
                    .collect(),
            },
            Group {
                heading: "Albums".into(),
                items: albums
                    .into_iter()
                    .take(COMMAND_EACH)
                    .map(|a| Item {
                        glyph: arkui::glyphs::ALBUM,
                        label: a.name.clone(),
                        detail: Some(a.creator.clone()),
                        choose: chosen(Message::Select(Source::Album(a.id.clone(), a.name.clone()))),
                    })
                    .collect(),
            },
            Group {
                heading: "Artists".into(),
                items: artists
                    .into_iter()
                    .take(COMMAND_EACH)
                    .map(|a| Item {
                        glyph: arkui::glyphs::ARTIST,
                        label: a.name.clone(),
                        detail: None,
                        choose: chosen(Message::Select(Source::Artist(a.name.clone()))),
                    })
                    .collect(),
            },
            Group {
                heading: "Composers".into(),
                items: composers
                    .into_iter()
                    .take(COMMAND_EACH)
                    .map(|c| Item {
                        glyph: arkui::glyphs::COMPOSER,
                        label: c.name.clone(),
                        detail: None,
                        choose: chosen(Message::Select(Source::Artist(c.name.clone()))),
                    })
                    .collect(),
            },
        ]
    }

    /// The queue closed: its sheet slides out, and a drag of one of its
    /// rows is let go of where it was.
    fn close_queue(&mut self) {
        self.queue_panel = None;
        self.queue_drag = None;
        if let Some(sheet) = self.queue_sheet.as_mut() {
            sheet.hide(iced::time::Instant::now());
        }
    }

    /// The ids of what plays next, in order.
    pub fn upcoming_ids(&self) -> Vec<crate::domain::Id<crate::domain::Media>> {
        self.queue_rows().into_iter().skip(1).map(|(_, t)| t.id).collect()
    }

    /// What plays next as it is drawn: in the order a row was just dropped
    /// into, until the queue says so itself.
    pub fn upcoming_drawn(&self) -> Vec<(usize, listening::Track)> {
        let rows: Vec<_> = self.queue_rows().into_iter().skip(1).collect();
        match &self.queue_settle {
            Some(s) if self.upcoming_ids() == s.before => arkui::reorder::moved(&rows, s.from, s.to),
            _ => rows,
        }
    }

    /// The row held on the queue let go: a click plays it, as a click on
    /// any row does; a drag moves it there and lets it settle. (`step`: the
    /// update this is part of reacts once, after.)
    fn drop_queue_row(&mut self) -> Task<Message> {
        let Some(drag) = self.queue_drag.take() else {
            return Task::none();
        };
        let now = iced::time::Instant::now();
        let rows = self.queue_rows();
        match drag.drop(now) {
            arkui::reorder::Drop::Click(row) => match rows.get(row + 1) {
                Some((index, _)) => self.step(Message::QueueJump(*index)),
                None => Task::none(),
            },
            arkui::reorder::Drop::Moved { from, to, settle } => {
                let before = self.upcoming_ids();
                if from != to {
                    if let (Some((a, _)), Some((b, _))) = (rows.get(from + 1), rows.get(to + 1)) {
                        let (a, b) = (*a, *b);
                        let _ = self.step(Message::QueueMove(a, b));
                    }
                }
                self.queue_panel = Some(to + 1);
                self.queue_settle = Some(QueueSettle {
                    settle,
                    from,
                    to,
                    before,
                    since: now,
                });
                Task::none()
            }
        }
    }

    /// Whether anything is on its way at `now`: what the frame clock runs
    /// for.
    fn moving(&self, now: iced::time::Instant) -> bool {
        // Anything closed and still drawn keeps it running until a tick has
        // dropped it (`settle_motion`): stopped a tick before, it would stay
        // on the screen at its last frame, and its backdrop take clicks.
        self.adding_leaving.is_some()
            || self.command_leaving.is_some()
            || self.queue_sheet.is_some_and(|s| !s.is_shown())
            || self.adding.as_ref().is_some_and(|a| a.shown.moving(now))
            || self.command.as_ref().is_some_and(|c| c.shown.moving(now))
            || self.queue_sheet.is_some_and(|s| s.moving(now))
            || self.queue_drag.as_ref().is_some_and(|d| d.moving(now))
            || self.queue_settle.as_ref().is_some_and(|s| {
                // …and while the queue has yet to say a row moved, so the
                // move drawn ahead of it is let go of in time if it never does.
                s.settle.moving(now) || (s.from != s.to && self.upcoming_ids() == s.before)
            })
    }

    /// What has finished moving by `now`, dropped: a dialog faded out, a
    /// sheet slid away, and a row settled once the queue has it where it
    /// was dropped (or has not said so in [`QUEUE_PENDING`]).
    fn settle_motion(&mut self, now: iced::time::Instant) {
        if self.adding_leaving.as_ref().is_some_and(|a| a.shown.gone(now)) {
            self.adding_leaving = None;
        }
        if self.command_leaving.as_ref().is_some_and(|c| c.shown.gone(now)) {
            self.command_leaving = None;
        }
        if self.queue_sheet.is_some_and(|s| s.gone(now)) {
            self.queue_sheet = None;
        }
        let ids = self.upcoming_ids();
        if let Some(s) = &self.queue_settle {
            let arrived = ids != s.before || s.from == s.to;
            if !s.settle.moving(now) && (arrived || now.saturating_duration_since(s.since) > QUEUE_PENDING) {
                self.queue_settle = None;
            }
        }
    }

    /// The device picker's rows: every device of this account standing in the
    /// session, and — drawn, and not a target — the one the sound stayed
    /// with after its socket went, because that is the row somebody most
    /// needs to see. Each says its volume, whether it plays what the output
    /// plays, and whether it can join the output's group or leave it; and
    /// when the output is a group, the group itself comes first, its volume
    /// every member's.
    pub fn picker_rows(&self) -> Vec<PickerRow> {
        let Some(s) = &self.session else {
            return vec![];
        };
        let leader = s.leader();
        let follows_output = |d: &listening::Device| leader.is_some_and(|l| d.follows.as_deref() == Some(l));
        let mut rows: Vec<PickerRow> = s
            .devices
            .iter()
            .map(|d| PickerRow {
                id: d.id.clone(),
                name: d.name.clone(),
                takeable: d.can_play,
                away: false,
                volume: d.can_volume.then_some(d.volume),
                muted: d.muted,
                playing: leader == Some(d.id.as_str()) || follows_output(d),
                join: match follows_output(d) {
                    true => Some(false),
                    false => s.joinable(&d.id).then_some(true),
                },
                group: false,
            })
            .collect();
        if let Some(listening::Output::Away { id, name }) = s.output() {
            rows.push(PickerRow {
                id: id.clone(),
                name: name.clone(),
                away: true,
                ..PickerRow::default()
            });
        }
        rows.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        if let Some(l) = leader.filter(|_| s.grouped()) {
            let name = s.device(l).map_or(l, |d| d.name.as_str());
            rows.insert(
                0,
                PickerRow {
                    name: format!("{name} + {}", s.followers(l).len()),
                    volume: s.group_volume(l),
                    muted: s.group_muted(l),
                    playing: true,
                    group: true,
                    ..PickerRow::default()
                },
            );
        }
        rows
    }

    /// What a key does on the device picker's row under the cursor: `+` and
    /// `-` its volume, `M` its mute, `J` joins it to what is playing or
    /// takes it out.
    pub fn device_key(&self, c: char) -> Option<Message> {
        let row = self.picker_rows().into_iter().nth(self.devices?)?;
        match c {
            'J' => row.join.map(|join| match join {
                true => Message::Join(row.id.clone()),
                false => Message::Unjoin(row.id.clone()),
            }),
            _ => row.volume.and_then(|_| volume_key(c, row.level())),
        }
    }

    /// What a volume control moves comes to, now — `None` where there is
    /// nothing it could move: a device with no volume, a group with no
    /// output, a desktop alone.
    pub fn volumed(&self, level: &Level) -> Option<Volumed> {
        let Some(s) = &self.session else {
            return (*level == Level::Output && Player::AUDIBLE).then_some(Volumed::Local);
        };
        let device = |id: &str| s.device(id).filter(|d| d.can_volume).map(|_| Volumed::Device(id.to_string()));
        match level {
            Level::Group => s.leader().and_then(|l| s.group_volume(l)).map(|_| Volumed::Group),
            Level::Device(id) => device(id),
            Level::Output => match s.leader() {
                Some(l) => s.group_volume(l).map(|_| Volumed::Group),
                None => device(self.listening.me()),
            },
        }
    }

    /// Where it is now, 0 to 100, and whether it is muted.
    pub fn level_of(&self, v: &Volumed) -> Option<(i64, bool)> {
        let s = self.session.as_ref();
        match v {
            Volumed::Group => {
                let s = s?;
                let l = s.leader()?;
                Some((s.group_volume(l)?, s.group_muted(l)))
            }
            Volumed::Device(id) => s?.device(id).map(|d| (d.volume, d.muted)),
            Volumed::Local => Some(self.player.level()),
        }
    }

    /// The press a volume control is: to this level, by so much, or muted
    /// the other way. `None` for this device's own player, which is moved
    /// here and not in the session.
    pub fn volume_call(&self, v: &Volumed, change: Change) -> Option<harken_player::Press> {
        use harken_player::Change as To;
        let muted = self.level_of(v).is_some_and(|(_, m)| m);
        let level = match v {
            Volumed::Local => return None,
            Volumed::Group => harken_player::Level::Group,
            Volumed::Device(id) => harken_player::Level::Device { id: id.clone() },
        };
        let change = match change {
            Change::To(level) => To::To { level },
            Change::By(delta) => To::By { delta },
            Change::Mute => To::Mute { muted: !muted },
        };
        Some(harken_player::Press::Loud { level, change })
    }

    /// Move a volume: a transition, or this device's own player alone.
    fn change_volume(&mut self, level: &Level, change: Change) {
        let Some(v) = self.volumed(level) else { return };
        match self.volume_call(&v, change) {
            Some(press) => self.call(&press),
            None => {
                let (was, muted) = self.player.level();
                match change {
                    Change::To(level) => self.player.set_volume(level, false),
                    Change::By(delta) => self.player.set_volume((was + delta).clamp(0, 100), false),
                    Change::Mute => self.player.set_volume(was, !muted),
                }
            }
        }
    }

    /// What the throttle is holding of a dragged slider, sent if its window
    /// has passed — or now, when `force`d by letting go.
    fn release_volume(&mut self, force: bool) {
        let now = client::monotonic_ms();
        if !force && now < self.volume_sent + client::THROTTLE_MS {
            return;
        }
        if let Some((level, to)) = self.volume_held.take() {
            self.volume_sent = now;
            self.change_volume(&level, Change::To(to));
        }
    }

    /// The row menu's entries, for one track: every one something this
    /// window could already do, asked *about a row*. Built once when the menu
    /// opens, and the one definition — the view draws them and `<Enter>`
    /// runs them.
    pub fn row_entries(&self, id: Id) -> Option<(String, Vec<Entry<Message>>)> {
        let item = self.peer.row(id)?;
        let detail = self.peer.detail_of(id);
        let album = Source::Album(detail.album_id, detail.album);
        let mine = self.peer.mine_of(id);
        let play = match mine.filter(|l| !l.finished && l.position_ms > 0) {
            Some(l) => format!("Resume from {}", arkui::format::clock(l.resume_ms() as f64 / 1000.0)),
            None => "Play".into(),
        };
        let mut entries = vec![
            Entry::run(glyphs::PLAY, play, Message::PlayItem(id)),
            Entry::submenu(glyphs::ADD_TO, "Add to playlist", Message::OpenPicker),
            // The session's queue: after the track playing, or at its end.
            Entry::run(glyphs::NEXT, "Play next", Message::PlayNext(id)),
            Entry::run(glyphs::PLAYLIST, "Add to queue", Message::AddToQueue(id)),
        ];
        // "Go to" takes the glyph of the *place* it goes, which is the one
        // table the sidebar draws from too.
        // An episode goes to its show, whose title is its creator, and to
        // everybody on it.
        let places = match item.kind.as_str() {
            "song" => vec![album, Source::Artist(item.creator.clone())],
            kind => {
                let people = self.peer.people_of(id).into_iter().map(|p| Source::Artist(p.name));
                self.peer.show_named(kind, &item.creator).into_iter().chain(people).collect()
            }
        };
        for source in places {
            if !source.title().is_empty() {
                entries.push(Entry::run(source.glyph(), format!("Go to {}", source.title()), Message::Select(source)));
            }
        }
        // An episode: played or not, by hand, and each of its chapters.
        if item.kind != "song" {
            let finished = mine.is_some_and(|l| l.finished);
            entries.push(match finished {
                true => Entry::run(glyphs::SHOW, "Mark as unplayed", Message::MarkPlayed(id, false)),
                false => Entry::run(glyphs::SHOW, "Mark as played", Message::MarkPlayed(id, true)),
            });
            for c in self.peer.chapters_of(id) {
                let label = format!("{}  {}", arkui::format::clock(c.start_ms as f64 / 1000.0), c.title);
                entries.push(Entry::run(glyphs::PLAY, label, Message::PlayFrom(id, c.start_ms)));
            }
        }
        Some((item.title.clone(), entries))
    }

    /// What the account button at the top right says and offers — or
    /// nothing while nobody is signed in to a server, which is only ever on
    /// the way to a sign-in, or after one came back refused and the status
    /// line says why.
    pub fn account(&self) -> Option<Account<Message>> {
        if cfg!(feature = "demo") {
            return Some(Account::alone("A demo \u{2014} what you do stays on this device"));
        }
        if self.peer.client.lock().is_alone() {
            return Some(Account::alone("Alone \u{2014} what you do is kept on this device"));
        }
        let login = self.login.as_ref()?;
        let who = Who {
            name: login.user.name.clone(),
            id: login.user.id.clone(),
        };
        Some(match login.token.is_empty() {
            true => Account::turned_away(&who, Message::SignOut),
            false => {
                let mut a = Account::signed_in(&who, Message::SignOut);
                let settings = arkui::menu::Entry::run(arkui::glyphs::LIBRARY, "Settings", Message::Select(Source::Settings));
                a.entries.insert(0, settings);
                a
            }
        })
    }

    /// Where the signed-in person's picture is loaded from: the server's
    /// `/auth/picture`, which fetches it from their provider — a page
    /// cannot, from a provider that does not answer it with CORS, so the
    /// desktop goes the same way and the two are one path. None while
    /// nobody is signed in, or their provider has no picture of them.
    pub fn account_picture_url(&self) -> Option<String> {
        let login = self.login.as_ref()?;
        ark_auth::picture_url(&self.server, &login.user.id, &login.user.picture)
    }

    /// The picture the account button draws, once it has arrived.
    pub fn account_picture(&self) -> Option<&iced::widget::image::Handle> {
        self.covers.handle(&self.account_picture_url()?)
    }

    /// Ask for the signed-in person's picture: once a login, the cache
    /// knowing it from then on.
    fn want_picture(&mut self) -> Task<Message> {
        match self.account_picture_url().and_then(|url| self.covers.want(url)) {
            Some(t) => t.map(Message::Cover),
            None => Task::none(),
        }
    }

    /// Where the account button ends: the page's right-hand edge.
    pub fn account_right(window: Size) -> f32 {
        window.width - PAGE_PADDING
    }

    /// Where the ⋯ column ends: the list's right-hand edge, less the page's
    /// gutter and the scrollbar — so a menu opened from it ends where the
    /// button is.
    pub fn dots_right(window: Size) -> f32 {
        window.width - GUTTER - arkui::SCROLLBAR
    }

    /// Where the device picker goes: hanging off the speaker at the right-hand
    /// end of the play bar, and off nothing else — no pointer, so it cannot
    /// slide about under the hand that opened it.
    pub fn devices_origin(window: Size, rows: usize) -> Point {
        arkui::panel::hang_above(
            window.width - PAGE_PADDING,
            window.height - BAR_HEIGHT,
            DEVICES_WIDTH,
            Self::devices_height(rows),
        )
    }

    /// The device picker's height: its padding, its title line and a row per
    /// device — the panel's own numbers.
    pub fn devices_height(rows: usize) -> f32 {
        arkui::panel::titled_height(rows)
    }

    fn open_picker(&mut self) -> Task<Message> {
        // Whatever the menu was opened for, or the track under the cursor.
        // The menu stays up: this is its submenu.
        let wanted = match self.ctx.menu.is_some() {
            true => self.menu_about,
            false => None,
        };
        let found = wanted.or_else(|| self.row_under_cursor()).and_then(|id| self.peer.row(id)).cloned();
        let Some(item) = found else {
            return Task::none();
        };
        let on: Vec<Id> = self.peer.playlists_of(item.id).into_iter().map(|p| p.id).collect();
        let choices = self
            .peer
            .playlists()
            .iter()
            .map(|p| Choice {
                on: on.contains(&p.id),
                value: Pick::Playlist(p.id),
                name: p.name.clone(),
            })
            .collect();
        self.picker_about = Some(item.id);
        self.voice_about = None;
        self.ctx
            .open_picker(Picker::new(item.title, choices, Some(NEW_PLAYLIST.into())), self.window);
        Task::none()
    }

    /// Who the voice saying line `at` of the transcript is: the episode's
    /// people, whoever it is ticked, and anybody else by name.
    fn open_voice_picker(&mut self, at: usize) -> Task<Message> {
        let (Some(now), Some(line)) = (self.now().map(|n| n.track().id.0), self.cues().into_iter().nth(at)) else {
            return Task::none();
        };
        if line.voice.is_empty() {
            return Task::none();
        }
        let mut names: Vec<String> = self.peer.people_of(now).into_iter().map(|p| p.name).collect();
        if line.speaker != line.voice && !names.contains(&line.speaker) {
            names.push(line.speaker.clone());
        }
        let choices = names
            .into_iter()
            .map(|name| Choice {
                on: name == line.speaker,
                value: Pick::Person(name.clone()),
                name,
            })
            .collect();
        self.picker_about = None;
        self.voice_about = Some((now, line.voice.clone()));
        let title = format!("Who is {}?", line.speaker);
        self.ctx.open_picker(Picker::new(title, choices, Some(SOMEBODY_ELSE.into())), self.window);
        Task::none()
    }

    /// The voice the picker is about named; nobody, to take it back.
    fn name_voice(&mut self, name: String) {
        let Some((media, voice)) = self.voice_about.take() else {
            return;
        };
        let what = match name.trim().is_empty() {
            true => format!("forget who {voice} is"),
            false => format!("say {voice} is {}", name.trim()),
        };
        let input = NameVoice {
            media_id: key(media),
            voice,
            name,
        };
        let _ = self.author(what, "name_voice", &input);
        self.ctx.close_picker();
    }

    /// Open what the cursor is on.
    fn activate(&mut self) -> Task<Message> {
        let at = self.at(self.pane);
        match self.pane {
            // The cursor already chose it on the way past, so opening means
            // "and now I want to be in it".
            Pane::Sidebar => {
                self.show_under_cursor();
                self.pane = Pane::Tracks;
                self.reveal()
            }
            Pane::Tracks => {
                // A name, a button, a record on a shelf: what its click does.
                if let Some(m) = self.layout().message(at).cloned() {
                    return self.update(m);
                }
                if let Some(source) = self.card_under_cursor(at) {
                    return self.update(Message::Select(source));
                }
                // A transcript's line: the episode from there.
                if self.peer.source == Source::Transcript {
                    return match self.cues().get(at) {
                        Some(c) => self.update(Message::Cue(c.start_ms)),
                        None => Task::none(),
                    };
                }
                match self.peer.rows().get(at).map(|i| i.id) {
                    Some(id) => self.update(Message::PlayItem(id)),
                    None => Task::none(),
                }
            }
        }
    }

    /// A cover's URL, from what the log carries — spelt exactly as
    /// `media.file`, so it is the same join.
    pub fn art_url(&self, art: &str) -> String {
        match art.is_empty() {
            true => String::new(),
            false => media_url(&self.server, art),
        }
    }

    /// Ask for every cover the index pages would draw — all of them, because
    /// scrolling does not go through `update` and a cover that starts loading
    /// only once visible is never there when you look. `want` is idempotent.
    fn want_covers(&mut self) -> Task<Message> {
        let p = &self.peer;
        let urls: Vec<String> = p
            .albums()
            .iter()
            .map(|a| a.art.clone())
            .chain(p.artists().iter().map(|a| a.art.clone()))
            .chain(p.composers().iter().map(|c| c.art.clone()))
            .chain(p.works().iter().map(|w| w.art.clone()))
            .chain(p.recordings().iter().map(|r| r.art.clone()))
            .chain(p.shows().iter().map(|s| s.art.clone()))
            .chain(self.faces_here())
            .filter(|art| !art.is_empty())
            .map(|art| media_url(&self.server, &art))
            .collect();
        let banners: Vec<String> = p
            .artists()
            .iter()
            .map(|a| a.banner.clone())
            .chain(p.composers().iter().map(|c| c.banner.clone()))
            .filter(|b| !b.is_empty())
            .map(|b| media_url(&self.server, &b))
            .collect();
        let mut tasks: Vec<Task<Message>> = urls
            .into_iter()
            .filter_map(|url| self.covers.want(url))
            .map(|t| t.map(Message::Cover))
            .collect();
        tasks.extend(
            banners
                .into_iter()
                .filter_map(|url| self.banners.want(url))
                .map(|t| t.map(Message::Banner)),
        );
        Task::batch(tasks)
    }

    /// The pictures of everybody on the page's episodes — a show's, or the
    /// ones a person is on, whose own is among them.
    fn faces_here(&self) -> Vec<String> {
        let p = &self.peer;
        let episodes = p.rows().iter().chain(p.appearances()).filter(|i| i.kind != "song");
        episodes.flat_map(|i| p.people_of(i.id)).map(|person| person.art).collect()
    }

    /// The works a composer's page lists, in the order it lists them: every
    /// one by catalogue when there are few or "See All" was pressed, and
    /// otherwise the best known ([`known`]) up to [`KNOWN_WORKS`].
    pub fn listed_works(&self) -> Vec<&rows::Work> {
        let works = self.peer.works();
        match self.all_works || works.len() <= KNOWN_WORKS {
            true => works.iter().collect(),
            false => known(works).into_iter().take(KNOWN_WORKS).collect(),
        }
    }

    /// What the cursor opens on this page, if the page has things to open:
    /// `None` is a page where `<Enter>` plays.
    pub fn card_under_cursor(&self, at: usize) -> Option<Source> {
        let p = &self.peer;
        match &p.source {
            Source::Albums => p.albums().get(at).map(|a| Source::Album(a.id.clone(), a.name.clone())),
            Source::Artists => p.artists().get(at).map(|a| Source::Artist(a.name.clone())),
            Source::Composers => p.composers().get(at).map(|c| Source::Artist(c.name.clone())),
            Source::Podcasts => p.shows_of("podcast").get(at).map(|s| Source::Show(s.id.clone(), s.title.clone())),
            Source::Sermons => p.shows_of("sermon").get(at).map(|s| Source::Show(s.id.clone(), s.title.clone())),
            Source::Artist(_) if !p.works().is_empty() => self.listed_works().get(at).map(|w| Source::Work(w.id.clone(), w.title.clone())),
            Source::Work(..) => p.recordings().get(at).map(|r| Source::Recording(r.id.clone(), r.performers.clone())),
            _ => None,
        }
    }

    /// Move through the queue, stopping at either end rather than wrapping —
    /// a list that loops silently is hard to tell from one that is stuck.
    fn skip(&mut self, delta: i32) {
        let next = self.playing_at() as i32 + delta;
        if self.player.track().is_none() || next < 0 || next as usize >= self.queue.len() {
            return;
        }
        self.start_at(next as usize, 0, true);
    }

    /// Where in the queue the player is; zero when nothing is playing.
    fn playing_at(&self) -> usize {
        loaded_at(&self.player, &self.queue).unwrap_or(0)
    }

    /// Put this device's player on `at` in the queue: the one place a queued
    /// track becomes a sound, so a hand-off and a click land in the same code.
    fn start_at(&mut self, at: usize, position_ms: i64, playing: bool) {
        if at < self.queue.len() && !play_at(&mut self.player, &self.server, &self.queue, at, position_ms, playing) {
            self.note = "nothing to stream — this one has no file".into();
        }
    }

    /// Where a transport button goes — the bar, a key, a row, a lock
    /// screen — and the only place that is decided.
    ///
    /// Part of the session, it is a transition, made by anybody and obeyed
    /// by the device the session names as its output — this one, or another
    /// — through nothing but the session ([`App::react`] reconciles). Its own
    /// session, unlinked, it is done here, to this device's player.
    fn press(&mut self, press: Press) -> Task<Message> {
        use harken_player::Press as P;
        let p = match press {
            Press::PlayPause => P::Toggle,
            Press::Play => P::Play,
            Press::Pause => P::Pause,
            Press::Skip(by) => P::Skip { by: by as i64 },
            Press::Seek(ms) => P::Seek { ms },
            Press::Start(at, position_ms) => P::Start {
                queue: self.queue.clone(),
                at: at as i64,
                position_ms,
                from: self.from(),
            },
        };
        if self.listening.session(&self.peer.client.lock()).is_none() {
            self.obey(press);
        } else {
            self.call(&p);
        }
        Task::none()
    }

    /// A press, as the session's transition, a refusal noted; this device's
    /// own player is what a play presses when the session has none.
    fn call(&mut self, press: &harken_player::Press) {
        let App {
            listening,
            peer,
            player,
            server,
            queue,
            queued_from,
            note,
            ..
        } = self;
        let local = Sounding::new(player, server, queue, queued_from);
        if let Err(why) = listening.call(&mut peer.client.lock(), press, Some(&local)) {
            *note = why;
        }
    }

    /// What is playing now: the one state the bar draws, the list lights,
    /// the queue panel starts from and the tab's title and the media
    /// session say. Part of a session, the session's — its queue, its
    /// track, its clock counted against the server's — whichever device
    /// sounds it, so a window watching a phone says the phone's track; its
    /// own session, this device's player in its own queue.
    /// Whether what plays is an episode of a show — no album, and its
    /// creator a show's title — whose transport skips back and on a few
    /// seconds rather than a track.
    pub fn jogs(&self) -> bool {
        self.now().is_some_and(|n| {
            let t = n.track();
            t.album.is_empty() && self.peer.shows().iter().any(|s| s.title == t.creator)
        })
    }

    /// What the transport's left and right do: a track either way, or, for
    /// an episode, back [`JOG_BACK`] and on [`JOG_FORWARD`] seconds.
    pub fn back(&self) -> Message {
        if self.jogs() {
            Message::Jog(-JOG_BACK)
        } else {
            Message::Skip(-1)
        }
    }

    pub fn forward(&self) -> Message {
        if self.jogs() {
            Message::Jog(JOG_FORWARD)
        } else {
            Message::Skip(1)
        }
    }

    pub fn now(&self) -> Option<Now<'_>> {
        match &self.session {
            Some(s) => {
                let a = s.session.as_ref()?;
                let at = (a.at.max(0) as usize).min(a.queue.len().checked_sub(1)?);
                Some(Now {
                    queue: &a.queue,
                    at,
                    playing: a.clock.playing(),
                    position: listening::Remote::position_ms(a) as f64 / 1000.0,
                    duration: a.queue[at].duration_ms as f64 / 1000.0,
                })
            }
            None => Some(Now {
                queue: &self.queue,
                at: loaded_at(&self.player, &self.queue)?,
                playing: self.player.is_playing(),
                position: self.player.position(),
                duration: self.player.duration(),
            }),
        }
    }

    /// The window's title, the tab's in a browser: [`App::now`]'s track
    /// and who made it, or the app's name.
    pub fn title(&self) -> String {
        match self.now().map(|n| n.track()) {
            Some(t) if t.creator.is_empty() => t.title.clone(),
            Some(t) => format!("{} \u{2014} {}", t.title, t.creator),
            None => "harken".into(),
        }
    }

    /// Where a queue was pressed from: the page it was on, as the address
    /// bar names it.
    fn from(&self) -> String {
        use arkui::route::Route;
        self.peer.source.place().fragment()
    }

    /// Start `rows` at `at` (the first when it is not among them), the queue
    /// `rows` themselves, taken now: skipping follows the list you pressed
    /// play in. The rows are copied rather than referred to — a device handed
    /// the session may not have that album yet.
    fn play_rows(&mut self, rows: &[rows::Item], at: Option<Id>, from: String) -> Task<Message> {
        let Some(at) = self.load_queue(rows, at) else {
            return Task::none();
        };
        // An episode begun picks up where its listener left it.
        let position = self.queue.get(at).and_then(|t| self.peer.mine_of(t.id.0)).map_or(0, |l| l.resume_ms());
        self.queued_from = from;
        self.press(Press::Start(at, position))
    }

    /// `rows` as the queue, and where `at` is in it: none for no rows.
    fn load_queue(&mut self, rows: &[rows::Item], at: Option<Id>) -> Option<usize> {
        if rows.is_empty() {
            return None;
        }
        self.queue = rows
            .iter()
            .map(|i| listening::Track {
                id: key(i.id),
                title: i.title.clone(),
                creator: i.creator.clone(),
                album: self.peer.detail_of(i.id).album,
                duration_ms: i.duration_ms,
                // The path as the log carries it: each device joins it to
                // *its* server.
                file: i.file.clone(),
            })
            .collect();
        Some(self.queue.iter().position(|t| Some(t.id.0) == at).unwrap_or(0))
    }

    /// Do it here: this device is its own session, unlinked.
    fn obey(&mut self, press: Press) {
        match press {
            Press::PlayPause if self.player.is_playing() => self.player.pause(),
            Press::PlayPause | Press::Play => self.player.resume(),
            Press::Pause => self.player.pause(),
            Press::Skip(by) => self.skip(by),
            Press::Seek(ms) => self.player.seek(ms as f64 / 1000.0),
            Press::Start(at, position_ms) => self.start_at(at, position_ms, true),
        }
    }

    /// Do the thing, then whatever follows from it ([`App::react`]), then make
    /// the address bar agree with what is on screen — here, after every
    /// message, rather than at the places that change what is shown: a rule
    /// each call site has to remember is already broken.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.step(message);
        if !self.held {
            auth::reveal();
        }
        let reacted = self.react();
        self.router.sync(&self.peer.source.place());
        Task::batch([task, reacted, self.want_covers_if_moved(), self.want_picture()])
    }

    /// Ask for covers when the lists have been rebuilt since the last ask.
    fn want_covers_if_moved(&mut self) -> Task<Message> {
        if self.peer.art_gen == self.art_seen {
            return Task::none();
        }
        self.art_seen = self.peer.art_gen;
        self.want_covers()
    }

    fn step(&mut self, message: Message) -> Task<Message> {
        // An entry that goes somewhere was picked: the menu has said all it
        // had to say, and its submenu goes with it.
        if matches!(
            message,
            Message::PlayItem(_) | Message::PlayRecording(_) | Message::PlayWork(_) | Message::Select(_) | Message::SignOut
        ) {
            self.ctx.close_menu();
        }
        match message {
            Message::ConnectUrl(url) => self.connect = url,
            Message::Started(server) => {
                #[cfg(not(feature = "demo"))]
                {
                    let (mut app, task) = match server {
                        Some(server) => App::open_server(server, self.user.clone()),
                        None => (App::alone(self.user.clone()), Task::none()),
                    };
                    app.window = self.window;
                    *self = app;
                    self.arrive();
                    return task;
                }
                #[cfg(feature = "demo")]
                let _ = server;
            }
            Message::Connect => return self.connect(),
            Message::SignedIn(outcome) => match outcome {
                Ok(login) => {
                    self.held = false;
                    self.signed_in(login);
                }
                // Said once, and not tried again until a reload: going
                // straight back would be a loop the person cannot stop.
                Err(e) => {
                    (self.sign_in_tried, self.held) = (true, false);
                    self.note = format!("could not sign in: {e} \u{2014} reload to try again");
                }
            },
            Message::Resync => {
                // A full look: the server walks its music again and syncs
                // its catalogue. Refused, `author` says why on the status
                // line; only the library's role may ask.
                let asked = self.author("resync the library".into(), "look", &crate::domain::LookAt { paths: Vec::new() });
                self.resync = Some(match asked {
                    true => "Asked. New music appears as the server finds it.".into(),
                    false => self.note.clone(),
                });
            }
            Message::Adding(true) => {
                self.adding = Some(Adding::new(self.shows_kind()));
                self.adding_leaving = None;
                // The keys go in the box: what opening it was for is typing.
                return iced::widget::operation::focus(QUERY);
            }
            // Closed, it fades out as it came, drawn but no longer the
            // keyboard's.
            Message::Adding(false) => {
                if let Some(mut leaving) = self.adding.take() {
                    leaving.shown.hide(iced::time::Instant::now());
                    self.adding_leaving = Some(leaving);
                }
            }
            Message::Query(query) => {
                let Some(adding) = self.adding.as_mut() else {
                    return Task::none();
                };
                adding.query = query.clone();
                adding.said = None;
                adding.at = 0;
                if directory::is_feed(&query) || query.trim().is_empty() {
                    adding.found.clear();
                    return Task::none();
                }
                return directory::after(directory::PAUSE).map(move |()| Message::SearchDue(query.clone()));
            }
            Message::AddingAt(at) => {
                if let Some(adding) = self.adding.as_mut() {
                    adding.at = at;
                }
            }
            Message::SearchDue(words) => {
                // Typed on since: a later pause searches for that.
                let Some(adding) = self.adding.as_mut().filter(|a| a.query == words) else {
                    return Task::none();
                };
                if directory::is_feed(&words) || words.trim().is_empty() {
                    return Task::none();
                }
                adding.said = Some("Searching\u{2026}".into());
                return directory::search(words).map(|(words, answer)| Message::Searched(words, answer));
            }
            Message::Searched(words, answer) => {
                let Some(adding) = self.adding.as_mut().filter(|a| a.query == words) else {
                    return Task::none();
                };
                match answer {
                    Ok(found) => {
                        adding.said = found.is_empty().then(|| format!("Nothing found for \u{201c}{}\u{201d}.", words.trim()));
                        adding.found = found;
                    }
                    Err(why) => {
                        adding.said = Some(why);
                        adding.found.clear();
                    }
                }
                adding.at = 0;
                let arts: Vec<String> = adding.found.iter().map(|f| f.art.clone()).filter(|a| !a.is_empty()).collect();
                let wanted: Vec<_> = arts
                    .into_iter()
                    .filter_map(|art| self.covers.want(art))
                    .map(|t| t.map(Message::Cover))
                    .collect();
                return Task::batch(wanted);
            }
            Message::FocusQuery => return iced::widget::operation::focus(QUERY),
            Message::FollowFeed(url) => {
                let kind = self.adding.as_ref().map_or(self.shows_kind(), |a| a.kind);
                let input = crate::domain::Subscribe {
                    kind: kind.into(),
                    url: url.trim().to_string(),
                };
                let followed = self.author("follow the feed".into(), "subscribe", &input);
                let note = self.note.clone();
                if let Some(adding) = self.adding.as_mut() {
                    adding.said = (!followed).then_some(note);
                }
            }
            Message::ShowMenu(id) => {
                let Some(sub) = self.peer.subscription_of(&id) else {
                    return Task::none();
                };
                let entries = vec![arkui::menu::Entry::run(glyphs::CLOSE, "Unfollow", Message::Unfollow(sub.url.clone()))];
                let title = self.peer.shows().iter().find(|s| s.id == id).map_or(sub.url.clone(), |s| s.title.clone());
                self.ctx
                    .open_menu(Menu::open(title, entries, self.cursor, self.window, menu::Anchor::Pointer));
                self.menu_about = None;
            }
            Message::Unfollow(url) => {
                let _ = self.author("unfollow the feed".into(), "unsubscribe", &crate::domain::Unsubscribe { url });
            }
            Message::FetchOlder(url, more) => {
                let _ = self.author("fetch older episodes".into(), "fetch_older", &crate::domain::FetchOlder { url, more });
            }
            Message::SignOut => {
                // The session revoked through the domain (pushed while
                // online, pending until the next sign-in otherwise), and
                // nothing dialled after it. The replica stays, and goes on
                // authoring as that person, offline: what they do now is
                // theirs when they sign in again.
                if self.login.take().is_some() {
                    let _ = self.peer.client.lock().sign_out();
                }
                if let Some(logins) = &self.logins {
                    logins.forget(&self.server);
                }
                // A device is a login, so signing out is leaving the session.
                self.listening.close();
                self.devices = None;
                self.note = "signed out — what you do now stays on this device; reload to sign in".into();
            }
            Message::AccountMenu => {
                if let Some(account) = self.account() {
                    self.ctx
                        .open_menu(account.menu(Self::account_right(self.window), ACCOUNT_BOTTOM, self.window));
                    self.menu_about = None;
                }
            }
            // With the command box up the keys are its: what it does not
            // answer itself (`CommandKey`) goes into its box. Ctrl+K is
            // always its.
            Message::Key(..) if self.command.is_some() => {}
            Message::Key(key, mods) if arkui::command::opens(&key, mods) => {}
            Message::CommandKey(press) => return self.command_key(press),
            Message::CommandQuery(query) => {
                if let Some(c) = self.command.as_mut() {
                    return c.typed(query, COMMAND_LIST);
                }
            }
            Message::CommandAt(at) => {
                if let Some(c) = self.command.as_mut() {
                    c.land(at);
                }
            }
            Message::CommandScrolled(y) => {
                if let Some(c) = self.command.as_mut() {
                    c.scrolled(y);
                }
            }
            Message::Command(chosen) => {
                self.close_command();
                return self.step(*chosen);
            }
            Message::CommandSong(id) => {
                // Its album's songs, in the library's order; a song on no
                // album alone.
                let album = self.peer.detail_of(id).album;
                let mut rows: Vec<_> = match album.is_empty() {
                    true => vec![],
                    false => self
                        .peer
                        .items()
                        .iter()
                        .filter(|i| i.kind == "song" && self.peer.detail_of(i.id).album == album)
                        .cloned()
                        .collect(),
                };
                if !rows.iter().any(|r| r.id == id) {
                    rows = self.peer.row(id).cloned().into_iter().collect();
                }
                return self.play_rows(&rows, Some(id), self.from());
            }
            Message::Key(key, mods) => {
                self.keyed = Some(self.cursor);
                // `None` is still typing: a count, a `g`, a search — drawn in
                // the status line so nothing swallowed is a mystery.
                let action = self.keys.press(&key, mods);
                // harken selects no ranges: visual mode is never left on, so `i`
                // stays the app's.
                self.keys.leave_visual();
                // A search typed on the Songs page narrows it as it is typed
                // (`docs/plan-db.md` D4): the domain's `search`, a view over
                // the needle so far. `<Enter>` keeps it and lands on the first
                // row; `<Esc>` widens the page back to the library.
                if let (vim::Mode::Search(q), Pane::Tracks, Source::Library) = (self.keys.mode(), self.pane, &self.peer.source) {
                    let q = q.clone();
                    self.narrow(Some(&q));
                }
                if let Some(action) = action {
                    return self.act(action);
                }
                return Task::none();
            }
            Message::Resized(size) => {
                self.window = size;
            }
            Message::Wheel(delta) => {
                self.swipe.allow(self.router.can_go(-1), self.router.can_go(1));
                self.swipe.feed(delta, iced::time::Instant::now());
            }
            Message::HoverAt(at) => {
                // Not while a context window is up. A backdrop stops a click
                // and a wheel, but a hover is published by the row itself and
                // falls through every layer — so the cursor the menu is about
                // would creep away under it.
                if !matches!(self.focus(), Focus::Pane(_)) || self.command.is_some() {
                    return Task::none();
                }
                // Nor while the keys are driving and the pointer has not moved.
                if self.keyed.is_some() {
                    return Task::none();
                }
                // Takes the keyboard as well as the highlight: the cursor is
                // only drawn in the pane that has it.
                self.pane = Pane::Tracks;
                self.cursors[Pane::Tracks as usize] = at;
            }
            Message::Cover(done) => self.covers.loaded(done),
            Message::Banner(done) => self.banners.loaded(done),
            Message::Hover(at) => {
                if self.keyed.is_some_and(|k| k != at) {
                    self.keyed = None;
                }
                self.cursor = at;
            }
            Message::MenuAt(at) => self.ctx.land(at),
            Message::MenuActivate => {
                if let Some(m) = self.ctx.chosen() {
                    return self.update(m);
                }
            }
            Message::CloseMenu => self.ctx.close_menu(),
            Message::RowMenu(id, anchor) => {
                // The selection moves to what the menu is about before the
                // menu takes the keyboard off it — or the window draws a menu
                // about one row and a highlight on another.
                let row = self.peer.rows().iter().position(|i| i.id == id).or_else(|| {
                    let k = self.peer.appearances().iter().position(|i| i.id == id)?;
                    self.layout().cell(nav::Mark::Episode(k))
                });
                if let Some(row) = row {
                    self.pane = Pane::Tracks;
                    self.cursors[Pane::Tracks as usize] = row;
                }
                if let Some((title, entries)) = self.row_entries(id) {
                    let anchor = match anchor {
                        Anchor::Pointer => menu::Anchor::Pointer,
                        Anchor::Dots => menu::Anchor::RightEdge(Self::dots_right(self.window)),
                    };
                    self.ctx.open_menu(Menu::open(title, entries, self.cursor, self.window, anchor));
                    self.menu_about = Some(id);
                }
            }
            Message::OpenPicker => return self.open_picker(),
            Message::NameVoice(at) => return self.open_voice_picker(at),
            Message::ClosePicker => self.ctx.close_picker(),
            Message::PickerAt(at) => self.ctx.picker_at(at),
            Message::PickerName(name) => {
                if let Some(picker) = &mut self.ctx.picker {
                    picker.naming = Some(name);
                }
            }
            Message::PickerActivate => match self.ctx.activate_picker() {
                // A person ticked is who the voice is; unticked, nobody.
                Some(Picked::Toggled {
                    value: Pick::Person(name),
                    on,
                }) => self.name_voice(if on { name } else { String::new() }),
                Some(Picked::Toggled {
                    value: Pick::Playlist(value),
                    on,
                }) => {
                    let Some(media) = self.picker_about else {
                        return Task::none();
                    };
                    let title = self.peer.row(media).map(|i| i.title.clone()).unwrap_or_default();
                    let list = self
                        .peer
                        .playlists()
                        .iter()
                        .find(|p| p.id == value)
                        .map(|p| p.name.clone())
                        .unwrap_or_default();
                    let done = match on {
                        true => {
                            let a = AddToPlaylist {
                                playlist_id: key(value),
                                media_id: key(media),
                            };
                            self.author(format!("put {title} on {list}"), "add_to_playlist", &a)
                        }
                        false => {
                            let a = RemoveFromPlaylist {
                                playlist_id: key(value),
                                media_id: key(media),
                            };
                            self.author(format!("take {title} off {list}"), "remove_from_playlist", &a)
                        }
                    };
                    // Refused at once: the tick the panel shows is not true.
                    if !done {
                        let value = Pick::Playlist(value);
                        if let Some(choice) = self.ctx.picker.as_mut().and_then(|p| p.choices.iter_mut().find(|c| c.value == value)) {
                            choice.on = !on;
                        }
                    }
                }
                // Put the keyboard in the box rather than making somebody
                // reach for the mouse to finish what a key started.
                Some(Picked::Naming) => return arkui::picker::focus_naming(),
                None => {}
            },
            Message::PickerCreate => {
                let Some(name) = self.ctx.picker.as_ref().map(|p| p.naming.clone().unwrap_or_default()) else {
                    return Task::none();
                };
                if self.voice_about.is_some() {
                    if !name.trim().is_empty() {
                        self.name_voice(name);
                    }
                    return Task::none();
                }
                // Rebuilt from the playlists view rather than guessing the new
                // row: its id is chosen inside the mutation, and the refresh
                // has just spliced it in. The track is not added to it here —
                // one tap away, on a row that now exists.
                if self.author(format!("make the playlist {}", name.trim()), "create_playlist", &CreatePlaylist { name }) {
                    self.peer.refresh();
                    let at = self.ctx.picker.as_ref().map(|p| p.at);
                    let _ = self.open_picker();
                    if let (Some(picker), Some(at)) = (self.ctx.picker.as_mut(), at) {
                        picker.land(at);
                    }
                }
            }
            // Play and pause are two verbs, decided against whatever is
            // actually sounding.
            Message::PlayPause => return self.press(Press::PlayPause),
            // A scrubber being dragged: where this device's is, to everybody,
            // as the ephemeral `scrub` — and the seek when it is let go.
            Message::Seek(secs) => {
                self.scrubbing = Some(secs);
                if self.listening.joined(&self.peer.client.lock()) {
                    let at_ms = Some((secs as f64 * 1000.0) as i64);
                    self.call(&harken_player::Press::Scrub { at_ms });
                }
            }
            Message::SeekDone => {
                if let Some(secs) = self.scrubbing.take() {
                    if self.listening.joined(&self.peer.client.lock()) {
                        self.call(&harken_player::Press::Scrub { at_ms: None });
                    }
                    return self.press(Press::Seek((secs as f64 * 1000.0) as i64));
                }
            }
            Message::Skip(delta) => return self.press(Press::Skip(delta)),
            Message::Jog(secs) => {
                if let Some(now) = self.now() {
                    let to = (now.position + secs as f64).clamp(0.0, now.duration.max(0.0));
                    return self.press(Press::Seek((to * 1000.0) as i64));
                }
            }
            // The queue is what is on screen: the list the row is in.
            Message::PlayItem(id) => {
                let rows = self.peer.list_of(id).to_vec();
                return self.play_rows(&rows, Some(id), self.from());
            }
            // An episode from one of its chapters: the page's list, from it.
            Message::PlayFrom(id, ms) => {
                let rows = self.peer.list_of(id).to_vec();
                let Some(at) = self.load_queue(&rows, Some(id)) else {
                    return Task::none();
                };
                self.queued_from = self.from();
                return self.press(Press::Start(at, ms));
            }
            // A line of the transcript: what plays, from there — and the
            // line being said is the one pressed, so it is followed again.
            Message::Cue(ms) => {
                self.following = true;
                return self.press(Press::Seek(ms));
            }
            Message::Follow(follow) => {
                self.following = follow;
                // Put back on the line being said, wherever it was left.
                if follow {
                    self.said = None;
                }
            }
            Message::OpenLink(url) => open_link(&url),
            Message::MarkPlayed(id, finished) => {
                let input = crate::domain::MarkPlayed {
                    media_id: key(id),
                    finished,
                    name: String::new(),
                };
                let what = if finished { "mark it played" } else { "mark it unplayed" };
                let _ = self.author(what.into(), "mark_played", &input);
            }
            // The performance's movements, read when it is pressed: the page
            // it is pressed on lists performances, not tracks.
            Message::PlayRecording(take) => {
                let rows = self.peer.recording_rows(&take);
                let from = arkui::route::Route::fragment(&Place::Recording(take));
                return self.play_rows(&rows, None, from);
            }
            Message::PlayWork(work) => {
                let Some(take) = self.peer.recordings_of(&work).into_iter().next() else {
                    return Task::none();
                };
                return self.update(Message::PlayRecording(take.id));
            }
            Message::AllWorks(all) => {
                self.all_works = all;
                self.cursors[Pane::Tracks as usize] = 0;
            }
            Message::OpenQueue if self.queue_panel.is_some() => self.close_queue(),
            Message::OpenQueue => {
                self.devices = None;
                self.queue_panel = Some(0);
                let now = iced::time::Instant::now();
                match self.queue_sheet.as_mut() {
                    Some(sheet) => sheet.show(now),
                    None => self.queue_sheet = Some(arkui::sheet::shown(now)),
                }
            }
            Message::CloseQueue => self.close_queue(),
            // A row passed under a row being dragged is not the cursor's.
            Message::QueueAt(_) if self.queue_drag.as_ref().is_some_and(|d| d.dragging()) => {}
            Message::QueueAt(at) => self.queue_panel = Some(at),
            Message::QueueGrab(row) => {
                let upcoming = self.queue_rows().len().saturating_sub(1);
                if row < upcoming {
                    self.queue_panel = Some(row + 1);
                    self.queue_settle = None;
                    self.queue_drag = Some(arkui::reorder::Reorder::press(
                        row,
                        upcoming,
                        arkui::panel::ENTRY,
                        self.cursor.y,
                        iced::time::Instant::now(),
                    ));
                }
            }
            Message::QueueDrag(at) => {
                self.cursor = at;
                if let Some(drag) = self.queue_drag.as_mut() {
                    drag.drag(at.y, iced::time::Instant::now());
                }
            }
            Message::QueueDrop => return self.drop_queue_row(),
            Message::QueueJump(_) | Message::QueueRemove(_) => {
                self.edit_queue(&message);
                // The cursor stays on a row there is.
                let n = self.queue_rows().len();
                if let Some(at) = self.queue_panel.as_mut() {
                    *at = (*at).min(n.saturating_sub(1));
                }
            }
            Message::QueueMove(from, to) => {
                self.edit_queue(&message);
                // The cursor goes with the track it moved.
                if let Some(at) = self.queue_panel.as_mut() {
                    *at = if to > from { *at + 1 } else { at.saturating_sub(1) };
                }
            }
            Message::PlayNext(id) => self.enqueue(id, true),
            Message::AddToQueue(id) => self.enqueue(id, false),
            Message::OpenDevices => {
                self.close_queue();
                // The cursor starts on the device that has the sound — or,
                // while it is nowhere, on this one — so `<Enter>` straight
                // away changes nothing.
                let output = self.session.as_ref().and_then(|s| s.output()).and_then(|o| o.id()).map(str::to_string);
                let id = output.unwrap_or_else(|| self.listening.me().to_string());
                let at = self.picker_rows().iter().position(|r| r.id == id).unwrap_or(0);
                self.devices = Some(at);
            }
            Message::DeviceAt(at) => self.devices = Some(at),
            Message::PickDevice(to) => {
                self.call(&harken_player::Press::Transfer { to });
                self.devices = None;
            }
            Message::CloseDevices => self.devices = None,
            // A slider dragged: drawn where it is at once, and sent at most
            // once a throttle's window — the last value whenever it lands.
            // `call_throttled` sends an ordered call at once, its order being
            // the point of it, so the throttle for a volume is this window's.
            Message::Volume(level, to) => {
                self.volume_drag = Some((level.clone(), to));
                self.volume_held = Some((level, to.round() as i64));
                self.release_volume(false);
            }
            Message::VolumeDone => {
                self.release_volume(true);
                self.volume_drag = None;
            }
            Message::Mute(level) => self.change_volume(&level, Change::Mute),
            Message::Nudge(level, by) => self.change_volume(&level, Change::By(by)),
            Message::Join(id) => {
                self.call(&harken_player::Press::Join { device: id });
            }
            Message::Unjoin(id) => {
                self.call(&harken_player::Press::Unjoin { device: id });
            }
            Message::Swallow => {}
            Message::Select(source) => {
                // A click moves the cursor as well, or the keyboard would carry
                // on from wherever it was.
                let at = self.peer.choices.iter().position(|c| c.source == source);
                self.peer.source = source;
                self.peer.search = None;
                self.all_works = false;
                self.peer.open_page();
                if let Some(at) = at {
                    self.cursors[Pane::Sidebar as usize] = at;
                }
                // Always: landing on row seventeen of a record you just opened
                // is a cursor that remembers the wrong list.
                self.cursors[Pane::Tracks as usize] = 0;
                // A new visit opens at the top; back and forward where it was.
                let back_to = match std::mem::take(&mut self.travelling) {
                    true => self.scrolls.get(&self.peer.source.place()).copied(),
                    false => None,
                };
                self.scroll_due = Some(back_to.unwrap_or(0.0));
                // A transcript opens following the line being said, its
                // cursor on it — put in the middle once the page is drawn,
                // as any page's scroll is.
                if self.peer.source == Source::Transcript {
                    self.following = true;
                    self.said = self.cue_now();
                    self.cursors[Pane::Tracks as usize] = self.said.unwrap_or(0);
                    self.scroll_due = None;
                    return Task::done(Message::Follow(true));
                }
            }
            Message::PageScrolled(y) => {
                self.scrolls.insert(self.peer.source.place(), y);
            }
            Message::ScrollPage(y) => {
                return iced::widget::operation::scroll_to(TRACKS, iced::widget::scrollable::AbsoluteOffset { x: 0.0, y });
            }
            // A submenu opens by being pointed at, after a beat: arkui counts
            // the rest and answers the entry's message, and the picker it opens
            // is shown rather than entered. The clock runs only while it can.
            Message::Tick => {
                self.release_volume(false);
                // A swipe let go far enough: the bar moves, and the next
                // update arrives where it went.
                if let Some(by) = self.swipe.tick(iced::time::Instant::now()) {
                    self.router.go(by);
                }
                self.settle_motion(iced::time::Instant::now());
                if let Some(m) = self.ctx.tick() {
                    return self.update(m);
                }
            }
            // What woke the window is whatever `react` finds.
            Message::Wake | Message::Due => {}
        }
        Task::none()
    }

    /// What every update ends with, whatever the message was
    /// (`docs/plan-reactive.md`): the peer pumped — frames in, the
    /// settle, what this window authored sent — the deadlines looked at, the
    /// listening session moved both ways, the player's own changes noticed,
    /// and the address bar read. It used to be the 50 ms tick's work; it is
    /// done when something has happened, which is the only time any of it
    /// has anything to do.
    fn react(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        tasks.push(self.pump());
        // A lock-screen button leaves a note and rings; it goes where every
        // transport button goes.
        #[cfg(target_arch = "wasm32")]
        {
            let press = match self.player.take_remote() {
                Some(Remote::Play) => Some(Press::Play),
                Some(Remote::Pause) => Some(Press::Pause),
                Some(Remote::Next) if self.jogs() => {
                    tasks.push(self.update(Message::Jog(JOG_FORWARD)));
                    None
                }
                Some(Remote::Previous) if self.jogs() => {
                    tasks.push(self.update(Message::Jog(-JOG_BACK)));
                    None
                }
                Some(Remote::Next) => Some(Press::Skip(1)),
                Some(Remote::Previous) => Some(Press::Skip(-1)),
                Some(Remote::Seek(secs)) => Some(Press::Seek((secs * 1000.0) as i64)),
                None => None,
            };
            if let Some(press) = press {
                tasks.push(self.press(press));
            }
        }
        // The listening session. A track that ran out moves on through the
        // queue, as a phone's player does — its own session, or the output,
        // whose move the reconciler says — and never on a silent device.
        let s = self.listening.session(&self.peer.client.lock());
        if self.player.ended() && s.is_none_or(|s| self.listening.outputs_here(&s)) {
            self.skip(1);
        }
        let App {
            listening,
            peer,
            player,
            server,
            queue,
            queued_from,
            ..
        } = self;
        let mut sound = Sounding::new(player, server, queue, queued_from);
        if let Some(why) = listening.reconcile(&mut peer.client.lock(), &mut sound).pop() {
            self.note = format!("the session refused that: {why}");
        }
        self.session = self.listening.session(&self.peer.client.lock());
        // The back button, and a link somebody was sent — consumed only once
        // the sidebar has something to resolve it against, which is why it is
        // asked after every update and not only when the bar moved.
        let ready = !self.peer.choices.is_empty();
        if let Some(place) = self.router.poll(ready) {
            let wanted = self.peer.source_of(&place);
            if wanted != self.peer.source {
                self.travelling = true;
                tasks.push(self.update(Message::Select(wanted)));
            }
        }
        // A page just opened is drawn before this message comes back, so the
        // scroll lands on its list rather than the one it replaced.
        if let Some(y) = self.scroll_due.take() {
            tasks.push(Task::done(Message::ScrollPage(y)));
        }
        // The line being said moves on as what plays does.
        tasks.push(self.follow());
        #[cfg(target_arch = "wasm32")]
        {
            let now = self.now().map(|n| (n.track().clone(), n.playing, n.duration, n.position));
            self.player.announce(&self.title(), now);
        }
        Task::batch(tasks)
    }

    /// The one timer this window may need for something that has not
    /// happened: the peer's redial, when the window pumps it by hand — a
    /// driven peer keeps its own.
    pub fn deadline(&self) -> Option<u64> {
        self.peer.client.deadline()
    }

    /// How often the window needs a clock, if at all: only while something on
    /// screen moves without anything having happened. A submenu's dwell is
    /// an animation (50 ms); audio
    /// playing here, or on another device this one is drawing, moves the
    /// scrubber and the media session's position, and `D` counts how long ago
    /// the room spoke (250 ms). `None` is a window at rest, which runs no
    /// timer at all.
    pub fn clock(&self) -> Option<Duration> {
        // A volume the throttle holds is sent by the clock, the last value
        // of a drag that stopped moving without being let go.
        // A swipe under way draws a frame at a time: the page follows the
        // fingers, and springs back.
        if self.swipe.active() {
            return Some(Duration::from_millis(16));
        }
        // …and so does a dialog or a sheet coming or going, and a row of
        // the queue sliding out of the way of one dragged, or settling.
        if self.moving(iced::time::Instant::now()) {
            return Some(Duration::from_millis(16));
        }
        if self.ctx.dwelling() || self.volume_held.is_some() {
            return Some(Duration::from_millis(50));
        }
        let sounding = self.now().is_some_and(|n| n.playing);
        (sounding || self.debug).then_some(Duration::from_millis(250))
    }

    /// Move frames, and bring the lists up to date with whatever moved —
    /// what this window authored, or what arrived.
    fn pump(&mut self) -> Task<Message> {
        let pumped = self.peer.client.pump();
        let mut turned_away = Task::none();
        if let Some(reason) = pumped.denied {
            // Turned away from the other end — an expired token, a revoked
            // session. The replica and the pending work stay; signing in again
            // as the same person offers them.
            self.note = format!("signed out by the server: {reason}");
            if let Some(logins) = &self.logins {
                logins.forget(&self.server);
            }
            if let Some(login) = &mut self.login {
                login.token.clear();
            }
            // Straight back to sign in, once a load (`sign_in_unasked`).
            if self.sign_in_tried {
                self.note = format!("{} \u{2014} reload to sign in again", self.note);
            }
            turned_away = self.sign_in_unasked();
        }
        for (id, reason) in self.peer.client.lock().take_rejections() {
            let what = self
                .edits
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.what.clone())
                .unwrap_or_else(|| "a change".into());
            self.note = format!("the server refused to {what}: {reason}");
            self.refused.push((what, reason));
        }
        self.peer.refresh();
        turned_away
    }

    /// Everything that can start an update, and no clock unless one is
    /// needed (`wake.rs`, `docs/plan-reactive.md`).
    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            // The socket, and in a browser the page's own events.
            self.bell.subscription().map(|()| Message::Wake),
            iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
            // Only the presses no widget took: a focused text box consumes its
            // own, so typing a playlist's name cannot walk the cursor.
            vim::presses().map(|(key, mods)| Message::Key(key, mods)),
            // Ctrl+K, and the command box's own keys while it is up, whatever
            // has the keyboard: its box takes `<Esc>` for itself.
            arkui::command::keys(self.command.is_some()).map(Message::CommandKey),
            // The wheel, when it moves mostly sideways — whichever widget
            // took it, since a list scrolls down and a swipe is sideways.
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
                    let (x, y) = match delta {
                        iced::mouse::ScrollDelta::Pixels { x, y } | iced::mouse::ScrollDelta::Lines { x, y } => (x, y),
                    };
                    // One that moved nowhere is the fingers lifting, where
                    // the system says so (`arkui::swipe`).
                    (x.abs() > y.abs() || (x == 0.0 && y == 0.0)).then_some(Message::Wheel(delta))
                }
                _ => None,
            }),
        ];
        // One timer for the next deadline, keyed by it: the same deadline on
        // every call is the timer already running. Two milliseconds late
        // rather than one early, so that it is due when it fires.
        if let Some(at) = self.deadline() {
            let delay = Duration::from_millis(at.saturating_sub(client::monotonic_ms()) + 2);
            subs.push(arkui::wake::at(at, delay).map(|()| Message::Due));
        }
        if let Some(every) = self.clock() {
            subs.push(iced::time::every(every).map(|_| Message::Tick));
        }
        // A transcript following the line being said stops when the wheel
        // scrolls it by hand.
        if self.peer.source == Source::Transcript && self.following {
            subs.push(iced::event::listen_with(|event, _, _| match event {
                iced::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
                    let (x, y) = match delta {
                        iced::mouse::ScrollDelta::Pixels { x, y } | iced::mouse::ScrollDelta::Lines { x, y } => (x, y),
                    };
                    (y.abs() > x.abs()).then_some(Message::Follow(false))
                }
                _ => None,
            }));
        }
        Subscription::batch(subs)
    }
}

/// Open somebody's web page outside the app: a tab of the browser the app
/// is in, or the desktop's browser.
fn open_link(url: &str) {
    #[cfg(target_arch = "wasm32")]
    if let Some(window) = web_sys::window() {
        let _ = window.open_with_url_and_target(url, "_blank");
    }
    #[cfg(not(target_arch = "wasm32"))]
    ark_auth::client::open_browser(url);
}

/// A blocking sign-in on a thread of its own — it takes as long as a person
/// does — with the answer handed back as a message.
#[cfg(not(target_arch = "wasm32"))]
fn off_thread(work: impl FnOnce() -> Result<Login, String> + Send + 'static) -> Task<Message> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    Task::perform(
        async move { rx.await.unwrap_or_else(|_| Err("the sign-in thread went away".into())) },
        Message::SignedIn,
    )
}

#[cfg(target_arch = "wasm32")]
fn panic_to_console() {
    // A panic that reaches the console beats one that reports "unreachable
    // executed".
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&info.to_string().into());
    }));
}

pub fn main() -> iced::Result {
    #[cfg(target_arch = "wasm32")]
    panic_to_console();
    arkui::theme::install(palette::HARKEN);
    // What this device does goes to its server (docs/plan-observe.md).
    ark_peer::observe::install(ark_peer::observe::Sink::Outbox);

    iced::application(App::boot, App::update, App::view)
        .subscription(App::subscription)
        .title(App::title)
        .window_size((860.0, 600.0))
        .font(arkui::hero::FIRA_SANS_BOLD)
        .run()
}
