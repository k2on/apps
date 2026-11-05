//! What the keys can do next, in a small panel, as neovim's which-key
//! plugin shows it: a prefix typed (`g`, `z`, an app's `c`) and not
//! finished, and after a moment ([`DELAY`]) a panel lists what can follow
//! it and what each does. It goes once the sequence is finished or given
//! up (`<Esc>`). `?` opens it for the keys from the top, and closes it.
//!
//! What it lists is read from the bindings ([`crate::vim::GRAMMAR`] and an
//! app's own table beside the code that dispatches them), the one place
//! they are written down, so the panel cannot drift from what the keys do.
//! A window keeps a [`Which`] and tells it what the grammar has pending
//! after each key; it arms the timer [`Which::waiting`] asks for and draws
//! [`view`] while [`Which::showing`] says so.
use std::time::Duration;

use iced::widget::{column, container, row, text};
use iced::{Alignment, Background, Border, Element, Length, Padding};

use crate::vim::{Binding, Group};
use crate::{hero, style, theme};

/// How long a prefix waits before the panel opens: long enough that a
/// sequence typed without stopping never shows it.
pub const DELAY: Duration = Duration::from_millis(400);

/// Whether the panel is up, and for what.
#[derive(Debug, Default)]
pub struct Which {
    /// What the grammar has pending: a prefix, or nothing.
    prefix: String,
    /// Counts the prefixes seen, so a timer is the prefix's it was armed
    /// for ([`crate::wake::at`] keyed by it).
    pub key: u64,
    /// The delay has gone by for this prefix.
    due: bool,
    /// Opened by `?`, for the keys from the top.
    asked: bool,
}

impl Which {
    pub fn new() -> Which {
        Which::default()
    }

    /// What the grammar has pending after a key ([`crate::vim::Keys::pending`]).
    /// A count in front of it is not part of the prefix: `5g` waits as `g`.
    pub fn typed(&mut self, pending: &str) {
        let prefix = pending.trim_start_matches(|c: char| c.is_ascii_digit()).to_string();
        if prefix != self.prefix {
            self.prefix = prefix;
            self.key += 1;
            self.due = false;
        }
        if !self.prefix.is_empty() {
            self.asked = false;
        }
    }

    /// `?`: the panel for the keys from the top, or closed if it was up.
    pub fn ask(&mut self) {
        self.asked = !self.asked;
    }

    /// The timer [`Which::waiting`] asked for went off.
    pub fn due(&mut self, key: u64) {
        if key == self.key {
            self.due = true;
        }
    }

    /// Closed, whatever it was showing: `<Esc>`.
    pub fn dismiss(&mut self) {
        self.asked = false;
        self.prefix.clear();
        self.due = false;
    }

    /// The timer to arm, keyed by [`Which::key`]: while a prefix waits for
    /// the panel.
    pub fn waiting(&self) -> Option<u64> {
        (!self.prefix.is_empty() && !self.due).then_some(self.key)
    }

    /// The prefix the panel is up for, `""` for the keys from the top;
    /// `None` while it is not up.
    pub fn showing(&self) -> Option<&str> {
        match (self.asked, self.due && !self.prefix.is_empty()) {
            (_, true) => Some(&self.prefix),
            (true, false) => Some(""),
            _ => None,
        }
    }
}

/// What can follow `prefix`, from `tables` in order, as the panel lists
/// it: the keys after the prefix and what they do. From the top, a key
/// that starts longer sequences is listed once, as its group (`g  +go
/// to`), and `groups` names it; a prefix with no group is named for what
/// its first sequence does.
pub fn entries(tables: &[&[Binding]], groups: &[&[Group]], prefix: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    for b in tables.iter().flat_map(|t| t.iter()) {
        let Some(rest) = b.keys.strip_prefix(prefix) else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        // A named key (`<Tab>`) is one key; otherwise the first character.
        let first_len = match rest.starts_with('<') {
            true => rest.len(),
            false => rest.chars().next().map_or(0, char::len_utf8),
        };
        let (first, more) = rest.split_at(first_len);
        if more.is_empty() {
            if !out.iter().any(|(k, _)| k == first) {
                out.push((first.to_string(), b.what.to_string()));
            }
            continue;
        }
        let whole = format!("{prefix}{first}");
        if out.iter().any(|(k, _)| k == first) {
            continue;
        }
        let name = groups.iter().flat_map(|g| g.iter()).find(|g| g.key == whole).map_or(b.what, |g| g.what);
        out.push((first.to_string(), format!("+{name}")));
    }
    out
}

