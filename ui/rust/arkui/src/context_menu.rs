//! A thing's context menu, held by the thing itself: right-click (or a
//! long press on a touch screen, or `Shift+F10`/the menu key while the
//! pointer is on it) opens ArkUI's menu at the pointer, with every item the
//! thing offers, in order.
//!
//! [`crate::menu`] is a menu the app keeps — the app opens it, routes its
//! keys and closes it — which is right for a window's own menus, the ones a
//! key opens from anywhere. A row drawn from a view laid out in Rust
//! (`ark_app::view`, [`crate::view`]) has no app state of its own to keep a
//! menu in: the view is a function of the screen's record. So this keeps
//! its own — whether it is up, where, which entry the keys are on — in the
//! widget's tree, as iced's `tooltip` keeps its own, and draws the same
//! panel as [`crate::menu::view`] does ([`crate::panel`]: the ground, the
//! border, the corner, a row's height, the lit row's wash). The title line
//! says what the menu is about, as every ArkUI menu does.
//!
//! How it behaves is AppKit's contextual menu, as [`crate::menu::Anchor`]
//! says: its corner on the pointer, flipped to stay on the glass
//! ([`crate::menu::Menu::origin_for`]); the entry under the pointer lit;
//! `↑`/`↓` (and `j`/`k`) move, `Enter` runs, `Esc` or a press outside
//! closes without running anything, and the press outside goes no further.
//! It opens with no motion: a context menu is seen tens of times a day
//! (ui/docs/motion.md, the frequency rule; iced has no group opacity to
//! fade it with in any case).
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, tree, Tree, Widget};
use iced::advanced::{mouse, overlay, renderer, Clipboard, Shell};
use iced::keyboard::{key::Named, Key};
use iced::widget::{column, svg, text, Column};
use iced::{Background, Border, Element, Event, Length, Point, Rectangle, Size, Vector};

use crate::menu::{Anchor, Menu, MAX_WIDTH, MIN_WIDTH};
use crate::panel::{self, ENTRY, PADDING, TITLE};
use crate::{fit, icon, style};

/// How long a touch is held before it opens the menu, in milliseconds:
/// Android's long press.
const HOLD_MS: u128 = 500;

/// One thing the menu offers.
#[derive(Debug, Clone)]
pub struct Item<M> {
    pub glyph: Option<&'static [u8]>,
    pub label: String,
    /// In the theme's danger colour: closing, deleting.
    pub destructive: bool,
    pub message: M,
}

/// `content` with a context menu about `title` offering `items`.
pub fn context_menu<'a, M: Clone + 'a>(content: impl Into<Element<'a, M>>, title: impl Into<String>, items: Vec<Item<M>>) -> Element<'a, M> {
    let title = title.into();
    let menu = panel_of(&title, &items);
    Element::new(ContextMenu {
        content: content.into(),
        width: width_for(&items),
        items,
        menu,
    })
}

/// As wide as its longest label, within a menu's range.
fn width_for<M>(items: &[Item<M>]) -> f32 {
    panel::width_for(items.iter().map(|i| (i.label.as_str(), 0.0)), MIN_WIDTH, MAX_WIDTH)
}

/// The panel, drawn with nothing lit: the lit row's wash is drawn under it
/// by the overlay, which is what knows where the pointer and the keys are.
fn panel_of<'a, M: 'a>(title: &str, items: &[Item<M>]) -> Element<'a, M> {
    let width = width_for(items);
    let budget = panel::chars(width, panel::ENTRY_CHROME);
    let rows: Column<'a, M> = items.iter().fold(column![].spacing(0), |col, i| {
        let destructive = i.destructive;
        let label = text(fit::tail(&i.label, budget))
            .size(13)
            .width(Length::Fill)
            .wrapping(text::Wrapping::None)
            .style(move |theme| text::Style {
                color: Some(match destructive {
                    true => crate::theme::of(theme).danger.base.color,
                    false => style::entry_text(theme, false),
                }),
            });
        let glyph = i.glyph.map(|g| {
            let g: Element<'a, M> = match destructive {
                true => svg(svg::Handle::from_memory(g))
                    .width(icon::SIZE)
                    .height(icon::SIZE)
                    .style(|theme, _| svg::Style {
                        color: Some(crate::theme::of(theme).danger.base.color),
                    })
                    .into(),
                false => icon::line(g, false).into(),
            };
            g
        });
        col.push(panel::entry(glyph, label.into(), None))
    });
    let title_chars = panel::chars(width, panel::TITLE_CHROME);
    panel::panel(column![panel::title(fit::middle(title, title_chars)), rows].spacing(0), width).into()
}

struct ContextMenu<'a, M> {
    content: Element<'a, M>,
    items: Vec<Item<M>>,
    width: f32,
    menu: Element<'a, M>,
}

/// Where the menu is, while it is up.
#[derive(Debug, Default)]
struct State {
    /// Its pointer, from the content's corner.
    open: Option<Vector>,
    /// The entry the keys are on.
    at: Option<usize>,
    /// Where a touch went down and when, while it might become a long press.
    touch: Option<(Point, iced::time::Instant)>,
    /// Where the pointer last was over the content.
    pointer: Option<Point>,
}

