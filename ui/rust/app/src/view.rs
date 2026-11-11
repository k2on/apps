//! How a screen is laid out, once, for every toolkit: a view is a
//! function of the screen's record that returns a [`View`] built from
//! ArkUI's parts (ui/docs/parts.md), SwiftUI's way.
//!
//! ```ignore
//! #[view]
//! pub fn accounts(p: &AccountsPage) -> View<AccountsMsg> {
//!     page("Accounts", (
//!         hero("Net Worth", p.net_worth, "Every open account, less what's owed"),
//!         for shelf in p.shelves {
//!             section(for a in shelf.accounts {
//!                 list_row(a.name).amount(a.balance).on_tap(AccountsMsg::OpenAccount { id: a.id })
//!             })
//!             .header(shelf.title)
//!         },
//!     ))
//! }
//! ```
//!
//! `#[view]` (ark-macros) makes the function generic over a [`Backend`]
//! the author never names, rewriting what reads the record — `p.net_worth`,
//! `for shelf in p.shelves`, `if a.closed`, `if let Some(f) = p.adding` —
//! into the backend's calls, and writes its entry points beside it
//! (`accounts::view(&page)`, `accounts::record()`; a toolkit draws the
//! view, `arkui::view::element` and `arkui_web::render`). There are
//! two backends:
//!
//! - [`Real`]: the record itself. A `for` makes a row for each item, an
//!   `if` takes its branch, and the [`View`] holds values: what iced
//!   (`arkui::view`) and the browser (`arkui-web`) draw.
//! - [`Recorder`]: the record as paths (`shelf.title`). A `for` is walked
//!   once with a symbolic item ([`View::Each`]), an `if` both ways
//!   ([`View::When`]), and the [`View`] holds paths: what `ark-ui-gen`
//!   writes out as SwiftUI and Compose.
//!
//! What a view says is what the backends can follow: a part, a field read,
//! a `for`, an `if`, an `if let Some`, a message built from fields. Any
//! other Rust over the record does not compile.

use std::fmt::Debug;
use std::marker::PhantomData;
use std::rc::Rc;

/// A value a view reads: plain data.
pub trait Value: Clone + Debug + 'static {}
impl<T: Clone + Debug + 'static> Value for T {}

/// A value read from the record: the value itself, or (recorded) the path
/// it is read from.
#[derive(Clone, Debug, PartialEq)]
pub enum Val<T> {
    Is(T),
    At(String),
}

impl<T> From<T> for Val<T> {
    fn from(t: T) -> Val<T> {
        Val::Is(t)
    }
}

impl From<&'static str> for Val<String> {
    fn from(s: &'static str) -> Val<String> {
        Val::Is(s.into())
    }
}

impl<T: Clone> Val<T> {
    /// The value; `or` while it is only a path.
    pub fn or(&self, or: T) -> T {
        match self {
            Val::Is(t) => t.clone(),
            Val::At(_) => or,
        }
    }
}

impl<T: Clone + Default> Val<T> {
    /// The value, or its default while it is only a path.
    pub fn get(&self) -> T {
        self.or(T::default())
    }
}

/// A value the recorder can stand in for with its path, in a message built
/// in a view (`OpenAccount { id: a.id }`): text.
pub trait Probe: Sized {
    fn probe(path: &str) -> Self;
}

/// How a probe's path is marked in the text of a message.
pub const PROBE_OPEN: char = '\u{27e6}';
pub const PROBE_CLOSE: char = '\u{27e7}';

impl Probe for String {
    fn probe(path: &str) -> String {
        format!("{PROBE_OPEN}{path}{PROBE_CLOSE}")
    }
}

