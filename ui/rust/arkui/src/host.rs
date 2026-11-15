//! An Ark app on the desktop, hosted once for every app (feature `host`;
//! ui/docs/app.md, "The App's Part of Its Host"): the window is the place
//! its part runs over ([`DeskPlace`]: a replica of its domain on this
//! machine's disk and the views it holds, the time, its device modules, who
//! is signed in where), and [`DesktopHost`] drives `ark_app::host::Hosting`
//! over it and draws the window the part says (`ark_app::window::Windowed`),
//! in ArkUI's iced parts (`crate::view`): its shell, the names it hosts,
//! its sheets and its Settings. The browser's page is the same over the
//! DOM (arkui-web's `host`), so the desktop and the browser draw one app.
//!
//! The household is a server's ([`live`]): the window signs in through it
//! as the phones do (the system's browser, back on a loopback port), keeps
//! a replica in the app's data directory and syncs over the server's
//! socket; the first run is every app's (`ark_app::welcome`). With `-demo
//! [day]` it is the app's demo, its own authority, with no server, as the
//! Mac's `-demo` is: what a demo and a test run.
//!
//! The keys are every app's ([`keys`]): the app's commands by their
//! shortcuts and in the command box (`:`, Ctrl-K), and the vim layer over
//! the rows drawn (`crate::view::cursor`: `j`/`k`, counts, `gg`/`G`,
//! `Enter`, `/` and `n`/`N`, `h`/`l` and `Tab` between panes, `?` the
//! keys panel), as arkui-web's are over the DOM.
//!
//! An app names nothing of its own here: its entry point is generated from
//! its app.toml (ui/prebuild.py's `desktop`), a [`Desktop`] of its name,
//! scheme, accent, domain and demo, and [`run`]. Its launch is every
//! shell's: `-screen budget`, `-inspector 1`, `-appearance dark`, a link
//! (`money://account/<id>/checking`), `--server URL` and `--user NAME`.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::sync::Arc;

use ark_app::host::{HostedApp, Hosting, Link, Place, Session, ShellAsk};
use ark_app::window::Windowed;
use ark_app::Modules;
use ark_auth::onboarding::Event;
use ark_peer::store::Store;
use ark_peer::Peer;
use iced::keyboard::{Key as Pressed, Modifiers};
use iced::widget::{column, container, row, stack, Space};
use iced::{Alignment, Element, Length, Size, Subscription, Task};

use crate::shell::{Frame, Keyboard, Pane};
use crate::style;
use crate::text::text;
use crate::view::cursor::{self, Cursor, Drawn};

mod devices;
pub mod keys;
pub mod live;
mod walk;

/// What an app is on the desktop, beside its part of its host: what its
/// generated entry point says of its app.toml.
pub trait Desktop: 'static {
    /// Its part of its host, drawn in a window (money's `money_app::host::Money`).
    type App: Windowed;
    /// Its name as a person reads it (`Money`), and the scheme its links
    /// come back on (`money`).
    const NAME: &'static str;
    const SCHEME: &'static str;
    /// Its accent's palette (ui/theme.toml's `[accent.<name>]`) and version.
    const PALETTE: crate::theme::Palette;
    const VERSION: &'static str;
    /// Its icon (icon.svg): the mark its first run's pages show.
    const MARK: &'static str;
    /// The domain its replica's log is of.
    fn domain() -> Arc<ark::domain::Domain>;
    /// Its demo's household on a peer of its own, seeded up to `today` at
    /// `now_ms`.
    fn demo(today: &str, now_ms: i64) -> Peer;
}

type App<W> = <W as Desktop>::App;
type ViewKey<W> = <App<W> as HostedApp>::Key;
type Change<W> = <App<W> as HostedApp>::Change;
type AppMsg<W> = <App<W> as Windowed>::Msg;

