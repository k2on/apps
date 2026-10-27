//! arkui's documentation, drawn by arkui.
//!
//! A sidebar of the components (`arkui::sidebar`), a theme dropdown
//! (`arkui::menu`, anchored to its control) and, for each component, how to
//! use it on the desktop — a working example, its API, its rules — and on a
//! phone, Android or iOS, with the latest
//! capture from a real device in a frame of that device ([`screens`]).
//! What it says is [`components`] and [`catalogue`]; how each example is
//! built is [`examples`].
//!
//! `docs --json` prints the catalogue for a site that wants it as data.
// `--json` and the tests are native; in a browser the page only draws the
// entries.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
mod catalogue;
mod components;
mod examples;
mod pages;
mod screens;

use arkui::context::{Context, Layer};
use arkui::menu::{self, Anchor, Entry, Menu};
use arkui::picker::{self, Choice, Picked, Picker};
use arkui::route::{Route, Router};
use arkui::wake::Bell;
use arkui::{glyphs, icon, layer, sidebar, style, vim};
use iced::keyboard::{Key, Modifiers};
use iced::widget::{button, column, container, mouse_area, row, rule, scrollable, stack, text, Space};
use iced::{Alignment, Element, Length, Padding, Point, Size, Subscription, Task, Theme};

use components::{Section, COMPONENTS};
use screens::{Fetched, Phone, Pictures};

/// The chrome's padding: above the top bar and either side of the page.
pub const PADDING: f32 = 16.0;
/// The top bar's height, the account's, so the dropdown hangs off its foot.
pub const BAR: f32 = arkui::account::HEIGHT;
/// The sidebar's scrollable, and the page's.
const SIDEBAR: &str = "sidebar";
/// Below this the window is a phone's: no sidebar beside the page, but a
/// dropdown of the same places in the top bar.
pub const NARROW: f32 = 760.0;
const PAGE: &str = "page";

/// A page of the docs, which is also its fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Overview,
    Component(&'static str),
}

impl Place {
    /// Every place, in the sidebar's order.
    pub fn all() -> Vec<Place> {
        let mut out = vec![Place::Overview];
        for section in Section::ALL {
            out.extend(COMPONENTS.iter().filter(|c| c.section == section).map(|c| Place::Component(c.id)));
        }
        out
    }
}

impl Route for Place {
    fn fragment(&self) -> String {
        match self {
            Place::Overview => "#overview".into(),
            Place::Component(c) => format!("#{c}"),
        }
    }

    fn parse(fragment: &str) -> Self {
        // A component, or an arkui module's name for the component it is in.
        let id = fragment.trim_start_matches('#');
        components::find(id)
            .or_else(|| components::of_module(id))
            .map(|c| Place::Component(c.id))
            .unwrap_or(Place::Overview)
    }
}

/// A finger's scrolling, from the page (`web/index.html`): iced's
/// scrollable does not scroll for a touch in a browser, so the page measures
/// the drag and says how far, and the next update scrolls by it.
mod finger {
    use std::cell::{Cell, RefCell};

    use arkui::wake::Bell;

    thread_local! {
        static OWED: Cell<f32> = const { Cell::new(0.0) };
        static BELL: RefCell<Option<Bell>> = const { RefCell::new(None) };
    }

    /// Ring `bell` when a finger has moved.
    pub fn wire(bell: &Bell) {
        BELL.with(|b| *b.borrow_mut() = Some(bell.clone()));
    }

    /// How far fingers have scrolled since the last ask, down positive.
    pub fn take() -> f32 {
        OWED.with(|o| o.replace(0.0))
    }

    /// Scroll the page `dy` logical pixels, down positive. Called from the
    /// page's script; nothing calls it natively.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
    pub fn scroll_by(dy: f32) {
        OWED.with(|o| o.set(o.get() + dy));
        BELL.with(|b| {
            if let Some(b) = &*b.borrow() {
                b.ring();
            }
        });
    }
}

