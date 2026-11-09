//! What the last thing done came to, said for a moment in a small pill
//! floating at the foot of the window, and then gone: a refusal, a sync, an
//! amount that would not parse. As a Mac or a phone says one — feedback
//! that comes and goes, not a bar that is always there.
//!
//! **It takes no room.** The apps used to say this on a line under the page,
//! in the page's flow, and only while there was something to say; but
//! something was nearly always to say ("signed in as …" from the moment
//! the window opened), so the line was nearly always there, and it cut off
//! whatever the page had at its foot — an inspector's last section, a
//! list's last row — for words nobody had asked to keep reading. The pill
//! is laid over the page in the window's stack instead, so the page runs to
//! the bottom whether or not anything is being said. It hears nothing: the
//! pointer goes through it to whatever is under it.
//!
//! **Only news is said.** A window hands its note to [`Toast::heard`] after
//! every update; a note it has not heard before is shown for [`LIFE`], and
//! the same note heard again is not news, so it is not shown again. The
//! timer is [`crate::wake::at_in`] keyed by a count of notes, so a newer
//! note replaces the older one's timer rather than being hidden by it, and
//! a window at rest runs nothing. A note cleared (`<Esc>`) hides the pill
//! at once. What is only who is signed in is not news at all: the account
//! button says that, all the time, and an app does not make it a note.
//!
//! **While keys are being typed, it says them** ([`typed`]): a `/search`
//! as it is typed, and a count in front of a command. A prefix alone (`g`,
//! an app's `c`) is the which-key panel's to show ([`crate::which`]), which
//! lists what can follow it, so it is not said twice. What is typed stays
//! up as long as it is being typed, and the note comes back after only if
//! its time has not run out.
//!
//! **It appears and goes at once, without a fade.** iced has no opacity for
//! a group of widgets, and [`crate::motion`]'s fade is a veil the colour of
//! what is behind — which, under a pill over a page of rows, is not one
//! colour. So rather than a fade that would show a box of the page's
//! ground over a row, it is simply there and then not.
use std::time::Duration;

use iced::widget::{container, row, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Shadow, Subscription, Vector};

use crate::{glyphs, icon, style, theme};

/// How long a note is up: long enough to read a line of it twice.
pub const LIFE: Duration = Duration::from_secs(4);

/// How far the pill stands off the window's foot, or off whatever is there
/// already (a play bar) when a window says so.
pub const MARGIN: f32 = 16.0;

/// What is being said, and the note it was.
#[derive(Debug, Default)]
pub struct Toast {
    /// The note last heard, said or not: only a different one is news.
    heard: String,
    /// The note being said, until its time runs out.
    saying: Option<String>,
    /// Counts the notes heard, so a timer is the note's it was armed for.
    pub key: u64,
}

impl Toast {
    pub fn new() -> Toast {
        Toast::default()
    }

    /// The window's note, after an update. A new one is said from now; an
    /// empty one stops whatever was being said; the same one again does
    /// nothing, so a note that stands is said once.
    pub fn heard(&mut self, note: &str) {
        if note == self.heard {
            return;
        }
        self.heard = note.to_string();
        self.key += 1;
        self.saying = (!note.is_empty()).then(|| note.to_string());
    }

    /// The timer [`Toast::timer`] armed went off: the note it was for goes.
    /// A timer for an older note does nothing.
    pub fn due(&mut self, key: u64) {
        if key == self.key {
            self.saying = None;
        }
    }

    /// The note being said, if its time has not run out.
    pub fn saying(&self) -> Option<&str> {
        self.saying.as_deref()
    }

    /// While a note is said, the one-shot timer that ends it, yielding the
    /// key it was armed for (for [`Toast::due`]). In a space of its own, so
    /// it cannot be one of the app's own deadlines.
    pub fn timer(&self) -> Option<Subscription<u64>> {
        let key = self.key;
        self.saying
            .as_ref()
            .map(|_| crate::wake::at_in("arkui::toast", key, LIFE).with(key).map(|(key, ())| key))
    }
}