impl<M: Clone> Widget<M, iced::Theme, iced::Renderer> for ContextMenu<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content), Tree::new(&self.menu)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.content, &self.menu]);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let state = tree.state.downcast_mut::<State>();
        if !self.items.is_empty() && state.open.is_none() {
            let opened = match event {
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => cursor.position_over(bounds),
                Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                    state.pointer = cursor.position_over(bounds);
                    None
                }
                Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. })
                    if matches!(key, Key::Named(Named::ContextMenu)) || (modifiers.shift() && matches!(key, Key::Named(Named::F10))) =>
                {
                    state.pointer.filter(|p| bounds.contains(*p))
                }
                Event::Touch(iced::touch::Event::FingerPressed { position, .. }) if bounds.contains(*position) => {
                    state.touch = Some((*position, iced::time::Instant::now()));
                    None
                }
                Event::Touch(iced::touch::Event::FingerMoved { position, .. }) => {
                    if state.touch.is_some_and(|(at, _)| at.distance(*position) > 10.0) {
                        state.touch = None;
                    }
                    None
                }
                Event::Touch(iced::touch::Event::FingerLifted { .. } | iced::touch::Event::FingerLost { .. }) => {
                    state.touch = None;
                    None
                }
                Event::Window(iced::window::Event::RedrawRequested(now)) => match state.touch {
                    Some((at, since)) if now.duration_since(since).as_millis() >= HOLD_MS => {
                        state.touch = None;
                        Some(at)
                    }
                    Some(_) => {
                        shell.request_redraw();
                        None
                    }
                    None => None,
                },
                _ => None,
            };
            if let Some(at) = opened {
                state.open = Some(at - bounds.position());
                state.at = None;
                shell.capture_event();
                shell.request_redraw();
                return;
            }
            if state.touch.is_some() {
                shell.request_redraw();
            }
        }
        self.content
            .as_widget_mut()
            .update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
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
        self.content
            .as_widget()
            .draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer, operation: &mut dyn widget::Operation) {
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        let open = tree.state.downcast_ref::<State>().open;
        let (state, children) = (&mut tree.state, &mut tree.children);
        let (content_tree, menu_tree) = children.split_at_mut(1);
        let content = self
            .content
            .as_widget_mut()
            .overlay(&mut content_tree[0], layout, renderer, viewport, translation);
        let menu = open.map(|offset| {
            overlay::Element::new(Box::new(Up {
                pointer: layout.position() + translation + offset,
                width: self.width,
                items: &self.items,
                menu: &mut self.menu,
                tree: &mut menu_tree[0],
                state: state.downcast_mut::<State>(),
            }))
        });
        match (content, menu) {
            (None, None) => None,
            (Some(c), None) => Some(c),
            (None, Some(m)) => Some(m),
            (Some(c), Some(m)) => Some(overlay::Group::with_children(vec![c, m]).overlay()),
        }
    }
}

/// The menu, up.
struct Up<'a, 'b, M> {
    pointer: Point,
    width: f32,
    items: &'b [Item<M>],
    menu: &'b mut Element<'a, M>,
    tree: &'b mut Tree,
    state: &'b mut State,
}

impl<M> Up<'_, '_, M> {
    /// Which entry `p` is on, in the panel laid out at `bounds`.
    fn entry_at(&self, bounds: Rectangle, p: Point) -> Option<usize> {
        if !bounds.contains(p) {
            return None;
        }
        let y = p.y - bounds.y - PADDING - TITLE;
        (y >= 0.0).then(|| (y / ENTRY) as usize).filter(|i| *i < self.items.len())
    }

    fn close(&mut self, shell: &mut Shell<'_, M>) {
        self.state.open = None;
        self.state.at = None;
        shell.request_redraw();
    }
}

