//! One of many, found by typing (`ark_app::view::search_picker`,
//! ui/docs/parts.md "SearchPicker"), on the desktop: a form's field saying
//! what is chosen (or what is still to choose, after a question mark in the
//! caution colour); pressed, a dropdown anchored under it — a search line
//! that has the keys, and under it the choices found by their groups, each
//! its glyph, its title, its figure in its tint and a tick on the chosen
//! one. What is found and where the keys are is `ark_app::view::search`'s.
//!
//! It keeps its own state — whether it is open, what is typed, which row
//! is lit, how far down its list is — in the widget's tree, as
//! [`crate::context_menu`] does: a view laid out in Rust is a function of
//! the screen's record and has nowhere else to keep it. The search line is
//! drawn from the typed keys the dropdown hears, not a text box, so the
//! keys are the dropdown's from the press that opens it.
//!
//! **Declared, not measured** ([`crate::panel`]): a heading is
//! [`HEADING`] tall and a row [`ROW`], so which row the pointer is on, and
//! how far the list is scrolled, is arithmetic. The list shows whole lines
//! from `first`, as many as fit in `dropdown-height`, and the wheel and the
//! keys move `first`.
//!
//! **Keys**: `↑`/`↓` the lit row, `Enter` chooses it, `Esc` closes,
//! `Backspace` erases, anything else typed is typed. **Pointer**: the row
//! under it lit, a click chooses, a press outside closes and goes no
//! further. **Motion**: none — it is opened tens of times a day, and iced
//! has no group opacity to fade it with (motion.md, the frequency rule).
use ark_app::view::search::{self, Choice, Done, Searching};
use ark_app::view::Input;
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, tree, Tree, Widget};
use iced::advanced::{mouse, overlay, renderer, Clipboard, Shell};
use iced::keyboard::{key::Named, Key};
use iced::widget::{column, container, row, rule, Column, Space};
use iced::{Alignment, Background, Border, Element, Event, Length, Point, Rectangle, Size, Vector};

use crate::layout_tokens::{DROPDOWN_HEIGHT, DROPDOWN_WIDTH};
use crate::panel::{self, ENTRY_GAP, ENTRY_PAD_X, PADDING};
use crate::parts::Tint;
use crate::text::{self, text};
use crate::theme::of;
use crate::{glyphs, icon, style};

/// The search line's height, a group's heading's and a row's.
pub const SEARCH: f32 = 34.0;
pub const HEADING: f32 = panel::TITLE;
pub const ROW: f32 = panel::ENTRY;
/// What the list says when nothing is found.
pub const EMPTY: &str = "No Matches";

/// A field of `label` choosing one of `choices`, the one tagged `selected`
/// chosen (`unset` said while none is); `prompt` in the search line until
/// something is typed; choosing sends `pick` with its tag.
pub fn search_picker<M: Clone + 'static>(
    label: &str,
    selected: String,
    unset: String,
    prompt: &str,
    choices: Vec<Choice>,
    pick: Input<String, M>,
) -> Element<'static, M> {
    let chosen = choices.iter().find(|c| c.tag == selected).cloned();
    let value: Element<'static, M> = match &chosen {
        Some(c) => row![
            icon::tinted(shape(&c.glyph), icon::SIZE, |p| p.background.base.text.scale_alpha(0.6)),
            text(c.title.clone()).size(13).width(Length::Fill).wrapping(text::Wrapping::None),
        ]
        .spacing(ENTRY_GAP)
        .align_y(Alignment::Center)
        .into(),
        None => row![
            icon::tinted(glyphs::UNCHOSEN, icon::SIZE, |p| p.warning.base.color),
            text(unset)
                .size(13)
                .width(Length::Fill)
                .wrapping(text::Wrapping::None)
                .style(Tint::Caution.text()),
        ]
        .spacing(ENTRY_GAP)
        .align_y(Alignment::Center)
        .into(),
    };
    let face = column![
        text(label.to_string()).size(12).style(style::dim),
        container(
            row![
                value,
                icon::tinted(glyphs::CHOOSE, icon::SIZE, |p| p.background.base.text.scale_alpha(0.5))
            ]
            .spacing(ENTRY_GAP)
            .align_y(Alignment::Center)
        )
        .padding([6, 8])
        .width(Length::Fill)
        .style(field),
    ]
    .spacing(4);
    Element::new(Picker {
        face: face.into(),
        choices,
        selected,
        prompt: prompt.to_string(),
        pick,
    })
}