/// What reads the record: [`Real`] or [`Recorder`]. Only `#[view]`'s
/// rewriting names it.
pub trait Backend {
    /// A part of the record.
    type Ref<'a, T: Value>: Clone;
    /// The field of `r` called `name`.
    fn get<'a, T: Value + 'a, F: Value>(r: &Self::Ref<'a, T>, name: &'static str, f: fn(&T) -> &F) -> Self::Ref<'a, F>;
    /// A part of the record as a part's input.
    fn val<T: Value>(r: Self::Ref<'_, T>) -> Val<T>;
    /// The field of `r` called `name`, as the value itself: for a message.
    fn probe<T: Value, F: Value + Probe>(r: &Self::Ref<'_, T>, name: &'static str, f: fn(&T) -> &F) -> F;
    /// `body` for each of `list`'s items, called `item`.
    fn each<'a, T: Value + 'a, M>(list: Self::Ref<'a, Vec<T>>, item: &'static str, body: impl FnMut(Self::Ref<'a, T>) -> View<M>) -> View<M>;
    /// `yes` while `c` holds, else `no`.
    fn when<M>(c: Self::Ref<'_, bool>, yes: impl FnOnce() -> View<M>, no: impl FnOnce() -> View<M>) -> View<M>;
    /// `yes` with what `o` holds, called `item`, else `no`.
    fn when_some<'a, T: Value + 'a, M>(
        o: Self::Ref<'a, Option<T>>,
        item: &'static str,
        yes: impl FnOnce(Self::Ref<'a, T>) -> View<M>,
        no: impl FnOnce() -> View<M>,
    ) -> View<M>;
}

/// The record itself.
pub struct Real;

impl Backend for Real {
    type Ref<'a, T: Value> = &'a T;
    fn get<'a, T: Value + 'a, F: Value>(r: &&'a T, _: &'static str, f: fn(&T) -> &F) -> &'a F {
        f(r)
    }
    fn val<T: Value>(r: &T) -> Val<T> {
        Val::Is(r.clone())
    }
    fn probe<T: Value, F: Value + Probe>(r: &&T, _: &'static str, f: fn(&T) -> &F) -> F {
        f(r).clone()
    }
    fn each<'a, T: Value + 'a, M>(list: &'a Vec<T>, _: &'static str, body: impl FnMut(&'a T) -> View<M>) -> View<M> {
        View::List(list.iter().map(body).collect())
    }
    fn when<M>(c: &bool, yes: impl FnOnce() -> View<M>, no: impl FnOnce() -> View<M>) -> View<M> {
        if *c {
            yes()
        } else {
            no()
        }
    }
    fn when_some<'a, T: Value + 'a, M>(
        o: &'a Option<T>,
        _: &'static str,
        yes: impl FnOnce(&'a T) -> View<M>,
        no: impl FnOnce() -> View<M>,
    ) -> View<M> {
        match o {
            Some(t) => yes(t),
            None => no(),
        }
    }
}

/// The record as paths: what a view says, for the emitters.
pub struct Recorder;

/// A path into the record, standing in for a value of `T`.
pub struct Sym<T>(pub String, PhantomData<fn() -> T>);

impl<T> Clone for Sym<T> {
    fn clone(&self) -> Self {
        Sym(self.0.clone(), PhantomData)
    }
}

impl<T> Sym<T> {
    /// The record itself, called `name` in the emitted code.
    pub fn root(name: &str) -> Sym<T> {
        Sym(name.into(), PhantomData)
    }
}

impl Backend for Recorder {
    type Ref<'a, T: Value> = Sym<T>;
    fn get<'a, T: Value + 'a, F: Value>(r: &Sym<T>, name: &'static str, _: fn(&T) -> &F) -> Sym<F> {
        Sym(format!("{}.{name}", r.0), PhantomData)
    }
    fn val<T: Value>(r: Sym<T>) -> Val<T> {
        Val::At(r.0)
    }
    fn probe<T: Value, F: Value + Probe>(r: &Sym<T>, name: &'static str, _: fn(&T) -> &F) -> F {
        F::probe(&format!("{}.{name}", r.0))
    }
    fn each<'a, T: Value + 'a, M>(list: Sym<Vec<T>>, item: &'static str, mut body: impl FnMut(Sym<T>) -> View<M>) -> View<M> {
        View::Each {
            list: list.0,
            item: item.into(),
            body: Box::new(body(Sym::root(item))),
        }
    }
    fn when<M>(c: Sym<bool>, yes: impl FnOnce() -> View<M>, no: impl FnOnce() -> View<M>) -> View<M> {
        View::When {
            cond: c.0,
            yes: Box::new(yes()),
            no: Box::new(no()),
        }
    }
    fn when_some<'a, T: Value + 'a, M>(
        o: Sym<Option<T>>,
        item: &'static str,
        yes: impl FnOnce(Sym<T>) -> View<M>,
        no: impl FnOnce() -> View<M>,
    ) -> View<M> {
        View::WhenSome {
            opt: o.0,
            item: item.into(),
            yes: Box::new(yes(Sym::root(item))),
            no: Box::new(no()),
        }
    }
}

// * The view

/// What a screen is laid out as.
#[derive(Clone, Debug)]
pub enum View<M> {
    Empty,
    /// Several, one after another in whatever holds them.
    List(Vec<View<M>>),
    /// Recorded: `body` for each item of `list`.
    Each {
        list: String,
        item: String,
        body: Box<View<M>>,
    },
    /// Recorded: `yes` while `cond` holds, else `no`.
    When {
        cond: String,
        yes: Box<View<M>>,
        no: Box<View<M>>,
    },
    /// Recorded: `yes` with what `opt` holds, else `no`.
    WhenSome {
        opt: String,
        item: String,
        yes: Box<View<M>>,
        no: Box<View<M>>,
    },
    Part(Part<M>),
}

/// A message a field's input makes: shared, so that a view can be mapped
/// into another app's messages ([`View::map`]).
pub struct Input<T, M>(pub Rc<dyn Fn(T) -> M>);

impl<T, M> Input<T, M> {
    pub fn new(f: impl Fn(T) -> M + 'static) -> Input<T, M> {
        Input(Rc::new(f))
    }

    /// The message `t` makes.
    pub fn call(&self, t: T) -> M {
        (self.0)(t)
    }
}

impl<T, M> Clone for Input<T, M> {
    fn clone(&self) -> Self {
        Input(self.0.clone())
    }
}

impl<T, M> Debug for Input<T, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Input")
    }
}

/// One of ArkUI's parts, and what it is handed.
#[derive(Clone, Debug)]
pub enum Part<M> {
    /// A tab's page: its large title (the account's face at its end), then
    /// its content; pulled down, `refresh`.
    Page {
        title: Val<String>,
        refresh: Option<M>,
        top: Box<View<M>>,
        content: Box<View<M>>,
    },
    /// The page's one large figure.
    Hero {
        label: &'static str,
        cents: Val<i64>,
        note: &'static str,
    },
    /// The page's figure in a card: its label, the amount large, the two
    /// it is made of under it (what is owned, what is owed), and a
    /// sparkline of how it went (oldest first).
    Summary {
        label: &'static str,
        cents: Val<i64>,
        first: &'static str,
        first_cents: Val<i64>,
        second: &'static str,
        second_cents: Val<i64>,
        trend: Val<Vec<i64>>,
    },
    /// An inspector's or a detail's page: its cards one under another,
    /// scrolling (a grouped list on Apple's platforms).
    Detail {
        content: Box<View<M>>,
    },
    /// What an inspector or a detail is about, in a card at its top: its
    /// glyph, its title, its figure (in a `StatusBadge` of its tint, or
    /// large), a line in the tint saying what needs attention, a quiet
    /// caption, and its actions under a divider.
    HeaderCard(Box<HeaderCard<M>>),
    /// How far toward a goal: a bar in its tint and the words under it.
    GoalProgress {
        fraction: Val<f64>,
        tint: Val<String>,
        note: Val<String>,
    },
    /// A group of rows in a card, under its heading, a note under it.
    Section(Section<M>),
    Row(Box<ListRow<M>>),
    /// A row of a thing toward a goal: its title (the app's mark before
    /// it, faded while `faded`), a slim bar of how far (in its tint), words
    /// under it in a tint and quiet ones at their end; on a regular width
    /// two figures in columns; its figure in a status badge at its end.
    MeterRow(Box<MeterRow<M>>),
    /// A page's headline: what it is, its figure large in its tint, a
    /// caption under it, and its one action at its end (a prominent
    /// capsule).
    Headline(Box<Headline<M>>),
    /// A sheet: its title, its content; put away, `dismiss`; kept with
    /// its confirming action, while there is one (the iPhone's checkmark in
    /// the sheet's bar, a full-width button under the form elsewhere).
    Sheet {
        title: &'static str,
        dismiss: M,
        confirm: Option<Confirm<M>>,
        content: Box<View<M>>,
    },
    /// A group of a form's fields, a heading over it and a note under it:
    /// a `Form`'s `Section` on the iPhone, a plain column elsewhere.
    FormSection {
        header: Option<Val<String>>,
        footer: Option<Val<String>>,
        content: Box<View<M>>,
    },
    /// One of several, chosen: the system's picker on the iPhone (a menu
    /// in a form's row), filter chips on Android and the desktop. `choices`
    /// are [`Part::Choice`]s; `selected` is the chosen one's tag.
    Picker {
        label: &'static str,
        selected: Val<String>,
        pick: Input<String, M>,
        choices: Box<View<M>>,
    },
    /// One choice of a picker: its title, its glyph, and the tag choosing
    /// it sends.
    Choice {
        title: Val<String>,
        glyph: Val<String>,
        tag: Val<String>,
    },
    /// An amount entered the app's way: the iPhone's large figure over its
    /// own keypad (ArkUI's `AmountEntry`, in cents, `prompt` under it), a
    /// text field elsewhere (`label`, what was typed, the problem with it).
    AmountEntry {
        label: Val<String>,
        prompt: Val<String>,
        typed: Val<String>,
        cents: Val<i64>,
        problem: Val<Option<String>>,
        on_typed: Input<String, M>,
        on_cents: Input<i64, M>,
    },
    /// An alert with one text field.
    Prompt {
        title: &'static str,
        text: Val<String>,
        typed: Input<String, M>,
        confirm: &'static str,
        ok: M,
        cancel: M,
    },
    NameField {
        label: &'static str,
        value: Val<String>,
        problem: Val<Option<String>>,
        typed: Input<String, M>,
    },
    AmountField {
        label: Val<String>,
        typed: Val<String>,
        problem: Val<Option<String>>,
        on: Input<String, M>,
    },
    /// Choices in a wrapping row.
    Chips(Box<View<M>>),
    Chip {
        label: Val<String>,
        glyph: Val<String>,
        selected: Val<bool>,
        tap: M,
    },
    Toggle {
        title: &'static str,
        /// A line under the title, while there is one.
        note: Option<Val<String>>,
        on: Val<bool>,
        change: Input<bool, M>,
    },
    Button {
        title: &'static str,
        enabled: Val<bool>,
        tap: M,
    },
    /// Bars against the largest, each its label over it and its figure said
    /// at its end, in the chart's first colour (ArkUI's `LabelledBars`).
    BarChart {
        labels: Val<Vec<String>>,
        values: Val<Vec<i64>>,
        said: Val<Vec<String>>,
    },
    /// A day chosen (`2026-11-11`): the system's date picker in a form's
    /// row on the phones, a date field on the desktop and in the browser.
    DateField {
        label: &'static str,
        day: Val<String>,
        typed: Input<String, M>,
    },

    // A table (ui/docs/parts.md, Tables): rows of cells under column
    // headings on a regular width, each row a list's two lines (its cells
    // by their columns' slots) when compact. The parts decide which.
    /// A table: its columns, then its rows ([`Part::TableRow`]s and
    /// [`Part::TableHeading`]s).
    Table {
        columns: Vec<TableColumn>,
        rows: Box<View<M>>,
    },
    /// One row of a table: a cell per column, marked (a dot before it: new,
    /// unread) while `marked`, lit while `selected`; tapped, `tap`; held or
    /// right-clicked, its menu.
    TableRow(Box<TableRow<M>>),
    /// A heading among a table's rows (a day): no cells, no press.
    TableHeading {
        title: Val<String>,
    },

    // An app's first run (`crate::welcome`): any app's pages before it
    // has a server, and its sign-in, each the app's own words and mark.
    /// A first run's page: the app's mark (a glyph on glass in its
    /// accent), a large title, a sentence, and what to do under them,
    /// centred and no wider than a phone.
    FirstRunPage {
        glyph: Val<String>,
        title: Val<String>,
        sentence: Val<String>,
        content: Box<View<M>>,
    },
    /// A server's address: the keyboard for a URL, no capitals or
    /// corrections; `prompt` its placeholder, `note` under it until there
    /// is a `problem`; Go sends `submit`.
    AddressField {
        label: &'static str,
        prompt: Val<String>,
        text: Val<String>,
        note: Val<String>,
        problem: Val<Option<String>>,
        enabled: Val<bool>,
        typed: Input<String, M>,
        submit: M,
    },
    /// A button the width of its page: the accent filled (`prominent`) or
    /// glass; while `busy`, a spinner before its title and no press.
    WideButton {
        title: Val<String>,
        prominent: bool,
        enabled: Val<bool>,
        busy: Val<bool>,
        tap: M,
    },
    /// What a page waits on, quietly (a spinner beside it while `busy`),
    /// and what went wrong, in the error colour; either may be none.
    StatusLine {
        text: Val<Option<String>>,
        busy: Val<bool>,
        problem: Val<Option<String>>,
    },
    /// A QR code: `size` modules a side, row by row from the top left,
    /// each dark or not; drawn with the quiet zone a scanner needs.
    CodeImage {
        size: Val<i64>,
        dark: Val<Vec<bool>>,
    },
    /// A photo, by its URL: `file:` (a file this device holds,
    /// `Views::file_url`), `data:` (its bytes) or `https:`; empty while it
    /// is being fetched, a spinner in its place. Fitted to the width it is
    /// given, `label` said for it. With `zoom` it fills the room it is
    /// given, and on the phones pinches and double-taps to zoom, panned
    /// while zoomed, rubber-banded past its least and most size and
    /// settling back on `Springs.move`.
    Photo {
        url: Val<String>,
        label: Val<String>,
        zoom: bool,
    },
    /// A slip of paper (a receipt as it was printed): monospace, dark on
    /// white whatever the appearance; its heading bold and the line under
    /// it centred, then a rule, each of `names` with its `figures` against
    /// the right and its `notes` (empty for none) under it, a rule, and
    /// `total` bold against the right of TOTAL.
    Paper {
        heading: Val<String>,
        date: Val<String>,
        names: Val<Vec<String>>,
        figures: Val<Vec<String>>,
        notes: Val<Vec<String>>,
        total: Val<String>,
    },

    // The shell (ui/docs/design.md): what a standard Mac app is made of,
    // each part adapting to the size class inside itself.
    /// The app's sections: a sidebar on a regular width and the Mac, a tab
    /// bar when compact (SwiftUI's `TabView` `.sidebarAdaptable`, Compose's
    /// navigation suite, iced's sidebar). `tabs` are [`Part::SectionTab`]s;
    /// `selected` is the shown one's tag, `select` sends another's.
    Tabs {
        selected: Val<String>,
        select: Input<String, M>,
        tabs: Box<View<M>>,
        /// At the sidebar's foot (`.tabViewSidebarBottomBar`): who is signed
        /// in, the household. Not on a compact tab bar.
        footer: Box<View<M>>,
    },
    /// One section: its title, glyph and tag, a count at its end, what it
    /// shows. A tag `parent/child` is a child of the section tagged
    /// `parent`: its own place in the sidebar, the parent's tab (and the
    /// parent's pushed detail) when compact.
    SectionTab {
        title: Val<String>,
        glyph: Val<String>,
        tag: Val<String>,
        badge: Option<Val<Option<i64>>>,
        content: Box<View<M>>,
    },
    /// A group of sections under a heading (`TabSection`): in the sidebar
    /// only, never on a compact tab bar — Mail's mailboxes. Unless it is a
    /// place of its own (`place`: its glyph, its tag and what it shows, an
    /// index of its sections): then, short of a sidebar, the group is one
    /// tab, its sections pushed over that index, as Settings' are.
    TabGroup {
        title: &'static str,
        tabs: Box<View<M>>,
        place: Option<(&'static str, &'static str, Box<View<M>>)>,
    },
    /// What a list opens, pushed onto the section's stack at every width
    /// while `showing` (`navigationDestination(isPresented:)`), `back` to
    /// leave it.
    Push {
        content: Box<View<M>>,
        detail: Box<View<M>>,
        showing: Val<bool>,
        back: M,
    },
    /// A list beside what it opens: `NavigationSplitView`'s content and
    /// detail, a stack when compact (the detail pushed while `showing`,
    /// `back` to leave it).
    Split {
        content: Box<View<M>>,
        detail: Box<View<M>>,
        showing: Val<bool>,
        back: M,
    },
    /// A section's title and toolbar over its content: its items
    /// ([`Part::BarItem`]s) by placement.
    Toolbar {
        title: Val<String>,
        items: Box<View<M>>,
        content: Box<View<M>>,
    },
    /// A toolbar's button.
    BarItem {
        title: &'static str,
        glyph: &'static str,
        placement: Placement,
        enabled: Val<bool>,
        tap: M,
    },
    /// `content`'s search field: in the toolbar on a regular width and the
    /// Mac, under the title when compact; `/` on a keyboard.
    Searchable {
        prompt: &'static str,
        text: Val<String>,
        typed: Input<String, M>,
        content: Box<View<M>>,
    },
    /// A pane at the trailing edge beside `content` (a sheet when compact):
    /// `.inspector`, shown while `shown`.
    Inspector {
        title: &'static str,
        shown: Val<bool>,
        toggle: Input<bool, M>,
        content: Box<View<M>>,
        inspector: Box<View<M>>,
    },
    /// A screen the shell hosts by name: the app's own on each platform
    /// (hand-written, or a view generated apart), until it is moved here.
    Hosted {
        name: &'static str,
    },
    /// Long-form writing, as Markdown (`crate::writing`): headings,
    /// paragraphs, lists, code, callouts, tables, pictures.
    Writing {
        markdown: Val<String>,
    },
    /// A question before something that cannot be undone
    /// (`.confirmationDialog`): its actions, then Cancel.
    Confirm {
        title: Val<String>,
        message: Val<String>,
        actions: Vec<DialogAction<M>>,
        cancel: M,
    },
    /// Something the person has to know (`.alert`): OK.
    Alert {
        title: Val<String>,
        message: Val<String>,
        ok: M,
    },
    /// The Settings scene: its panes ([`Part::SettingsPane`]), a window of
    /// tabs on the Mac and the desktop, a list of screens elsewhere.
    SettingsPanes {
        /// The shown pane's title; choosing another sends `select` with its.
        selected: Val<String>,
        select: Input<String, M>,
        panes: Box<View<M>>,
    },
    SettingsPane {
        title: &'static str,
        glyph: &'static str,
        content: Box<View<M>>,
    },
}

/// Where a toolbar item goes (SwiftUI's `ToolbarItemPlacement`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// At the leading edge: back, the sidebar's toggle.
    Navigation,
    /// In the middle: a month's pager.
    Principal,
    /// The section's main action, at the trailing edge (an app bar's
    /// action on Android).
    Primary,
    /// Less used: in the overflow menu on a phone.
    Secondary,
}

impl Placement {
    /// As the parts' enum spells it in Swift and Kotlin (`BarPlacement`).
    pub fn case(self) -> &'static str {
        match self {
            Placement::Navigation => "navigation",
            Placement::Principal => "principal",
            Placement::Primary => "primary",
            Placement::Secondary => "secondary",
        }
    }
}

/// One action of a [`Part::Confirm`].
#[derive(Clone, Debug)]
pub struct DialogAction<M> {
    pub title: Val<String>,
    pub destructive: bool,
    pub tap: M,
}

impl<M> From<Part<M>> for View<M> {
    fn from(p: Part<M>) -> View<M> {
        View::Part(p)
    }
}

impl<M> From<()> for View<M> {
    fn from(_: ()) -> View<M> {
        View::Empty
    }
}

impl<M> From<Vec<View<M>>> for View<M> {
    fn from(v: Vec<View<M>>) -> View<M> {
        View::List(v)
    }
}

macro_rules! tuples {
    ($($t:ident)+) => {
        #[allow(non_snake_case)]
        impl<M, $($t: Into<View<M>>),+> From<($($t,)+)> for View<M> {
            fn from(($($t,)+): ($($t,)+)) -> View<M> {
                View::List(vec![$($t.into()),+])
            }
        }
    };
}
tuples!(A);
tuples!(A B);
tuples!(A B C);
tuples!(A B C D);
tuples!(A B C D E);
tuples!(A B C D E F);
tuples!(A B C D E F G);
tuples!(A B C D E F G H);

// * The parts, as builders

/// A tab's page.
pub fn tab_page<M>(title: impl Into<Val<String>>, content: impl Into<View<M>>) -> PageBuilder<M> {
    PageBuilder {
        title: title.into(),
        refresh: None,
        top: View::Empty,
        content: content.into(),
    }
}

pub struct PageBuilder<M> {
    title: Val<String>,
    refresh: Option<M>,
    top: View<M>,
    content: View<M>,
}

impl<M> PageBuilder<M> {
    /// Pulled down, `m`.
    pub fn refresh(mut self, m: M) -> Self {
        self.refresh = Some(m);
        self
    }

    /// What sits under the title, before the content: the page's figure.
    pub fn top(mut self, v: impl Into<View<M>>) -> Self {
        self.top = v.into();
        self
    }
}

impl<M> From<PageBuilder<M>> for View<M> {
    fn from(b: PageBuilder<M>) -> View<M> {
        Part::Page {
            title: b.title,
            refresh: b.refresh,
            top: Box::new(b.top),
            content: Box::new(b.content),
        }
        .into()
    }
}

/// The page's one large figure.
pub fn hero_figure<M>(label: &'static str, cents: impl Into<Val<i64>>, note: &'static str) -> View<M> {
    Part::Hero {
        label,
        cents: cents.into(),
        note,
    }
    .into()
}

/// The page's figure in a card, made of `first` and `second` (what is
/// owned and what is owed), with a sparkline of `trend`, oldest first.
pub fn summary_card<M>(
    label: &'static str,
    cents: impl Into<Val<i64>>,
    first: &'static str,
    first_cents: impl Into<Val<i64>>,
    second: &'static str,
    second_cents: impl Into<Val<i64>>,
    trend: impl Into<Val<Vec<i64>>>,
) -> View<M> {
    Part::Summary {
        label,
        cents: cents.into(),
        first,
        first_cents: first_cents.into(),
        second,
        second_cents: second_cents.into(),
        trend: trend.into(),
    }
    .into()
}

/// A group of rows in a card.
#[derive(Clone, Debug)]
pub struct Section<M> {
    pub title: Option<Val<String>>,
    pub amount: Option<Val<i64>>,
    /// While the amount shows; always when none.
    pub shows: Option<Val<bool>>,
    pub footer: Option<Val<Option<String>>>,
    /// Words in the caution's tint where the total would be, while there
    /// are any ("$40.00 not covered").
    pub note: Option<Val<Option<String>>>,
    /// On a regular width, two more figures before the total, each under
    /// its column's title (and the total under the third).
    pub columns: Option<Box<Columns>>,
    /// Its rows shown while this holds; its heading, tapped, sends the message.
    pub open: Option<(Val<bool>, M)>,
    /// What holding its heading offers.
    pub menu: Vec<MenuItem<M>>,
    pub rows: Box<View<M>>,
}

/// A section's figures in columns ([`Section::columns`]).
#[derive(Clone, Debug)]
pub struct Columns {
    pub titles: [&'static str; 3],
    pub first: Val<i64>,
    pub second: Val<i64>,
}

/// ArkUI's `HeaderCard` ([`Part::HeaderCard`]).
#[derive(Clone, Debug)]
pub struct HeaderCard<M> {
    pub title: Val<String>,
    pub figure: Val<String>,
    /// `neutral`, `positive`, `caution`, `negative` or `accent`.
    pub tint: Val<String>,
    pub glyph: Option<Val<String>>,
    /// The figure in a status badge; else large and bold.
    pub badge: bool,
    pub line: Option<Val<String>>,
    pub caption: Option<Val<String>>,
    pub actions: Box<View<M>>,
}

/// A card at the top of an inspector or a detail: `title` (none: the empty
/// text, the glyph alone), `figure` in `tint` ([`HeaderCard`]).
pub fn header_card<M>(title: impl Into<Val<String>>, figure: impl Into<Val<String>>, tint: impl Into<Val<String>>) -> HeaderCard<M> {
    HeaderCard {
        title: title.into(),
        figure: figure.into(),
        tint: tint.into(),
        glyph: None,
        badge: false,
        line: None,
        caption: None,
        actions: Box::new(View::Empty),
    }
}

impl<M> HeaderCard<M> {
    pub fn glyph(mut self, g: impl Into<Val<String>>) -> Self {
        self.glyph = Some(g.into());
        self
    }
    /// The figure in a status badge (a category's available).
    pub fn badge(mut self) -> Self {
        self.badge = true;
        self
    }
    /// What needs attention, in the tint: "Overspent by $21.02".
    pub fn line(mut self, l: impl Into<Val<String>>) -> Self {
        self.line = Some(l.into());
        self
    }
    /// A quiet word under it: the group, the account.
    pub fn caption(mut self, c: impl Into<Val<String>>) -> Self {
        self.caption = Some(c.into());
        self
    }
    /// Its actions, rows under a divider in the card.
    pub fn actions(mut self, a: impl Into<View<M>>) -> Self {
        self.actions = Box::new(a.into());
        self
    }
}

impl<M> From<HeaderCard<M>> for View<M> {
    fn from(h: HeaderCard<M>) -> View<M> {
        Part::HeaderCard(Box::new(h)).into()
    }
}

/// An inspector's or a detail's page of cards ([`Part::Detail`]).
pub fn detail_page<M>(content: impl Into<View<M>>) -> View<M> {
    Part::Detail {
        content: Box::new(content.into()),
    }
    .into()
}

/// How far toward a goal (`fraction`, 0 to 1), in `tint`, `note` under it.
pub fn goal_progress<M>(fraction: impl Into<Val<f64>>, tint: impl Into<Val<String>>, note: impl Into<Val<String>>) -> View<M> {
    Part::GoalProgress {
        fraction: fraction.into(),
        tint: tint.into(),
        note: note.into(),
    }
    .into()
}

pub fn card_section<M>(rows: impl Into<View<M>>) -> Section<M> {
    Section {
        title: None,
        amount: None,
        shows: None,
        footer: None,
        note: None,
        columns: None,
        open: None,
        menu: vec![],
        rows: Box::new(rows.into()),
    }
}

impl<M> Section<M> {
    pub fn header(mut self, title: impl Into<Val<String>>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// A figure at the heading's end, in the error's red below nothing.
    pub fn amount(mut self, cents: impl Into<Val<i64>>) -> Self {
        self.amount = Some(cents.into());
        self
    }

    /// The figure only while `b` holds.
    pub fn shows_amount(mut self, b: impl Into<Val<bool>>) -> Self {
        self.shows = Some(b.into());
        self
    }

    /// A note under the card, while there is one.
    pub fn footer(mut self, text: impl Into<Val<Option<String>>>) -> Self {
        self.footer = Some(text.into());
        self
    }

    /// Words in the caution's tint in the total's place, while there are any.
    pub fn total_note(mut self, text: impl Into<Val<Option<String>>>) -> Self {
        self.note = Some(text.into());
        self
    }

    /// Two figures before the total on a regular width, under `titles`'
    /// first two (the total under the third).
    pub fn columns(mut self, titles: [&'static str; 3], first: impl Into<Val<i64>>, second: impl Into<Val<i64>>) -> Self {
        self.columns = Some(Box::new(Columns {
            titles,
            first: first.into(),
            second: second.into(),
        }));
        self
    }

    /// Its rows only while `open`; its heading, tapped, `toggle`.
    pub fn collapsible(mut self, open: impl Into<Val<bool>>, toggle: M) -> Self {
        self.open = Some((open.into(), toggle));
        self
    }

    /// `title` in what holding the heading offers, while `shown` holds.
    pub fn menu_if(
        mut self,
        title: impl Into<Val<String>>,
        glyph: impl Into<Val<String>>,
        tap: M,
        destructive: bool,
        shown: impl Into<Val<bool>>,
    ) -> Self {
        self.menu.push(MenuItem {
            title: title.into(),
            glyph: glyph.into(),
            tap,
            destructive: Val::Is(destructive),
            shown: Some(shown.into()),
        });
        self
    }
}

/// ArkUI's `MeterRow` ([`Part::MeterRow`]).
#[derive(Clone, Debug)]
pub struct MeterRow<M> {
    pub title: Val<String>,
    /// The app's mark of this kind before the title, while there is one.
    pub mark: Option<Val<Option<String>>>,
    pub faded: Option<Val<bool>>,
    /// How far, 0 to 1, while there is a goal; and the bar's tint.
    pub fraction: Option<Val<Option<f64>>>,
    pub meter_tint: Option<Val<String>>,
    /// Words under the bar, in `note_tint`, while there are any.
    pub note: Option<Val<Option<String>>>,
    pub note_tint: Option<Val<String>>,
    /// Quiet words at the end of the note's line.
    pub note_end: Option<Val<Option<String>>>,
    pub columns: Option<(Val<i64>, Val<i64>)>,
    pub status: Val<String>,
    pub status_tint: Val<String>,
    pub said: Option<Val<String>>,
    pub tap: Option<M>,
    pub menu: Vec<MenuItem<M>>,
}

/// A row toward a goal: `title`, and `status` in a badge of `tint` at its end.
pub fn meter_row<M>(title: impl Into<Val<String>>, status: impl Into<Val<String>>, tint: impl Into<Val<String>>) -> MeterRow<M> {
    MeterRow {
        title: title.into(),
        mark: None,
        faded: None,
        fraction: None,
        meter_tint: None,
        note: None,
        note_tint: None,
        note_end: None,
        columns: None,
        status: status.into(),
        status_tint: tint.into(),
        said: None,
        tap: None,
        menu: vec![],
    }
}

impl<M> MeterRow<M> {
    /// The app's mark of `kind` before the title, while there is one.
    pub fn mark(mut self, kind: impl Into<Val<Option<String>>>) -> Self {
        self.mark = Some(kind.into());
        self
    }
    /// Its title quiet while `b` holds (hidden, not gone).
    pub fn faded(mut self, b: impl Into<Val<bool>>) -> Self {
        self.faded = Some(b.into());
        self
    }
    pub fn meter(mut self, fraction: impl Into<Val<Option<f64>>>, tint: impl Into<Val<String>>) -> Self {
        self.fraction = Some(fraction.into());
        self.meter_tint = Some(tint.into());
        self
    }
    pub fn note(mut self, text: impl Into<Val<Option<String>>>, tint: impl Into<Val<String>>) -> Self {
        self.note = Some(text.into());
        self.note_tint = Some(tint.into());
        self
    }
    pub fn note_end(mut self, text: impl Into<Val<Option<String>>>) -> Self {
        self.note_end = Some(text.into());
        self
    }
    /// Two figures in columns before the badge, on a regular width.
    pub fn columns(mut self, first: impl Into<Val<i64>>, second: impl Into<Val<i64>>) -> Self {
        self.columns = Some((first.into(), second.into()));
        self
    }
    pub fn said(mut self, s: impl Into<Val<String>>) -> Self {
        self.said = Some(s.into());
        self
    }
    pub fn on_tap(mut self, m: M) -> Self {
        self.tap = Some(m);
        self
    }
    /// `title` in what holding the row offers, while `shown` holds;
    /// destructive when `destructive`.
    pub fn menu_if(
        mut self,
        title: impl Into<Val<String>>,
        glyph: impl Into<Val<String>>,
        tap: M,
        destructive: bool,
        shown: impl Into<Val<bool>>,
    ) -> Self {
        self.menu.push(MenuItem {
            title: title.into(),
            glyph: glyph.into(),
            tap,
            destructive: Val::Is(destructive),
            shown: Some(shown.into()),
        });
        self
    }
}

impl<M> From<MeterRow<M>> for View<M> {
    fn from(r: MeterRow<M>) -> View<M> {
        Part::MeterRow(Box::new(r)).into()
    }
}

/// A page's headline ([`Part::Headline`]).
#[derive(Clone, Debug)]
pub struct Headline<M> {
    pub label: Val<String>,
    pub figure: Val<String>,
    pub tint: Val<String>,
    pub caption: Val<String>,
    /// Its one action: its title, its glyph, its message.
    pub action: Option<(&'static str, &'static str, M)>,
}

/// What [`headline`] builds.
pub type HeadlineBuilder<M> = Headline<M>;

/// A page's headline: `label`, `figure` large in `tint`, `caption` under it.
pub fn headline<M>(
    label: impl Into<Val<String>>,
    figure: impl Into<Val<String>>,
    tint: impl Into<Val<String>>,
    caption: impl Into<Val<String>>,
) -> HeadlineBuilder<M> {
    Headline {
        label: label.into(),
        figure: figure.into(),
        tint: tint.into(),
        caption: caption.into(),
        action: None,
    }
}

impl<M> HeadlineBuilder<M> {
    /// Its one action at its end: `title` with `glyph`, tapped `tap`.
    pub fn action(mut self, title: &'static str, glyph: &'static str, tap: M) -> Self {
        self.action = Some((title, glyph, tap));
        self
    }
}

impl<M> From<Headline<M>> for View<M> {
    fn from(h: Headline<M>) -> View<M> {
        Part::Headline(Box::new(h)).into()
    }
}

impl<M> From<Section<M>> for View<M> {
    fn from(s: Section<M>) -> View<M> {
        Part::Section(s).into()
    }
}

/// Where a row's picture comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum Picture {
    None,
    /// A glyph on a rounded tile, in the quiet colour.
    Tile(&'static str),
    /// The app's mark for a kind of thing (an account's): drawn by its
    /// glyph on the desktop and in the browser, by the app's own mark of
    /// `kind` on the phones.
    Mark {
        glyph: Val<String>,
        kind: Val<String>,
    },
    /// A bare glyph in the accent colour: an action row.
    Glyph(&'static str),
}

/// One thing a row's menu offers.
#[derive(Clone, Debug)]
pub struct MenuItem<M> {
    pub title: Val<String>,
    pub glyph: Val<String>,
    pub tap: M,
    /// Destructive while this holds.
    pub destructive: Val<bool>,
    /// Offered only while this holds; always when none.
    pub shown: Option<Val<bool>>,
}

/// ArkUI's `ListRow`.
#[derive(Clone, Debug)]
pub struct ListRow<M> {
    pub title: Val<String>,
    pub subtitle: Option<Val<String>>,
    pub picture: Picture,
    pub badge: Option<Val<Option<i64>>>,
    pub amount: Option<Val<i64>>,
    /// Words at its end, quiet (a date, a status), where an amount would be.
    pub value: Option<Val<String>>,
    /// The amount in the quiet colour (a total).
    pub quiet: bool,
    /// In the accent's colour: an action, not a place.
    pub accent: bool,
    /// Its tap opens a page: a `NavigationLink`'s row on the iPhone (its
    /// chevron, its highlight), a chevron at its end elsewhere.
    pub pushes: bool,
    pub tap: Option<M>,
    pub said: Option<Val<String>>,
    pub menu: Vec<MenuItem<M>>,
}

pub fn list_row<M>(title: impl Into<Val<String>>) -> ListRow<M> {
    ListRow {
        title: title.into(),
        subtitle: None,
        picture: Picture::None,
        badge: None,
        amount: None,
        value: None,
        quiet: false,
        accent: false,
        pushes: false,
        tap: None,
        said: None,
        menu: vec![],
    }
}

impl<M> ListRow<M> {
    pub fn subtitle(mut self, s: impl Into<Val<String>>) -> Self {
        self.subtitle = Some(s.into());
        self
    }
    pub fn tile(mut self, glyph: &'static str) -> Self {
        self.picture = Picture::Tile(glyph);
        self
    }
    pub fn mark(mut self, glyph: impl Into<Val<String>>, kind: impl Into<Val<String>>) -> Self {
        self.picture = Picture::Mark {
            glyph: glyph.into(),
            kind: kind.into(),
        };
        self
    }
    pub fn glyph(mut self, glyph: &'static str) -> Self {
        self.picture = Picture::Glyph(glyph);
        self
    }
    /// A count in a badge, while there is one.
    pub fn badge(mut self, count: impl Into<Val<Option<i64>>>) -> Self {
        self.badge = Some(count.into());
        self
    }
    pub fn amount(mut self, cents: impl Into<Val<i64>>) -> Self {
        self.amount = Some(cents.into());
        self
    }
    /// Words at its end, quiet ([`ListRow::value`]).
    pub fn value(mut self, v: impl Into<Val<String>>) -> Self {
        self.value = Some(v.into());
        self
    }
    pub fn quiet(mut self) -> Self {
        self.quiet = true;
        self
    }
    pub fn accent(mut self) -> Self {
        self.accent = true;
        self
    }
    /// Its tap opens a page (see [`ListRow::pushes`]).
    pub fn pushes(mut self) -> Self {
        self.pushes = true;
        self
    }
    pub fn on_tap(mut self, m: M) -> Self {
        self.tap = Some(m);
        self
    }
    pub fn said(mut self, s: impl Into<Val<String>>) -> Self {
        self.said = Some(s.into());
        self
    }
    /// `title` in what holding the row offers.
    pub fn menu(mut self, title: impl Into<Val<String>>, glyph: &'static str, tap: M, destructive: impl Into<Val<bool>>) -> Self {
        self.menu.push(MenuItem {
            title: title.into(),
            glyph: glyph.into(),
            tap,
            destructive: destructive.into(),
            shown: None,
        });
        self
    }
    /// `title` with a glyph read from the record (it changes with the row).
    pub fn menu_glyph(mut self, title: impl Into<Val<String>>, glyph: impl Into<Val<String>>, tap: M, destructive: impl Into<Val<bool>>) -> Self {
        self.menu.push(MenuItem {
            title: title.into(),
            glyph: glyph.into(),
            tap,
            destructive: destructive.into(),
            shown: None,
        });
        self
    }
    /// `title`, never destructive, offered only while `shown` holds.
    pub fn menu_when(mut self, title: &'static str, glyph: &'static str, tap: M, shown: impl Into<Val<bool>>) -> Self {
        self.menu.push(MenuItem {
            title: title.into(),
            glyph: glyph.into(),
            tap,
            destructive: Val::Is(false),
            shown: Some(shown.into()),
        });
        self
    }
}

impl<M> ListRow<M> {
    /// `title` in what holding the row offers, never destructive.
    pub fn menu_fixed(mut self, title: &'static str, glyph: &'static str, tap: M) -> Self {
        self.menu.push(MenuItem {
            title: title.into(),
            glyph: glyph.into(),
            tap,
            destructive: Val::Is(false),
            shown: None,
        });
        self
    }
}

impl<M> MenuItem<M> {
    /// Whether it is offered: what a real view draws.
    pub fn offered(&self) -> bool {
        self.shown.as_ref().is_none_or(|s| s.get())
    }
}

impl<M> From<ListRow<M>> for View<M> {
    fn from(r: ListRow<M>) -> View<M> {
        Part::Row(Box::new(r)).into()
    }
}

/// What keeps a sheet: its title, whether it can yet, and its message.
#[derive(Clone, Debug)]
pub struct Confirm<M> {
    pub title: &'static str,
    pub enabled: Val<bool>,
    pub tap: M,
}

/// A sheet with a form in it, put away with `dismiss`. (`FormSheet` on the
/// phones: an app's own `Sheet` type would hide a part of that name.)
pub fn form_sheet<M>(title: &'static str, dismiss: M, content: impl Into<View<M>>) -> SheetBuilder<M> {
    SheetBuilder {
        title,
        dismiss,
        confirm: None,
        content: content.into(),
    }
}

pub struct SheetBuilder<M> {
    title: &'static str,
    dismiss: M,
    confirm: Option<Confirm<M>>,
    content: View<M>,
}

impl<M> SheetBuilder<M> {
    /// Kept with `title` while `enabled` holds, sending `tap`.
    pub fn confirm(mut self, title: &'static str, enabled: impl Into<Val<bool>>, tap: M) -> Self {
        self.confirm = Some(Confirm {
            title,
            enabled: enabled.into(),
            tap,
        });
        self
    }
}

impl<M> From<SheetBuilder<M>> for View<M> {
    fn from(b: SheetBuilder<M>) -> View<M> {
        Part::Sheet {
            title: b.title,
            dismiss: b.dismiss,
            confirm: b.confirm,
            content: Box::new(b.content),
        }
        .into()
    }
}

/// A group of a form's fields.
pub fn form_section<M>(content: impl Into<View<M>>) -> FormSectionBuilder<M> {
    FormSectionBuilder {
        header: None,
        footer: None,
        content: content.into(),
    }
}

pub struct FormSectionBuilder<M> {
    header: Option<Val<String>>,
    footer: Option<Val<String>>,
    content: View<M>,
}

impl<M> FormSectionBuilder<M> {
    pub fn header(mut self, t: impl Into<Val<String>>) -> Self {
        self.header = Some(t.into());
        self
    }
    pub fn footer(mut self, t: impl Into<Val<String>>) -> Self {
        self.footer = Some(t.into());
        self
    }
}

impl<M> From<FormSectionBuilder<M>> for View<M> {
    fn from(b: FormSectionBuilder<M>) -> View<M> {
        Part::FormSection {
            header: b.header,
            footer: b.footer,
            content: Box::new(b.content),
        }
        .into()
    }
}

/// One of `choices` ([`picker_choice`]s), the one tagged `selected` chosen;
/// choosing another sends `pick` with its tag.
pub fn option_picker<M: 'static>(
    label: &'static str,
    selected: impl Into<Val<String>>,
    pick: fn(String) -> M,
    choices: impl Into<View<M>>,
) -> View<M> {
    Part::Picker {
        label,
        selected: selected.into(),
        pick: Input::new(pick),
        choices: Box::new(choices.into()),
    }
    .into()
}

/// One choice of an [`option_picker`].
pub fn picker_choice<M>(title: impl Into<Val<String>>, glyph: impl Into<Val<String>>, tag: impl Into<Val<String>>) -> View<M> {
    Part::Choice {
        title: title.into(),
        glyph: glyph.into(),
        tag: tag.into(),
    }
    .into()
}

/// An amount entered the app's way (see [`Part::AmountEntry`]).
pub fn amount_entry<M: 'static>(
    label: impl Into<Val<String>>,
    prompt: impl Into<Val<String>>,
    typed: impl Into<Val<String>>,
    cents: impl Into<Val<i64>>,
    problem: impl Into<Val<Option<String>>>,
    on_typed: fn(String) -> M,
    on_cents: fn(i64) -> M,
) -> View<M> {
    Part::AmountEntry {
        label: label.into(),
        prompt: prompt.into(),
        typed: typed.into(),
        cents: cents.into(),
        problem: problem.into(),
        on_typed: Input::new(on_typed),
        on_cents: Input::new(on_cents),
    }
    .into()
}

/// An alert with one text field.
pub fn prompt<M: 'static>(
    title: &'static str,
    text: impl Into<Val<String>>,
    typed: fn(String) -> M,
    confirm: &'static str,
    ok: M,
    cancel: M,
) -> View<M> {
    Part::Prompt {
        title,
        text: text.into(),
        typed: Input::new(typed),
        confirm,
        ok,
        cancel,
    }
    .into()
}

/// A field for a name, the domain's sentence under it.
pub fn name_field<M: 'static>(
    label: &'static str,
    value: impl Into<Val<String>>,
    problem: impl Into<Val<Option<String>>>,
    typed: fn(String) -> M,
) -> View<M> {
    Part::NameField {
        label,
        value: value.into(),
        problem: problem.into(),
        typed: Input::new(typed),
    }
    .into()
}

/// A field for an amount.
pub fn amount_field<M: 'static>(
    label: impl Into<Val<String>>,
    typed: impl Into<Val<String>>,
    problem: impl Into<Val<Option<String>>>,
    on: fn(String) -> M,
) -> View<M> {
    Part::AmountField {
        label: label.into(),
        typed: typed.into(),
        problem: problem.into(),
        on: Input::new(on),
    }
    .into()
}

/// Choices in a wrapping row.
pub fn chip_row<M>(content: impl Into<View<M>>) -> View<M> {
    Part::Chips(Box::new(content.into())).into()
}

pub fn chip<M>(label: impl Into<Val<String>>, glyph: impl Into<Val<String>>, selected: impl Into<Val<bool>>, tap: M) -> View<M> {
    Part::Chip {
        label: label.into(),
        glyph: glyph.into(),
        selected: selected.into(),
        tap,
    }
    .into()
}

/// A switch with its title and a note under it.
pub fn switch_row<M: 'static>(title: &'static str, note: impl Into<Val<String>>, on: impl Into<Val<bool>>, change: fn(bool) -> M) -> View<M> {
    Part::Toggle {
        title,
        note: Some(note.into()),
        on: on.into(),
        change: Input::new(change),
    }
    .into()
}