/// Everything the window can be told.
#[derive(Clone, Debug)]
pub enum Msg<M> {
    /// For one of the app's screens, or its window.
    App(M),
    /// The socket has something to say, or the peer's deadline came: it is
    /// pumped.
    Wake,
    /// A device's operation answered after its message: the screens take it.
    Answered,
    /// The replica read from this machine's disk, on a thread.
    Opened(crate::later::Slot<Result<Peer, String>>),
    /// What came of one of the sign-in machine's effects.
    Onboard(Box<Event>),
    /// A press on the first run's page.
    Welcome(ark_app::welcome::WelcomeMsg),
    /// A key no widget took.
    Key(Pressed, Modifiers),
    /// The window's new size.
    Resized(Size),
    /// The command box: what is typed, the row the pointer is on, the
    /// list scrolled, a key it hears, closed.
    Command(keys::CommandMsg),
    /// The keys panel's delay, run out.
    WhichDue(u64),
    /// A notice's time on the screen, run out.
    ToastDue(u64),
    /// A file the window's chooser gave (none: cancelled), for the scan
    /// that waited for it.
    Chosen(Option<crate::files::Picked>),
    /// A file dropped on the window: what a scan waiting takes.
    Dropped(crate::files::Picked),
    /// Taken, and nothing more.
    Swallow,
}

/// The window: its replica and the views it holds, and the app's part
/// running every screen over it.
pub struct DesktopHost<W: Desktop> {
    pub peer: Peer,
    pub store: Store<ViewKey<W>>,
    /// The session's day (`2026-11-10`): the demo's, or today.
    pub today: String,
    /// The demo's household: its day is fixed, its time noon on it.
    pub demo: bool,
    pub hosting: Hosting<App<W>>,
    /// On a server, signed in there; none for the demo.
    pub live: Option<live::Live>,
    /// The replica is still being read: the window says so.
    opening: bool,
    /// The window's width.
    width: f32,
    /// Which pane has the keys, the vim layer and the keys panel.
    keyboard: Keyboard,
    /// Where the cursor is in each pane.
    cursor: Cursor,
    /// What the last draw left for the keys.
    drawn: RefCell<Drawn>,
    /// The field a walk's step tapped last: what its next `type` types into.
    walk_focus: RefCell<Option<String>>,
    /// The command box, while it is up.
    command: Option<crate::command::Command>,
    /// What `/` searched the rows for last: what `n`/`N` find again.
    search: String,
    /// What the window says for a moment.
    toast: crate::toast::Toast,
    /// What its places said to say (a refusal, a sync's news).
    said: Vec<String>,
    /// The desktop's devices of the app's modules.
    devices: devices::Devices,
    w: PhantomData<W>,
}

/// Noon on `today`, in milliseconds: a demo's time.
pub fn noon(today: &str) -> i64 {
    ark_format::noon_of(today).unwrap_or(0)
}

/// The window as the place the app's part runs over.
pub struct DeskPlace<'a, W: Desktop> {
    peer: &'a mut Peer,
    store: &'a mut Store<ViewKey<W>>,
    live: Option<&'a live::Live>,
    today: &'a str,
    demo: bool,
    devices: &'a devices::Devices,
    said: &'a mut Vec<String>,
}

/// The views as a screen reads them: a file a row names read from this
/// machine's copy (`data:`), fetched when it lacks it.
fn read_files<K: ark_app::ViewKey>(peer: &Peer, store: &Store<K>, f: &mut dyn FnMut(&dyn ark::views::Views<K>)) {
    f(&ark::views::WithFiles {
        views: store,
        url: |file: &ark::file::FileRef| peer.file_url(file),
    })
}

/// The time on the desktop: the clock, or noon on the demo's day.
fn clock(today: &str, demo: bool) -> i64 {
    match demo {
        true => noon(today),
        false => ark_peer::now_ms(),
    }
}