impl<M: Clone> overlay::Overlay<M, iced::Theme, iced::Renderer> for Up<'_, '_, M> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let origin = Menu::<M>::origin_for(self.pointer, bounds, self.items.len(), Anchor::Pointer, self.width);
        let node = self
            .menu
            .as_widget_mut()
            .layout(self.tree, renderer, &layout::Limits::new(Size::ZERO, bounds));
        node.move_to(origin)
    }

    fn draw(&self, renderer: &mut iced::Renderer, theme: &iced::Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor) {
        let bounds = layout.bounds();
        // Under the panel's rows, the lit one's wash: the keys' entry, or
        // the pointer's.
        let lit = self.state.at.or_else(|| cursor.position().and_then(|p| self.entry_at(bounds, p)));
        self.menu.as_widget().draw(self.tree, renderer, theme, style, layout, cursor, &bounds);
        if let Some(i) = lit {
            let row = Rectangle {
                x: bounds.x + PADDING,
                y: bounds.y + PADDING + TITLE + ENTRY * i as f32,
                width: bounds.width - PADDING * 2.0,
                height: ENTRY,
            };
            let wash = style::entry(theme, true, true);
            if let Some(Background::Color(c)) = wash.background {
                use iced::advanced::Renderer as _;
                renderer.with_layer(row, |r| {
                    r.fill_quad(
                        renderer::Quad {
                            bounds: row,
                            border: Border {
                                radius: panel::ENTRY_RADIUS.into(),
                                ..Border::default()
                            },
                            ..renderer::Quad::default()
                        },
                        Background::Color(c),
                    );
                });
                // The row's own glyph and label again, over the wash.
                self.menu.as_widget().draw(self.tree, renderer, theme, style, layout, cursor, &row);
            }
        }
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
                if cursor.position().and_then(|p| self.entry_at(bounds, p)).is_some() {
                    self.state.at = None;
                }
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
                if let Some(i) = cursor.position().and_then(|p| self.entry_at(bounds, p)) {
                    let m = self.items[i].message.clone();
                    self.close(shell);
                    shell.publish(m);
                }
                shell.capture_event();
            }
            Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) => {
                let n = self.items.len();
                let step = |at: Option<usize>, down: bool| match (at, down) {
                    (None, true) => 0,
                    (None, false) => n - 1,
                    (Some(i), true) => (i + 1) % n,
                    (Some(i), false) => (i + n - 1) % n,
                };
                match key.as_ref() {
                    Key::Named(Named::Escape) => self.close(shell),
                    Key::Named(Named::ArrowDown) | Key::Character("j") => self.state.at = Some(step(self.state.at, true)),
                    Key::Named(Named::ArrowUp) | Key::Character("k") => self.state.at = Some(step(self.state.at, false)),
                    Key::Named(Named::Enter | Named::Space) => {
                        if let Some(i) = self.state.at {
                            let m = self.items[i].message.clone();
                            self.close(shell);
                            shell.publish(m);
                        }
                    }
                    _ => return,
                }
                shell.capture_event();
                shell.request_redraw();
            }
            _ => {}
        }
    }

    fn mouse_interaction(&self, layout: Layout<'_>, cursor: mouse::Cursor, _renderer: &iced::Renderer) -> mouse::Interaction {
        match cursor.position().and_then(|p| self.entry_at(layout.bounds(), p)) {
            Some(_) => mouse::Interaction::Pointer,
            None => mouse::Interaction::Idle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::container;

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Open,
        Rename,
        Reconcile,
        Close,
    }

    fn row() -> Element<'static, Msg> {
        let items = vec![
            Item {
                glyph: None,
                label: "Rename".into(),
                destructive: false,
                message: Msg::Rename,
            },
            Item {
                glyph: None,
                label: "Reconcile…".into(),
                destructive: false,
                message: Msg::Reconcile,
            },
            Item {
                glyph: None,
                label: "Close Account".into(),
                destructive: true,
                message: Msg::Close,
            },
        ];
        let b = iced::widget::button(text("Checking")).on_press(Msg::Open).width(Length::Fill);
        context_menu(container(b).width(Length::Fill).height(40), "Checking", items)
    }

    /// A right click opens the whole menu, and each entry runs its own
    /// item. Falsified by the first item alone: what the old row's right
    /// click ran (`Rename`) whatever was asked for.
    #[test]
    fn every_item_is_offered_and_runs() {
        for (i, want) in [Msg::Rename, Msg::Reconcile, Msg::Close].into_iter().enumerate() {
            let mut ui = iced_test::simulator(row());
            let at = Point::new(20.0, 20.0);
            ui.point_at(at);
            let _ = ui.simulate(vec![Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))]);
            // The menu's corner is on the pointer; its rows under its title.
            ui.point_at(Point::new(at.x + 40.0, at.y + PADDING + TITLE + ENTRY * (i as f32 + 0.5)));
            let _ = ui.simulate(iced_test::simulator::click());
            assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![want], "entry {i}");
        }
    }

    /// A press outside closes it without running anything, and goes no
    /// further: the row under it is not opened.
    #[test]
    fn a_press_outside_closes_it() {
        let mut ui = iced_test::simulator(row());
        ui.point_at(Point::new(300.0, 20.0));
        let _ = ui.simulate(vec![Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))]);
        ui.point_at(Point::new(5.0, 5.0));
        let _ = ui.simulate(iced_test::simulator::click());
        assert!(ui.into_messages().next().is_none());
    }

    /// The keys move and run, and Esc closes without running anything.
    #[test]
    fn the_keys_move_and_run() {
        let mut ui = iced_test::simulator(row());
        ui.point_at(Point::new(20.0, 20.0));
        let _ = ui.simulate(vec![Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))]);
        let _ = ui.tap_key(Key::Named(Named::ArrowDown));
        let _ = ui.tap_key(Key::Named(Named::ArrowDown));
        let _ = ui.tap_key(Key::Named(Named::Enter));
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![Msg::Reconcile]);
    }

    /// A left click on the row is the row's own, the menu shut.
    #[test]
    fn a_click_is_the_rows_own() {
        let mut ui = iced_test::simulator(row());
        let _ = ui.click("Checking");
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![Msg::Open]);
    }
}