/// A switch with its title alone (its note in the form section's footer).
pub fn switch<M: 'static>(title: &'static str, on: impl Into<Val<bool>>, change: fn(bool) -> M) -> View<M> {
    Part::Toggle {
        title,
        note: None,
        on: on.into(),
        change: Input::new(change),
    }
    .into()
}

/// A form's button.
pub fn form_button<M>(title: &'static str, enabled: impl Into<Val<bool>>, tap: M) -> View<M> {
    Part::Button {
        title,
        enabled: enabled.into(),
        tap,
    }
    .into()
}

/// Bars of `values` against the largest, each `labels`' over it and
/// `said`'s at its end ([`Part::BarChart`]).
pub fn bar_chart<M>(labels: impl Into<Val<Vec<String>>>, values: impl Into<Val<Vec<i64>>>, said: impl Into<Val<Vec<String>>>) -> View<M> {
    Part::BarChart {
        labels: labels.into(),
        values: values.into(),
        said: said.into(),
    }
    .into()
}

/// A day chosen, as `2026-11-11` ([`Part::DateField`]).
pub fn date_field<M: 'static>(label: &'static str, day: impl Into<Val<String>>, typed: fn(String) -> M) -> View<M> {
    Part::DateField {
        label,
        day: day.into(),
        typed: Input::new(typed),
    }
    .into()
}

