//! The account: a picture at the right-hand end of a window's top bar of
//! who is signed in, and the menu it opens.
//!
//! **The button is only a face.** A circle: the person's picture once it has
//! arrived, their initials on their own colour until then or when they have
//! none ([`face`]), and a figure when nobody is (alone). Their name is the
//! menu's to say.
//!
//! **The menu is an ordinary [`Menu`].** It goes into the app's
//! [`crate::context::Context`] like a row's menu does, so the keyboard,
//! `<Esc>`, the backdrop and which layer has the keys are the ones every
//! other menu has — nothing here decides any of them. What this adds is what
//! is *in* it, for the states an account is in ([`Account::signed_in`],
//! [`Account::turned_away`], [`Account::alone`]), and where it goes: hung
//! off the button's bottom right corner, opening downwards (see
//! [`origin`]).
//!
//! **Its header is the menu's title**, drawn quietly across the top like
//! every menu's — "Signed in as Ada" is a fact about the menu rather than a
//! thing to run, so it is not an entry the cursor could land on.
//!
//! The app says what each entry does: the messages are its own, and nothing
//! here knows what a server, a session or a login is.
use iced::widget::{container, image, mouse_area, text};
use iced::{mouse, Border, Color, ContentFit, Element, Length, Point, Size, Theme};

use crate::menu::{self, Anchor, Entry, Menu};
use crate::{art, glyphs, icon, panel};

/// How tall the account button is — and as wide: a circle. Declared,
/// because the menu is placed before iced lays the button out: a bar
/// holding it is given this height, and the menu hangs from its bottom.
pub const HEIGHT: f32 = 28.0;

/// Who is signed in, as the app knows them: the name they gave, and the id
/// the server knows them by — either may be empty.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Who {
    pub name: String,
    pub id: String,
}

impl Who {
    /// The name, else the id.
    pub fn full(&self) -> &str {
        match self.name.is_empty() {
            true => &self.id,
            false => &self.name,
        }
    }

    /// What the face says without a picture: the first letters of the
    /// name's first and last words (`Ada Lovelace` is `AL`, `Ada` is `A`),
    /// or the id's first letter or digit. Upper case.
    pub fn initials(&self) -> String {
        let first = |w: &str| w.chars().find(|c| c.is_alphanumeric());
        let words: Vec<&str> = self.name.split_whitespace().filter(|w| first(w).is_some()).collect();
        let letters: Vec<char> = match words.as_slice() {
            [] => first(&self.id).into_iter().collect(),
            [one] => first(one).into_iter().collect(),
            [a, .., z] => first(a).into_iter().chain(first(z)).collect(),
        };
        letters.into_iter().flat_map(char::to_uppercase).collect()
    }
}

/// What the account button draws and what its menu offers.
#[derive(Debug, Clone)]
pub struct Account<M> {
    /// Who the face is: initials and the colour they are drawn on, picked
    /// by [`art::pair`] from the name, as a phone picks it. `None` alone.
    pub who: Option<Who>,
    /// Across the top of the menu.
    pub header: String,
    pub entries: Vec<Entry<M>>,
}

impl<M> Account<M> {
    /// Somebody signed in: signing out. Whether the window is talking to
    /// its server is the window's own business — it goes offline by
    /// itself when it cannot, and says so — and not a thing to ask for.
    pub fn signed_in(who: &Who, sign_out: M) -> Self {
        Account {
            who: Some(who.clone()),
            header: format!("Signed in as {}", who.full()),
            entries: vec![Entry::run(glyphs::SIGN_OUT, "Sign out", sign_out)],
        }
    }

    /// A login the server turned away: the login is still here to clear.
    pub fn turned_away(who: &Who, sign_out: M) -> Self {
        Account {
            who: Some(who.clone()),
            header: format!("{} \u{2014} signed out by the server", who.full()),
            entries: vec![Entry::run(glyphs::SIGN_OUT, "Sign out", sign_out)],
        }
    }

    /// Nobody to be: a window that is its own authority. `header` says where
    /// what is done is kept; there is nothing to sign out of, so it offers
    /// nothing.
    pub fn alone(header: impl Into<String>) -> Self {
        Account {
            who: None,
            header: header.into(),
            entries: Vec::new(),
        }
    }

    /// Its menu, hung off a button whose right-hand edge is at `right` and
    /// whose bottom is at `bottom`, in the window.
    pub fn menu(self, right: f32, bottom: f32, window: Size) -> Menu<M> {
        let mut m = Menu::open(self.header, self.entries, Point::new(right, bottom), window, Anchor::RightEdge(right));
        m.origin = origin(right, bottom, window, m.width, m.entries.len());
        m
    }
}

