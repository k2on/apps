//! How a screen is laid out, once, for every toolkit: a view is a
//! function of the screen's record that returns a [`View`] built from
//! ArkUI's parts (docs/arkui-parts.md), SwiftUI's way.
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
//! (`accounts::view(&page)`, `accounts::record()`, and under the crate's
//! features `accounts::iced(&page)` and `accounts::web(&page)`). There are
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

/// A message a field's input makes.
#[derive(Clone, Copy)]
pub struct Input<T, M>(pub fn(T) -> M);

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
        title: &'static str,
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
    /// A group of rows in a card, under its heading, a note under it.
    Section(Section<M>),
    Row(Box<ListRow<M>>),
    /// A sheet: its title, its content; put away, `dismiss`.
    Sheet {
        title: &'static str,
        dismiss: M,
        content: Box<View<M>>,
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
        note: Val<String>,
        on: Val<bool>,
        change: Input<bool, M>,
    },
    Button {
        title: &'static str,
        enabled: Val<bool>,
        tap: M,
    },
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
pub fn tab_page<M>(title: &'static str, content: impl Into<View<M>>) -> PageBuilder<M> {
    PageBuilder {
        title,
        refresh: None,
        top: View::Empty,
        content: content.into(),
    }
}

pub struct PageBuilder<M> {
    title: &'static str,
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

/// A group of rows in a card.
#[derive(Clone, Debug)]
pub struct Section<M> {
    pub title: Option<Val<String>>,
    pub amount: Option<Val<i64>>,
    /// While the amount shows; always when none.
    pub shows: Option<Val<bool>>,
    pub footer: Option<Val<Option<String>>>,
    pub rows: Box<View<M>>,
}

pub fn card_section<M>(rows: impl Into<View<M>>) -> Section<M> {
    Section {
        title: None,
        amount: None,
        shows: None,
        footer: None,
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
    pub glyph: &'static str,
    pub tap: M,
    /// Destructive while this holds.
    pub destructive: Val<bool>,
}

/// ArkUI's `ListRow`.
#[derive(Clone, Debug)]
pub struct ListRow<M> {
    pub title: Val<String>,
    pub subtitle: Option<Val<String>>,
    pub picture: Picture,
    pub badge: Option<Val<Option<i64>>>,
    pub amount: Option<Val<i64>>,
    /// The amount in the quiet colour (a total).
    pub quiet: bool,
    /// In the accent's colour: an action, not a place.
    pub accent: bool,
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
        quiet: false,
        accent: false,
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
    pub fn quiet(mut self) -> Self {
        self.quiet = true;
        self
    }
    pub fn accent(mut self) -> Self {
        self.accent = true;
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
            glyph,
            tap,
            destructive: destructive.into(),
        });
        self
    }
}

impl<M> ListRow<M> {
    /// `title` in what holding the row offers, never destructive.
    pub fn menu_fixed(mut self, title: &'static str, glyph: &'static str, tap: M) -> Self {
        self.menu.push(MenuItem {
            title: title.into(),
            glyph,
            tap,
            destructive: Val::Is(false),
        });
        self
    }
}

impl<M> From<ListRow<M>> for View<M> {
    fn from(r: ListRow<M>) -> View<M> {
        Part::Row(Box::new(r)).into()
    }
}

/// A sheet, put away with `dismiss`.
pub fn sheet<M>(title: &'static str, dismiss: M, content: impl Into<View<M>>) -> View<M> {
    Part::Sheet {
        title,
        dismiss,
        content: Box::new(content.into()),
    }
    .into()
}

/// An alert with one text field.
pub fn prompt<M>(title: &'static str, text: impl Into<Val<String>>, typed: fn(String) -> M, confirm: &'static str, ok: M, cancel: M) -> View<M> {
    Part::Prompt {
        title,
        text: text.into(),
        typed: Input(typed),
        confirm,
        ok,
        cancel,
    }
    .into()
}

/// A field for a name, the domain's sentence under it.
pub fn name_field<M>(label: &'static str, value: impl Into<Val<String>>, problem: impl Into<Val<Option<String>>>, typed: fn(String) -> M) -> View<M> {
    Part::NameField {
        label,
        value: value.into(),
        problem: problem.into(),
        typed: Input(typed),
    }
    .into()
}

/// A field for an amount.
pub fn amount_field<M>(
    label: impl Into<Val<String>>,
    typed: impl Into<Val<String>>,
    problem: impl Into<Val<Option<String>>>,
    on: fn(String) -> M,
) -> View<M> {
    Part::AmountField {
        label: label.into(),
        typed: typed.into(),
        problem: problem.into(),
        on: Input(on),
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
pub fn switch_row<M>(title: &'static str, note: impl Into<Val<String>>, on: impl Into<Val<bool>>, change: fn(bool) -> M) -> View<M> {
    Part::Toggle {
        title,
        note: note.into(),
        on: on.into(),
        change: Input(change),
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
    /// Several of another part (a row's menu).
    Items(Vec<Call<'a, M>>),
}

/// A part as a call: its name (the same in Swift and Kotlin, the Rust
/// builder's in Pascal case), its first input unlabelled when it has one,
/// its labelled inputs, and the views it holds in its slots. This is the
/// whole of what the emitters know of a part: the part itself is written
/// by hand in swift/ArkUI and kotlin/arkui under this name and signature.
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
                c("TabPage", Some(G::Text(title)), args, vec![("", &**content), ("top", &**top)])
            }
            Part::Hero { label, cents, note } => c(
                "HeroFigure",
                Some(G::Text(label)),
                vec![("cents", G::Val(lit(cents))), ("note", G::Text(note))],
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
                c("CardSection", None, args, vec![("", &*s.rows)])
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
                if r.quiet {
                    args.push(("quiet", G::Val("true".into())));
                }
                if r.accent {
                    args.push(("accent", G::Val("true".into())));
                }
                if let Some(s) = &r.said {
                    args.push(("said", G::Val(lit(s))));
                }
                if !r.menu.is_empty() {
                    let items = r
                        .menu
                        .iter()
                        .map(|m| {
                            c(
                                "MenuAction",
                                Some(G::Val(lit(&m.title))),
                                vec![
                                    ("glyph", G::Text(m.glyph)),
                                    ("destructive", G::Val(lit(&m.destructive))),
                                    ("onTap", G::Act(&m.tap)),
                                ],
                                vec![],
                            )
                        })
                        .collect();
                    args.push(("menu", G::Items(items)));
                }
                if let Some(t) = &r.tap {
                    args.push(("onTap", G::Act(t)));
                }
                c("ListRow", Some(G::Val(lit(&r.title))), args, vec![])
            }
            Part::Sheet { title, dismiss, content } => c(
                "Sheet",
                Some(G::Text(title)),
                vec![("onDismiss", G::Act(dismiss))],
                vec![("", &**content)],
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
            Part::Toggle { title, note, on, change } => c(
                "SwitchRow",
                Some(G::Text(title)),
                vec![("note", G::Val(lit(note))), ("isOn", G::Val(lit(on))), ("onChange", G::Switched(change))],
                vec![],
            ),
            Part::Button { title, enabled, tap } => c(
                "FormButton",
                Some(G::Text(title)),
                vec![("enabled", G::Val(lit(enabled))), ("onTap", G::Act(tap))],
                vec![],
            ),
        }
    }
}