/// How wide the panel's column of keys is.
const KEYS_WIDTH: f32 = 56.0;

/// The panel: `entries` after `prefix`, in a rounded surface at the
/// window's bottom right, `bottom` clear of the foot, or of a play bar there.
/// Small text, the keys in bold, what they do quieter beside them, two
/// columns when there are many.
pub fn view<'a, M: 'a>(prefix: &str, entries: &[(String, String)], bottom: f32) -> Element<'a, M> {
    let line = |(keys, what): &(String, String)| {
        let what = what.clone();
        row![
            text(format!("{prefix}{keys}")).size(12).font(hero::BOLD).width(Length::Fixed(KEYS_WIDTH)),
            text(what).size(12).style(style::faint(0.7)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    };
    let half = entries.len().div_ceil(2);
    let split = entries.len() > 10;
    let mut left = column![].spacing(4);
    let mut right = column![].spacing(4);
    for (i, e) in entries.iter().enumerate() {
        match split && i >= half {
            true => right = right.push(line(e)),
            false => left = left.push(line(e)),
        }
    }
    let title = match prefix.is_empty() {
        true => "keys".to_string(),
        false => prefix.to_string(),
    };
    let body = column![text(title).size(11).style(style::faint(0.5)), row![left, right].spacing(24)].spacing(8);
    let panel = container(body).padding([12, 16]).style(|t| {
        let p = theme::of(t);
        iced::widget::container::Style {
            background: Some(Background::Color(style::surface_color(t))),
            text_color: Some(p.background.base.text),
            border: Border {
                radius: 10.0.into(),
                width: 1.0,
                color: style::edge_color(t),
            },
            ..iced::widget::container::Style::default()
        }
    });
    container(panel)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding {
            top: 16.0,
            right: 16.0,
            bottom,
            left: 16.0,
        })
        .align_x(Alignment::End)
        .align_y(Alignment::End)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vim::{GRAMMAR, GROUPS};

    /// After `z`, what `z` starts; from the top, `g` and `z` once each as
    /// their groups, and single keys as themselves. Falsified by listing a
    /// longer sequence whole from the top: `gg` and `zz` were beside `j`.
    #[test]
    fn a_prefix_lists_what_follows_it() {
        let z = entries(&[GRAMMAR], &[GROUPS], "z");
        assert_eq!(
            z,
            [
                ("z".to_string(), "scroll: the cursor in the middle".to_string()),
                ("t".to_string(), "scroll: the cursor at the top".to_string()),
                ("b".to_string(), "scroll: the cursor at the foot".to_string()),
            ]
        );
        let top = entries(&[GRAMMAR], &[GROUPS], "");
        assert!(top.contains(&("g".to_string(), "+go to".to_string())));
        assert!(top.contains(&("z".to_string(), "+scroll".to_string())));
        assert!(top.contains(&("j".to_string(), "down".to_string())));
        assert!(!top.iter().any(|(k, _)| k == "gg" || k == "zz"));
        assert_eq!(top.iter().filter(|(k, _)| k == "z").count(), 1);
    }

    /// The panel waits for its delay, then shows the prefix, and goes when
    /// the sequence is done; `?` shows the top and toggles. Falsified by
    /// showing at once: a `gg` typed quickly flashed the panel.
    #[test]
    fn the_panel_waits_and_goes() {
        let mut w = Which::new();
        w.typed("g");
        assert_eq!(w.showing(), None, "not before the delay");
        let key = w.waiting().expect("a timer for g");
        w.due(key);
        assert_eq!(w.showing(), Some("g"));
        w.typed("");
        assert_eq!(w.showing(), None, "gg typed: gone");
        assert_eq!(w.waiting(), None);
        w.ask();
        assert_eq!(w.showing(), Some(""), "? opens the top");
        w.ask();
        assert_eq!(w.showing(), None, "…and closes it");
        w.typed("2z");
        let stale = w.key - 1;
        w.due(stale);
        assert_eq!(w.showing(), None, "an old timer opens nothing");
    }
}