fn shape(name: &str) -> &'static [u8] {
    glyphs::named(name).unwrap_or(glyphs::NOTE)
}

/// The field's box: a text field's ground and border.
fn field(theme: &iced::Theme) -> container::Style {
    let p = of(theme);
    container::Style {
        background: Some(Background::Color(p.background.base.color)),
        border: Border {
            color: p.background.strong.color,
            width: 1.0,
            radius: panel::RADIUS.into(),
        },
        ..container::Style::default()
    }
}

struct Picker<M> {
    face: Element<'static, M>,
    choices: Vec<Choice>,
    selected: String,
    prompt: String,
    pick: Input<String, M>,
}

/// The dropdown, while it is open: what is typed and lit, and the first
/// line shown.
#[derive(Debug, Default)]
struct State {
    open: Option<Searching>,
    first: usize,
    pressed: bool,
}

/// A line of the list: a group's heading, or a choice by where it is in
/// the choices given.
#[derive(Clone, Debug, PartialEq)]
enum Line {
    Heading(String),
    Choice(usize),
}

impl Line {
    fn height(&self) -> f32 {
        match self {
            Line::Heading(_) => HEADING,
            Line::Choice(_) => ROW,
        }
    }
}

/// The lines `s` shows of `choices`: each group's heading (when it has a
/// name), then its choices found.
fn lines(choices: &[Choice], s: &Searching) -> Vec<Line> {
    let mut out = vec![];
    for g in s.found(choices) {
        if !g.name.is_empty() {
            out.push(Line::Heading(g.name));
        }
        out.extend(g.choices.into_iter().map(Line::Choice));
    }
    out
}

/// How tall the list may be.
const LIST: f32 = DROPDOWN_HEIGHT - SEARCH - 1.0 - PADDING * 2.0;

/// The lines shown from `first`: as many whole lines as fit.
fn shown(all: &[Line], first: usize) -> &[Line] {
    let mut h = 0.0;
    let mut end = first;
    while end < all.len() && h + all[end].height() <= LIST {
        h += all[end].height();
        end += 1;
    }
    &all[first.min(all.len())..end]
}

/// The furthest down `first` goes: the last lines filling the list.
fn last_first(all: &[Line]) -> usize {
    let mut h = 0.0;
    let mut i = all.len();
    while i > 0 && h + all[i - 1].height() <= LIST {
        h += all[i - 1].height();
        i -= 1;
    }
    i
}

/// `first` moved so the lit choice is shown, and a group's first choice
/// brings its heading.
fn reveal(all: &[Line], first: usize, at: Option<usize>) -> usize {
    let Some(k) = at.and_then(|a| all.iter().position(|l| *l == Line::Choice(a))) else {
        return first.min(last_first(all));
    };
    let top = if k > 0 && matches!(all[k - 1], Line::Heading(_)) { k - 1 } else { k };
    if top < first {
        return top;
    }
    let mut f = first;
    while k >= f + shown(all, f).len() {
        f += 1;
    }
    f
}

