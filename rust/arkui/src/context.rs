//! A menu and the picker that is its submenu — or a picker on its own — and
//! the rules for which of them has the keyboard.
//!
//! **A menu and its submenu are one thing.** The submenu belongs to an entry of
//! its parent, both are up at once, and you asked one question. So `<Esc>`
//! closes both, the menu's backdrop closes both, and the cursor leaving the
//! entry closes the submenu. A picker opened on its own has no parent and is
//! only itself.
//!
//! **"Has the keyboard" is one question with one answer**, [`Context::focus`]:
//! the picker if it has the keys, else the menu if one is up, else neither —
//! the page. Three places each deciding it for themselves once drew two
//! accent-filled rows on screen, the row `j` used to move and the entry `j`
//! actually moves.
//!
//! **Moving is the same everywhere.** A menu and a picker are both a
//! [`vim::Grid::column`], which refuses `h` and `l`; [`Context::travel`] steps
//! the grid and, on a refusal, says what is on the other side of that edge —
//! the submenu to the right of the entry that owns it, the parent entry to the
//! left of a submenu. None of that is in `vim`: a shape knows it has an edge,
//! and only this knows what is beyond one.
//!
//! The app keeps one of these, draws [`crate::menu::view`] and
//! [`crate::picker::view`] from it with [`crate::layer`], and routes its
//! messages here. What an entry *does* stays the app's: running one hands the
//! app its message.
use iced::Size;

use crate::menu::{self, Menu};
use crate::picker::{self, Picked, Picker};
use crate::vim::{self, Motion};

/// Which layer has the keyboard, when one does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Menu,
    Picker,
}

/// A menu, its submenu, or a picker alone.
#[derive(Debug, Clone)]
pub struct Context<M, T> {
    pub menu: Option<Menu<M>>,
    pub picker: Option<Picker<T>>,
    /// The dwell asked for the submenu, so the next picker opened beside the
    /// menu is *shown* rather than entered. Cleared by anything that moves.
    showing: bool,
}

impl<M, T> Default for Context<M, T> {
    fn default() -> Self {
        Context {
            menu: None,
            picker: None,
            showing: false,
        }
    }
}