// * A table's parts

/// How wide a table's column is: one of the theme's widths
/// (`ui/theme.toml`'s `[layout]`), or a share of what is left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnWidth {
    /// A column of figures (`amount-column-width`).
    Figure,
    /// A column of one mark (`mark-column-width`).
    Mark,
    /// A share of what is left after the others (a payee: 3, a memo: 2).
    Share(u16),
}

impl ColumnWidth {
    /// As the parts' enum spells it in Swift and Kotlin (`ColumnWidth`).
    pub fn case(self) -> &'static str {
        match self {
            ColumnWidth::Figure => "figure",
            ColumnWidth::Mark => "mark",
            ColumnWidth::Share(_) => "share",
        }
    }
}

/// Where a column's cells sit in their column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnAlign {
    Start,
    /// Figures: at the end, their digits in columns.
    End,
    Center,
}

impl ColumnAlign {
    /// As the parts' enum spells it in Swift and Kotlin (`ColumnAlign`).
    pub fn case(self) -> &'static str {
        match self {
            ColumnAlign::Start => "start",
            ColumnAlign::End => "end",
            ColumnAlign::Center => "center",
        }
    }
}

/// Where a column's cell goes when a compact table is a list, as the
/// iPhone's `List` row has them: the title and the words under it at the
/// leading end, a figure and the words under it at the trailing end. A
/// column with none is left out of a compact row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnSlot {
    None,
    Title,
    Subtitle,
    Trailing,
    TrailingSub,
}