impl<M: Clone + 'static> Widget<M, iced::Theme, iced::Renderer> for Picker<M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.face)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut [&mut self.face]);
    }

    fn size(&self) -> Size<Length> {
        self.face.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        self.face.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let over = cursor.is_over(layout.bounds());
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) | Event::Touch(iced::touch::Event::FingerPressed { .. }) if over => {
                state.pressed = true;
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) | Event::Touch(iced::touch::Event::FingerLifted { .. }) => {
                // Opened by a press begun on it and let go on it.
                let pressed = std::mem::take(&mut state.pressed);
                if pressed && over {
                    let s = Searching::open(&self.choices, &self.selected);
                    let all = lines(&self.choices, &s);
                    state.first = reveal(&all, 0, s.at);
                    state.open = Some(s);
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn a11y_nodes(&self, layout: Layout<'_>, tree: &Tree, cursor: mouse::Cursor) -> iced_accessibility::A11yTree {
        self.face.as_widget().a11y_nodes(layout, &tree.children[0], cursor)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.face
            .as_widget()
            .draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        match cursor.is_over(layout.bounds()) {
            true => mouse::Interaction::Pointer,
            false => mouse::Interaction::None,
        }
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer, operation: &mut dyn widget::Operation) {
        self.face.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &iced::Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        let state = tree.state.downcast_mut::<State>();
        let s = state.open.as_ref()?;
        let panel = drop_panel(&self.choices, &self.selected, &self.prompt, s, state.first);
        let tree = Tree::new(&panel);
        Some(overlay::Element::new(Box::new(Down {
            field: layout.bounds() + translation,
            panel,
            tree,
            state,
            choices: &self.choices,
            pick: &self.pick,
        })))
    }
}

/// The dropdown drawn: the search line, a rule, the lines shown.
fn drop_panel<M: 'static>(choices: &[Choice], selected: &str, prompt: &str, s: &Searching, first: usize) -> Element<'static, M> {
    let typed: Element<'static, M> = match s.query.is_empty() {
        true => text(prompt.to_string()).size(13).style(style::dim).into(),
        false => text(s.query.clone()).size(13).wrapping(text::Wrapping::None).into(),
    };
    // The caret, after what is typed: the keys are here.
    let caret = container(Space::new().width(1).height(16)).style(|t| container::Style {
        background: Some(Background::Color(of(t).background.base.text)),
        ..container::Style::default()
    });
    let search = container(
        row![
            icon::tinted(glyphs::SEARCH, icon::SIZE, |p| p.background.strong.text),
            row![typed, caret].align_y(Alignment::Center)
        ]
        .spacing(ENTRY_GAP)
        .align_y(Alignment::Center),
    )
    .padding([0.0, ENTRY_PAD_X])
    .center_y(Length::Fixed(SEARCH));
    let all = lines(choices, s);
    let list: Element<'static, M> = match all.is_empty() {
        true => container(text(EMPTY).size(13).style(style::dim))
            .center_x(Length::Fill)
            .center_y(Length::Fixed(ROW))
            .into(),
        false => shown(&all, first)
            .iter()
            .fold(Column::new(), |col, l| {
                col.push(match l {
                    Line::Heading(name) => Element::from(
                        container(text(name.clone()).size(11).style(style::dim))
                            .padding([0.0, ENTRY_PAD_X])
                            .center_y(Length::Fixed(HEADING)),
                    ),
                    Line::Choice(i) => choice_row(&choices[*i], choices[*i].tag == selected, s.at == Some(*i)),
                })
            })
            .into(),
    };
    panel::panel(column![search, rule::horizontal(1).style(style::edge), list], DROPDOWN_WIDTH).into()
}

fn choice_row<M: 'static>(c: &Choice, chosen: bool, lit: bool) -> Element<'static, M> {
    let tint = Tint::named(&c.tint);
    let mut line = row![
        icon::tinted(shape(&c.glyph), icon::SIZE, |p| p.background.base.text.scale_alpha(0.6)),
        text(c.title.clone()).size(13).width(Length::Fill).wrapping(text::Wrapping::None),
    ]
    .spacing(ENTRY_GAP)
    .align_y(Alignment::Center);
    if !c.detail.is_empty() {
        line = line.push(text(c.detail.clone()).size(12).style(tint.text()));
    }
    let tick: Element<'static, M> = match chosen {
        true => icon::tinted(glyphs::TICK, icon::SIZE, |p| p.primary.base.color).into(),
        false => Space::new().width(icon::SIZE).into(),
    };
    container(line.push(tick))
        .padding([0.0, ENTRY_PAD_X])
        .center_y(Length::Fixed(ROW))
        .width(Length::Fill)
        .style(move |t| {
            let mut s = style::entry(t, lit, true);
            s.border.radius = panel::ENTRY_RADIUS.into();
            s
        })
        .into()
}

/// The dropdown, open.
struct Down<'b, M> {
    field: Rectangle,
    panel: Element<'static, M>,
    tree: Tree,
    state: &'b mut State,
    choices: &'b [Choice],
    pick: &'b Input<String, M>,
}