impl<M: Clone, T: Clone> Context<M, T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Which layer has the keyboard: the one on top that wants it. `None` is
    /// the page behind — which is also the whole of the rule that the page
    /// behind a context window does not follow the pointer: an app refuses a
    /// hover on its rows while this is `Some`, because a backdrop can swallow
    /// a click and a wheel but not a hover.
    pub fn focus(&self) -> Option<Layer> {
        if self.picker.as_ref().is_some_and(|p| p.keys) {
            return Some(Layer::Picker);
        }
        if self.menu.is_some() {
            return Some(Layer::Menu);
        }
        None
    }

    /// Whether anything is up.
    pub fn is_open(&self) -> bool {
        self.menu.is_some() || self.picker.is_some()
    }

    /// Put a menu up, in place of whatever was.
    pub fn open_menu(&mut self, menu: Menu<M>) {
        self.menu = Some(menu);
        self.picker = None;
        self.showing = false;
    }

    /// Close the menu — and its submenu with it, which has no life of its own:
    /// it belongs to the entry it hangs off, and now there is no entry. This is
    /// what lets the menu's backdrop close both, and why a submenu needs no
    /// backdrop of its own. A picker standing alone is left alone.
    pub fn close_menu(&mut self) {
        self.menu = None;
        if self.picker.as_ref().is_some_and(|p| p.origin.is_some()) {
            self.picker = None;
        }
        self.showing = false;
    }

    /// Close the picker, and only it. The menu, if any, has the keys again.
    pub fn close_picker(&mut self) {
        self.picker = None;
        self.showing = false;
    }

    /// Close everything.
    pub fn close(&mut self) {
        self.menu = None;
        self.picker = None;
        self.showing = false;
    }

    /// Open a picker. With a menu up it is that menu's submenu, pinned beside
    /// the entry the cursor is on; alone, it is centred.
    ///
    /// It takes the keyboard — unless the dwell is what asked for it, in which
    /// case it is shown and the keys stay on the parent entry (see
    /// [`Context::tick`]). That is the only difference between resting on an
    /// entry and pressing `<Enter>` on it.
    pub fn open_picker(&mut self, mut picker: Picker<T>, window: Size) {
        picker.origin = self
            .menu
            .as_ref()
            .map(|m| m.submenu_origin(picker.rows(), picker.width, picker::MAX_HEIGHT, window));
        picker.keys = !std::mem::take(&mut self.showing);
        self.picker = Some(picker);
    }

    /// Put the menu's cursor on an entry, however it got there — a hover and a
    /// `j` have to mean the same thing, and everything the dwell does keys off
    /// *moving*.
    ///
    /// Moving off an entry closes the submenu it owned: a submenu belongs to
    /// its parent entry, so the cursor leaving is the submenu going — which is
    /// what makes pointing at the next entry mean the next entry.
    ///
    /// Landing on the entry it is already on is not nothing: the pointer may
    /// have come *back* out of the submenu, and the highlight belongs to
    /// whatever the pointer is over, so the keys come back to the parent. The
    /// submenu stays up — you pointed at the entry that owns it, not away.
    pub fn land(&mut self, at: usize) {
        let Some(menu) = &mut self.menu else {
            return;
        };
        let at = at.min(menu.entries.len().saturating_sub(1));
        self.showing = false;
        if menu.at == at {
            if let Some(picker) = &mut self.picker {
                picker.keys = false;
            }
            return;
        }
        menu.at = at;
        menu.dwell = 0;
        menu.offered = false;
        self.picker = None;
    }

    /// Put the picker's cursor on a row. Reaching a row of a submenu *is*
    /// entering it: the pointer has left the entry it hangs off.
    pub fn picker_at(&mut self, at: usize) {
        if let Some(picker) = &mut self.picker {
            picker.land(at);
            picker.keys = true;
        }
    }

    /// One tick of the app's clock (50ms). When the cursor has rested on an
    /// entry that owns a submenu for [`menu::DWELL`] ticks, answers that
    /// entry's message: run it the way any message is run, and the picker it
    /// opens with [`Context::open_picker`] is *shown*, not entered.
    ///
    /// Offered once per rest: `<Esc>` out of the submenu leaves the cursor on
    /// its entry, and without that the next tick would open it again.
    pub fn tick(&mut self) -> Option<M> {
        let menu = self.menu.as_mut()?;
        if self.picker.is_some() || menu.offered {
            return None;
        }
        let entry = menu.entries.get(menu.at).filter(|e| e.opens_submenu())?;
        let message = entry.message().clone();
        menu.dwell = menu.dwell.saturating_add(1);
        if menu.dwell < menu::DWELL {
            return None;
        }
        menu.offered = true;
        self.showing = true;
        Some(message)
    }

    /// What `<Enter>` on the menu runs: the message of the entry the cursor is
    /// on. An entry that owns a submenu answers the message that opens it, and
    /// a picker opened now takes the keys — `<Enter>` enters.
    pub fn chosen(&mut self) -> Option<M> {
        self.showing = false;
        Some(self.menu.as_ref()?.current()?.message().clone())
    }

    /// `<Enter>` on the picker.
    pub fn activate_picker(&mut self) -> Option<Picked<T>> {
        self.picker.as_mut()?.activate()
    }

    /// `<Esc>`: the menu and its submenu together, from either side of the
    /// keyboard — or, with no menu, the picker alone.
    ///
    /// It closed the innermost once, which is what a *stack* of menus does,
    /// and this is not one: dismissing one question twice is the same
    /// complaint as a menu that stays up after it has been answered. The click
    /// already works this way — a submenu has no backdrop, so the menu's closes
    /// both — and this is the keyboard agreeing with the pointer.
    pub fn cancel(&mut self) {
        match self.menu.is_some() {
            true => self.close_menu(),
            false => self.close_picker(),
        }
    }

    /// Move the cursor of whichever layer has the keyboard.
    ///
    /// A refused step means "not mine", and this answers what is beyond each
    /// edge: `l` on the entry that owns a submenu steps into it — taking the
    /// keys if the pointer has already shown it, and otherwise answering the
    /// entry's message for the app to run, which opens it entered — and `h`
    /// in a submenu hands the keys back to the parent entry, leaving the panel
    /// up, which is exactly what pointing back at that entry does. A picker
    /// with no parent has nowhere for `h` to go.
    pub fn travel(&mut self, motion: Motion) -> Option<M> {
        match self.focus()? {
            Layer::Menu => {
                let menu = self.menu.as_ref()?;
                match menu.grid().step(menu.at, motion) {
                    Some(at) => {
                        self.land(at);
                        None
                    }
                    None if matches!(motion, Motion::Right(_)) => self.enter_submenu(),
                    None => None,
                }
            }
            Layer::Picker => {
                let picker = self.picker.as_mut()?;
                match picker.grid().step(picker.at, motion) {
                    Some(at) => picker.land(at),
                    None if matches!(motion, Motion::Left(_)) && self.menu.is_some() => picker.keys = false,
                    None => {}
                }
                None
            }
        }
    }

    /// Step into the submenu the menu's cursor is on, if that entry owns one.
    fn enter_submenu(&mut self) -> Option<M> {
        let entry = self.menu.as_ref()?.current().filter(|e| e.opens_submenu())?;
        // Already up because the pointer rested here: step in rather than
        // opening it again, which would rebuild it and put the cursor back on
        // the first row of a panel you are already looking at.
        if let Some(picker) = &mut self.picker {
            picker.keys = true;
            return None;
        }
        let message = entry.message().clone();
        self.showing = false;
        Some(message)
    }

    /// The menu's grid and cursor, or the picker's, for whichever has the keys.
    pub fn grid(&self) -> Option<(vim::Grid, usize)> {
        match self.focus()? {
            Layer::Menu => self.menu.as_ref().map(|m| (m.grid(), m.at)),
            Layer::Picker => self.picker.as_ref().map(|p| (p.grid(), p.at)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyphs;
    use crate::menu::{Anchor, Entry, SUBMENU_OVERLAP};
    use crate::panel::{EDGE, PADDING};
    use crate::picker::Choice;
    use iced::{Point, Size};

    /// A stand-in for an app's messages.
    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Play,
        OpenPicker,
        GoTo(String),
    }

    /// harken's row menu, in miniature: `Play`, the entry that owns the
    /// submenu, and a `Go to` per name.
    fn entries(album: &str) -> Vec<Entry<Msg>> {
        vec![
            Entry::run(glyphs::PLAY, "Play", Msg::Play),
            Entry::submenu(glyphs::ADD_TO, "Add to playlist", Msg::OpenPicker),
            Entry::run(glyphs::ALBUM, format!("Go to {album}"), Msg::GoTo(album.into())),
        ]
    }

    fn lists() -> Picker<u32> {
        Picker::new(
            "Air",
            vec![
                Choice {
                    value: 1,
                    name: "Favorites".into(),
                    on: true,
                },
                Choice {
                    value: 2,
                    name: "Piano".into(),
                    on: false,
                },
            ],
            Some("New playlist\u{2026}".into()),
        )
    }

    const WINDOW: Size = Size::new(860.0, 600.0);

    /// What harken's ⋯ column is: the list's right-hand edge, less the page's
    /// padding and the scrollbar.
    fn dots(window: Size) -> Anchor {
        Anchor::RightEdge(window.width - 16.0 - crate::SCROLLBAR)
    }

    fn with_menu(at: Point) -> Context<Msg, u32> {
        let mut ctx = Context::new();
        ctx.open_menu(Menu::open("Air on the G String", entries("Water Music"), at, WINDOW, Anchor::Pointer));
        ctx
    }

    /// The app's side of the dwell and of `l`: run the message, which for the
    /// submenu entry opens the picker.
    fn run(ctx: &mut Context<Msg, u32>, message: Option<Msg>, window: Size) {
        if let Some(Msg::OpenPicker) = message {
            ctx.open_picker(lists(), window);
        }
    }

    /// A menu is as wide as its longest entry, and nothing in it is cut — and
    /// it stops growing at the maximum.
    ///
    /// Falsified by returning `MIN_WIDTH` from `Menu::width_for`: the first
    /// assertion fails with `"Go to Goldberg Variations, BWV 988" is cut in a
    /// menu 198px wide`, which is the bug it was written for.
    #[test]
    fn a_menu_is_as_wide_as_its_longest_entry() {
        let mut widest: f32 = 0.0;
        for album in [
            "Water Music",
            "Goldberg Variations, BWV 988",
            "The Well-Tempered Clavier, Book I",
            "Messiah",
        ] {
            let menu = Menu::open("t", entries(album), Point::ORIGIN, WINDOW, Anchor::Pointer);
            widest = widest.max(menu.width);
            for entry in &menu.entries {
                assert_eq!(
                    crate::fit::tail(&entry.label, menu.label_chars()),
                    entry.label,
                    "{:?} is cut in a menu {}px wide",
                    entry.label,
                    menu.width
                );
            }
        }
        assert!(widest > menu::MIN_WIDTH, "nothing earned a menu wider than the minimum ({widest}px)");
        let long = [Entry::run(glyphs::PLAY, "Go to ".to_owned() + &"a".repeat(200), Msg::Play)];
        assert_eq!(Menu::width_for(&long), menu::MAX_WIDTH);
        let short = [Entry::run(glyphs::PLAY, "Play", Msg::Play)];
        assert_eq!(Menu::width_for(&short), menu::MIN_WIDTH);
    }

    /// One keyboard, one cursor drawn: a menu takes the keys off the page, the
    /// submenu off the menu, and closing hands them back one layer at a time.
    /// Falsified by making `focus` answer `Menu` whenever a menu is up: the
    /// second assertion fails.
    #[test]
    fn a_context_window_takes_the_keyboard_from_what_is_behind() {
        let mut ctx: Context<Msg, u32> = Context::new();
        assert_eq!(ctx.focus(), None, "nothing is over the page");
        ctx.open_menu(Menu::open("t", entries("x"), Point::ORIGIN, WINDOW, Anchor::Pointer));
        assert_eq!(ctx.focus(), Some(Layer::Menu));
        ctx.open_picker(lists(), WINDOW);
        assert_eq!(ctx.focus(), Some(Layer::Picker), "the submenu has it now");
        ctx.close_picker();
        assert_eq!(ctx.focus(), Some(Layer::Menu), "back to the parent");
        ctx.close_menu();
        assert_eq!(ctx.focus(), None, "and back to the page");
    }

    /// The two AppKit rules, and they are not the same rule: a contextual menu
    /// opens on the pointer, a control's menu on the control.
    ///
    /// Falsified by making `origin_for` ignore the anchor: whichever branch is
    /// kept, one of the two halves fails.
    #[test]
    fn a_right_click_opens_on_the_pointer_and_a_control_on_the_control() {
        let window = Size::new(1280.0, 720.0);
        let wide = menu::MIN_WIDTH;
        let at = |x, anchor| Menu::<Msg>::origin_for(Point::new(x, 300.0), window, 4, anchor, wide);
        assert_eq!(at(220.0, Anchor::Pointer), Point::new(220.0, 300.0));
        assert_eq!(at(600.0, Anchor::Pointer), Point::new(600.0, 300.0));

        let (a, b) = (at(220.0, dots(window)), at(1240.0, dots(window)));
        assert_eq!(a, b, "one button, one place");
        assert_eq!(a.y, 300.0, "…and how far down is still the row's");
        assert_eq!(a.x + wide, window.width - 16.0 - crate::SCROLLBAR, "the menu ends where the dots do");
    }

    /// A submenu opens by being pointed at, and a sweep past is not pointing.
    ///
    /// **The tick counts are numbers and not `DWELL`**: written
    /// `for _ in 1..DWELL`, setting the constant to 1 empties the range and
    /// the "not yet" assertion never runs — the test shrinks with the thing it
    /// holds. One tick is a crossing; twenty is somebody who stopped.
    /// Falsified both ways: `DWELL = 1` fails the first half, `DWELL = 30` the
    /// second; and deleting `offered = true` fails "closed stays closed".
    #[test]
    fn a_submenu_opens_by_being_pointed_at_and_not_by_being_passed_over() {
        let mut ctx = with_menu(Point::new(100.0, 100.0));
        ctx.land(1);
        let asked = ctx.tick();
        run(&mut ctx, asked, WINDOW);
        assert!(ctx.picker.is_none(), "a sweep across is not a rest on");
        for _ in 0..20 {
            let asked = ctx.tick();
            run(&mut ctx, asked, WINDOW);
        }
        assert!(ctx.picker.is_some(), "…and resting on it opens it");

        // `<Esc>` out of the submenu alone leaves the cursor on its entry;
        // without `offered` the very next tick reopens it and there is no way
        // out.
        ctx.close_picker();
        for _ in 0..20 {
            let asked = ctx.tick();
            run(&mut ctx, asked, WINDOW);
        }
        assert!(ctx.picker.is_none(), "closed stays closed while you are on it");

        // …and leaving the entry and coming back offers it again.
        ctx.land(0);
        ctx.land(1);
        for _ in 0..20 {
            let asked = ctx.tick();
            run(&mut ctx, asked, WINDOW);
        }
        assert!(ctx.picker.is_some(), "and it is offered a second time");
    }

    /// A submenu opened by pointing is *shown*, not entered: the highlight
    /// stays on the entry it hangs off. The pointer reaching one of its rows
    /// hands the keys over, and so does `<Enter>` on the parent.
    ///
    /// Falsified by making `open_picker` always take the keys: the first
    /// assertion fails with `Picker`.
    #[test]
    fn a_submenu_opened_by_pointing_does_not_take_the_keyboard() {
        let mut ctx = with_menu(Point::new(100.0, 100.0));
        ctx.land(1);
        for _ in 0..20 {
            let asked = ctx.tick();
            run(&mut ctx, asked, WINDOW);
        }
        assert!(ctx.picker.is_some(), "it is up");
        assert_eq!(ctx.focus(), Some(Layer::Menu), "…and the entry still has the keys");
        assert_eq!(ctx.menu.as_ref().unwrap().at, 1);

        ctx.picker_at(0);
        assert_eq!(ctx.focus(), Some(Layer::Picker), "the pointer reached a row of it");

        ctx.land(0);
        assert!(ctx.picker.is_none(), "moving off closed it");
        ctx.land(1);
        let chosen = ctx.chosen();
        run(&mut ctx, chosen, WINDOW);
        assert_eq!(ctx.focus(), Some(Layer::Picker), "<Enter> enters it");
    }

    /// `l` steps into a submenu and `h` steps back out, which is the one rule
    /// a refused edge has everywhere.
    ///
    /// Falsified by deleting the `Motion::Right` arm of `travel`: `l` opens
    /// nothing. Deleting the `Motion::Left` arm and `h` never comes back.
    #[test]
    fn l_and_h_walk_into_a_submenu_and_back_out() {
        let mut ctx = with_menu(Point::new(100.0, 100.0));
        ctx.land(1);
        assert_eq!(ctx.focus(), Some(Layer::Menu), "on the entry that owns one");
        let asked = ctx.travel(Motion::Right(1));
        assert_eq!(asked, Some(Msg::OpenPicker), "`l` asks for it");
        run(&mut ctx, asked, WINDOW);
        assert_eq!(ctx.focus(), Some(Layer::Picker), "…and it opened entered");

        assert_eq!(ctx.travel(Motion::Left(1)), None);
        assert_eq!(ctx.focus(), Some(Layer::Menu), "`h` stepped back out");
        assert!(ctx.picker.is_some(), "…and left the panel up");

        // Shown by the pointer and then `l`: it steps in rather than reopening.
        assert_eq!(ctx.travel(Motion::Right(1)), None, "already up, so nothing to run");
        assert_eq!(ctx.focus(), Some(Layer::Picker));

        // `l` on an entry with no submenu is the nothing `l` in a list is.
        let mut bare = with_menu(Point::new(100.0, 100.0));
        assert_eq!(bare.travel(Motion::Right(1)), None, "`Play` has nowhere to go");
        assert!(bare.picker.is_none());

        // …and `h` in a picker with no parent has nowhere to go either.
        let mut alone: Context<Msg, u32> = Context::new();
        alone.open_picker(lists(), WINDOW);
        alone.travel(Motion::Left(1));
        assert_eq!(alone.focus(), Some(Layer::Picker));
    }

    /// Pointing back at the parent takes the keys back from its submenu, and
    /// leaves the submenu up — you pointed at the entry that owns it.
    ///
    /// Falsified by restoring a bare `return` for "already here" in `land`:
    /// the focus stays `Picker`.
    #[test]
    fn pointing_back_at_the_parent_takes_the_keys_from_the_submenu() {
        let mut ctx = with_menu(Point::new(100.0, 100.0));
        ctx.land(1);
        let chosen = ctx.chosen();
        run(&mut ctx, chosen, WINDOW);
        ctx.picker_at(0);
        assert_eq!(ctx.focus(), Some(Layer::Picker));
        ctx.land(1);
        assert_eq!(ctx.focus(), Some(Layer::Menu), "pointing at the parent is pointing at the parent");
        assert!(ctx.picker.is_some(), "and its submenu is still up");
    }

    /// A submenu belongs to its parent entry, so the cursor leaving is the
    /// submenu going. Falsified by dropping `self.picker = None` in `land`.
    #[test]
    fn moving_off_the_parent_closes_the_submenu() {
        let mut ctx = with_menu(Point::new(100.0, 100.0));
        ctx.land(1);
        let chosen = ctx.chosen();
        run(&mut ctx, chosen, WINDOW);
        assert!(ctx.picker.is_some(), "<Enter> opens it without waiting");
        ctx.land(0);
        assert!(ctx.picker.is_none(), "the cursor left the entry it hung off");
        assert_eq!(ctx.focus(), Some(Layer::Menu), "…and the keyboard came back");
    }

    /// `<Esc>` takes the menu and its submenu together, from either side of
    /// the keyboard; and a picker alone is only itself.
    ///
    /// Falsified by making `cancel` close the picker first when there is one:
    /// the menu is still up after the first `<Esc>`.
    #[test]
    fn escape_closes_a_menu_and_its_submenu_together() {
        for entered in [false, true] {
            let mut ctx = with_menu(Point::new(100.0, 100.0));
            ctx.land(1);
            let chosen = ctx.chosen();
            run(&mut ctx, chosen, WINDOW);
            if !entered {
                ctx.picker.as_mut().unwrap().keys = false;
            }
            ctx.cancel();
            assert!(ctx.picker.is_none(), "the submenu went (entered: {entered})");
            assert!(ctx.menu.is_none(), "…and so did the menu (entered: {entered})");
            assert_eq!(ctx.focus(), None);
        }

        let mut alone: Context<Msg, u32> = Context::new();
        alone.open_picker(lists(), WINDOW);
        assert!(alone.picker.as_ref().unwrap().origin.is_none(), "alone, it is centred");
        alone.cancel();
        assert!(!alone.is_open());
    }

    /// The menu's backdrop closes both, and leaves a picker that is not its
    /// submenu alone. Falsified by making `close_menu` drop every picker.
    #[test]
    fn closing_a_menu_takes_only_its_own_submenu() {
        let mut ctx: Context<Msg, u32> = Context::new();
        ctx.open_picker(lists(), WINDOW);
        ctx.menu = Some(Menu::open("t", entries("x"), Point::ORIGIN, WINDOW, Anchor::Pointer));
        ctx.close_menu();
        assert!(ctx.picker.is_some(), "a picker standing alone is not the menu's");
    }

    /// A picker is as wide as its longest name and no wider, and a name too
    /// long for the maximum is shortened rather than the panel widened.
    ///
    /// Both bounds are asserted, because "fits the content" passes trivially
    /// if the answer is always the maximum. Falsified by returning
    /// `MAX_WIDTH` from `width_for`: the first assertion fails.
    #[test]
    fn a_submenu_is_as_wide_as_its_longest_name() {
        let picker = lists();
        let narrow = picker.width;
        assert!(narrow < picker::MAX_WIDTH, "Favorites and Piano do not need {narrow}px");
        assert!(narrow >= picker::MIN_WIDTH, "…but not narrower than its own last row: {narrow}");
        assert_eq!(narrow, Picker::width_for(&picker.choices, picker.new_row.as_deref()));

        let mut long = picker.choices.clone();
        long[0].name = "Music to read the collected works of Gibbon to".to_string();
        let wide = Picker::width_for(&long, picker.new_row.as_deref());
        assert!(wide > narrow, "a longer name is a wider panel");
        assert_eq!(wide, picker::MAX_WIDTH, "and it stops at the cap");
        let budget = Picker { width: wide, ..picker }.label_chars();
        assert!(budget < long[0].name.chars().count(), "so that name is shortened, not the panel widened");
    }

    /// `<Enter>` on a row toggles it and says what it is now; on the last row
    /// it starts naming. Falsified by not flipping `on`: the first toggle
    /// reports the old value.
    #[test]
    fn a_picker_toggles_and_its_last_row_makes_one() {
        let mut picker = lists();
        assert_eq!(picker.activate(), Some(Picked::Toggled { value: 1, on: false }));
        assert_eq!(picker.activate(), Some(Picked::Toggled { value: 1, on: true }));
        picker.land(99);
        assert_eq!(picker.at, 2, "the new row is a row, and the last one");
        assert_eq!(picker.activate(), Some(Picked::Naming));
        assert_eq!(picker.naming.as_deref(), Some(""));
    }

    /// A submenu opens beside its parent, never over it, and laps it by exactly
    /// one padding — so the menu's lit row touches the submenu's edge.
    ///
    /// **Said twice, the second time as a literal.** Against `SUBMENU_OVERLAP`
    /// the lap holds nothing: both sides move together, so eight or zero
    /// pass. Falsified by doubling the constant (the literal fails), and by
    /// placing with `fit` again (the menu is covered at every width, since a
    /// menu on the ⋯ is always near the right-hand edge).
    #[test]
    fn a_submenu_opens_beside_its_parent_and_never_over_it() {
        for width in 320..=3000 {
            let window = Size::new(width as f32, 720.0);
            let mut ctx: Context<Msg, u32> = Context::new();
            ctx.open_menu(Menu::open("t", entries("Water Music"), Point::new(0.0, 100.0), window, dots(window)));
            ctx.land(1);
            ctx.open_picker(lists(), window);
            let menu = ctx.menu.as_ref().unwrap();
            let panel = ctx.picker.as_ref().unwrap();
            let at = panel.origin.unwrap();
            let wide = panel.width;
            assert!(at.x >= EDGE, "at {width}px it starts at {}", at.x);
            if width >= 600 {
                let right = at.x > menu.origin.x;
                let lit = match right {
                    true => menu.origin.x + menu.width - PADDING,
                    false => menu.origin.x + PADDING,
                };
                let edge = match right {
                    true => at.x,
                    false => at.x + wide,
                };
                assert_eq!(edge, lit, "at {width}px the submenu's edge {edge} does not meet the highlight at {lit}");
                let lap = match right {
                    true => menu.origin.x + menu.width - at.x,
                    false => at.x + wide - menu.origin.x,
                };
                assert_eq!(lap, 4.0, "at {width}px the submenu laps the menu by {lap}");
                assert_eq!(lap, SUBMENU_OVERLAP);
            }
        }
    }

    /// …and it slides up to fit rather than flipping over its parent, and its
    /// first row is level with the entry it hangs off.
    ///
    /// Falsified by placing the submenu with `fit` and `picker::MAX_HEIGHT`
    /// again: at every height under about 600px the flip fires, though three
    /// rows would have fitted, and the panel jumps to the top of the window.
    #[test]
    fn a_submenu_slides_to_fit_rather_than_flipping_over_its_parent() {
        let mut ctx = with_menu(Point::new(600.0, 120.0));
        let big = Size::new(1280.0, 900.0);
        ctx.open_menu(Menu::open("t", entries("x"), Point::new(600.0, 120.0), big, dots(big)));
        ctx.land(1);
        ctx.open_picker(lists(), big);
        let top = ctx.menu.as_ref().unwrap().entry_top();
        let first_row = ctx.picker.as_ref().unwrap().origin.unwrap().y + PADDING;
        assert_eq!(first_row, top, "the submenu's first row is not level");

        let height = ctx.picker.as_ref().unwrap().submenu_height();
        for h in 200..=900 {
            let window = Size::new(1280.0, h as f32);
            ctx.open_menu(Menu::open("t", entries("x"), Point::new(600.0, 120.0), window, dots(window)));
            ctx.land(1);
            ctx.open_picker(lists(), window);
            let y = ctx.picker.as_ref().unwrap().origin.unwrap().y;
            assert!(y >= EDGE, "at {h}px tall it starts at {y}");
            let top = ctx.menu.as_ref().unwrap().entry_top();
            assert!(y + PADDING <= top, "it slid *down* at {h}px tall");
            // Wherever there is room below the entry, it is level with it —
            // which is what a flip gets wrong.
            if top - PADDING + height + EDGE <= h as f32 {
                assert_eq!(y + PADDING, top, "at {h}px tall there was room beside the entry and it moved");
            }
            if height + EDGE * 2.0 <= h as f32 {
                assert!(y + height <= h as f32 - EDGE, "at {h}px tall the last row is off the bottom");
            }
        }
    }

    /// A menu on a control stays on the glass at every window a person can
    /// drag, at both ends of its own width range.
    ///
    /// **It starts at 120 and not at 320**: the right-aligned x only goes
    /// negative below about 232px for the narrowest menu, and a loop that
    /// never reaches that case passes with the `.max` taken out. Falsified by
    /// dropping the `.max` in `right_aligned`: it fails at 120.
    #[test]
    fn a_menu_never_hangs_off_the_glass() {
        for wide in [menu::MIN_WIDTH, menu::MAX_WIDTH] {
            for width in 120..=4000 {
                let window = Size::new(width as f32, 720.0);
                let at = Menu::<Msg>::origin_for(Point::new(0.0, 100.0), window, 4, dots(window), wide);
                assert!(at.x >= EDGE, "at {width}px a {wide}px menu starts at {} and is clipped", at.x);
            }
        }
    }

    /// The arrows are `h` and `l`, which is what makes every test above cover
    /// both. Falsified by mapping `ArrowRight` to anything else.
    #[test]
    fn the_arrows_are_the_same_motions_as_hl() {
        use iced::keyboard::{key::Named, Key, Modifiers};
        let mut keys = vim::Keys::new();
        let letter = |keys: &mut vim::Keys, c: &str| keys.press(&Key::Character(c.into()), Modifiers::default());
        let named = |keys: &mut vim::Keys, n: Named| keys.press(&Key::Named(n), Modifiers::default());
        assert_eq!(letter(&mut keys, "l"), named(&mut keys, Named::ArrowRight), "l and the right arrow");
        assert_eq!(letter(&mut keys, "h"), named(&mut keys, Named::ArrowLeft), "h and the left arrow");
    }
}
