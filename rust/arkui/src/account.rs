//! The account: a button at the right-hand end of a window's top bar saying
//! who is signed in, and the menu it opens.
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
use iced::widget::{button as pressable, container, row, text};
use iced::{Alignment, Element, Length, Padding, Point, Size};

use crate::fit;
use crate::menu::{self, Anchor, Entry, Menu};
use crate::{glyphs, icon, panel, style};

/// How tall the account button is. Declared, because the menu is placed
/// before iced lays the button out: a bar holding it is given this height,
/// and the menu hangs from its bottom.
pub const HEIGHT: f32 = 28.0;

/// The most of a name the button shows. A name is a label on a button, not
/// a heading: past this it is cut, and the menu's header says it whole.
pub const BUTTON_CHARS: usize = 20;

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

    /// …short enough for the button. A name loses its end (`Johann Sebasti…`),
    /// an id its middle, whose ends are what tell two ids apart
    /// ([`fit::Cut`]).
    pub fn short(&self) -> String {
        match self.name.is_empty() {
            true => fit::middle(&self.id, BUTTON_CHARS),
            false => fit::tail(&self.name, BUTTON_CHARS),
        }
    }
}

/// Whether the window is talking to its server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Online,
    Offline,
}

/// What the account button says and what its menu offers.
#[derive(Debug, Clone)]
pub struct Account<M> {
    /// On the button.
    pub label: String,
    /// Across the top of the menu.
    pub header: String,
    pub entries: Vec<Entry<M>>,
}

impl<M> Account<M> {
    /// Somebody signed in: going offline or back online, and signing out.
    pub fn signed_in(who: &Who, link: Link, toggle: M, sign_out: M) -> Self {
        let (glyph, go) = match link {
            Link::Online => (glyphs::OFFLINE, "Go offline"),
            Link::Offline => (glyphs::ONLINE, "Go online"),
        };
        Account {
            label: who.short(),
            header: format!("Signed in as {}", who.full()),
            entries: vec![Entry::run(glyph, go, toggle), Entry::run(glyphs::SIGN_OUT, "Sign out", sign_out)],
        }
    }

    /// A login the server turned away: there is no link to go on or off, and
    /// the login is still here to clear.
    pub fn turned_away(who: &Who, sign_out: M) -> Self {
        Account {
            label: who.short(),
            header: format!("{} \u{2014} signed out by the server", who.full()),
            entries: vec![Entry::run(glyphs::SIGN_OUT, "Sign out", sign_out)],
        }
    }

    /// Nobody to be: a window that is its own authority. `header` says where
    /// what is done is kept; there is nothing to sign out of, so it offers
    /// nothing.
    pub fn alone(header: impl Into<String>) -> Self {
        Account {
            label: "Alone".into(),
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

/// The button: who, and a chevron saying it opens downwards. `press` opens
/// the menu; with the menu up its backdrop is over the button, so a second
/// click closes it as a click anywhere else does.
pub fn button<'a, M: Clone + 'a>(account: &Account<M>, press: M) -> Element<'a, M> {
    let face = row![
        icon::line(glyphs::ARTIST, false),
        text(account.label.clone()).size(13).wrapping(text::Wrapping::None),
        icon::line(glyphs::DOWN, false),
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    // A button lays its content at its top; the row is centred in the
    // button's height by a container as tall as it is.
    pressable(container(face).center_y(Length::Fill))
        .height(Length::Fixed(HEIGHT))
        .padding(Padding::from([0.0, panel::ENTRY_PAD_X]))
        .style(style::quiet)
        .on_press(press)
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
        Toggle,
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

    /// What the menu offers in each state, and what each entry sends.
    ///
    /// Falsified by swapping the two arms of `link` in `signed_in`: online
    /// offers `Go online`, and the first assertion names it.
    #[test]
    fn the_menu_offers_what_the_account_can_do() {
        let on = Account::signed_in(&ada(), Link::Online, Msg::Toggle, Msg::SignOut);
        assert_eq!(labels(&on), ["Go offline", "Sign out"]);
        assert_eq!(on.header, "Signed in as Ada");
        assert_eq!(on.label, "Ada");
        let messages: Vec<&Msg> = on.entries.iter().map(|e| e.message()).collect();
        assert_eq!(messages, [&Msg::Toggle, &Msg::SignOut]);
        assert!(on.entries.iter().all(|e| !e.opens_submenu()));

        let off = Account::signed_in(&ada(), Link::Offline, Msg::Toggle, Msg::SignOut);
        assert_eq!(labels(&off), ["Go online", "Sign out"]);

        let away = Account::turned_away(&ada(), Msg::SignOut);
        assert_eq!(labels(&away), ["Sign out"]);
        assert!(away.header.contains("signed out by the server"), "{}", away.header);

        let alone: Account<Msg> = Account::alone("Alone, on this device");
        assert!(alone.entries.is_empty(), "nothing to sign out of");
        assert_eq!(alone.header, "Alone, on this device");
    }

    /// The button says the name, else the id, cut when long — a name at its
    /// end, an id in its middle — and the header says it whole.
    ///
    /// Falsified by making `short` cut an id with `tail`: the id's last
    /// characters, which tell it from its neighbours, are gone.
    #[test]
    fn the_button_says_who_briefly_and_the_header_whole() {
        let id = Who {
            name: String::new(),
            id: "user-0123456789abcdef-xyz".into(),
        };
        assert_eq!(id.full(), "user-0123456789abcdef-xyz");
        assert_eq!(id.short().chars().count(), BUTTON_CHARS);
        assert!(id.short().ends_with("-xyz"), "{}", id.short());
        let long = Who {
            name: "Johann Sebastian Bach of Eisenach".into(),
            id: "u".into(),
        };
        assert_eq!(long.short(), "Johann Sebastian Ba\u{2026}");
        let a = Account::signed_in(&long, Link::Online, Msg::Toggle, Msg::SignOut);
        assert_eq!(a.header, "Signed in as Johann Sebastian Bach of Eisenach");
        assert_eq!(ada().short(), "Ada", "what fits is left alone");
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
            let a = Account::signed_in(&ada(), Link::Online, Msg::Toggle, Msg::SignOut);
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
        let m = Account::signed_in(&ada(), Link::Online, Msg::Toggle, Msg::SignOut).menu(844.0, bottom, Size::new(860.0, 600.0));
        assert_eq!((m.origin.x, m.origin.y, m.width), (646.0, 44.0, 198.0));
    }

    /// It is a menu like any other once it is up: it has the keys, `j` walks
    /// it, `<Enter>` answers the entry's message, `<Esc>` closes it — and a
    /// menu with nothing in it (alone) walks nowhere without falling over.
    ///
    /// Falsified by `Context::chosen` answering the first entry whatever the
    /// cursor: `<Enter>` after `j` answers `Toggle`.
    #[test]
    fn the_menu_is_driven_like_every_other() {
        let window = Size::new(860.0, 600.0);
        let mut ctx: Context<Msg, u32> = Context::new();
        ctx.open_menu(Account::signed_in(&ada(), Link::Online, Msg::Toggle, Msg::SignOut).menu(844.0, 44.0, window));
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