/// What half-typed keys ([`crate::vim::Keys::pending`]) the pill says: a
/// search as it is typed (`/` and what follows, even nothing yet), and keys
/// with a count in front (`5`, `5g`), which the which-key panel does not
/// show. A prefix alone is the panel's, and nothing typed is nothing.
pub fn typed(pending: &str) -> Option<&str> {
    match pending.chars().next() {
        Some('/') => Some(pending),
        Some(c) if c.is_ascii_digit() => Some(pending),
        _ => None,
    }
}

/// What the pill says now, of `toast` and the keys `pending`: what is
/// being typed, while it is, and otherwise the note.
pub fn says<'a>(toast: &'a Toast, pending: &'a str) -> Option<&'a str> {
    typed(pending).or(toast.saying())
}

/// The pill: `words` on a rounded surface with a hairline and a soft
/// shadow, at the window's foot and centred, `bottom` clear of it. A
/// search (`words` starting `/`) ends in a caret — drawn, because U+2582
/// is not in the font — so a query half typed does not look like one that
/// matched nothing.
pub fn view<'a, M: 'a>(words: &str, bottom: f32) -> Element<'a, M> {
    let search = words.starts_with('/');
    let mut line = row![text(words.to_string()).size(13)].spacing(6).align_y(Alignment::Center);
    if search {
        line = line.push(icon::tinted(glyphs::STOP, 9.0, |p| p.primary.base.color));
    }
    let pill = container(line).padding([7, 16]).max_width(560.0).style(|t| {
        let p = theme::of(t);
        iced::widget::container::Style {
            background: Some(Background::Color(style::surface_color(t))),
            text_color: Some(p.background.base.text),
            border: Border {
                radius: 16.0.into(),
                width: 1.0,
                color: style::edge_color(t),
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
                offset: Vector::new(0.0, 2.0),
                blur_radius: 12.0,
            },
            ..iced::widget::container::Style::default()
        }
    });
    container(pill)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding {
            top: MARGIN,
            right: MARGIN,
            bottom,
            left: MARGIN,
        })
        .align_x(Alignment::Center)
        .align_y(Alignment::End)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new note is said until its timer, the same note again is not said
    /// again, and a newer note outlives the older one's timer. Falsified by
    /// hiding on any timer: the newer note went with the older one's.
    #[test]
    fn a_note_is_said_once_for_a_while() {
        let mut t = Toast::new();
        t.heard("");
        assert_eq!(t.saying(), None, "nothing to say");
        assert!(t.timer().is_none(), "and no timer for it");
        t.heard("reconciled at $5.00");
        assert_eq!(t.saying(), Some("reconciled at $5.00"));
        assert!(t.timer().is_some());
        let first = t.key;
        t.heard("\"x\" is not an amount");
        t.due(first);
        assert_eq!(t.saying(), Some("\"x\" is not an amount"), "an older timer hides nothing");
        t.due(t.key);
        assert_eq!(t.saying(), None, "its own does");
        t.heard("\"x\" is not an amount");
        assert_eq!(t.saying(), None, "the same note again is not news");
        t.heard("something else");
        t.heard("");
        assert_eq!(t.saying(), None, "a note cleared stops it at once");
    }

    /// A search and a count are said while typed, over a note; a prefix
    /// alone is the which-key panel's. Falsified by saying every pending
    /// key: `g` was in the pill and the panel both.
    #[test]
    fn what_is_typed_is_said_over_the_note() {
        let mut t = Toast::new();
        t.heard("synced");
        assert_eq!(says(&t, "/jo"), Some("/jo"));
        assert_eq!(says(&t, "/"), Some("/"), "a search with nothing in it yet");
        assert_eq!(says(&t, "5g"), Some("5g"));
        assert_eq!(says(&t, "g"), Some("synced"), "a prefix: the note, and the panel");
        assert_eq!(says(&t, ""), Some("synced"));
        t.due(t.key);
        assert_eq!(says(&t, "c"), None);
    }
}
