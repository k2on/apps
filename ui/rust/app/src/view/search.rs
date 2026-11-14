//! What a searchable dropdown finds and where its keys are
//! ([`super::search_picker`], ui/docs/parts.md "SearchPicker"): one rule
//! for every toolkit, said here once and in Rust for the desktop and the
//! browser, which draw from it; the phones' parts (`SearchPicker.swift`,
//! `SearchPicker.kt`) follow the same rule in their own words.
//!
//! **What is found.** What is typed is folded — lower case, accents off
//! ("Café" is "cafe") — and cut into words; a choice is found when every
//! word is somewhere in its title or its group's name, folded the same
//! way. So "groc" finds Groceries, "food" every choice under Food, and
//! "cafe din" Café Dinners. Nothing typed finds everything.
//!
//! **In what order.** As the choices were given, under their groups, each
//! group where its first choice was: the app's order, never re-sorted by
//! how well a choice matched.
//!
//! **The keys.** One choice is lit: the chosen one as it opens, else the
//! first; the first found after each change to what is typed. `↑`/`↓`
//! move it, stopping at either end; `Return` chooses it; `Esc` closes
//! without choosing. A press or a tap chooses what it is on.

use super::{Part, View};

/// One choice, as the toolkits draw it: its title, its glyph (a Lucide
/// name), the tag choosing it sends, the group it is listed under (none:
/// ungrouped), words at its end (a balance) in `tint`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Choice {
    pub title: String,
    pub glyph: String,
    pub tag: String,
    pub group: String,
    pub detail: String,
    pub tint: String,
}

/// The choices a picker's slot holds, as a real view has them: its
/// [`Part::SearchChoice`]s, in order.
pub fn choices<M>(v: &View<M>) -> Vec<Choice> {
    let mut out = vec![];
    gather(v, &mut out);
    out
}

fn gather<M>(v: &View<M>, out: &mut Vec<Choice>) {
    match v {
        View::List(vs) => vs.iter().for_each(|v| gather(v, out)),
        View::Part(Part::SearchChoice {
            title,
            glyph,
            tag,
            group,
            detail,
            tint,
        }) => out.push(Choice {
            title: title.get(),
            glyph: glyph.get(),
            tag: tag.get(),
            group: group.get(),
            detail: detail.get(),
            tint: tint.get(),
        }),
        _ => {}
    }
}

/// `s` as it is matched: lower case, its accents taken off.
pub fn fold(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).map(plain).collect()
}

/// A letter without its accent: Latin's, which is what a name in a budget
/// or a library is written in. Anything else is itself.
fn plain(c: char) -> char {
    match c {
        'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'ď' | 'đ' => 'd',
        'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ĥ' | 'ħ' => 'h',
        'ì'..='ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
        'ĵ' => 'j',
        'ķ' => 'k',
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò'..='ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' => 's',
        'ţ' | 'ť' | 'ŧ' => 't',
        'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ŵ' => 'w',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        c => c,
    }
}

/// Whether `c` is found by `query`: every word of it in its title or its
/// group's name.
pub fn matches(query: &str, c: &Choice) -> bool {
    let hay = fold(&format!("{} {}", c.title, c.group));
    fold(query).split_whitespace().all(|w| hay.contains(w))
}

/// The choices found under one group: its name, and theirs by where they
/// are in the choices given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub choices: Vec<usize>,
}

/// What `query` finds of `choices`, under their groups, in the order given.
pub fn found(choices: &[Choice], query: &str) -> Vec<Group> {
    let mut groups: Vec<Group> = vec![];
    for (i, c) in choices.iter().enumerate().filter(|(_, c)| matches(query, c)) {
        match groups.iter_mut().find(|g| g.name == c.group) {
            Some(g) => g.choices.push(i),
            None => groups.push(Group {
                name: c.group.clone(),
                choices: vec![i],
            }),
        }
    }
    groups
}

/// The choices found, in the order they are listed: down each group.
pub fn listed(groups: &[Group]) -> Vec<usize> {
    groups.iter().flat_map(|g| g.choices.iter().copied()).collect()
}

/// A key the open list answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    /// `Return`.
    Choose,
    /// `Esc`.
    Close,
}