/// Where the account menu goes: **its right edge on the button's, its top on
/// the button's bottom** — a control's menu opens at the control.
///
/// Not [`menu::fit`]: that flips a menu upward when it has no room below,
/// which from a button at the top of the window would put it over the button
/// and off the glass. This slides instead, and only as far as staying on the
/// glass takes — `pin` clips, so a panel placed past an edge loses its rows.
pub fn origin(right: f32, bottom: f32, window: Size, width: f32, rows: usize) -> Point {
    let height = Menu::<()>::height(rows);
    let right = right.min(window.width - panel::EDGE);
    let y = bottom.min(window.height - height - panel::EDGE).max(panel::EDGE);
    Point::new(menu::right_aligned(right, width), y)
}

/// The face, `side` across: `picture` when there is one, cropped to the
/// circle; else `who`'s initials on their colour, in black or white,
/// whichever reads on it; else (nobody) a figure on the bar's own grey.
pub fn face<'a, M: 'a>(who: Option<&Who>, picture: Option<&image::Handle>, side: f32) -> Element<'a, M> {
    if let Some(p) = picture {
        return image(p.clone())
            .width(Length::Fixed(side))
            .height(Length::Fixed(side))
            .content_fit(ContentFit::Cover)
            .border_radius(side / 2.0)
            .into();
    }
    let round = Border {
        radius: (side / 2.0).into(),
        ..Border::default()
    };
    let inside: Element<'a, M> = match who {
        Some(w) => text(w.initials()).size((side * 0.4).round()).into(),
        None => icon::line(glyphs::ARTIST, false).into(),
    };
    let seed = who.map(|w| w.full().to_string());
    container(inside)
        .width(Length::Fixed(side))
        .height(Length::Fixed(side))
        .center(Length::Fixed(side))
        .style(move |theme: &Theme| {
            let fill = match &seed {
                Some(seed) => art::pair(theme, seed)[0],
                None => theme.extended_palette().background.strong.color,
            };
            container::Style {
                background: Some(fill.into()),
                text_color: Some(ink_on(fill)),
                border: round,
                ..container::Style::default()
            }
        })
        .into()
}

/// Black or white, whichever is further from `fill`'s lightness.
fn ink_on(fill: Color) -> Color {
    let lightness = 0.2126 * fill.r + 0.7152 * fill.g + 0.0722 * fill.b;
    match lightness > 0.55 {
        true => Color::BLACK,
        false => Color::WHITE,
    }
}