/// What the dropdown at the top offers: iced's guess from the system, or
/// either side by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone)]
pub enum Message {
    Go(Place),
    Key(Key, Modifiers),
    Hover(Point),
    Resized(Size),
    Wake,
    Swallow,
    // The dropdown and the example menus, through one context.
    ThemeMenu,
    SetTone(Tone),
    RowMenu,
    MenuAt(usize),
    MenuActivate,
    CloseMenu,
    Ran(&'static str),
    OpenPicker,
    PickerAt(usize),
    PickerActivate,
    PickerName(String),
    PickerCreate,
    ClosePicker,
    Tick,
    // The examples' own state.
    Dialog(bool),
    Frame,
    RowAt(usize),
    CardAt(usize),
    SideAt(usize),
    // The phones.
    Phone(Phone),
    Manifest(Fetched),
    Picture(arkui::images::Loaded),
    SystemTheme(iced::theme::Mode),
    Play,
    /// The places, as a dropdown on a narrow window.
    Places(bool),
}

pub struct App {
    pub place: Place,
    pub keys: vim::Keys,
    /// The last thing the keyboard grammar said, for the Keys example.
    pub last: String,
    router: Router<Place>,
    bell: Bell,
    pub rung: u64,
    pub ctx: Context<Message, usize>,
    pub pointer: Point,
    pub window: Size,
    pub tone: Tone,
    /// What the last menu entry run was, for the Menu example.
    pub ran: String,
    pub playlists: Vec<(String, bool)>,
    pub dialog: Option<iced::time::Instant>,
    pub row: usize,
    pub card: usize,
    pub side: usize,
    /// Which phone the Mobile half shows.
    pub phone: Phone,
    pub fetched: Fetched,
    pub pictures: Pictures,
    /// What the system said, for "follow the system".
    pub system_dark: bool,
    /// Frames played since the page opened, for a recording.
    pub played: u64,
    /// Whether a narrow window's places are open over the page.
    pub places: bool,
}

impl App {
    fn boot() -> (App, Task<Message>) {
        let bell = Bell::new();
        let ring = bell.clone();
        arkui::wake::page_changes(move || ring.ring());
        finger::wire(&bell);
        let mut app = App {
            place: Place::Overview,
            keys: vim::Keys::new(),
            last: String::new(),
            router: Router::new(),
            bell,
            rung: 0,
            ctx: Context::new(),
            pointer: Point::ORIGIN,
            window: Size::new(1100.0, 760.0),
            tone: Tone::System,
            ran: String::new(),
            playlists: vec![("Favorites".into(), true), ("Road trip".into(), false), ("Sunday".into(), false)],
            dialog: None,
            row: 1,
            card: 0,
            side: 1,
            phone: Phone::Android,
            fetched: Fetched::Asking,
            pictures: Pictures::new(),
            system_dark: false,
            played: 0,
            places: false,
        };
        // A fragment typed into the bar lands where it says.
        if let Some(place) = app.router.poll(true) {
            app.place = place;
        }
        let ask = Task::batch([screens::fetch().map(Message::Manifest), iced::system::theme().map(Message::SystemTheme)]);
        (app, ask)
    }

    fn title(&self) -> String {
        match self.place {
            Place::Overview => "arkui".into(),
            Place::Component(c) => format!("arkui — {}", components::find(c).map(|c| c.name).unwrap_or(c)),
        }
    }

    fn theme(&self) -> Option<Theme> {
        match self.tone {
            Tone::System => None,
            Tone::Light => Some(Theme::Light),
            Tone::Dark => Some(Theme::Dark),
        }
    }

    /// Whether the page is drawn dark: what the dropdown says, or what the
    /// system said.
    pub fn dark(&self) -> bool {
        match self.tone {
            Tone::System => self.system_dark,
            Tone::Light => false,
            Tone::Dark => true,
        }
    }

    /// The captures the page in view shows on the phone in view, asked for:
    /// on arrival, a switch of phone or theme, and when the manifest lands.
    fn want_pictures(&mut self) -> Task<Message> {
        let Place::Component(id) = self.place else { return Task::none() };
        let Some(c) = components::find(id) else { return Task::none() };
        let Some(m) = self.fetched.manifest().cloned() else {
            return Task::none();
        };
        let dark = self.dark();
        let mut tasks = Vec::new();
        for shot in c.shots {
            if let Some((_, got)) = self.fetched.find(self.phone, shot.id) {
                let got = got.clone();
                tasks.push(
                    self.pictures
                        .want(&got.kind, screens::urls(&m, self.phone, &got, dark))
                        .map(Message::Picture),
                );
            }
        }
        Task::batch(tasks)
    }