/// How the list was put away.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Done {
    /// This choice's tag chosen: the picker's message is sent with it.
    Chose(String),
    /// Closed, nothing chosen.
    Closed,
}

/// The list while it is open: what is typed, and which choice is lit (by
/// where it is in the choices given).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Searching {
    pub query: String,
    pub at: Option<usize>,
}

impl Searching {
    /// Opened, nothing typed: the choice tagged `selected` lit, else the
    /// first.
    pub fn open(choices: &[Choice], selected: &str) -> Searching {
        Searching {
            query: String::new(),
            at: choices.iter().position(|c| c.tag == selected).or((!choices.is_empty()).then_some(0)),
        }
    }

    /// What `query` finds of `choices`.
    pub fn found(&self, choices: &[Choice]) -> Vec<Group> {
        found(choices, &self.query)
    }

    /// What is typed is now `query`: the first found lit.
    pub fn typed(&mut self, choices: &[Choice], query: impl Into<String>) {
        self.query = query.into();
        self.at = listed(&self.found(choices)).first().copied();
    }

    /// The pointer onto choice `at`.
    pub fn land(&mut self, at: usize) {
        self.at = Some(at);
    }

    /// Where the lit choice is in the list as it is shown, if it is shown.
    pub fn row(&self, choices: &[Choice]) -> Option<usize> {
        let shown = listed(&self.found(choices));
        self.at.and_then(|a| shown.iter().position(|&i| i == a))
    }