impl ColumnSlot {
    /// As the parts' enum spells it in Swift and Kotlin (`ColumnSlot`).
    pub fn case(self) -> &'static str {
        match self {
            ColumnSlot::None => "none",
            ColumnSlot::Title => "title",
            ColumnSlot::Subtitle => "subtitle",
            ColumnSlot::Trailing => "trailing",
            ColumnSlot::TrailingSub => "trailingSub",
        }
    }
}

/// A table's column: its heading, its width, how its cells line up and
/// where they go when the table is compact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableColumn {
    pub heading: &'static str,
    pub width: ColumnWidth,
    pub align: ColumnAlign,
    pub slot: ColumnSlot,
}

/// A column `heading`, `width` wide, its cells at the start, left out when
/// compact (see [`TableColumn::slot`]).
pub fn table_column(heading: &'static str, width: ColumnWidth) -> TableColumn {
    TableColumn {
        heading,
        width,
        align: ColumnAlign::Start,
        slot: ColumnSlot::None,
    }
}

impl TableColumn {
    /// Its cells at the end, figures in columns.
    pub fn end(mut self) -> Self {
        self.align = ColumnAlign::End;
        self
    }
    pub fn center(mut self) -> Self {
        self.align = ColumnAlign::Center;
        self
    }
    /// Where its cell goes in a compact table's row.
    pub fn slot(mut self, slot: ColumnSlot) -> Self {
        self.slot = slot;
        self
    }
}

/// One cell of a table's row: its words, their tint (`""` the ink;
/// `quiet`, `positive`, `caution`, `negative`, `accent`), and a glyph
/// before them (a Lucide name, `""` for none). A cell with a glyph and no
/// words is the glyph alone, said by the row's `said`. A record holds a
/// row's cells as three lists ([`Cells`]), which every toolkit reads as
/// plain strings.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct TableCell {
    pub text: String,
    pub tint: String,
    pub glyph: String,
}

impl TableCell {
    /// Words in the ink.
    pub fn plain(text: impl Into<String>) -> TableCell {
        TableCell {
            text: text.into(),
            ..TableCell::default()
        }
    }
    /// Words in `tint`.
    pub fn tinted(text: impl Into<String>, tint: &str) -> TableCell {
        TableCell {
            text: text.into(),
            tint: tint.into(),
            glyph: String::new(),
        }
    }
    /// A glyph alone, in `tint`.
    pub fn glyph(glyph: &str, tint: &str) -> TableCell {
        TableCell {
            text: String::new(),
            tint: tint.into(),
            glyph: glyph.into(),
        }
    }
}

/// A row's cells as a record holds them: their words, tints and glyphs,
/// one of each a column ([`TableCell`]).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Cells {
    pub text: Vec<String>,
    pub tint: Vec<String>,
    pub glyph: Vec<String>,
}

impl Cells {
    pub fn of(cells: Vec<TableCell>) -> Cells {
        let mut c = Cells::default();
        for t in cells {
            c.text.push(t.text);
            c.tint.push(t.tint);
            c.glyph.push(t.glyph);
        }
        c
    }

    /// The cells again, one a column (a list cut short is the ink, no glyph).
    pub fn cells(text: Vec<String>, tint: Vec<String>, glyph: Vec<String>) -> Vec<TableCell> {
        text.into_iter()
            .enumerate()
            .map(|(i, text)| TableCell {
                text,
                tint: tint.get(i).cloned().unwrap_or_default(),
                glyph: glyph.get(i).cloned().unwrap_or_default(),
            })
            .collect()
    }
}

/// A table of `columns` over `rows` ([`table_row`]s, [`table_heading`]s).
pub fn data_table<M, const N: usize>(columns: [TableColumn; N], rows: impl Into<View<M>>) -> View<M> {
    Part::Table {
        columns: columns.to_vec(),
        rows: Box::new(rows.into()),
    }
    .into()
}

/// A heading among a table's rows.
pub fn table_heading<M>(title: impl Into<Val<String>>) -> View<M> {
    Part::TableHeading { title: title.into() }.into()
}

/// ArkUI's `DataTableRow` ([`Part::TableRow`]).
#[derive(Clone, Debug)]
pub struct TableRow<M> {
    /// A cell's words for each column, in their order; their tints and
    /// glyphs beside them ([`TableCell`]).
    pub cells: Val<Vec<String>>,
    pub tints: Option<Val<Vec<String>>>,
    pub glyphs: Option<Val<Vec<String>>>,
    pub marked: Option<Val<bool>>,
    pub selected: Option<Val<bool>>,
    pub said: Option<Val<String>>,
    pub tap: Option<M>,
    pub menu: Vec<MenuItem<M>>,
}

/// A table's row of `cells`' words, one per column.
pub fn table_row<M>(cells: impl Into<Val<Vec<String>>>) -> TableRow<M> {
    TableRow {
        cells: cells.into(),
        tints: None,
        glyphs: None,
        marked: None,
        selected: None,
        said: None,
        tap: None,
        menu: vec![],
    }
}

impl<M> TableRow<M> {
    /// Each cell's tint ([`TableCell::tint`]).
    pub fn tints(mut self, t: impl Into<Val<Vec<String>>>) -> Self {
        self.tints = Some(t.into());
        self
    }
    /// Each cell's glyph ([`TableCell::glyph`]).
    pub fn glyphs(mut self, g: impl Into<Val<Vec<String>>>) -> Self {
        self.glyphs = Some(g.into());
        self
    }
    /// Its cells, as the toolkits draw them.
    pub fn table_cells(&self) -> Vec<TableCell> {
        Cells::cells(
            self.cells.get(),
            self.tints.as_ref().map(|t| t.get()).unwrap_or_default(),
            self.glyphs.as_ref().map(|g| g.get()).unwrap_or_default(),
        )
    }
    /// A dot before it while `b` holds: new, not yet looked at.
    pub fn marked(mut self, b: impl Into<Val<bool>>) -> Self {
        self.marked = Some(b.into());
        self
    }
    /// Lit while `b` holds: what the inspector is about.
    pub fn selected(mut self, b: impl Into<Val<bool>>) -> Self {
        self.selected = Some(b.into());
        self
    }
    pub fn said(mut self, s: impl Into<Val<String>>) -> Self {
        self.said = Some(s.into());
        self
    }
    pub fn on_tap(mut self, m: M) -> Self {
        self.tap = Some(m);
        self
    }
    /// `title` in what holding the row offers, while `shown` holds.
    pub fn menu_if(
        mut self,
        title: impl Into<Val<String>>,
        glyph: impl Into<Val<String>>,
        tap: M,
        destructive: bool,
        shown: impl Into<Val<bool>>,
    ) -> Self {
        self.menu.push(MenuItem {
            title: title.into(),
            glyph: glyph.into(),
            tap,
            destructive: Val::Is(destructive),
            shown: Some(shown.into()),
        });
        self
    }
    /// `title` in what holding the row offers, always.
    pub fn menu(mut self, title: impl Into<Val<String>>, glyph: impl Into<Val<String>>, tap: M, destructive: bool) -> Self {
        self.menu.push(MenuItem {
            title: title.into(),
            glyph: glyph.into(),
            tap,
            destructive: Val::Is(destructive),
            shown: None,
        });
        self
    }
}

impl<M> From<TableRow<M>> for View<M> {
    fn from(r: TableRow<M>) -> View<M> {
        Part::TableRow(Box::new(r)).into()
    }
}

// * A first run's parts

/// A first run's page: the app's mark, its title and sentence, then `content`.
pub fn first_run_page<M>(
    glyph: impl Into<Val<String>>,
    title: impl Into<Val<String>>,
    sentence: impl Into<Val<String>>,
    content: impl Into<View<M>>,
) -> View<M> {
    Part::FirstRunPage {
        glyph: glyph.into(),
        title: title.into(),
        sentence: sentence.into(),
        content: Box::new(content.into()),
    }
    .into()
}