impl<W: Desktop> Place<ViewKey<W>, Change<W>> for DeskPlace<'_, W> {
    fn read(&self, f: &mut dyn FnMut(&dyn ark::views::Views<ViewKey<W>>)) {
        read_files(self.peer, self.store, f)
    }
    fn now_ms(&self) -> i64 {
        clock(self.today, self.demo)
    }
    /// The machine's own, demo or not: what a sound is timed by.
    fn clock(&self) -> ark_app::host::Clock {
        std::sync::Arc::new(ark_peer::now_ms)
    }
    fn send(&mut self, change: Change<W>) -> Result<(), String> {
        let (name, input) = App::<W>::encode(&change);
        let r = self.peer.change(name, input);
        self.store.settle(self.peer);
        r
    }
    fn hold(&mut self, key: ViewKey<W>) {
        self.store.hold(self.peer, key);
        self.store.settle(self.peer);
    }
    fn release(&mut self, key: &ViewKey<W>) {
        self.store.release(self.peer, key)
    }
    fn modules(&self) -> Modules {
        self.devices.modules()
    }
    /// A device that answers later rings the window, which drains its screens.
    fn waker(&self) -> ark_app::host::Waker {
        self.devices.waker()
    }
    fn want(&mut self, file: ark::file::FileRef) {
        let _ = self.peer.open_file(&file);
    }
    fn keep(&mut self, bytes: &[u8], media: &str) -> Option<ark::file::FileRef> {
        self.peer.put_file(bytes, media).ok()
    }
    fn session(&self) -> Session {
        live::session_of::<W>(self.peer, self.live, self.today)
    }
    fn notice(&mut self, said: String) {
        self.said.push(said);
    }
    fn dial(&mut self) {
        if let Some(l) = self.live {
            self.peer.connect(&ark_auth::socket_url(&l.server));
        }
    }
}

/// The window as a place only read (a draw, the ready signal): it sends
/// and holds nothing.
pub struct Seen<'a, W: Desktop> {
    peer: &'a Peer,
    store: &'a Store<ViewKey<W>>,
    live: Option<&'a live::Live>,
    today: &'a str,
    demo: bool,
}

impl<W: Desktop> Place<ViewKey<W>, Change<W>> for Seen<'_, W> {
    fn read(&self, f: &mut dyn FnMut(&dyn ark::views::Views<ViewKey<W>>)) {
        read_files(self.peer, self.store, f)
    }
    fn now_ms(&self) -> i64 {
        clock(self.today, self.demo)
    }
    fn send(&mut self, _: Change<W>) -> Result<(), String> {
        Err("a window drawn sends nothing".into())
    }
    fn hold(&mut self, _: ViewKey<W>) {}
    fn release(&mut self, _: &ViewKey<W>) {}
    fn session(&self) -> Session {
        live::session_of::<W>(self.peer, self.live, self.today)
    }
}

/// The launch's `-demo [day]`: the demo's household, on that day or today.
fn demo_day(args: &[String]) -> Option<String> {
    let i = args.iter().position(|a| a == "-demo" || a == "--demo")?;
    let today = ark_format::day_of(ark_peer::now_ms());
    Some(args.get(i + 1).filter(|d| ark_format::noon_of(d).is_some()).cloned().unwrap_or(today))
}

/// The window's size when it opens.
const WINDOW: (f32, f32) = (1200.0, 760.0);