/// The button: the face ([`face`]), [`HEIGHT`] across. `press` opens the
/// menu; with the menu up its backdrop is over the button, so a second
/// click closes it as a click anywhere else does.
pub fn button<'a, M: Clone + 'a>(account: &Account<M>, picture: Option<&image::Handle>, press: M) -> Element<'a, M> {
    mouse_area(face(account.who.as_ref(), picture, HEIGHT))
        .on_press(press)
        .interaction(mouse::Interaction::Pointer)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{Context, Layer};
    use crate::panel::EDGE;
    use crate::vim::Motion;

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        SignOut,
    }

    fn labels(a: &Account<Msg>) -> Vec<&str> {
        a.entries.iter().map(|e| e.label.as_str()).collect()
    }

    fn ada() -> Who {
        Who {
            name: "Ada".into(),
            id: "u-1".into(),
        }
    }

    /// What the menu offers in each state, and what each entry sends —
    /// signed in or turned away, signing out and nothing else: no going
    /// offline or online, which a window does by itself.
    ///
    /// Falsified by putting a `Go offline` entry back in `signed_in`: the
    /// first assertion names it.
    #[test]
    fn the_menu_offers_what_the_account_can_do() {
        let on = Account::signed_in(&ada(), Msg::SignOut);
        assert_eq!(labels(&on), ["Sign out"]);
        assert_eq!(on.header, "Signed in as Ada");
        assert_eq!(on.who.as_ref(), Some(&ada()));
        let messages: Vec<&Msg> = on.entries.iter().map(|e| e.message()).collect();
        assert_eq!(messages, [&Msg::SignOut]);
        assert!(on.entries.iter().all(|e| !e.opens_submenu()));

        let away = Account::turned_away(&ada(), Msg::SignOut);
        assert_eq!(labels(&away), ["Sign out"]);
        assert!(away.header.contains("signed out by the server"), "{}", away.header);

        let alone: Account<Msg> = Account::alone("Alone, on this device");
        assert!(alone.entries.is_empty(), "nothing to sign out of");
        assert!(alone.who.is_none(), "nobody's face");
        assert_eq!(alone.header, "Alone, on this device");
    }

    /// The face without a picture says the name's first and last initials,
    /// else the id's first letter; the header says the name whole.
    ///
    /// Falsified by taking the second letter from the second word rather
    /// than the last: `Johann Sebastian Bach` is `JS`.
    #[test]
    fn the_face_says_who_by_initials_and_the_header_whole() {
        let who = |name: &str, id: &str| Who {
            name: name.into(),
            id: id.into(),
        };
        assert_eq!(ada().initials(), "A");
        assert_eq!(who("Johann Sebastian Bach", "u").initials(), "JB");
        assert_eq!(who("  ada   lovelace ", "u").initials(), "AL");
        assert_eq!(who("", "user-0123").initials(), "U");
        assert_eq!(who("", "").initials(), "");
        assert_eq!(who("\u{e9}mile (\u{2014})", "u").initials(), "\u{c9}");
        let long = who("Johann Sebastian Bach of Eisenach", "u");
        let a = Account::signed_in(&long, Msg::SignOut);
        assert_eq!(a.header, "Signed in as Johann Sebastian Bach of Eisenach");
    }

    /// Hung off the button: its right edge on the button's, its top on the
    /// button's bottom — on every window, and on the glass when the window is
    /// too small to hold it, where it slides rather than flipping over the
    /// button.
    ///
    /// Said once as literals: 860 − 16 = 844 is harken's button's right edge,
    /// and 844 − 198 = 646 a minimum-width menu's left. Falsified by dropping
    /// the `.min` in `origin`: on the 120px window the menu runs off the foot
    /// of the glass.
    #[test]
    fn the_menu_hangs_off_the_buttons_bottom_right() {
        let right = |w: f32| w - 16.0;
        let bottom = 16.0 + HEIGHT;
        for (w, h) in [(860.0, 600.0), (1600.0, 1000.0), (420.0, 300.0), (240.0, 120.0)] {
            let window = Size::new(w, h);
            let a = Account::signed_in(&ada(), Msg::SignOut);
            let m = a.menu(right(w), bottom, window);
            let height = Menu::<Msg>::height(m.entries.len());
            assert!(
                m.origin.x >= EDGE && m.origin.x + m.width <= w - EDGE,
                "{w}×{h}: off the side at {:?}",
                m.origin
            );
            assert!(
                m.origin.y >= EDGE && m.origin.y + height <= h - EDGE,
                "{w}×{h}: off the glass at {:?}",
                m.origin
            );
            if m.width + 2.0 * EDGE <= w {
                assert_eq!(m.origin.x + m.width, right(w), "{w}×{h}: the right edges part");
            }
            if bottom + height + EDGE <= h {
                assert_eq!(m.origin.y, bottom, "{w}×{h}: it does not open down from the button");
            }
        }
        let m = Account::signed_in(&ada(), Msg::SignOut).menu(844.0, bottom, Size::new(860.0, 600.0));
        assert_eq!((m.origin.x, m.origin.y, m.width), (646.0, 44.0, 198.0));
    }

    /// It is a menu like any other once it is up: it has the keys, `j` walks
    /// it, `<Enter>` answers the entry's message, `<Esc>` closes it — and a
    /// menu with nothing in it (alone) walks nowhere without falling over.
    ///
    /// Falsified by a `Context::chosen` that answers nothing: `<Enter>` on
    /// `Sign out` signs nobody out.
    #[test]
    fn the_menu_is_driven_like_every_other() {
        let window = Size::new(860.0, 600.0);
        let mut ctx: Context<Msg, u32> = Context::new();
        ctx.open_menu(Account::signed_in(&ada(), Msg::SignOut).menu(844.0, 44.0, window));
        assert_eq!(ctx.focus(), Some(Layer::Menu));
        assert_eq!(ctx.travel(Motion::Down(1)), None);
        assert_eq!(ctx.chosen(), Some(Msg::SignOut));
        ctx.cancel();
        assert!(!ctx.is_open());

        ctx.open_menu(Account::<Msg>::alone("Alone").menu(844.0, 44.0, window));
        assert_eq!(ctx.travel(Motion::Down(1)), None);
        assert_eq!(ctx.travel(Motion::Right(1)), None);
        assert_eq!(ctx.chosen(), None, "nothing to run");
        assert_eq!(ctx.focus(), Some(Layer::Menu));
    }
}