/// A server's address field.
#[allow(clippy::too_many_arguments)]
pub fn address_field<M: 'static>(
    label: &'static str,
    prompt: impl Into<Val<String>>,
    text: impl Into<Val<String>>,
    note: impl Into<Val<String>>,
    problem: impl Into<Val<Option<String>>>,
    enabled: impl Into<Val<bool>>,
    typed: fn(String) -> M,
    submit: M,
) -> View<M> {
    Part::AddressField {
        label,
        prompt: prompt.into(),
        text: text.into(),
        note: note.into(),
        problem: problem.into(),
        enabled: enabled.into(),
        typed: Input::new(typed),
        submit,
    }
    .into()
}

/// A page-wide button: `prominent` the accent filled, glass otherwise.
pub fn wide_button<M>(title: impl Into<Val<String>>, prominent: bool, enabled: impl Into<Val<bool>>, busy: impl Into<Val<bool>>, tap: M) -> View<M> {
    Part::WideButton {
        title: title.into(),
        prominent,
        enabled: enabled.into(),
        busy: busy.into(),
        tap,
    }
    .into()
}

/// What a page waits on and what went wrong.
pub fn status_line<M>(text: impl Into<Val<Option<String>>>, busy: impl Into<Val<bool>>, problem: impl Into<Val<Option<String>>>) -> View<M> {
    Part::StatusLine {
        text: text.into(),
        busy: busy.into(),
        problem: problem.into(),
    }
    .into()
}

/// A QR code of `size` modules a side.
pub fn code_image<M>(size: impl Into<Val<i64>>, dark: impl Into<Val<Vec<bool>>>) -> View<M> {
    Part::CodeImage {
        size: size.into(),
        dark: dark.into(),
    }
    .into()
}

/// A photo by its URL, fitted to its width ([`Part::Photo`]).
pub fn photo<M>(url: impl Into<Val<String>>, label: impl Into<Val<String>>) -> View<M> {
    Part::Photo {
        url: url.into(),
        label: label.into(),
        zoom: false,
    }
    .into()
}

/// A photo by its URL filling the room it is given, zoomed by a pinch on
/// the phones ([`Part::Photo`]).
pub fn zoom_photo<M>(url: impl Into<Val<String>>, label: impl Into<Val<String>>) -> View<M> {
    Part::Photo {
        url: url.into(),
        label: label.into(),
        zoom: true,
    }
    .into()
}

/// A slip of paper: a receipt as it was printed ([`Part::Paper`]).
pub fn paper<M>(
    heading: impl Into<Val<String>>,
    date: impl Into<Val<String>>,
    names: impl Into<Val<Vec<String>>>,
    figures: impl Into<Val<Vec<String>>>,
    notes: impl Into<Val<Vec<String>>>,
    total: impl Into<Val<String>>,
) -> View<M> {
    Part::Paper {
        heading: heading.into(),
        date: date.into(),
        names: names.into(),
        figures: figures.into(),
        notes: notes.into(),
        total: total.into(),
    }
    .into()
}

// * Each part's call, by rule