    /// The fastest recording on the page in view whose frames have all
    /// arrived, which the flipbook's clock runs at.
    fn playing(&self) -> Option<u32> {
        let Place::Component(id) = self.place else { return None };
        let m = self.fetched.manifest()?;
        components::find(id)?
            .shots
            .iter()
            .filter_map(|s| self.fetched.find(self.phone, s.id))
            .filter(|(_, got)| got.kind == "recording" && got.fps > 0)
            .filter(|(_, got)| {
                screens::urls(m, self.phone, got, self.dark())
                    .iter()
                    .all(|u| self.pictures.frames.handle(u).is_some())
            })
            .map(|(_, got)| got.fps)
            .max()
    }

    /// A phone's width, or a window made that narrow.
    pub fn narrow(&self) -> bool {
        self.window.width < NARROW
    }

    /// How wide the page's content is: the window less the sidebar, the
    /// gutters and, on the desktop, the scrollbar.
    pub fn content(&self) -> f32 {
        let side = if self.narrow() { 0.0 } else { sidebar::WIDTH };
        let bar = if self.narrow() { 0.0 } else { arkui::SCROLLBAR };
        (self.window.width - side - 2.0 * pages::gutter(self) - bar).max(200.0)
    }

    fn cursor(&self) -> usize {
        Place::all().iter().position(|p| *p == self.place).unwrap_or(0)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let task = self.apply(message);
        self.router.sync(&self.place);
        task
    }

    fn go(&mut self, place: Place) -> Task<Message> {
        if self.place == place {
            return Task::none();
        }
        self.place = place;
        self.played = 0;
        let pictures = self.want_pictures();
        let all = Place::all();
        let at = all.iter().position(|p| *p == place).unwrap_or(0);
        Task::batch([
            pictures,
            arkui::scroll::reveal(SIDEBAR, vim::Grid::column(all.len()), at),
            iced::widget::operation::snap_to(PAGE, iced::widget::scrollable::RelativeOffset::START),
        ])
    }

