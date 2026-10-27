//! arkui's documentation, drawn by arkui.
//!
//! A sidebar of the modules (`arkui::sidebar`, harken's), a theme dropdown
//! (`arkui::menu`, anchored to its control) and, for each module, what it is,
//! a working example, its API, its rules, and where harken draws with it.
//! What it says is [`catalogue`]; how each example is built is [`examples`].
//!
//! `docs --json` prints the catalogue for a site that wants it as data.
// `--json` and the tests are native; in a browser the page only draws the
// entries.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
mod catalogue;
mod examples;
mod pages;

use arkui::context::{Context, Layer};
use arkui::menu::{self, Anchor, Entry, Menu};
use arkui::picker::{self, Choice, Picked, Picker};
use arkui::route::{Route, Router};
use arkui::wake::Bell;
use arkui::{glyphs, icon, layer, sidebar, style, vim};
use iced::keyboard::{Key, Modifiers};
use iced::widget::{button, column, container, mouse_area, row, rule, scrollable, stack, text, Space};
use iced::{Alignment, Element, Length, Padding, Point, Size, Subscription, Task, Theme};

use catalogue::{Group, ENTRIES};

/// The chrome's padding: above the top bar and either side of the page.
pub const PADDING: f32 = 16.0;
/// The top bar's height, the account's, so the dropdown hangs off its foot.
pub const BAR: f32 = arkui::account::HEIGHT;
/// The sidebar's scrollable, and the page's.
const SIDEBAR: &str = "sidebar";
const PAGE: &str = "page";

/// A page of the docs, which is also its fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Overview,
    Harken,
    Module(&'static str),
}

impl Place {
    /// Every place, in the sidebar's order.
    pub fn all() -> Vec<Place> {
        let mut out = vec![Place::Overview, Place::Harken];
        for group in Group::ALL {
            out.extend(ENTRIES.iter().filter(|e| e.group == group).map(|e| Place::Module(e.module)));
        }
        out
    }
}

impl Route for Place {
    fn fragment(&self) -> String {
        match self {
            Place::Overview => "#overview".into(),
            Place::Harken => "#harken".into(),
            Place::Module(m) => format!("#{m}"),
        }
    }

    fn parse(fragment: &str) -> Self {
        match fragment.trim_start_matches('#') {
            "harken" => Place::Harken,
            m => catalogue::find(m).map(|e| Place::Module(e.module)).unwrap_or(Place::Overview),
        }
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
}

impl App {
    fn boot() -> (App, Task<Message>) {
        let bell = Bell::new();
        let ring = bell.clone();
        arkui::wake::page_changes(move || ring.ring());
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
        };
        // A fragment typed into the bar lands where it says.
        if let Some(place) = app.router.poll(true) {
            app.place = place;
        }
        (app, Task::none())
    }

    fn title(&self) -> String {
        match self.place {
            Place::Overview => "arkui".into(),
            Place::Harken => "arkui — harken".into(),
            Place::Module(m) => format!("arkui — {}", catalogue::find(m).map(|e| e.name).unwrap_or(m)),
        }
    }

    fn theme(&self) -> Option<Theme> {
        match self.tone {
            Tone::System => None,
            Tone::Light => Some(Theme::Light),
            Tone::Dark => Some(Theme::Dark),
        }
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
        let all = Place::all();
        let at = all.iter().position(|p| *p == place).unwrap_or(0);
        Task::batch([
            arkui::scroll::reveal(SIDEBAR, vim::Grid::column(all.len()), at),
            iced::widget::operation::snap_to(PAGE, iced::widget::scrollable::RelativeOffset::START),
        ])
    }

    fn apply(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Go(place) => return self.go(place),
            Message::Key(key, mods) => return self.key(&key, mods),
            Message::Hover(at) => self.pointer = at,
            Message::Resized(size) => self.window = size,
            Message::Wake => {
                self.rung = self.bell.rung();
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
        ];
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
            scrollable(pages::view(self)).id(PAGE).style(style::bars).height(Length::Fill)
        ]
        .spacing(8);
        let base = container(row![
            self.view_sidebar(),
            rule::vertical(1).style(style::edge),
            container(page).padding(Padding {
                top: PADDING,
                right: 0.0,
                bottom: 0.0,
                left: 0.0,
            })
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(style::page);

        let mut layers = stack![mouse_area(base).on_move(Message::Hover)];
        let focus = self.ctx.focus();

        if let Some(since) = self.dialog {
            let open = arkui::dialog::opened(since, iced::time::Instant::now());
            let close = button(icon::tinted(glyphs::CLOSE, 14.0, |p| p.background.base.text))
                .style(style::quiet)
                .padding(6)
                .on_press(Message::Dialog(false));
            let body = column![
                text("A dialog rises over the page as the page dims. A click on the dimmed page, the ×, or <Esc> closes it.").size(13),
                text("harken follows a podcast in one.").size(13).style(style::dim),
            ]
            .spacing(8);
            layers = layers
                .push(arkui::dialog::backdrop(Message::Dialog(false), Message::Swallow, open))
                .push(arkui::dialog::view(
                    "Follow a podcast".into(),
                    close.into(),
                    body.into(),
                    420.0,
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

    /// Overview and harken, then every module under its group.
    fn view_sidebar(&self) -> Element<'_, Message> {
        let items: Vec<sidebar::Item<Message>> = Place::all()
            .into_iter()
            .map(|place| match place {
                Place::Overview => sidebar::Item {
                    glyph: glyphs::HOME,
                    label: "Overview".into(),
                    heading: Some("arkui"),
                    count: Some(ENTRIES.len() as i64),
                    press: Message::Go(place),
                },
                Place::Harken => sidebar::Item {
                    glyph: glyphs::NOTE,
                    label: "In harken".into(),
                    heading: Some("arkui"),
                    count: Some(catalogue::HARKEN_ONLY.len() as i64),
                    press: Message::Go(place),
                },
                Place::Module(m) => {
                    let e = catalogue::find(m).expect("a place is an entry");
                    sidebar::Item {
                        glyph: e.glyph,
                        label: e.name.into(),
                        heading: Some(e.group.heading()),
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
            Place::Harken => "arkui · in harken".to_string(),
            Place::Module(m) => format!("arkui::{m}"),
        };
        let dropdown = button(
            row![text(format!("Theme: {tone}")).size(12), icon::plain(glyphs::DOWN, true)]
                .spacing(4)
                .align_y(Alignment::Center),
        )
        .style(style::quiet)
        .padding([4, 8])
        .on_press(Message::ThemeMenu);
        container(
            row![text(path).size(12).style(style::dim), Space::new().width(Length::Fill), dropdown]
                .align_y(Alignment::Center)
                .height(Length::Fixed(BAR)),
        )
        .padding(Padding {
            top: 0.0,
            right: PADDING,
            bottom: 0.0,
            left: pages::GUTTER,
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