/// What a part is handed, as the phones' code spells it.
pub enum Given<'a, M> {
    /// A value read, or a literal.
    Val(String),
    /// Fixed words.
    Text(&'static str),
    /// A message sent when it is used.
    Act(&'a M),
    /// A message made of what was typed.
    Typed(&'a Input<String, M>),
    /// A message made of where a switch was put.
    Switched(&'a Input<bool, M>),
    /// A message made of an amount entered, in cents.
    Counted(&'a Input<i64, M>),
    /// Several of another part (a row's menu).
    Items(Vec<Call<'a, M>>),
    /// One case of a parts' enum: `.primary` in Swift, `BarPlacement.Primary`
    /// in Kotlin.
    Case { of: &'static str, case: &'static str },
}

/// A part as a call: its name (the same in Swift and Kotlin, the Rust
/// builder's in Pascal case), its first input unlabelled when it has one,
/// its labelled inputs, and the views it holds in its slots. This is the
/// whole of what the emitters know of a part: the part itself is written
/// by hand in ui/swift and ui/kotlin under this name and signature.
pub struct Call<'a, M> {
    pub name: &'static str,
    pub first: Option<Given<'a, M>>,
    pub args: Vec<(&'static str, Given<'a, M>)>,
    /// The content slot first, then any named slots.
    pub slots: Vec<(&'static str, &'a View<M>)>,
}

/// A value as the phones read it: its path, or the literal (`None` is `nil`/`null`, each emitter's).
fn lit<T: Debug>(v: &Val<T>) -> String {
    match v {
        Val::At(p) => p.clone(),
        Val::Is(t) => format!("{t:?}"),
    }
}

/// A menu's items as the phones' `MenuAction`s.
fn menu_items<M>(menu: &[MenuItem<M>]) -> Given<'_, M> {
    Given::Items(
        menu.iter()
            .map(|m| {
                let mut args = vec![("glyph", Given::Val(lit(&m.glyph))), ("destructive", Given::Val(lit(&m.destructive)))];
                if let Some(s) = &m.shown {
                    args.push(("shown", Given::Val(lit(s))));
                }
                args.push(("onTap", Given::Act(&m.tap)));
                Call {
                    name: "MenuAction",
                    first: Some(Given::Val(lit(&m.title))),
                    args,
                    slots: vec![],
                }
            })
            .collect(),
    )
}

impl<M> Part<M> {
    /// The part as a call (see [`Call`]).
    pub fn call(&self) -> Call<'_, M> {
        use Given as G;
        let c = |name, first, args, slots| Call { name, first, args, slots };
        match self {
            Part::Page {
                title,
                refresh,
                top,
                content,
            } => {
                let mut args = vec![];
                if let Some(r) = refresh {
                    args.push(("onRefresh", G::Act(r)));
                }
                c("TabPage", Some(G::Val(lit(title))), args, vec![("", &**content), ("top", &**top)])
            }
            Part::Summary {
                label,
                cents,
                first,
                first_cents,
                second,
                second_cents,
                trend,
            } => c(
                "SummaryCard",
                Some(G::Text(label)),
                vec![
                    ("cents", G::Val(lit(cents))),
                    ("first", G::Text(first)),
                    ("firstCents", G::Val(lit(first_cents))),
                    ("second", G::Text(second)),
                    ("secondCents", G::Val(lit(second_cents))),
                    ("trend", G::Val(lit(trend))),
                ],
                vec![],
            ),
            Part::Hero { label, cents, note } => c(
                "HeroFigure",
                Some(G::Text(label)),
                vec![("cents", G::Val(lit(cents))), ("note", G::Text(note))],
                vec![],
            ),
            Part::HeaderCard(h) => {
                let mut args = vec![("figure", G::Val(lit(&h.figure))), ("tint", G::Val(lit(&h.tint)))];
                if let Some(g) = &h.glyph {
                    args.push(("glyph", G::Val(lit(g))));
                }
                if h.badge {
                    args.push(("badge", G::Val("true".into())));
                }
                if let Some(l) = &h.line {
                    args.push(("line", G::Val(lit(l))));
                }
                if let Some(c) = &h.caption {
                    args.push(("caption", G::Val(lit(c))));
                }
                c("HeaderCard", Some(G::Val(lit(&h.title))), args, vec![("", &*h.actions)])
            }
            Part::Detail { content } => c("DetailPage", None, vec![], vec![("", &**content)]),
            Part::GoalProgress { fraction, tint, note } => c(
                "GoalProgress",
                Some(G::Val(lit(fraction))),
                vec![("tint", G::Val(lit(tint))), ("note", G::Val(lit(note)))],
                vec![],
            ),
            Part::Section(s) => {
                let mut args = vec![];
                if let Some(t) = &s.title {
                    args.push(("title", G::Val(lit(t))));
                }
                if let Some(a) = &s.amount {
                    args.push(("total", G::Val(lit(a))));
                }
                if let Some(b) = &s.shows {
                    args.push(("showsTotal", G::Val(lit(b))));
                }
                if let Some(f) = &s.footer {
                    args.push(("footer", G::Val(lit(f))));
                }
                if let Some(n) = &s.note {
                    args.push(("totalNote", G::Val(lit(n))));
                }
                if let Some(k) = &s.columns {
                    args.push(("firstTitle", G::Text(k.titles[0])));
                    args.push(("secondTitle", G::Text(k.titles[1])));
                    args.push(("totalTitle", G::Text(k.titles[2])));
                    args.push(("first", G::Val(lit(&k.first))));
                    args.push(("second", G::Val(lit(&k.second))));
                }
                if let Some((open, toggle)) = &s.open {
                    args.push(("open", G::Val(lit(open))));
                    args.push(("onToggle", G::Act(toggle)));
                }
                if !s.menu.is_empty() {
                    args.push(("menu", menu_items(&s.menu)));
                }
                c("CardSection", None, args, vec![("", &*s.rows)])
            }
            Part::MeterRow(r) => {
                let mut args = vec![("status", G::Val(lit(&r.status))), ("statusTint", G::Val(lit(&r.status_tint)))];
                if let Some(m) = &r.mark {
                    args.push(("mark", G::Val(lit(m))));
                }
                if let Some(f) = &r.faded {
                    args.push(("faded", G::Val(lit(f))));
                }
                if let (Some(f), Some(t)) = (&r.fraction, &r.meter_tint) {
                    args.push(("fraction", G::Val(lit(f))));
                    args.push(("meterTint", G::Val(lit(t))));
                }
                if let (Some(n), Some(t)) = (&r.note, &r.note_tint) {
                    args.push(("note", G::Val(lit(n))));
                    args.push(("noteTint", G::Val(lit(t))));
                }
                if let Some(e) = &r.note_end {
                    args.push(("noteEnd", G::Val(lit(e))));
                }
                if let Some((a, b)) = &r.columns {
                    args.push(("first", G::Val(lit(a))));
                    args.push(("second", G::Val(lit(b))));
                }
                if let Some(s) = &r.said {
                    args.push(("said", G::Val(lit(s))));
                }
                if !r.menu.is_empty() {
                    args.push(("menu", menu_items(&r.menu)));
                }
                if let Some(t) = &r.tap {
                    args.push(("onTap", G::Act(t)));
                }
                c("MeterRow", Some(G::Val(lit(&r.title))), args, vec![])
            }
            Part::Headline(h) => {
                let mut args = vec![
                    ("figure", G::Val(lit(&h.figure))),
                    ("tint", G::Val(lit(&h.tint))),
                    ("caption", G::Val(lit(&h.caption))),
                ];
                if let Some((title, glyph, tap)) = &h.action {
                    args.push(("actionTitle", G::Text(title)));
                    args.push(("actionGlyph", G::Text(glyph)));
                    args.push(("onAction", G::Act(tap)));
                }
                c("Headline", Some(G::Val(lit(&h.label))), args, vec![])
            }
            Part::Row(r) => {
                let mut args = vec![];
                if let Some(s) = &r.subtitle {
                    args.push(("subtitle", G::Val(lit(s))));
                }
                match &r.picture {
                    Picture::None => {}
                    Picture::Tile(g) => args.push(("tile", G::Text(g))),
                    Picture::Mark { kind, .. } => args.push(("mark", G::Val(lit(kind)))),
                    Picture::Glyph(g) => args.push(("glyph", G::Text(g))),
                }
                if let Some(b) = &r.badge {
                    args.push(("badge", G::Val(lit(b))));
                }
                if let Some(a) = &r.amount {
                    args.push(("amount", G::Val(lit(a))));
                }
                if let Some(v) = &r.value {
                    args.push(("value", G::Val(lit(v))));
                }
                if r.quiet {
                    args.push(("quiet", G::Val("true".into())));
                }
                if r.accent {
                    args.push(("accent", G::Val("true".into())));
                }
                if r.pushes {
                    args.push(("pushes", G::Val("true".into())));
                }
                if let Some(s) = &r.said {
                    args.push(("said", G::Val(lit(s))));
                }
                if !r.menu.is_empty() {
                    args.push(("menu", menu_items(&r.menu)));
                }
                if let Some(t) = &r.tap {
                    args.push(("onTap", G::Act(t)));
                }
                c("ListRow", Some(G::Val(lit(&r.title))), args, vec![])
            }
            Part::Sheet {
                title,
                dismiss,
                confirm,
                content,
            } => {
                let mut args = vec![("onDismiss", G::Act(dismiss))];
                if let Some(k) = confirm {
                    args.push(("confirm", G::Text(k.title)));
                    args.push(("canConfirm", G::Val(lit(&k.enabled))));
                    args.push(("onConfirm", G::Act(&k.tap)));
                }
                c("FormSheet", Some(G::Text(title)), args, vec![("", &**content)])
            }
            Part::FormSection { header, footer, content } => {
                let mut args = vec![];
                if let Some(h) = header {
                    args.push(("header", G::Val(lit(h))));
                }
                if let Some(f) = footer {
                    args.push(("footer", G::Val(lit(f))));
                }
                c("FormSection", None, args, vec![("", &**content)])
            }
            Part::Picker {
                label,
                selected,
                pick,
                choices,
            } => c(
                "OptionPicker",
                Some(G::Text(label)),
                vec![("selection", G::Val(lit(selected))), ("onChange", G::Typed(pick))],
                vec![("", &**choices)],
            ),
            Part::Choice { title, glyph, tag } => c(
                "PickerChoice",
                Some(G::Val(lit(title))),
                vec![("glyph", G::Val(lit(glyph))), ("tag", G::Val(lit(tag)))],
                vec![],
            ),
            Part::AmountEntry {
                label,
                prompt,
                typed,
                cents,
                problem,
                on_typed,
                on_cents,
            } => c(
                "AmountEntry",
                Some(G::Val(lit(label))),
                vec![
                    ("prompt", G::Val(lit(prompt))),
                    ("text", G::Val(lit(typed))),
                    ("cents", G::Val(lit(cents))),
                    ("problem", G::Val(lit(problem))),
                    ("onType", G::Typed(on_typed)),
                    ("onCents", G::Counted(on_cents)),
                ],
                vec![],
            ),
            Part::Prompt {
                title,
                text,
                typed,
                confirm,
                ok,
                cancel,
            } => c(
                "Prompt",
                Some(G::Text(title)),
                vec![
                    ("text", G::Val(lit(text))),
                    ("onChange", G::Typed(typed)),
                    ("confirm", G::Text(confirm)),
                    ("onConfirm", G::Act(ok)),
                    ("onCancel", G::Act(cancel)),
                ],
                vec![],
            ),
            Part::NameField {
                label,
                value,
                problem,
                typed,
            } => c(
                "NameField",
                Some(G::Text(label)),
                vec![
                    ("text", G::Val(lit(value))),
                    ("problem", G::Val(lit(problem))),
                    ("onChange", G::Typed(typed)),
                ],
                vec![],
            ),
            Part::AmountField { label, typed, problem, on } => c(
                "AmountField",
                Some(G::Val(lit(label))),
                vec![
                    ("text", G::Val(lit(typed))),
                    ("problem", G::Val(lit(problem))),
                    ("onChange", G::Typed(on)),
                ],
                vec![],
            ),
            Part::Chips(content) => c("ChipRow", None, vec![], vec![("", &**content)]),
            Part::Chip { label, glyph, selected, tap } => c(
                "Chip",
                Some(G::Val(lit(label))),
                vec![("glyph", G::Val(lit(glyph))), ("selected", G::Val(lit(selected))), ("onTap", G::Act(tap))],
                vec![],
            ),
            Part::Toggle { title, note, on, change } => {
                let mut args = vec![];
                if let Some(n) = note {
                    args.push(("note", G::Val(lit(n))));
                }
                args.push(("isOn", G::Val(lit(on))));
                args.push(("onChange", G::Switched(change)));
                c("SwitchRow", Some(G::Text(title)), args, vec![])
            }
            Part::FirstRunPage {
                glyph,
                title,
                sentence,
                content,
            } => c(
                "FirstRunPage",
                Some(G::Val(lit(glyph))),
                vec![("title", G::Val(lit(title))), ("sentence", G::Val(lit(sentence)))],
                vec![("", &**content)],
            ),
            Part::AddressField {
                label,
                prompt,
                text,
                note,
                problem,
                enabled,
                typed,
                submit,
            } => c(
                "AddressField",
                Some(G::Text(label)),
                vec![
                    ("prompt", G::Val(lit(prompt))),
                    ("text", G::Val(lit(text))),
                    ("note", G::Val(lit(note))),
                    ("problem", G::Val(lit(problem))),
                    ("enabled", G::Val(lit(enabled))),
                    ("onChange", G::Typed(typed)),
                    ("onSubmit", G::Act(submit)),
                ],
                vec![],
            ),
            Part::WideButton {
                title,
                prominent,
                enabled,
                busy,
                tap,
            } => c(
                "WideButton",
                Some(G::Val(lit(title))),
                vec![
                    ("prominent", G::Val(prominent.to_string())),
                    ("enabled", G::Val(lit(enabled))),
                    ("busy", G::Val(lit(busy))),
                    ("onTap", G::Act(tap)),
                ],
                vec![],
            ),
            Part::StatusLine { text, busy, problem } => c(
                "StatusLine",
                Some(G::Val(lit(text))),
                vec![("busy", G::Val(lit(busy))), ("problem", G::Val(lit(problem)))],
                vec![],
            ),
            Part::CodeImage { size, dark } => c("CodeImage", Some(G::Val(lit(size))), vec![("dark", G::Val(lit(dark)))], vec![]),
            Part::Photo { url, label, zoom } => c(
                "Photo",
                Some(G::Val(lit(url))),
                vec![("label", G::Val(lit(label))), ("zoom", G::Val(zoom.to_string()))],
                vec![],
            ),
            Part::Paper {
                heading,
                date,
                names,
                figures,
                notes,
                total,
            } => c(
                "Paper",
                Some(G::Val(lit(heading))),
                vec![
                    ("date", G::Val(lit(date))),
                    ("names", G::Val(lit(names))),
                    ("figures", G::Val(lit(figures))),
                    ("notes", G::Val(lit(notes))),
                    ("total", G::Val(lit(total))),
                ],
                vec![],
            ),
            Part::BarChart { labels, values, said } => c(
                "BarChart",
                Some(G::Val(lit(labels))),
                vec![("values", G::Val(lit(values))), ("said", G::Val(lit(said)))],
                vec![],
            ),
            Part::DateField { label, day, typed } => c(
                "DateField",
                Some(G::Text(label)),
                vec![("day", G::Val(lit(day))), ("onChange", G::Typed(typed))],
                vec![],
            ),
            Part::Table { columns, rows } => c(
                "DataTable",
                None,
                vec![(
                    "columns",
                    G::Items(
                        columns
                            .iter()
                            .map(|k| {
                                let share = match k.width {
                                    ColumnWidth::Share(n) => n,
                                    _ => 0,
                                };
                                Call {
                                    name: "DataColumn",
                                    first: Some(G::Text(k.heading)),
                                    args: vec![
                                        (
                                            "width",
                                            G::Case {
                                                of: "ColumnWidth",
                                                case: k.width.case(),
                                            },
                                        ),
                                        ("share", G::Val(share.to_string())),
                                        (
                                            "align",
                                            G::Case {
                                                of: "ColumnAlign",
                                                case: k.align.case(),
                                            },
                                        ),
                                        (
                                            "slot",
                                            G::Case {
                                                of: "ColumnSlot",
                                                case: k.slot.case(),
                                            },
                                        ),
                                    ],
                                    slots: vec![],
                                }
                            })
                            .collect(),
                    ),
                )],
                vec![("", &**rows)],
            ),
            Part::TableRow(r) => {
                let mut args = vec![];
                if let Some(t) = &r.tints {
                    args.push(("tints", G::Val(lit(t))));
                }
                if let Some(g) = &r.glyphs {
                    args.push(("glyphs", G::Val(lit(g))));
                }
                if let Some(m) = &r.marked {
                    args.push(("marked", G::Val(lit(m))));
                }
                if let Some(s) = &r.selected {
                    args.push(("selected", G::Val(lit(s))));
                }
                if let Some(s) = &r.said {
                    args.push(("said", G::Val(lit(s))));
                }
                if !r.menu.is_empty() {
                    args.push(("menu", menu_items(&r.menu)));
                }
                if let Some(t) = &r.tap {
                    args.push(("onTap", G::Act(t)));
                }
                c("DataTableRow", Some(G::Val(lit(&r.cells))), args, vec![])
            }
            Part::TableHeading { title } => c("DataTableHeading", Some(G::Val(lit(title))), vec![], vec![]),
            Part::Button { title, enabled, tap } => c(
                "FormButton",
                Some(G::Text(title)),
                vec![("enabled", G::Val(lit(enabled))), ("onTap", G::Act(tap))],
                vec![],
            ),
            Part::Tabs {
                selected,
                select,
                tabs,
                footer,
            } => c(
                "AdaptiveTabs",
                None,
                vec![("selection", G::Val(lit(selected))), ("onSelect", G::Typed(select))],
                vec![("", &**tabs), ("footer", &**footer)],
            ),
            Part::SectionTab {
                title,
                glyph,
                tag,
                badge,
                content,
            } => {
                let mut args = vec![("glyph", G::Val(lit(glyph))), ("tag", G::Val(lit(tag)))];
                if let Some(b) = badge {
                    args.push(("badge", G::Val(lit(b))));
                }
                c("SectionTab", Some(G::Val(lit(title))), args, vec![("", &**content)])
            }
            Part::TabGroup { title, tabs, .. } => c("SectionGroup", Some(G::Text(title)), vec![], vec![("", &**tabs)]),
            Part::Push {
                content,
                detail,
                showing,
                back,
            } => c(
                "PushedDetail",
                None,
                vec![("showing", G::Val(lit(showing))), ("onBack", G::Act(back))],
                vec![("", &**content), ("detail", &**detail)],
            ),
            Part::Split {
                content,
                detail,
                showing,
                back,
            } => c(
                "SplitColumns",
                None,
                vec![("showing", G::Val(lit(showing))), ("onBack", G::Act(back))],
                vec![("", &**content), ("detail", &**detail)],
            ),
            Part::Toolbar { title, items, content } => c(
                "WithToolbar",
                Some(G::Val(lit(title))),
                vec![],
                vec![("", &**content), ("items", &**items)],
            ),
            Part::BarItem {
                title,
                glyph,
                placement,
                enabled,
                tap,
            } => c(
                "BarButton",
                Some(G::Text(title)),
                vec![
                    ("glyph", G::Text(glyph)),
                    (
                        "placement",
                        G::Case {
                            of: "BarPlacement",
                            case: placement.case(),
                        },
                    ),
                    ("enabled", G::Val(lit(enabled))),
                    ("onTap", G::Act(tap)),
                ],
                vec![],
            ),
            Part::Searchable {
                prompt,
                text,
                typed,
                content,
            } => c(
                "WithSearch",
                Some(G::Text(prompt)),
                vec![("text", G::Val(lit(text))), ("onChange", G::Typed(typed))],
                vec![("", &**content)],
            ),
            Part::Inspector {
                title,
                shown,
                toggle,
                content,
                inspector,
            } => c(
                "WithInspector",
                Some(G::Text(title)),
                vec![("isShown", G::Val(lit(shown))), ("onToggle", G::Switched(toggle))],
                vec![("", &**content), ("inspector", &**inspector)],
            ),
            Part::Hosted { name } => c("HostedScreen", Some(G::Text(name)), vec![], vec![]),
            Part::Writing { markdown } => c("Writing", Some(G::Val(lit(markdown))), vec![], vec![]),
            Part::Confirm {
                title,
                message,
                actions,
                cancel,
            } => {
                let items = actions
                    .iter()
                    .map(|a| {
                        c(
                            "DialogAction",
                            Some(G::Val(lit(&a.title))),
                            vec![("destructive", G::Val(a.destructive.to_string())), ("onTap", G::Act(&a.tap))],
                            vec![],
                        )
                    })
                    .collect();
                c(
                    "ConfirmDialog",
                    Some(G::Val(lit(title))),
                    vec![
                        ("message", G::Val(lit(message))),
                        ("actions", G::Items(items)),
                        ("onCancel", G::Act(cancel)),
                    ],
                    vec![],
                )
            }
            Part::Alert { title, message, ok } => c(
                "NoticeAlert",
                Some(G::Val(lit(title))),
                vec![("message", G::Val(lit(message))), ("onOk", G::Act(ok))],
                vec![],
            ),
            Part::SettingsPanes { selected, select, panes } => c(
                "SettingsPanes",
                None,
                vec![("selection", G::Val(lit(selected))), ("onSelect", G::Typed(select))],
                vec![("", &**panes)],
            ),
            Part::SettingsPane { title, glyph, content } => c(
                "SettingsPane",
                Some(G::Text(title)),
                vec![("glyph", G::Text(glyph))],
                vec![("", &**content)],
            ),
        }
    }
}

// * Mapping a view's messages

impl<M: 'static> View<M> {
    /// The same view sending `f` of each of its messages: a screen hosted
    /// in another's messages (the iced window draws the shell's view in
    /// its own).
    pub fn map<N: 'static>(self, f: impl Fn(M) -> N + 'static) -> View<N> {
        let f: Rc<dyn Fn(M) -> N> = Rc::new(f);
        self.map_rc(&f)
    }

    fn map_rc<N: 'static>(self, f: &Rc<dyn Fn(M) -> N>) -> View<N> {
        let b = |v: Box<View<M>>| Box::new(v.map_rc(f));
        match self {
            View::Empty => View::Empty,
            View::List(vs) => View::List(vs.into_iter().map(|v| v.map_rc(f)).collect()),
            View::Each { list, item, body } => View::Each { list, item, body: b(body) },
            View::When { cond, yes, no } => View::When {
                cond,
                yes: b(yes),
                no: b(no),
            },
            View::WhenSome { opt, item, yes, no } => View::WhenSome {
                opt,
                item,
                yes: b(yes),
                no: b(no),
            },
            View::Part(p) => View::Part(p.map_rc(f)),
        }
    }
}