    /// `key` answered: the lit choice moved, or the list put away.
    pub fn key(&mut self, choices: &[Choice], key: Key) -> Option<Done> {
        let shown = listed(&self.found(choices));
        let here = self.at.and_then(|a| shown.iter().position(|&i| i == a));
        match key {
            Key::Close => return Some(Done::Closed),
            Key::Choose => return here.map(|r| Done::Chose(choices[shown[r]].tag.clone())),
            Key::Down => {
                let r = here.map_or(0, |r| (r + 1).min(shown.len().saturating_sub(1)));
                self.at = shown.get(r).copied();
            }
            Key::Up => {
                let r = here.map_or(0, |r| r.saturating_sub(1));
                self.at = shown.get(r).copied();
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{search_choice, search_picker, Val};

    fn c(title: &str, tag: &str, group: &str) -> Choice {
        Choice {
            title: title.into(),
            glyph: "tag".into(),
            tag: tag.into(),
            group: group.into(),
            detail: String::new(),
            tint: "neutral".into(),
        }
    }

    fn budget() -> Vec<Choice> {
        vec![
            c("Rent", "rent", "Bills"),
            c("Électricité", "power", "Bills"),
            c("Groceries", "groc", "Food"),
            c("Café Dinners", "cafe", "Food"),
            c("Gas & Transit", "gas", "Getting Around"),
        ]
    }

    fn titles(cs: &[Choice], q: &str) -> Vec<String> {
        listed(&found(cs, q)).into_iter().map(|i| cs[i].title.clone()).collect()
    }

    /// Nothing typed finds every choice, in the order given, under its
    /// group where its first choice was.
    #[test]
    fn nothing_typed_finds_everything_in_order() {
        let cs = budget();
        let gs = found(&cs, "");
        let names: Vec<_> = gs.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["Bills", "Food", "Getting Around"]);
        assert_eq!(listed(&gs), [0, 1, 2, 3, 4]);
    }

    /// Case and accents do not matter, either way round. Falsified by a
    /// plain `contains`: "cafe" would not find Café, nor "ELEC" Électricité.
    #[test]
    fn case_and_accents_do_not_matter() {
        let cs = budget();
        assert_eq!(titles(&cs, "cafe"), ["Café Dinners"]);
        assert_eq!(titles(&cs, "CAFÉ"), ["Café Dinners"]);
        assert_eq!(titles(&cs, "elec"), ["Électricité"]);
        assert_eq!(titles(&cs, "GROC"), ["Groceries"]);
    }

    /// A group's name finds every choice under it, and each word must be
    /// found, in the title or the group.
    #[test]
    fn a_group_name_finds_its_choices_and_every_word_counts() {
        let cs = budget();
        assert_eq!(titles(&cs, "food"), ["Groceries", "Café Dinners"]);
        assert_eq!(titles(&cs, "food din"), ["Café Dinners"]);
        assert_eq!(titles(&cs, "  around  "), ["Gas & Transit"]);
        assert_eq!(titles(&cs, "food rent"), Vec::<String>::new());
    }

    /// What finds nothing is no groups at all: the toolkits say "No Matches".
    #[test]
    fn nothing_found_is_no_groups() {
        assert!(found(&budget(), "zzz").is_empty());
    }

    /// Opened, the chosen one is lit; typing lights the first found; the
    /// arrows stop at the ends of what is shown; Return chooses the lit
    /// one, Esc closes.
    #[test]
    fn the_keys_walk_what_is_shown_and_choose() {
        let cs = budget();
        let mut s = Searching::open(&cs, "groc");
        assert_eq!(s.at, Some(2));
        assert_eq!(s.key(&cs, Key::Up), None);
        assert_eq!(s.at, Some(1));
        s.typed(&cs, "food");
        assert_eq!(s.at, Some(2));
        s.key(&cs, Key::Down);
        s.key(&cs, Key::Down);
        assert_eq!(s.at, Some(3), "stops at the last shown");
        s.key(&cs, Key::Up);
        s.key(&cs, Key::Up);
        assert_eq!(s.at, Some(2), "stops at the first shown");
        assert_eq!(s.key(&cs, Key::Choose), Some(Done::Chose("groc".into())));
        assert_eq!(s.key(&cs, Key::Close), Some(Done::Closed));
    }

    /// Nothing found, Return chooses nothing and the list stays.
    #[test]
    fn return_on_nothing_found_chooses_nothing() {
        let cs = budget();
        let mut s = Searching::open(&cs, "");
        assert_eq!(s.at, Some(0), "nothing chosen yet: the first lit");
        s.typed(&cs, "zzz");
        assert_eq!(s.at, None);
        assert_eq!(s.key(&cs, Key::Choose), None);
        assert_eq!(s.key(&cs, Key::Down), None);
        assert_eq!(s.at, None);
    }

    /// The part holds its choices, read back as the toolkits read them, and
    /// choosing one sends the picker's message with its tag. Falsified by
    /// a choice's tag lost on the way: the message would carry its title.
    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Category { key: String },
    }

    #[test]
    fn the_part_sends_the_chosen_tag() {
        let v: View<Msg> = search_picker(
            "Category",
            "rent",
            |key| Msg::Category { key },
            View::List(vec![
                search_choice("Rent", "house", "rent")
                    .group("Bills")
                    .detail("$1,200.00", "positive")
                    .into(),
                search_choice("Groceries", "shopping-cart", "groc").group("Food").into(),
            ]),
        )
        .unset("Needs a Category")
        .prompt("Search Categories")
        .into();
        let View::Part(Part::SearchPicker {
            label,
            selected,
            unset,
            prompt,
            pick,
            choices: slot,
        }) = &v
        else {
            panic!("a search picker: {v:?}")
        };
        assert_eq!((*label, *prompt), ("Category", "Search Categories"));
        assert_eq!(selected, &Val::Is("rent".to_string()));
        assert_eq!(unset, &Val::Is("Needs a Category".to_string()));
        let cs = choices(slot);
        assert_eq!(cs[0].detail, "$1,200.00");
        assert_eq!(cs[0].tint, "positive");
        assert_eq!(cs[1].tint, "neutral", "no detail: the neutral tint");
        let mut s = Searching::open(&cs, &selected.get());
        s.typed(&cs, "food");
        let Some(Done::Chose(tag)) = s.key(&cs, Key::Choose) else { panic!() };
        assert_eq!(pick.call(tag), Msg::Category { key: "groc".into() });
        // And mapped into another app's messages, it still sends the tag.
        let mapped = v.map(|m| match m {
            Msg::Category { key } => format!("chose {key}"),
        });
        let View::Part(Part::SearchPicker { pick, .. }) = mapped else { panic!() };
        assert_eq!(pick.call("rent".into()), "chose rent");
    }
}