impl<M> Down<'_, M> {
    fn searching(&mut self) -> &mut Searching {
        self.state.open.get_or_insert_with(Searching::default)
    }

    /// The choice at `p`, in the panel laid out at `bounds`.
    fn choice_at(&self, bounds: Rectangle, p: Point) -> Option<usize> {
        if !bounds.contains(p) {
            return None;
        }
        let s = self.state.open.as_ref()?;
        let all = lines(self.choices, s);
        let mut y = bounds.y + PADDING + SEARCH + 1.0;
        for l in shown(&all, self.state.first) {
            if p.y >= y && p.y < y + l.height() {
                return match l {
                    Line::Choice(i) => Some(*i),
                    Line::Heading(_) => None,
                };
            }
            y += l.height();
        }
        None
    }

    fn close(&mut self, shell: &mut Shell<'_, M>) {
        self.state.open = None;
        shell.request_redraw();
    }

    fn choose(&mut self, tag: String, shell: &mut Shell<'_, M>) {
        self.close(shell);
        shell.publish(self.pick.call(tag));
    }

    /// The list scrolled to keep the lit choice in sight.
    fn reveal(&mut self) {
        let s = self.searching().clone();
        let all = lines(self.choices, &s);
        self.state.first = reveal(&all, self.state.first, s.at);
    }
}

