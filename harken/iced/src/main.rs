//! The harken library, in iced — on the desktop and in a browser.
//!
//! One client among several: the phones are the others. They all run the
//! same domain — this one links `harken_domain`'s procedures natively — and
//! none of them contains a line of domain logic.
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
mod explore;
mod listening;
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

#[cfg(test)]
mod tests;

use std::time::Duration;

use ark_auth::Login;
use ark_client::{args, Domain, Id, Value};
use arkui::cards::Shelf;
use arkui::context::{Context, Layer};
use arkui::glyphs;
use arkui::images::{self, Images};
use arkui::menu::{self, Entry, Menu};
use arkui::picker::{Choice, Picked, Picker};
use arkui::route::Router;
use arkui::vim;
use iced::{Point, Size, Subscription, Task};

use peer::Peer;
use places::{Focus, Pane, Place, Source};
use player::{Player, Track};
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
/// the domain's (`harken_domain::listening::url`), because three things join
/// it: this, the phone, and a speaker with no client at all.
fn media_url(server: &str, file: &str) -> String {
    harken_domain::listening::url(server, file)
}

/// Who played it and on what terms, as one line. Two facts on two rows —
/// `credit` and `recording.licence` — put back together for the one column a
/// table has: a recording that reserves nothing is just the names, and one
/// that asks for attribution says so in the same breath.
fn credit(performer: &str, licence: &str) -> String {
    match (performer.is_empty(), licence.is_empty()) {
        (_, true) => performer.to_string(),
        (true, false) => format!("({licence})"),
        (false, false) => format!("{performer} ({licence})"),
    }
}

/// Where a row's menu opens, which is decided by what asked for it: a right
/// click on the pointer, the row's ⋯ (or `m`) on the ⋯ column. The two rules
/// AppKit already has; see `arkui::menu::Anchor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Pointer,
    Dots,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Go offline on purpose, or come back.
    ToggleLink,
    /// Show a playlist, an album, an artist, a page — or everything.
    Select(Source),
    /// Start this one, and make what is on screen the queue it plays through.
    PlayItem(Id),
    PlayPause,
    Skip(i32),
    /// Dragging the bar's progress, in seconds.
    Seek(f32),
    SignIn,
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
    /// A key nothing on screen wanted.
    Key(iced::keyboard::Key, iced::keyboard::Modifiers),
    Resized(Size),
    /// Which device is making the sound: open the picker, walk it, pick.
    OpenDevices,
    DeviceAt(usize),
    /// `None` is the last row: stop it everywhere.
    PickDevice(Option<listening::DeviceId>),
    CloseDevices,
    /// Pump the replica and the player. Nothing else drives a sans-io peer.
    Tick,
    /// A wheel over a context window's backdrop, whose whole job is not to
    /// reach the page: a menu is pinned to a window coordinate, and a list
    /// scrolling under it would leave it beside a row it is not about.
    Swallow,
    /// For the explorer, while it is open (`E`; `docs/plan-guards.md` D4).
    Explore(ark_explorer::Msg),
}

/// One change this window made, kept so its fate can be said about *it*.
///
/// A refusal arrives as an id and a sentence; "the server refused a change"
/// says nothing about which. So every mutation this window authors is
/// written down with what it was, and `standing` follows each.
#[derive(Debug, Clone)]
pub struct Edit {
    pub id: Id,
    /// What it was, as a person would say it: "put Air on Favorites".
    pub what: String,
}