fn input<T: 'static, M: 'static, N: 'static>(i: Input<T, M>, f: &Rc<dyn Fn(M) -> N>) -> Input<T, N> {
    let f = f.clone();
    Input::new(move |t| f(i.call(t)))
}

impl<M: 'static> Part<M> {
    fn map_rc<N: 'static>(self, f: &Rc<dyn Fn(M) -> N>) -> Part<N> {
        let b = |v: Box<View<M>>| Box::new(v.map_rc(f));
        match self {
            Part::Page {
                title,
                refresh,
                top,
                content,
            } => Part::Page {
                title,
                refresh: refresh.map(|m| f(m)),
                top: b(top),
                content: b(content),
            },
            Part::Hero { label, cents, note } => Part::Hero { label, cents, note },
            Part::Summary {
                label,
                cents,
                first,
                first_cents,
                second,
                second_cents,
                trend,
            } => Part::Summary {
                label,
                cents,
                first,
                first_cents,
                second,
                second_cents,
                trend,
            },
            Part::HeaderCard(h) => {
                let h = *h;
                Part::HeaderCard(Box::new(HeaderCard {
                    title: h.title,
                    figure: h.figure,
                    tint: h.tint,
                    glyph: h.glyph,
                    badge: h.badge,
                    line: h.line,
                    caption: h.caption,
                    actions: b(h.actions),
                }))
            }
            Part::GoalProgress { fraction, tint, note } => Part::GoalProgress { fraction, tint, note },
            Part::Detail { content } => Part::Detail { content: b(content) },
            Part::Section(s) => Part::Section(Section {
                title: s.title,
                amount: s.amount,
                shows: s.shows,
                footer: s.footer,
                note: s.note,
                columns: s.columns,
                open: s.open.map(|(o, t)| (o, f(t))),
                menu: map_menu(s.menu, f),
                rows: b(s.rows),
            }),
            Part::MeterRow(r) => {
                let r = *r;
                Part::MeterRow(Box::new(MeterRow {
                    title: r.title,
                    mark: r.mark,
                    faded: r.faded,
                    fraction: r.fraction,
                    meter_tint: r.meter_tint,
                    note: r.note,
                    note_tint: r.note_tint,
                    note_end: r.note_end,
                    columns: r.columns,
                    status: r.status,
                    status_tint: r.status_tint,
                    said: r.said,
                    tap: r.tap.map(|m| f(m)),
                    menu: map_menu(r.menu, f),
                }))
            }
            Part::Headline(h) => {
                let h = *h;
                Part::Headline(Box::new(Headline {
                    label: h.label,
                    figure: h.figure,
                    tint: h.tint,
                    caption: h.caption,
                    action: h.action.map(|(t, g, m)| (t, g, f(m))),
                }))
            }
            Part::Row(r) => {
                let r = *r;
                Part::Row(Box::new(ListRow {
                    title: r.title,
                    subtitle: r.subtitle,
                    picture: r.picture,
                    badge: r.badge,
                    amount: r.amount,
                    value: r.value,
                    quiet: r.quiet,
                    accent: r.accent,
                    pushes: r.pushes,
                    tap: r.tap.map(|m| f(m)),
                    said: r.said,
                    menu: r
                        .menu
                        .into_iter()
                        .map(|m| MenuItem {
                            title: m.title,
                            glyph: m.glyph,
                            tap: f(m.tap),
                            destructive: m.destructive,
                            shown: m.shown,
                        })
                        .collect(),
                }))
            }
            Part::Sheet {
                title,
                dismiss,
                confirm,
                content,
            } => Part::Sheet {
                title,
                dismiss: f(dismiss),
                confirm: confirm.map(|k| Confirm {
                    title: k.title,
                    enabled: k.enabled,
                    tap: f(k.tap),
                }),
                content: b(content),
            },
            Part::FormSection { header, footer, content } => Part::FormSection {
                header,
                footer,
                content: b(content),
            },
            Part::Picker {
                label,
                selected,
                pick,
                choices,
            } => Part::Picker {
                label,
                selected,
                pick: input(pick, f),
                choices: b(choices),
            },
            Part::Choice { title, glyph, tag } => Part::Choice { title, glyph, tag },
            Part::AmountEntry {
                label,
                prompt,
                typed,
                cents,
                problem,
                on_typed,
                on_cents,
            } => Part::AmountEntry {
                label,
                prompt,
                typed,
                cents,
                problem,
                on_typed: input(on_typed, f),
                on_cents: input(on_cents, f),
            },
            Part::Prompt {
                title,
                text,
                typed,
                confirm,
                ok,
                cancel,
            } => Part::Prompt {
                title,
                text,
                typed: input(typed, f),
                confirm,
                ok: f(ok),
                cancel: f(cancel),
            },
            Part::NameField {
                label,
                value,
                problem,
                typed,
            } => Part::NameField {
                label,
                value,
                problem,
                typed: input(typed, f),
            },
            Part::AmountField { label, typed, problem, on } => Part::AmountField {
                label,
                typed,
                problem,
                on: input(on, f),
            },
            Part::Chips(c) => Part::Chips(b(c)),
            Part::Chip { label, glyph, selected, tap } => Part::Chip {
                label,
                glyph,
                selected,
                tap: f(tap),
            },
            Part::Toggle { title, note, on, change } => Part::Toggle {
                title,
                note,
                on,
                change: input(change, f),
            },
            Part::FirstRunPage {
                glyph,
                title,
                sentence,
                content,
            } => Part::FirstRunPage {
                glyph,
                title,
                sentence,
                content: b(content),
            },
            Part::AddressField {
                label,
                prompt,
                text,
                note,
                problem,
                enabled,
                typed,
                submit,
            } => Part::AddressField {
                label,
                prompt,
                text,
                note,
                problem,
                enabled,
                typed: input(typed, f),
                submit: f(submit),
            },
            Part::WideButton {
                title,
                prominent,
                enabled,
                busy,
                tap,
            } => Part::WideButton {
                title,
                prominent,
                enabled,
                busy,
                tap: f(tap),
            },
            Part::StatusLine { text, busy, problem } => Part::StatusLine { text, busy, problem },
            Part::CodeImage { size, dark } => Part::CodeImage { size, dark },
            Part::Photo { url, label, zoom } => Part::Photo { url, label, zoom },
            Part::Paper {
                heading,
                date,
                names,
                figures,
                notes,
                total,
            } => Part::Paper {
                heading,
                date,
                names,
                figures,
                notes,
                total,
            },
            Part::BarChart { labels, values, said } => Part::BarChart { labels, values, said },
            Part::DateField { label, day, typed } => Part::DateField {
                label,
                day,
                typed: input(typed, f),
            },
            Part::Table { columns, rows } => Part::Table { columns, rows: b(rows) },
            Part::TableRow(r) => {
                let r = *r;
                Part::TableRow(Box::new(TableRow {
                    cells: r.cells,
                    tints: r.tints,
                    glyphs: r.glyphs,
                    marked: r.marked,
                    selected: r.selected,
                    said: r.said,
                    tap: r.tap.map(|m| f(m)),
                    menu: map_menu(r.menu, f),
                }))
            }
            Part::TableHeading { title } => Part::TableHeading { title },
            Part::Button { title, enabled, tap } => Part::Button { title, enabled, tap: f(tap) },
            Part::Tabs {
                selected,
                select,
                tabs,
                footer,
            } => Part::Tabs {
                selected,
                select: input(select, f),
                tabs: b(tabs),
                footer: b(footer),
            },
            Part::TabGroup { title, tabs, place } => Part::TabGroup {
                title,
                tabs: b(tabs),
                place: place.map(|(g, t, v)| (g, t, b(v))),
            },
            Part::Push {
                content,
                detail,
                showing,
                back,
            } => Part::Push {
                content: b(content),
                detail: b(detail),
                showing,
                back: f(back),
            },
            Part::SectionTab {
                title,
                glyph,
                tag,
                badge,
                content,
            } => Part::SectionTab {
                title,
                glyph,
                tag,
                badge,
                content: b(content),
            },
            Part::Split {
                content,
                detail,
                showing,
                back,
            } => Part::Split {
                content: b(content),
                detail: b(detail),
                showing,
                back: f(back),
            },
            Part::Toolbar { title, items, content } => Part::Toolbar {
                title,
                items: b(items),
                content: b(content),
            },
            Part::BarItem {
                title,
                glyph,
                placement,
                enabled,
                tap,
            } => Part::BarItem {
                title,
                glyph,
                placement,
                enabled,
                tap: f(tap),
            },
            Part::Searchable {
                prompt,
                text,
                typed,
                content,
            } => Part::Searchable {
                prompt,
                text,
                typed: input(typed, f),
                content: b(content),
            },
            Part::Inspector {
                title,
                shown,
                toggle,
                content,
                inspector,
            } => Part::Inspector {
                title,
                shown,
                toggle: input(toggle, f),
                content: b(content),
                inspector: b(inspector),
            },
            Part::Hosted { name } => Part::Hosted { name },
            Part::Writing { markdown } => Part::Writing { markdown },
            Part::Confirm {
                title,
                message,
                actions,
                cancel,
            } => Part::Confirm {
                title,
                message,
                actions: actions
                    .into_iter()
                    .map(|a| DialogAction {
                        title: a.title,
                        destructive: a.destructive,
                        tap: f(a.tap),
                    })
                    .collect(),
                cancel: f(cancel),
            },
            Part::Alert { title, message, ok } => Part::Alert { title, message, ok: f(ok) },
            Part::SettingsPanes { selected, select, panes } => Part::SettingsPanes {
                selected,
                select: input(select, f),
                panes: b(panes),
            },
            Part::SettingsPane { title, glyph, content } => Part::SettingsPane {
                title,
                glyph,
                content: b(content),
            },
        }
    }
}

/// A menu's items sending `f` of their messages.
fn map_menu<M, N>(menu: Vec<MenuItem<M>>, f: &Rc<dyn Fn(M) -> N>) -> Vec<MenuItem<N>> {
    menu.into_iter()
        .map(|m| MenuItem {
            title: m.title,
            glyph: m.glyph,
            tap: f(m.tap),
            destructive: m.destructive,
            shown: m.shown,
        })
        .collect()
}