impl<M> overlay::Overlay<M, iced::Theme, iced::Renderer> for Down<'_, M> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let node = self
            .panel
            .as_widget_mut()
            .layout(&mut self.tree, renderer, &layout::Limits::new(Size::ZERO, bounds));
        let h = node.size().height;
        let below = self.field.y + self.field.height + 4.0;
        let y = match below + h > bounds.height - panel::EDGE {
            true => (self.field.y - 4.0 - h).max(panel::EDGE),
            false => below,
        };
        let x = self.field.x.min(bounds.width - DROPDOWN_WIDTH - panel::EDGE).max(panel::EDGE);
        node.move_to(Point::new(x, y))
    }

    fn draw(&self, renderer: &mut iced::Renderer, theme: &iced::Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor) {
        let bounds = layout.bounds();
        self.panel.as_widget().draw(&self.tree, renderer, theme, style, layout, cursor, &bounds);
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
    ) {
        let bounds = layout.bounds();
        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some(i) = cursor.position().and_then(|p| self.choice_at(bounds, p)) {
                    if self.searching().at != Some(i) {
                        self.searching().land(i);
                        shell.request_redraw();
                    }
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                let y = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / ROW,
                };
                let s = self.searching().clone();
                let all = lines(self.choices, &s);
                let f = self.state.first as f32 - y.round();
                self.state.first = (f.max(0.0) as usize).min(last_first(&all));
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonPressed(_)) | Event::Touch(iced::touch::Event::FingerPressed { .. }) => {
                // A press outside closes it, and goes no further.
                if !cursor.position().is_some_and(|p| bounds.contains(p)) {
                    self.close(shell);
                }
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(_)) | Event::Touch(iced::touch::Event::FingerLifted { .. }) => {
                if let Some(i) = cursor.position().and_then(|p| self.choice_at(bounds, p)) {
                    let tag = self.choices[i].tag.clone();
                    self.choose(tag, shell);
                }
                shell.capture_event();
            }
            Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, text, .. }) => {
                let choices = self.choices;
                let key_of = match key.as_ref() {
                    Key::Named(Named::Escape) => Some(search::Key::Close),
                    Key::Named(Named::ArrowUp) => Some(search::Key::Up),
                    Key::Named(Named::ArrowDown) => Some(search::Key::Down),
                    Key::Named(Named::Enter) => Some(search::Key::Choose),
                    _ => None,
                };
                match key_of {
                    Some(k) => match self.searching().key(choices, k) {
                        Some(Done::Closed) => self.close(shell),
                        Some(Done::Chose(tag)) => self.choose(tag, shell),
                        None => self.reveal(),
                    },
                    None if matches!(key.as_ref(), Key::Named(Named::Backspace)) => {
                        let mut q = self.searching().query.clone();
                        q.pop();
                        self.searching().typed(choices, q);
                        self.state.first = 0;
                        self.reveal();
                    }
                    None => {
                        let typed = text
                            .as_deref()
                            .filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
                            .filter(|_| !(modifiers.control() || modifiers.alt() || modifiers.logo()));
                        let Some(t) = typed else { return };
                        let q = format!("{}{t}", self.searching().query);
                        self.searching().typed(choices, q);
                        self.state.first = 0;
                        self.reveal();
                    }
                }
                shell.capture_event();
                shell.request_redraw();
            }
            _ => {}
        }
    }

    fn mouse_interaction(&self, layout: Layout<'_>, cursor: mouse::Cursor, _renderer: &iced::Renderer) -> mouse::Interaction {
        match cursor.position().and_then(|p| self.choice_at(layout.bounds(), p)) {
            Some(_) => mouse::Interaction::Pointer,
            None => mouse::Interaction::Idle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Category(String),
    }

    fn c(title: &str, tag: &str, group: &str) -> Choice {
        Choice {
            title: title.into(),
            glyph: "tag".into(),
            tag: tag.into(),
            group: group.into(),
            detail: "$1.00".into(),
            tint: "positive".into(),
        }
    }

    fn field() -> Element<'static, Msg> {
        let choices = vec![
            c("Rent", "rent", "Bills"),
            c("Groceries", "groc", "Food"),
            c("Café Dinners", "cafe", "Food"),
        ];
        let picker = search_picker(
            "Category",
            "rent".into(),
            "Needs a Category".into(),
            "Search Categories",
            choices,
            Input::new(Msg::Category),
        );
        container(picker).width(400).padding(10).into()
    }

    fn opened() -> iced_test::Simulator<'static, Msg> {
        let mut ui = iced_test::simulator(field());
        let _ = ui.click("Rent");
        ui
    }

    /// Typing filters, accents and all, and Return chooses the first found.
    #[test]
    fn typing_finds_and_return_chooses() {
        let mut ui = opened();
        let _ = ui.typewrite("cafe");
        let _ = ui.tap_key(Key::Named(Named::Enter));
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![Msg::Category("cafe".into())]);
    }

    /// The arrows move from the chosen one; Esc closes without choosing.
    #[test]
    fn the_arrows_move_and_esc_closes() {
        let mut ui = opened();
        let _ = ui.tap_key(Key::Named(Named::ArrowDown));
        let _ = ui.tap_key(Key::Named(Named::Enter));
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![Msg::Category("groc".into())]);
        let mut ui = opened();
        let _ = ui.tap_key(Key::Named(Named::Escape));
        let _ = ui.tap_key(Key::Named(Named::Enter));
        assert!(ui.into_messages().next().is_none());
    }

    /// What finds nothing is no lines (the list says [`EMPTY`]), and
    /// Return chooses nothing.
    #[test]
    fn nothing_found_chooses_nothing() {
        let choices = vec![c("Rent", "rent", "Bills")];
        let mut s = Searching::open(&choices, "rent");
        s.typed(&choices, "zzz");
        assert!(lines(&choices, &s).is_empty());
        let mut ui = opened();
        let _ = ui.typewrite("zzz");
        let _ = ui.tap_key(Key::Named(Named::Enter));
        assert!(ui.into_messages().next().is_none());
    }

    /// A click on a row chooses it: the panel hangs under the field, its
    /// lines at their declared heights (Bills, Rent, Food, Groceries).
    #[test]
    fn a_click_chooses() {
        let mut ui = opened();
        let rent = ui.find("Rent").expect("the field").bounds();
        let panel = rent.y + rent.height + 6.0 + 4.0;
        let groceries = panel + PADDING + SEARCH + 1.0 + HEADING + ROW + HEADING + ROW / 2.0;
        ui.point_at(Point::new(rent.x + 40.0, groceries));
        let _ = ui.simulate(iced_test::simulator::click());
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![Msg::Category("groc".into())]);
    }

    /// The list scrolls to keep the lit choice in sight, its heading with
    /// the group's first.
    #[test]
    fn the_lit_choice_is_kept_in_sight() {
        let many: Vec<Choice> = (0..40)
            .map(|i| c(&format!("C{i}"), &format!("{i}"), if i < 20 { "A" } else { "B" }))
            .collect();
        let s = Searching::open(&many, "30");
        let all = lines(&many, &s);
        let f = reveal(&all, 0, s.at);
        let shown = shown(&all, f);
        assert!(shown.contains(&Line::Choice(30)));
        assert!(f > 0);
        assert_eq!(
            reveal(&all, f, Some(20)),
            all.iter().position(|l| *l == Line::Heading("B".into())).unwrap()
        );
    }
}