    fn apply(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Go(place) => {
                self.places = false;
                return self.go(place);
            }
            Message::Places(open) => self.places = open,
            Message::Key(key, mods) => return self.key(&key, mods),
            Message::Hover(at) => self.pointer = at,
            Message::Resized(size) => self.window = size,
            Message::Wake => {
                self.rung = self.bell.rung();
                let dy = finger::take();
                if dy != 0.0 {
                    // The places over a narrow page scroll, or the page does.
                    let id = if self.narrow() && self.places { SIDEBAR } else { PAGE };
                    return iced::widget::operation::scroll_by(id, iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: dy });
                }
                if let Some(place) = self.router.poll(true) {
                    return self.go(place);
                }
            }
            Message::Swallow | Message::Frame => {}
            Message::ThemeMenu => {
                let entries = [(Tone::System, "Follow the system"), (Tone::Light, "Light"), (Tone::Dark, "Dark")]
                    .into_iter()
                    .map(|(tone, label)| {
                        let glyph = if tone == self.tone { glyphs::TICKED } else { glyphs::UNTICK };
                        Entry::run(glyph, label, Message::SetTone(tone))
                    })
                    .collect();
                // A control's menu opens at the control: its right edge under
                // the button's, hung off the bar's foot.
                let right = self.window.width - PADDING;
                let foot = Point::new(right, PADDING + BAR + 4.0);
                self.ctx
                    .open_menu(Menu::open("Theme", entries, foot, self.window, Anchor::RightEdge(right)));
            }
            Message::SetTone(tone) => {
                self.tone = tone;
                self.ctx.close_menu();
                return self.want_pictures();
            }
            Message::RowMenu => {
                let entries = vec![
                    Entry::run(glyphs::PLAY, "Play", Message::Ran("Play")),
                    Entry::run(glyphs::NEXT, "Play next", Message::Ran("Play next")),
                    Entry::run(glyphs::ALBUM, "Go to Goldberg Variations, BWV 988", Message::Ran("Go to the album")),
                    Entry::submenu(glyphs::ADD_TO, "Add to playlist", Message::OpenPicker),
                    Entry::run(glyphs::MINUS, "Remove from library", Message::Ran("Remove")),
                ];
                // A right click opens at the pointer.
                self.ctx
                    .open_menu(Menu::open("Aria", entries, self.pointer, self.window, Anchor::Pointer));
            }
            Message::MenuAt(at) => self.ctx.land(at),
            Message::MenuActivate => {
                if let Some(m) = self.ctx.chosen() {
                    return self.update(m);
                }
            }
            Message::CloseMenu => self.ctx.close_menu(),
            Message::Ran(what) => {
                self.ran = what.into();
                self.ctx.close_menu();
            }
            Message::OpenPicker => {
                let choices = self
                    .playlists
                    .iter()
                    .enumerate()
                    .map(|(i, (name, on))| Choice {
                        value: i,
                        name: name.clone(),
                        on: *on,
                    })
                    .collect();
                self.ctx
                    .open_picker(Picker::new("Add to playlist", choices, Some("New playlist…".into())), self.window);
            }
            Message::PickerAt(at) => self.ctx.picker_at(at),
            Message::PickerActivate => match self.ctx.activate_picker() {
                Some(Picked::Toggled { value, on }) => {
                    if let Some(p) = self.playlists.get_mut(value) {
                        p.1 = on;
                    }
                }
                Some(Picked::Naming) => return picker::focus_naming(),
                None => {}
            },
            Message::PickerName(name) => {
                if let Some(p) = &mut self.ctx.picker {
                    p.naming = Some(name);
                }
            }
            Message::PickerCreate => {
                let name = self.ctx.picker.as_ref().and_then(|p| p.naming.clone()).unwrap_or_default();
                if !name.trim().is_empty() {
                    self.playlists.push((name.trim().to_string(), true));
                }
                self.ctx.close_menu();
                self.ctx.close_picker();
            }
            Message::ClosePicker => self.ctx.close_picker(),
            Message::Tick => {
                if let Some(m) = self.ctx.tick() {
                    return self.update(m);
                }
            }
            Message::Dialog(open) => self.dialog = open.then(iced::time::Instant::now),
            Message::RowAt(at) => self.row = at,
            Message::CardAt(at) => self.card = at,
            Message::SideAt(at) => self.side = at,
            Message::Phone(phone) => {
                self.phone = phone;
                self.played = 0;
                return self.want_pictures();
            }
            Message::Manifest(fetched) => {
                self.fetched = fetched;
                return self.want_pictures();
            }
            Message::Picture(done) => self.pictures.loaded(done),
            Message::SystemTheme(mode) => {
                self.system_dark = mode == iced::theme::Mode::Dark;
                return self.want_pictures();
            }
            Message::Play => self.played += 1,
        }
        Task::none()
    }

    /// The keyboard: a layer that is up has it, then a dialog, then the
    /// sidebar — whose cursor is the page.
    fn key(&mut self, key: &Key, mods: Modifiers) -> Task<Message> {
        let Some(action) = self.keys.press(key, mods) else {
            self.last = format!("pending: {}", self.keys.pending());
            return Task::none();
        };
        self.last = format!("{action:?}");
        if self.ctx.focus().is_some() {
            return match action {
                vim::Action::Move(motion) => match self.ctx.travel(motion) {
                    Some(m) => self.update(m),
                    None => Task::none(),
                },
                vim::Action::Activate | vim::Action::Toggle => match self.ctx.focus() {
                    Some(Layer::Menu) => self.update(Message::MenuActivate),
                    _ => self.update(Message::PickerActivate),
                },
                vim::Action::Cancel => {
                    self.ctx.cancel();
                    Task::none()
                }
                _ => Task::none(),
            };
        }
        if self.places && action == vim::Action::Cancel {
            self.places = false;
            return Task::none();
        }
        if self.dialog.is_some() {
            if action == vim::Action::Cancel {
                self.dialog = None;
            }
            return Task::none();
        }
        match action {
            vim::Action::Move(motion) => {
                let all = Place::all();
                match vim::Grid::column(all.len()).step(self.cursor(), motion) {
                    Some(at) => self.go(all[at]),
                    None => Task::none(),
                }
            }
            _ => Task::none(),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            self.bell.subscription().map(|()| Message::Wake),
            iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
            vim::presses().map(|(key, mods)| Message::Key(key, mods)),
            iced::system::theme_changes().map(Message::SystemTheme),
        ];
        // A recording's clock, only while one is on screen with its frames in.
        if let Some(fps) = self.playing() {
            subs.push(iced::time::every(std::time::Duration::from_millis(1000 / fps as u64)).map(|_| Message::Play));
        }
        // A clock only while something moves: a submenu's dwell, a dialog
        // rising.
        if self.ctx.dwelling() {
            subs.push(iced::time::every(std::time::Duration::from_millis(50)).map(|_| Message::Tick));
        }
        if self.dialog.is_some_and(|since| arkui::dialog::opening(since, iced::time::Instant::now())) {
            subs.push(iced::time::every(std::time::Duration::from_millis(16)).map(|_| Message::Frame));
        }
        Subscription::batch(subs)
    }

    fn view(&self) -> Element<'_, Message> {
        let page = column![
            self.view_top(),
            scrollable(pages::view(self))
                .id(PAGE)
                // A bar on the desktop, where it is how a mouse finds the
                // page's length; none on a phone, where a finger scrolls and
                // the strip of chrome is room the page does not have.
                .direction(iced::widget::scrollable::Direction::Vertical(match self.narrow() {
                    true => iced::widget::scrollable::Scrollbar::hidden(),
                    false => iced::widget::scrollable::Scrollbar::default(),
                }))
                .style(style::bars)
                .height(Length::Fill)
        ]
        .spacing(8);
        let page = container(page).padding(Padding {
            top: PADDING,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        });
        let body: Element<'_, Message> = match self.narrow() {
            true => page.into(),
            false => row![self.view_sidebar(), rule::vertical(1).style(style::edge), page].into(),
        };
        let base = container(body).width(Length::Fill).height(Length::Fill).style(style::page);

        let mut layers = stack![mouse_area(base).on_move(Message::Hover)];
        let focus = self.ctx.focus();

        // A narrow window's places: the sidebar, over the page, from the top
        // bar's button down.
        if self.narrow() && self.places {
            layers = layers.push(layer::dimmed(Message::Places(false), Message::Swallow)).push(
                container(self.view_sidebar()).height(Length::Fill).padding(Padding {
                    top: PADDING + BAR + 4.0,
                    ..Padding::ZERO
                }),
            );
        }

        if let Some(since) = self.dialog {
            let open = arkui::dialog::opened(since, iced::time::Instant::now());
            let close = button(icon::tinted(glyphs::CLOSE, 14.0, |p| p.background.base.text))
                .style(style::quiet)
                .padding(6)
                .on_press(Message::Dialog(false));
            let body =
                column![text("A dialog rises over the page as the page dims. A click on the dimmed page, the ×, or <Esc> closes it.").size(13),]
                    .spacing(8);
            layers = layers
                .push(arkui::dialog::backdrop(Message::Dialog(false), Message::Swallow, open))
                .push(arkui::dialog::view(
                    "Follow a podcast".into(),
                    close.into(),
                    body.into(),
                    (self.window.width - 32.0).min(420.0),
                    300.0,
                    open,
                ));
        }

        if let Some(m) = &self.ctx.menu {
            layers = layers.push(layer::backdrop(Message::CloseMenu, Message::Swallow)).push(layer::pinned(
                menu::view(m, focus == Some(Layer::Menu), Message::MenuAt, Message::MenuActivate),
                m.origin,
            ));
        }

        if let Some(p) = &self.ctx.picker {
            let drawn = picker::view(
                p,
                focus == Some(Layer::Picker),
                picker::Messages {
                    at: Box::new(Message::PickerAt),
                    activate: Message::PickerActivate,
                    name: Box::new(Message::PickerName),
                    create: Message::PickerCreate,
                },
            );
            layers = match p.origin {
                Some(at) => layers.push(layer::pinned(drawn, at)),
                None => layers
                    .push(layer::dimmed(Message::ClosePicker, Message::Swallow))
                    .push(layer::centred(drawn)),
            };
        }

        layers.into()
    }

    /// The overview, then every component under its section.
    fn view_sidebar(&self) -> Element<'_, Message> {
        let items: Vec<sidebar::Item<Message>> = Place::all()
            .into_iter()
            .map(|place| match place {
                Place::Overview => sidebar::Item {
                    glyph: glyphs::HOME,
                    label: "Overview".into(),
                    heading: Some("arkui"),
                    count: Some(COMPONENTS.len() as i64),
                    press: Message::Go(place),
                },
                Place::Component(id) => {
                    let c = components::find(id).expect("a place is a component");
                    sidebar::Item {
                        glyph: c.glyph,
                        label: c.name.into(),
                        heading: Some(c.section.heading()),
                        count: None,
                        press: Message::Go(place),
                    }
                }
            })
            .collect();
        sidebar::view(
            &items,
            self.cursor(),
            self.ctx.focus().is_none() && self.dialog.is_none(),
            sidebar::WIDTH,
            PADDING,
            SIDEBAR,
        )
    }

    /// Where the page is, and the theme dropdown at the right.
    fn view_top(&self) -> Element<'_, Message> {
        let tone = match self.tone {
            Tone::System => "System",
            Tone::Light => "Light",
            Tone::Dark => "Dark",
        };
        let path = match self.place {
            Place::Overview => "arkui".to_string(),
            Place::Component(c) => {
                let c = components::find(c).expect("a place is a component");
                match c.desktop.is_empty() {
                    true => c.name.to_lowercase(),
                    false => c.desktop.iter().map(|m| format!("arkui::{m}")).collect::<Vec<_>>().join(" · "),
                }
            }
        };
        let dropdown = button(
            row![
                text(match self.narrow() {
                    true => tone.to_string(),
                    false => format!("Theme: {tone}"),
                })
                .size(12),
                icon::plain(glyphs::DOWN, true)
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .style(style::quiet)
        .padding([4, 8])
        .on_press(Message::ThemeMenu);
        // Narrow, the place is a button that opens every place; wide, the
        // sidebar is there and this says where in arkui the page is.
        let left: Element<'_, Message> = match self.narrow() {
            true => {
                let here = match self.place {
                    Place::Overview => "Overview",
                    Place::Component(c) => components::find(c).map(|c| c.name).unwrap_or("arkui"),
                };
                button(
                    row![icon::plain(glyphs::LIBRARY, false), text(here).size(13), icon::plain(glyphs::DOWN, true)]
                        .spacing(6)
                        .align_y(Alignment::Center),
                )
                .style(style::quiet)
                .padding([4, 8])
                .on_press(Message::Places(!self.places))
                .into()
            }
            false => text(path).size(12).style(style::dim).into(),
        };
        container(
            row![left, Space::new().width(Length::Fill), dropdown]
                .align_y(Alignment::Center)
                .height(Length::Fixed(BAR)),
        )
        .padding(Padding {
            top: 0.0,
            right: PADDING,
            bottom: 0.0,
            left: pages::gutter(self) - if self.narrow() { 8.0 } else { 0.0 },
        })
        .into()
    }
}

#[cfg(target_arch = "wasm32")]
fn panic_to_console() {
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&info.to_string().into());
    }));
}

pub fn main() -> iced::Result {
    #[cfg(target_arch = "wasm32")]
    panic_to_console();
    #[cfg(not(target_arch = "wasm32"))]
    if std::env::args().any(|a| a == "--json") {
        print!("{}", catalogue::json());
        return Ok(());
    }
    // No palette installed: arkui's own, Palette::NEUTRAL, is what this
    // page shows the components in. An app installs its own.
    iced::application(App::boot, App::update, App::view)
        .subscription(App::subscription)
        .title(App::title)
        .theme(App::theme)
        .window_size((1100.0, 760.0))
        .font(arkui::hero::FIRA_SANS_BOLD)
        .run()
}

#[cfg(test)]
mod tests;