/// The app in its window: the demo with `-demo [day]`, else its server's
/// (`--server`, a dev server on this machine by default), signed in as
/// the login this machine keeps there. With `ARK_SHOTS` set, its
/// screenshots are taken instead, headless (arkui-shots), each step a
/// walk's in the app (`walk`).
pub fn run<W: Desktop>() -> iced::Result
where
    AppMsg<W>: Send,
{
    crate::renderer::choose();
    crate::theme::install(W::PALETTE);
    crate::glyphs::set_app(W::MARK);
    crate::motion::reduced();
    let program = iced::application(DesktopHost::<W>::boot, DesktopHost::<W>::update, DesktopHost::<W>::view)
        .subscription(DesktopHost::<W>::subscription)
        .title(|_: &DesktopHost<W>| W::NAME.to_string())
        .window_size(WINDOW)
        .font(crate::hero::FIRA_SANS_BOLD)
        .font(crate::fonts::FIRA_MONO);
    if let Some(shots) = arkui_shots::asked() {
        shots.run_walk(
            program,
            DesktopHost::<W>::ready,
            arkui_shots::Walk {
                step: DesktopHost::<W>::walk,
                tree: DesktopHost::<W>::walk_tree,
            },
        );
    }
    program.run()
}

impl<W: Desktop> DesktopHost<W>
where
    AppMsg<W>: Send,
{
    /// The window at launch: the demo's at once, or a server's replica read
    /// from this machine's disk on a thread while the window says so (iced
    /// draws no window until this returns, and a household's log is reading
    /// and replaying).
    pub fn boot() -> (DesktopHost<W>, Task<Msg<AppMsg<W>>>) {
        let args: Vec<String> = std::env::args().collect();
        if let Some(today) = demo_day(&args) {
            let mut host = DesktopHost::<W>::demo_on(today);
            let task = host.launch(&args);
            return (host, Task::batch([task, crate::window::boot()]));
        }
        let (server, _) = live::config();
        let login = live::logins::<W>().recall(&server);
        let today = ark_format::day_of(ark_peer::now_ms());
        let mut host = DesktopHost::<W>::over(live::memory::<W>(), today, false, None);
        host.opening = true;
        let opening = crate::later::off_thread(move || live::open::<W>(&server, login.as_ref()), Msg::Opened);
        (host, Task::batch([opening, crate::window::boot()]))
    }

    /// The app over its demo's household, seeded to `today`.
    pub fn demo_on(today: String) -> DesktopHost<W> {
        let peer = W::demo(&today, noon(&today));
        DesktopHost::over(peer, today, true, None)
    }

    /// The window over `peer` (on its server when `live`): the app's part
    /// opened on its session.
    pub fn over(mut peer: Peer, today: String, demo: bool, live: Option<live::Live>) -> DesktopHost<W> {
        let mut store = Store::new();
        let devices = devices::Devices::new::<W>();
        let mut said = vec![];
        let hosting = {
            let mut place = DeskPlace::<W> {
                peer: &mut peer,
                store: &mut store,
                live: live.as_ref(),
                today: &today,
                demo,
                devices: &devices,
                said: &mut said,
            };
            let session = place.session();
            Hosting::open(&session, &mut place)
        };
        DesktopHost {
            peer,
            store,
            today,
            demo,
            hosting,
            live,
            opening: false,
            width: WINDOW.0,
            keyboard: Keyboard::default(),
            cursor: Cursor::default(),
            drawn: RefCell::new(Drawn::default()),
            walk_focus: RefCell::new(None),
            command: None,
            search: String::new(),
            toast: crate::toast::Toast::new(),
            said,
            devices,
            w: PhantomData,
        }
    }

    /// Where the launch asks the app to be: a link it was opened with
    /// (`money://account/<id>/checking`; a sign-in link goes to the
    /// sign-in machine), else its launch arguments (`-screen budget`).
    fn launch(&mut self, args: &[String]) -> Task<Msg<AppMsg<W>>> {
        match crate::navigator::link_arg(W::SCHEME) {
            Some(link) => match live::join_of(&link) {
                Some(join) => self.onboard(join),
                None => {
                    if link.trim_start_matches(&format!("{}://", W::SCHEME)).trim_matches('/') == "welcome" {
                        if let Some(l) = &mut self.live {
                            l.first_run.show_intro();
                        }
                    }
                    self.host(|h, place| h.link(Link::Address(link), place));
                    Task::none()
                }
            },
            None => {
                self.host(|h, place| h.launched(args.to_vec(), place));
                Task::none()
            }
        }
    }

    /// The window, as a place read.
    pub fn seen(&self) -> Seen<'_, W> {
        Seen {
            peer: &self.peer,
            store: &self.store,
            live: self.live.as_ref(),
            today: &self.today,
            demo: self.demo,
        }
    }

    /// The app's part, as it is.
    pub fn app(&self) -> &App<W> {
        self.hosting.app()
    }

    /// The time its pages are worked out at, now.
    pub fn now_ms(&self) -> i64 {
        self.hosting.now_ms(&self.seen())
    }

    /// `f` over the app's part and the window as its place; what it asked
    /// of the window carried out after: signing out, and the rest by the
    /// app's part in its window (Settings shown).
    pub fn host<R>(&mut self, f: impl FnOnce(&mut Hosting<App<W>>, &mut DeskPlace<'_, W>) -> R) -> R {
        let r = self.with_place(f);
        for n in self.hosting.take_natives() {
            if n == ShellAsk::SignOut {
                self.sign_out();
            }
            self.with_place(|h, place| h.with(place, |app, cx| app.native(n, cx)));
        }
        r
    }

    fn with_place<R>(&mut self, f: impl FnOnce(&mut Hosting<App<W>>, &mut DeskPlace<'_, W>) -> R) -> R {
        let DesktopHost {
            peer,
            store,
            today,
            demo,
            hosting,
            live,
            devices,
            said,
            ..
        } = self;
        let mut place = DeskPlace::<W> {
            peer,
            store,
            live: live.as_ref(),
            today,
            demo: *demo,
            devices,
            said,
        };
        f(hosting, &mut place)
    }

    /// Signed out of the server: the session revoked, the sign-in machine
    /// back to its start. The demo has no way out but closing the window.
    fn sign_out(&mut self) {
        if self.live.is_some() {
            let _ = self.peer.sign_out();
            let _ = self.onboard(Event::SignOut);
        }
    }

    /// Whether nothing is left to come in the window; with
    /// [`DesktopHost::version`] unchanged, what a screenshot waits for.
    pub fn is_idle(&self) -> bool {
        !self.opening && self.live.as_ref().is_none_or(|l| l.settled()) && self.hosting.idle(&self.seen())
    }

    /// The screens' versions summed: grows with every message any took.
    pub fn version(&self) -> u64 {
        self.hosting.version()
    }

    /// The ready signal arkui-shots waits for: the version, once idle.
    fn ready(&self) -> Option<u64> {
        self.is_idle().then(|| self.version())
    }

    pub fn update(&mut self, msg: Msg<AppMsg<W>>) -> Task<Msg<AppMsg<W>>> {
        let task = match msg {
            Msg::App(m) => {
                self.host(|h, place| h.with(place, |app, cx| app.take(m, cx)));
                Task::none()
            }
            Msg::Wake => {
                let task = self.wake();
                self.host(|h, place| h.moved(place));
                task
            }
            Msg::Answered => {
                self.host(|h, place| h.moved(place));
                Task::none()
            }
            Msg::Opened(slot) => match slot.take() {
                Some(opened) => self.opened(opened),
                None => Task::none(),
            },
            Msg::Onboard(e) => self.onboard(*e),
            Msg::Welcome(m) => self.first_run(m),
            Msg::Key(key, mods) => self.key(key, mods),
            Msg::Resized(size) => {
                self.width = size.width;
                Task::none()
            }
            Msg::Command(m) => self.command(m),
            Msg::WhichDue(key) => {
                self.keyboard.which.due(key);
                Task::none()
            }
            Msg::ToastDue(key) => {
                self.toast.due(key);
                Task::none()
            }
            Msg::Chosen(picked) => {
                self.devices.chosen(picked.map(|p| p.bytes));
                Task::none()
            }
            Msg::Dropped(picked) => {
                self.devices.dropped(picked.bytes);
                Task::none()
            }
            Msg::Swallow => Task::none(),
        };
        self.follow();
        let said = self.said.pop();
        self.said.clear();
        if let Some(said) = said {
            self.toast.heard(&said);
        }
        Task::batch([task, self.devices.after()])
    }

    /// What the window cannot follow: a page the app opens (a bank's
    /// sign-in) shown in the system's browser, taken as closed once opened.
    fn follow(&mut self) {
        if self.opening {
            return;
        }
        let now = self.now_ms();
        if let Some((url, opened)) = self.app().opens(&self.seen(), now) {
            ark_auth::client::open_browser(&url);
            self.host(|h, place| h.with(place, |app, cx| app.take(opened, cx)));
        }
    }

    /// The replica read: the window on its server over it, as the login
    /// this machine keeps there or on its way to sign in, and the launch
    /// taken.
    fn opened(&mut self, opened: Result<Peer, String>) -> Task<Msg<AppMsg<W>>> {
        let width = self.width;
        let today = self.today.clone();
        let (peer, note) = match opened {
            Ok(p) => (p, None),
            // A replica that will not open is no reason to show nothing:
            // the window works in memory, and says so.
            Err(e) => (
                live::memory::<W>(),
                Some(format!("The saved data would not open ({e}): working in memory")),
            ),
        };
        *self = live::on_server::<W>(peer, today);
        self.width = width;
        let task = self.signing_in();
        let args: Vec<String> = std::env::args().collect();
        let launched = self.launch(&args);
        if let Some(n) = note {
            self.toast.heard(&n);
        }
        Task::batch([task, launched])
    }

    /// The window: the first run's pages while the app is not running yet,
    /// else the app's shell with each name it hosts, its sheets over it,
    /// Settings in its place while it shows, and the keys' panels.
    pub fn view(&self) -> Element<'_, Msg<AppMsg<W>>> {
        if self.opening {
            return quiet(&format!("Opening {}\u{2026}", W::NAME));
        }
        if let Some(live) = self.live.as_ref().filter(|l| !l.running() || l.first_run.page().intro.is_some()) {
            let pages = crate::view::element(ark_app::welcome::welcome::view(&live.first_run.page()).map(Msg::Welcome));
            return crate::window::view(container(pages).center(Length::Fill).style(style::page));
        }
        let (window, drawn) = cursor::drawing(self.cursor, || self.window());
        *self.drawn.borrow_mut() = drawn;
        let mut layers = stack![container(window).width(Length::Fill).height(Length::Fill).style(style::page)];
        if let Some(prefix) = self.keyboard.which.showing() {
            let entries = crate::which::entries(&[crate::vim::GRAMMAR], &[crate::vim::GROUPS], prefix);
            if !entries.is_empty() {
                layers = layers.push(crate::which::view(prefix, &entries, crate::toast::MARGIN));
            }
        }
        if let Some(c) = &self.command {
            layers = layers.push(keys::command_box(c, &self.app().commands()));
        }
        let pending = self.keyboard.keys.pending();
        if let Some(words) = crate::toast::says(&self.toast, &pending) {
            layers = layers.push(crate::toast::view(words, crate::toast::MARGIN));
        }
        crate::window::view(layers)
    }

    /// The window the app's part says, drawn as the shell does: its
    /// sections, toolbars and inspectors, each name it hosts its screen's
    /// view, its sheets over it; Settings in its place while it shows.
    fn window(&self) -> Element<'_, Msg<AppMsg<W>>> {
        let seen = self.seen();
        let now = self.now_ms();
        let app = self.app();
        let frame = Frame {
            width: self.width,
            focus: self.cursor.focus,
        };
        let mut hosts = |name: &'static str| -> Element<'static, Msg<AppMsg<W>>> { crate::view::element(app.hosted(name, &seen, now).map(Msg::App)) };
        let base = match app.settings(&seen, now) {
            Some((panes, done)) => settings_window(crate::view::shell(panes.map(Msg::App), frame, &mut hosts), Msg::App(done)),
            None => crate::view::shell(app.window(&seen, now).map(Msg::App), frame, &mut hosts),
        };
        let over = app.over(&seen, now);
        match over {
            ark_app::view::View::Empty => base,
            over => stack![base, crate::view::shell(over.map(Msg::App), frame, &mut hosts)].into(),
        }
    }

    pub fn subscription(&self) -> Subscription<Msg<AppMsg<W>>> {
        let mut subs = vec![
            crate::vim::presses().map(|(key, mods)| Msg::Key(key, mods)),
            iced::window::resize_events().map(|(_, size)| Msg::Resized(size)),
            crate::window::wakes().map(|()| Msg::Swallow),
            crate::files::dropped().map(Msg::Dropped),
            self.devices.bell().subscription().map(|()| Msg::Answered),
            // A cover an artwork asked for has come.
            crate::images::arrivals().subscription().map(|()| Msg::Swallow),
        ];
        // A sound plays: its timeline counts on as it is drawn.
        if crate::view::timeline_running() {
            subs.push(iced::time::every(std::time::Duration::from_millis(250)).map(|_| Msg::Swallow));
        }
        if self.command.is_some() {
            subs.push(crate::command::keys(true).map(|p| Msg::Command(keys::CommandMsg::Press(p))));
        }
        if let Some(key) = self.keyboard.which.waiting() {
            subs.push(
                crate::wake::at(u64::MAX - key, crate::which::DELAY)
                    .with(key)
                    .map(|(key, ())| Msg::WhichDue(key)),
            );
        }
        if let Some(timer) = self.toast.timer() {
            subs.push(timer.map(Msg::ToastDue));
        }
        if let Some(l) = &self.live {
            subs.push(l.bell.subscription().map(|()| Msg::Wake));
            if let Some(at) = self.peer.deadline() {
                let delay = std::time::Duration::from_millis(at.saturating_sub(ark_peer::monotonic_ms()) + 2);
                subs.push(crate::wake::at(at, delay).map(|()| Msg::Wake));
            }
        }
        Subscription::batch(subs)
    }

    /// The panes on screen for the keys, left to right: the sidebar where
    /// the shell has one, and each pane that drew rows.
    fn panes(&self) -> Vec<Pane> {
        let drawn = self.drawn.borrow();
        let mut out = vec![];
        if crate::view::has_sidebar(self.width) {
            out.push(Pane::Sidebar);
        }
        for p in [Pane::Content, Pane::Detail, Pane::Inspector] {
            if cursor::index(p).is_some_and(|i| !drawn.rows[i].is_empty()) {
                out.push(p);
            }
        }
        out
    }
}

/// Settings in the window's place: its panes under a bar with its title
/// and Done, as a Mac's Settings window is (the browser's is a window of
/// its own: arkui-web's `settings_window`).
fn settings_window<'a, M: Clone + 'a>(panes: Element<'a, M>, done: M) -> Element<'a, M> {
    let bar = row![
        text("Settings").size(15).font(crate::hero::BOLD),
        Space::new().width(Length::Fill),
        iced::widget::button(text("Done").size(13))
            .padding([4, 10])
            .style(style::quiet)
            .on_press(done)
    ]
    .align_y(Alignment::Center)
    .padding([8, 12]);
    column![
        bar,
        iced::widget::rule::horizontal(1).style(style::edge),
        container(panes).height(Length::Fill)
    ]
    .into()
}

/// A word where there is nothing to show yet, quiet and centred.
fn quiet<'a, M: 'a>(words: &str) -> Element<'a, M> {
    container(text(words.to_string()).size(13).style(style::dim))
        .center(Length::Fill)
        .style(style::page)
        .into()
}