// The geometry, shared by the view that draws and the arithmetic that decides
// where the keyboard lands and where a panel goes. iced lays out after `view`,
// so these are declared rather than measured.
pub const SIDEBAR_WIDTH: f32 = 200.0;
pub const PAGE_PADDING: f32 = 16.0;
/// The gap either side of the rule between the sidebar and the page.
pub const PANE_GAP: f32 = 16.0;
/// The index pages' cards.
pub const SHELF: Shelf = Shelf::DEFAULT;
/// How tall the play bar is — declared because the device picker sits on it
/// and is placed before anything is laid out.
pub const BAR_HEIGHT: f32 = 48.0;
/// What the device picker draws a row at.
pub const DEVICES_WIDTH: f32 = 216.0;
/// Where a menu asked for with `m` opens, down the window: by key there is no
/// pointer, and nothing can say where the cursor's row currently is.
pub const MENU_BY_KEY_Y: f32 = 120.0;
/// The two scrollables a cursor keeps itself inside of.
pub const SIDEBAR: &str = "sidebar";
pub const TRACKS: &str = "tracks";
/// The last row of the playlist picker, which is a row and not a key.
pub const NEW_PLAYLIST: &str = "New playlist\u{2026}";

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
    /// `D` — every number the session holds.
    pub debug: bool,
    /// `E` — the explorer over this device's replica (`explore.rs`).
    pub explore: Option<explore::Explore>,
    /// A row's menu, and the playlist picker that is its submenu — or the one
    /// `a` opens alone. arkui holds the rules; the playlists are the values.
    pub ctx: Context<Message, Id>,
    /// The track the menu is about. Held by id rather than index: the list
    /// under it can move while it is up.
    pub menu_about: Option<Id>,
    /// …and the track the picker is about.
    pub picker_about: Option<Id>,
    /// The device picker, when it is up; the last row is "nowhere".
    pub devices: Option<usize>,
    /// How big the window is, so a panel near an edge opens the other way.
    pub window: Size,
    /// The address bar, reconciled after every update.
    pub router: Router<Place>,
    /// Where the pointer is.
    pub cursor: Point,
    pub player: Player,
    /// What `Skip` moves through: the list as it stood when play was pressed,
    /// as the tracks a hand-off carries.
    pub queue: Vec<listening::Track>,
    /// This account's listening session.
    pub listening: listening::Remote,
    /// Covers, fetched once and kept.
    pub covers: Images,
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
    pub logins: Option<ark_auth::remember::Logins>,
    /// A sign-in is in flight.
    pub signing_in: bool,
    /// What the connect entry holds, while this window is alone.
    pub connect: String,
    /// Offline on purpose.
    pub offline: bool,
    /// Ticks the link has been quiet since it opened, while this window waits
    /// to know whether the person signed in already has a playlist — see
    /// `default_playlist`. `None` when not waiting.
    pub quiet: Option<u32>,
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
        App {
            pane: Pane::Tracks,
            cursors: [0; 2],
            keys: vim::Keys::new(),
            search: String::new(),
            help: false,
            debug: false,
            explore: None,
            ctx: Context::new(),
            menu_about: None,
            picker_about: None,
            devices: None,
            window: Size::new(860.0, 600.0),
            router: Router::new(),
            cursor: Point::ORIGIN,
            player: Player::new(),
            queue: Vec::new(),
            listening: listening::Remote::new(),
            covers: Images::new("harken/covers"),
            art_seen: 0,
            server,
            user: None,
            login,
            logins: None,
            signing_in: false,
            connect: String::new(),
            offline: false,
            quiet: None,
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
        let peer = Peer::open(seed::seeded(Domain::new(&harken_domain::module())));
        let mut app = App::with_peer(peer, String::new(), None);
        app.note = "a demo — nothing here leaves your browser".into();
        app
    }

    /// No server named and none joined: this device's own replica, its own
    /// authority, for real (`docs/plan-alone.md` §4). What is done is kept
    /// here as local history until "connect" hands it to a server.
    #[cfg(not(feature = "demo"))]
    fn alone(user: Option<String>) -> App {
        let domain = Domain::new(&harken_domain::module());
        let (client, note) = match auth::open_alone(domain.clone()) {
            Ok(c) => (c, "alone \u{2014} what you do is kept on this device until you connect".to_string()),
            Err(e) => (
                ark_client::Peer::open_memory(domain, ark_client::Options::alone_as_nobody()).expect("a peer in memory opens"),
                format!("the saved library would not open ({e}) \u{2014} working in memory"),
            ),
        };
        let mut peer = Peer::open(client);
        peer.ensure_playlist();
        let mut app = App::with_peer(peer, String::new(), None);
        app.logins = Some(auth::logins());
        app.user = user;
        app.connect = auth::DEFAULT_SERVER.into();
        app.note = note;
        app
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
        if let Err(e) = self.peer.join(&ark_auth::socket_url(&server), None) {
            self.note = format!("could not connect to {server}: {e}");
            return Task::none();
        }
        if let Err(e) = auth::remember_joined(&server) {
            self.note = format!("connected, but this device will not remember it: {e}");
        }
        let pending = self.peer.client.pending_len();
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
        (app, task)
    }

    /// A link somebody was sent, read before anything is drawn.
    ///
    /// The address bar is reconciled after every update, and the first update
    /// is whatever iced sends first — a resize, the pointer. Reconciled before
    /// the tick had read the fragment, `#album/Water%20Music` was overwritten
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
                    let peer = ark_client::Peer::open_memory(Domain::new(&harken_domain::module()), ark_client::Options::alone_as_nobody())
                        .expect("a peer in memory opens");
                    let mut app = App::with_peer(Peer::open(peer), String::new(), None);
                    app.user = user;
                    app.note = format!("looking for a server at {origin}\u{2026}");
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
            let domain = Domain::new(&harken_domain::module());
            let (client, note) = match auth::open(domain.clone(), &server, auth::options(login.as_ref())) {
                Ok(c) => (c, String::new()),
                // A replica that will not open — written by another build,
                // or damaged — is not a reason to show nothing. This window
                // works in memory and says so.
                Err(e) => (
                    ark_client::Peer::open_memory(domain, auth::options(login.as_ref())).expect("a peer in memory opens"),
                    format!("the saved library would not open ({e}) — working in memory"),
                ),
            };
            let mut peer = Peer::open(client);
            // The roles the remembered login was signed in with, which the
            // author's `Ctx` carries: a library write is then refused on this
            // device as the server would (`docs/plan-guards.md` D1), and
            // shown as any refusal is.
            if let Some(l) = &login {
                peer.client.set_roles(l.user.roles.clone());
            }
            peer.client.connect(&ark_auth::socket_url(&server));
            // Signed out, the replica is the whole truth there is: its own
            // default playlist is made now. Signed in, not until the log has
            // been heard (`default_playlist`).
            if login.is_none() {
                peer.ensure_playlist();
            }
            let mut app = App::with_peer(peer, server, login);
            app.logins = Some(auth::logins());
            app.user = user;
            app.note = note;
            if let Some(login) = &app.login {
                app.listening.open(&login.session);
                return (app, Task::none());
            }
            // A page that just came back from signing in has the code in its
            // address; a desktop given a name can ask straight away.
            let task = app.sign_in();
            (app, task)
        }
    }

    /// Start a sign-in, if one can be started from here without a person.
    #[cfg(not(feature = "demo"))]
    fn sign_in(&mut self) -> Task<Message> {
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(code) = ark_auth::web::take_code() {
                self.signing_in = true;
                self.note = "signing in…".into();
                let server = self.server.clone();
                return Task::perform(async move { ark_auth::web::exchange(&server, &code).await }, Message::SignedIn);
            }
            Task::none()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.user.is_none() {
                return Task::none();
            }
            self.start_sign_in()
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
            self.signing_in = true;
            ark_auth::web::go_sign_in(&self.server, self.user.as_deref());
            Task::none()
        }
        #[cfg(all(not(feature = "demo"), not(target_arch = "wasm32")))]
        {
            // Blocking for as long as a person takes, so on a thread of its
            // own, with the answer handed back as a message.
            self.signing_in = true;
            self.note = "signing in — look for a browser tab if one opened".into();
            let server = self.server.clone();
            let user = self.user.clone();
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let outcome = ark_auth::client::login(&server, user.as_deref(), ark_auth::client::open_browser);
                let _ = tx.send(outcome);
            });
            Task::perform(
                async move { rx.await.unwrap_or_else(|_| Err("the sign-in thread went away".into())) },
                Message::SignedIn,
            )
        }
    }

    /// Somebody signed in. The replica this window has been using — signed
    /// out, or signed in as them before — moves on as them: whatever was
    /// done before becomes theirs and is pushed, and the link dials.
    pub fn signed_in(&mut self, login: Login) {
        if let Some(logins) = &self.logins {
            logins.remember(&self.server, &login);
        }
        match self
            .peer
            .client
            .sign_in(login.user.id.clone(), login.session.clone(), Some(login.token.clone()))
        {
            Ok(()) => {
                // The roles the sign-in said this login holds, which the
                // author's `Ctx` carries (`docs/plan-guards.md` D1).
                self.peer.client.set_roles(login.user.roles.clone());
                let pending = self.peer.client.pending_len();
                self.note = match pending {
                    0 => format!("signed in as {}", auth::who(&login)),
                    n => format!(
                        "signed in as {} — {n} change{} from before going up",
                        auth::who(&login),
                        if n == 1 { "" } else { "s" }
                    ),
                };
            }
            Err(e) => self.note = format!("could not sign in here: {e}"),
        }
        self.offline = false;
        self.peer.refresh();
        // A device is a login, so a new login is a new device — which is what
        // makes signing out and back in honestly a different row in somebody
        // else's picker, and a reloaded tab the same one.
        self.listening.open(&login.session);
        self.login = Some(login);
    }

    /// How long the link has to have been quiet, in ticks, before this window
    /// believes it has heard the whole log: a second. The server sends the
    /// backlog the moment `Hello` lands, so a second of nothing after the
    /// socket opened is a second of nothing left to send.
    pub const QUIET_TICKS: u32 = 20;

    /// Make the default "Favorites" — but only once it is known that this
    /// person has no playlist at all.
    ///
    /// **Not before the first sync.** A device that has not heard the log
    /// yet sees no playlists because nothing has arrived, not because there
    /// are none — and the domain renames a later playlist of the same name
    /// ("Favorites (1)", in the server's order), so every device making its
    /// own on first run would leave the person a numbered Favorites per
    /// device. So a signed-in window waits until the link has opened and then
    /// been quiet for [`App::QUIET_TICKS`]; signed out (or alone, the demo)
    /// the replica is the whole truth and it is made at once. A window that
    /// never reaches its server makes none, and "New playlist…" is still
    /// there.
    fn default_playlist(&mut self) {
        if self.login.is_some() && self.peer.playlists().is_empty() {
            self.peer.ensure_playlist();
        }
    }

    /// Author a change, and remember what it was. A refusal here is the
    /// domain's own verdict — a guard, a check — and is said at once; one the
    /// server reaches later is matched back to this by its id.
    pub fn author(&mut self, what: String, name: &str, a: ark_client::Args) -> bool {
        match self.peer.client.mutate(name, a) {
            Ok(id) => {
                self.edits.push(Edit { id, what });
                true
            }
            Err(e) => {
                self.note = format!("could not {what}: {e}");
                self.refused.push((what, e.to_string()));
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
        match self.ctx.focus() {
            Some(Layer::Picker) => Focus::Picker,
            Some(Layer::Menu) => Focus::Menu,
            None => Focus::Pane(self.pane),
        }
    }

    /// Whether this pane has the keyboard, which is the whole of what decides
    /// whether its cursor is drawn: a context window over it has the keys, and
    /// a cursor drawn where the next `j` will not go is a lie.
    pub fn has_keys(&self, pane: Pane) -> bool {
        self.focus() == Focus::Pane(pane)
    }

    /// How wide the page beside the sidebar is: the window less its padding
    /// either side, the sidebar, and the rule with a gap either side of it —
    /// every width `view` lays out before the page. Leaving the chrome out
    /// counted 65px that are not there, so at the widths where the last card
    /// needed them iced squeezed it to fit.
    pub fn pane_width(&self) -> f32 {
        self.window.width - PAGE_PADDING * 2.0 - SIDEBAR_WIDTH - PANE_GAP * 2.0 - 1.0
    }

    /// How many cards fit across an index page — the same division the view
    /// does, from the same width, which keeps the cursor under its card.
    pub fn columns(&self) -> usize {
        SHELF.columns(self.pane_width())
    }

    /// What shape a pane is, which is all `vim` needs to know about it.
    pub fn shape(&self, pane: Pane) -> vim::Grid {
        let p = &self.peer;
        match pane {
            Pane::Sidebar => vim::Grid::column(p.choices.len()),
            // The only pane ever more than one column: an index page is rows
            // of cards. A work's recordings are rows, so a column: `h` there
            // hands the cursor back to the sidebar.
            Pane::Tracks => match &p.source {
                Source::Albums => SHELF.grid(p.albums.len(), self.pane_width()),
                Source::Artists => SHELF.grid(p.artists.len(), self.pane_width()),
                Source::Composers => SHELF.grid(p.composers.len(), self.pane_width()),
                Source::Works(_) => SHELF.grid(p.works.len(), self.pane_width()),
                Source::Work(..) => vim::Grid::column(p.recordings.len()),
                _ => vim::Grid::column(p.rows().len()),
            },
        }
    }

    /// The text `/` searches, for whichever pane has the cursor: what is on
    /// screen, so on an index page the cards.
    pub fn labels(&self, pane: Pane) -> Vec<String> {
        let p = &self.peer;
        match pane {
            Pane::Sidebar => p.choices.iter().map(|c| c.label.clone()).collect(),
            Pane::Tracks => match &p.source {
                Source::Albums => p.albums.iter().map(|a| format!("{} {}", a.name, a.creator)).collect(),
                Source::Artists => p.artists.iter().map(|a| a.name.clone()).collect(),
                Source::Composers => p.composers.iter().map(|c| c.name.clone()).collect(),
                Source::Works(_) => p.works.iter().map(|w| format!("{} {}", w.title, w.catalogue)).collect(),
                Source::Work(..) => p.recordings.iter().map(|r| r.performers.clone()).collect(),
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
                vim::Action::Activate => match focus {
                    Focus::Menu => self.update(Message::MenuActivate),
                    Focus::Picker => self.update(Message::PickerActivate),
                    _ => self.pick_device(),
                },
                vim::Action::Cancel => match focus {
                    Focus::Devices => self.update(Message::CloseDevices),
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
                // Beside `D`: the explorer — every table this device holds,
                // the log as it knows it, a query console. Writes only
                // through the CRUD the domain exposes, as whoever is signed
                // in; never raw (`docs/plan-guards.md` D4).
                'E' => {
                    self.toggle_explorer();
                    Task::none()
                }
                // "Which lists is this on", which is the one question left
                // after the hearts went.
                'a' => self.update(Message::OpenPicker),
                // The same menu the dots and a right click open, on the ⋯
                // column — by key there is no pointer to land on.
                'm' => match self.peer.rows().get(self.at(Pane::Tracks)).map(|i| i.id) {
                    Some(id) => {
                        self.cursor.y = MENU_BY_KEY_Y;
                        self.update(Message::RowMenu(id, Anchor::Dots))
                    }
                    None => Task::none(),
                },
                'd' => self.update(Message::OpenDevices),
                '}' => self.update(Message::Skip(1)),
                '{' => self.update(Message::Skip(-1)),
                _ => Task::none(),
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
                let grid = vim::Grid::column(self.listening.devices().len() + 1);
                if let Some(at) = grid.step(self.devices.unwrap_or(0), motion) {
                    self.devices = Some(at);
                }
                Task::none()
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
        self.reveal()
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
    }

    /// The Songs page narrowed to a search, or widened back: the cursor goes
    /// to the top of whatever list that leaves, as a page opened does.
    fn narrow(&mut self, needle: Option<&str>) {
        if self.peer.search(needle) {
            self.cursors[Pane::Tracks as usize] = 0;
        }
    }

    /// Keep the cursor on screen.
    fn reveal(&self) -> Task<Message> {
        let id = match self.pane {
            Pane::Sidebar => SIDEBAR,
            Pane::Tracks => TRACKS,
        };
        arkui::scroll::reveal(id, self.shape(self.pane), self.at(self.pane))
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
        let to = match self.listening.devices().get(at) {
            Some(device) if !device.audible || !device.here => return Task::none(),
            Some(device) => Some(device.id.clone()),
            None => None,
        };
        self.update(Message::PickDevice(to))
    }

    /// The row menu's entries, for one track: every one something this
    /// window could already do, asked *about a row*. Built once when the menu
    /// opens, and the one definition — the view draws them and `<Enter>`
    /// runs them.
    pub fn row_entries(&self, id: Id) -> Option<(String, Vec<Entry<Message>>)> {
        let item = self.peer.rows().iter().find(|i| i.id == id)?;
        let album = self.peer.detail_of(id).album;
        let mut entries = vec![
            Entry::run(glyphs::PLAY, "Play", Message::PlayItem(id)),
            Entry::submenu(glyphs::ADD_TO, "Add to playlist", Message::OpenPicker),
        ];
        // "Go to" takes the glyph of the *place* it goes, which is the one
        // table the sidebar draws from too.
        for source in [Source::Album(album), Source::Artist(item.creator.clone())] {
            if !source.title().is_empty() {
                entries.push(Entry::run(source.glyph(), format!("Go to {}", source.title()), Message::Select(source)));
            }
        }
        Some((item.title.clone(), entries))
    }

    /// Where the ⋯ column ends: the list's right-hand edge, less the page's
    /// padding and the scrollbar — so a menu opened from it ends where the
    /// button is.
    pub fn dots_right(window: Size) -> f32 {
        window.width - PAGE_PADDING - arkui::SCROLLBAR
    }

    /// Where the device picker goes: hanging off the speaker at the right-hand
    /// end of the play bar, and off nothing else — no pointer, so it cannot
    /// slide about under the hand that opened it.
    pub fn devices_origin(window: Size, rows: usize) -> Point {
        arkui::panel::hang_above(
            window.width - PAGE_PADDING,
            window.height - PAGE_PADDING - BAR_HEIGHT,
            DEVICES_WIDTH,
            Self::devices_height(rows),
        )
    }

    /// The device picker's height: its padding, its title line, a row per
    /// device and the one that stops it everywhere — the panel's own numbers.
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
        let found = match wanted {
            Some(id) => self.peer.rows().iter().find(|i| i.id == id).cloned(),
            None => self.peer.rows().get(self.at(Pane::Tracks)).cloned(),
        };
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
                value: p.id,
                name: p.name.clone(),
            })
            .collect();
        self.picker_about = Some(item.id);
        self.ctx
            .open_picker(Picker::new(item.title, choices, Some(NEW_PLAYLIST.into())), self.window);
        Task::none()
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
                if let Some(source) = self.card_under_cursor(at) {
                    return self.update(Message::Select(source));
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
            .albums
            .iter()
            .map(|a| a.art.clone())
            .chain(p.artists.iter().map(|a| a.art.clone()))
            .chain(p.composers.iter().map(|c| c.art.clone()))
            .chain(p.works.iter().map(|w| w.art.clone()))
            .filter(|art| !art.is_empty())
            .map(|art| media_url(&self.server, &art))
            .collect();
        let tasks: Vec<Task<Message>> = urls
            .into_iter()
            .filter_map(|url| self.covers.want(url))
            .map(|t| t.map(Message::Cover))
            .collect();
        Task::batch(tasks)
    }

    /// What the cursor opens on this page, if the page has things to open:
    /// `None` is a page where `<Enter>` plays.
    pub fn card_under_cursor(&self, at: usize) -> Option<Source> {
        let p = &self.peer;
        match &p.source {
            Source::Albums => p.albums.get(at).map(|a| Source::Album(a.name.clone())),
            Source::Artists => p.artists.get(at).map(|a| Source::Artist(a.name.clone())),
            Source::Composers => p.composers.get(at).map(|c| Source::Works(c.name.clone())),
            Source::Works(_) => p.works.get(at).map(|w| Source::Work(w.id.clone(), w.title.clone())),
            Source::Work(..) => p.recordings.get(at).map(|r| Source::Recording(r.id.clone(), r.performers.clone())),
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
        let Some(id) = self.player.track().map(|t| rows::id_text(&t.id)) else {
            return 0;
        };
        self.queue.iter().position(|t| t.id == id).unwrap_or(0)
    }

    /// Put this device's player on `at` in the queue: the one place a queued
    /// track becomes a sound, so a hand-off and a click land in the same code.
    fn start_at(&mut self, at: usize, position_ms: i64, playing: bool) {
        let Some(track) = self.queue.get(at).cloned() else {
            return;
        };
        self.player.play(
            Track {
                id: rows::parse_id(&track.id).unwrap_or([0; 16]),
                title: track.title.clone(),
                creator: track.creator.clone(),
                album: track.album.clone(),
                ms: track.duration_ms,
            },
            &media_url(&self.server, &track.file),
        );
        if position_ms > 0 {
            self.player.seek(position_ms as f64 / 1000.0);
        }
        if !playing {
            self.player.pause();
        }
        if track.file.is_empty() {
            self.note = "nothing to stream — this one has no file".into();
        }
    }

    /// Where a transport button goes, and the only place that is decided: a
    /// *message* to the device making the sound if that is another one, an
    /// instruction here otherwise — including when nothing is the output,
    /// which is the start of every day.
    fn ask(&mut self, command: listening::Command) -> Task<Message> {
        if self.listening.elsewhere() {
            self.listening.ask(command);
            return Task::none();
        }
        self.obey(command)
    }

    /// Do it here. Reached from `ask` and from the server, which only ever
    /// sends a command to the output.
    fn obey(&mut self, command: listening::Command) -> Task<Message> {
        match command {
            listening::Command::Play => self.player.resume(),
            listening::Command::Pause => self.player.pause(),
            listening::Command::Next => self.skip(1),
            listening::Command::Previous => self.skip(-1),
            listening::Command::Seek { position_ms } => self.player.seek(position_ms as f64 / 1000.0),
            listening::Command::Start {
                queue,
                at,
                position_ms,
                playing,
            } => {
                self.queue = queue;
                self.start_at(at as usize, position_ms, playing);
            }
        }
        Task::none()
    }

    /// Do the thing, then make the address bar agree with what is on screen —
    /// here, after every message, rather than at the places that change what
    /// is shown: a rule each call site has to remember is already broken.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.step(message);
        self.router.sync(&self.peer.source.place());
        Task::batch([task, self.want_covers_if_moved()])
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
        if matches!(message, Message::PlayItem(_) | Message::Select(_)) {
            self.ctx.close_menu();
        }
        match message {
            Message::SignIn => return self.start_sign_in(),
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
            Message::SignedIn(outcome) => {
                self.signing_in = false;
                match outcome {
                    Ok(login) => self.signed_in(login),
                    Err(e) => self.note = format!("could not sign in: {e}"),
                }
            }
            Message::SignOut => {
                if let Some(login) = self.login.take() {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let server = self.server.clone();
                        std::thread::spawn(move || {
                            let _ = ark_auth::client::logout(&server, &login.token);
                        });
                    }
                    #[cfg(target_arch = "wasm32")]
                    let _ = login;
                }
                if let Some(logins) = &self.logins {
                    logins.forget(&self.server);
                }
                // The replica stays, and goes on authoring as that person,
                // offline: what they do now is theirs when they sign in again.
                self.peer.client.sign_out();
                // A device is a login, so signing out is leaving the session.
                self.listening.close();
                self.devices = None;
                self.note = "signed out — what you do now stays on this device until you sign in".into();
            }
            // The explorer has the keyboard while it is open: its own
            // grammar, its own cursor, and `<Esc>` from its top closes it.
            Message::Key(key, mods) if self.explore.is_some() => {
                self.explore(ark_explorer::Msg::Key(key, mods));
                return Task::none();
            }
            Message::Explore(m) => {
                self.explore(m);
                return Task::none();
            }
            Message::Key(key, mods) => {
                // `None` is still typing: a count, a `g`, a search — drawn in
                // the status line so nothing swallowed is a mystery.
                let action = self.keys.press(&key, mods);
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
            Message::Resized(size) => self.window = size,
            Message::HoverAt(at) => {
                // Not while a context window is up. A backdrop stops a click
                // and a wheel, but a hover is published by the row itself and
                // falls through every layer — so the cursor the menu is about
                // would creep away under it.
                if !matches!(self.focus(), Focus::Pane(_)) {
                    return Task::none();
                }
                // Takes the keyboard as well as the highlight: the cursor is
                // only drawn in the pane that has it.
                self.pane = Pane::Tracks;
                self.cursors[Pane::Tracks as usize] = at;
            }
            Message::Cover(done) => self.covers.loaded(done),
            Message::Hover(at) => self.cursor = at,
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
                if let Some(row) = self.peer.rows().iter().position(|i| i.id == id) {
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
            Message::ClosePicker => self.ctx.close_picker(),
            Message::PickerAt(at) => self.ctx.picker_at(at),
            Message::PickerName(name) => {
                if let Some(picker) = &mut self.ctx.picker {
                    picker.naming = Some(name);
                }
            }
            Message::PickerActivate => match self.ctx.activate_picker() {
                Some(Picked::Toggled { value, on }) => {
                    let Some(media) = self.picker_about else {
                        return Task::none();
                    };
                    let title = self
                        .peer
                        .rows()
                        .iter()
                        .find(|i| i.id == media)
                        .map(|i| i.title.clone())
                        .unwrap_or_default();
                    let list = self
                        .peer
                        .playlists()
                        .iter()
                        .find(|p| p.id == value)
                        .map(|p| p.name.clone())
                        .unwrap_or_default();
                    let a = args([("playlist_id", Value::Id(value)), ("media_id", Value::Id(media))]);
                    let done = match on {
                        true => self.author(format!("put {title} on {list}"), "add_to_playlist", a),
                        false => self.author(format!("take {title} off {list}"), "remove_from_playlist", a),
                    };
                    // Refused at once: the tick the panel shows is not true.
                    if !done {
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
                // Rebuilt from the playlists view rather than guessing the new
                // row: its id is chosen inside the mutation, and the refresh
                // has just spliced it in. The track is not added to it here —
                // one tap away, on a row that now exists.
                if self.author(
                    format!("make the playlist {}", name.trim()),
                    "create_playlist",
                    args([("name", Value::text(name))]),
                ) {
                    self.peer.refresh();
                    let at = self.ctx.picker.as_ref().map(|p| p.at);
                    let _ = self.open_picker();
                    if let (Some(picker), Some(at)) = (self.ctx.picker.as_mut(), at) {
                        picker.land(at);
                    }
                }
            }
            // Play and pause are two verbs, decided here against whichever
            // device is actually making the sound.
            Message::PlayPause => {
                let playing = match self.listening.elsewhere() {
                    true => self.listening.playing(),
                    false => self.player.is_playing(),
                };
                return self.ask(match playing {
                    true => listening::Command::Pause,
                    false => listening::Command::Play,
                });
            }
            Message::Seek(secs) => {
                return self.ask(listening::Command::Seek {
                    position_ms: (secs as f64 * 1000.0) as i64,
                })
            }
            Message::Skip(delta) => {
                return self.ask(match delta > 0 {
                    true => listening::Command::Next,
                    false => listening::Command::Previous,
                })
            }
            // The queue is what is on screen, taken now: skipping follows the
            // list you pressed play in. The rows are copied rather than
            // referred to — a device receiving a hand-off may not have that
            // album yet.
            Message::PlayItem(id) => {
                self.queue = self
                    .peer
                    .rows()
                    .iter()
                    .map(|i| listening::Track {
                        id: rows::id_text(&i.id),
                        title: i.title.clone(),
                        creator: i.creator.clone(),
                        album: self.peer.detail_of(i.id).album,
                        duration_ms: i.duration_ms,
                        // The path as the log carries it: each device joins it
                        // to *its* server.
                        file: i.file.clone(),
                    })
                    .collect();
                let wanted = rows::id_text(&id);
                let at = self.queue.iter().position(|t| t.id == wanted).unwrap_or(0) as u32;
                let queue = self.queue.clone();
                return self.ask(listening::Command::Start {
                    queue,
                    at,
                    position_ms: 0,
                    playing: true,
                });
            }
            Message::OpenDevices => {
                // The cursor starts on the device that has the sound, so
                // `<Enter>` straight away changes nothing.
                let output = self.listening.session().and_then(|s| s.output.clone());
                let at = output
                    .and_then(|id| self.listening.devices().iter().position(|d| d.id == id))
                    .unwrap_or(self.listening.devices().len());
                self.devices = Some(at);
            }
            Message::DeviceAt(at) => self.devices = Some(at),
            Message::PickDevice(to) => {
                self.listening.transfer(to);
                self.devices = None;
            }
            Message::CloseDevices => self.devices = None,
            Message::Swallow => {}
            Message::Select(source) => {
                // A click moves the cursor as well, or the keyboard would carry
                // on from wherever it was.
                let at = self.peer.choices.iter().position(|c| c.source == source);
                self.peer.source = source;
                self.peer.search = None;
                self.peer.open_page();
                if let Some(at) = at {
                    self.cursors[Pane::Sidebar as usize] = at;
                }
                // Always: landing on row seventeen of a record you just opened
                // is a cursor that remembers the wrong list.
                self.cursors[Pane::Tracks as usize] = 0;
            }
            Message::ToggleLink => {
                self.offline = !self.offline;
                match self.offline {
                    true => {
                        self.peer.client.disconnect();
                        self.note = "gone offline — changes pile up here".into();
                    }
                    false => {
                        self.peer.client.reconnect();
                        self.note = format!("reaching {}", self.server);
                    }
                }
            }
            Message::Tick => {
                if let Some(task) = self.tick() {
                    return task;
                }
            }
        }
        self.pump();
        Task::none()
    }

    /// Everything the 50ms clock does besides pumping.
    fn tick(&mut self) -> Option<Task<Message>> {
        if let Some(q) = self.quiet {
            match q + 1 >= Self::QUIET_TICKS {
                true => {
                    self.quiet = None;
                    self.default_playlist();
                }
                false => self.quiet = Some(q + 1),
            }
        }
        // A submenu opens by being pointed at, after a beat: arkui counts the
        // rest and answers the entry's message, and the picker it opens is
        // shown rather than entered.
        if let Some(m) = self.ctx.tick() {
            return Some(self.update(m));
        }
        if self.player.ended() {
            self.skip(1);
        }
        // The listening session: first what this device has been told to do
        // — the server only ever tells the output — then what it is doing.
        for command in self.listening.pump(&mut self.peer.client) {
            let _ = self.obey(command);
        }
        // Silent unless it is the output: losing the sound is not something
        // this device does, it is told, and this is the one place that can
        // notice.
        if self.listening.elsewhere() && self.player.is_playing() {
            self.player.pause();
        }
        // A device that has never played anything says nothing, so opening a
        // second tab does not claim the sound from the one using it.
        if self.player.track().is_some() {
            let at = self.playing_at() as u32;
            let playing = self.player.is_playing();
            let position_ms = (self.player.position() * 1000.0) as i64;
            let queue = std::mem::take(&mut self.queue);
            self.listening.report(&queue, at, playing, position_ms);
            self.queue = queue;
        }
        // The back button, and a link somebody was sent — consumed only once
        // the sidebar has something to resolve it against.
        let ready = !self.peer.choices.is_empty();
        if let Some(place) = self.router.poll(ready) {
            let wanted = self.peer.source_of(&place);
            if wanted != self.peer.source {
                return Some(self.update(Message::Select(wanted)));
            }
        }
        // A lock-screen button leaves a note; the tick collects it, and it
        // goes where every transport button goes.
        #[cfg(target_arch = "wasm32")]
        {
            let remote = self.player.take_remote();
            self.player.announce();
            let command = match remote {
                Some(Remote::Play) => Some(listening::Command::Play),
                Some(Remote::Pause) => Some(listening::Command::Pause),
                Some(Remote::Next) => Some(listening::Command::Next),
                Some(Remote::Previous) => Some(listening::Command::Previous),
                Some(Remote::Seek(secs)) => Some(listening::Command::Seek {
                    position_ms: (secs * 1000.0) as i64,
                }),
                None => None,
            };
            if let Some(command) = command {
                return Some(self.ask(command));
            }
        }
        None
    }

    /// Move frames, and bring the lists up to date with whatever moved —
    /// what this window authored, or what arrived.
    fn pump(&mut self) {
        let pumped = self.peer.client.pump();
        // The link opened, or something arrived while waiting: the quiet
        // starts again.
        if pumped.opened || (pumped.moved && self.quiet.is_some()) {
            self.quiet = Some(0);
        }
        if let Some(note) = pumped.note {
            self.note = note;
        }
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
        }
        for r in self.peer.client.take_rejections() {
            let what = self
                .edits
                .iter()
                .find(|e| e.id == r.id)
                .map(|e| e.what.clone())
                .unwrap_or_else(|| "a change".into());
            self.note = format!("the server refused to {what}: {}", r.reason);
            self.refused.push((what, r.reason));
        }
        self.peer.refresh();
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::time::every(Duration::from_millis(50)).map(|_| Message::Tick),
            iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
            // Only the presses no widget took: a focused text box consumes its
            // own, so typing a playlist's name cannot walk the cursor.
            vim::presses().map(|(key, mods)| Message::Key(key, mods)),
        ])
    }
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

    iced::application(App::boot, App::update, App::view)
        .subscription(App::subscription)
        .title("harken")
        .window_size((860.0, 600.0))
        .run()
}
