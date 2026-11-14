//! The scroll edge, as Apple draws it: where a list scrolls up under
//! something floating at the top of the window — the account's face — the
//! list goes soft under it, its ground laid over the band the face sits in
//! and thinning to nothing a little below. So a row passing under the face
//! is never drawn through it, and there is no hard line where it stops.
//!
//! **At rest there is nothing.** The ground comes in with the scroll: none
//! at the top of the list, all of it [`FADE`] down, so a page that has not
//! been scrolled looks exactly as it does without this, and one scrolled a
//! little has only a breath of it.
//!
//! **It is drawn by what scrolls, not over it.** A [`faded`] list is the
//! content of a `scrollable`: iced hands that content the visible part of
//! itself as the viewport, in the content's own coordinates, so how far it
//! is scrolled is where that viewport starts below the content's top —
//! known as it is drawn, with no message and nothing kept in the app. The
//! band is drawn there, which under the scroll is the top of what is
//! seen. It is in a layer of its own, as [`crate::motion`]'s veil is:
//! within one, iced draws text after every quad, and the rows' words would
//! show through it. It hears nothing; the pointer goes through to the rows.
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{mouse, overlay, renderer, Clipboard, Shell};
use iced::gradient::Linear;
use iced::{Background, Color, Element, Event, Gradient, Length, Radians, Rectangle, Size, Theme, Vector};

/// How far below the band the ground thins to nothing, and how far a list
/// scrolls before the ground is all there.
pub const FADE: f32 = 28.0;

/// `content`, the content of a scrollable whose top is at the window's
/// top, drawn with the scroll edge once it is scrolled: `ground` (the
/// colour of what it is on) solid over the top `band` — where the face
/// floats — then thinning over [`FADE`] below it.
pub fn faded<'a, M: 'a>(content: impl Into<Element<'a, M>>, band: f32, ground: fn(&Theme) -> Color) -> Element<'a, M> {
    Element::new(Faded {
        content: content.into(),
        band,
        ground,
    })
}

/// The ground's alpha `scrolled` down: none at rest, all of it [`FADE`]
/// down.
pub fn strength(scrolled: f32) -> f32 {
    (scrolled / FADE).clamp(0.0, 1.0)
}

struct Faded<'a, M> {
    content: Element<'a, M>,
    band: f32,
    ground: fn(&Theme) -> Color,
}

impl<M> Widget<M, Theme, iced::Renderer> for Faded<'_, M> {
    fn tag(&self) -> widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&mut self, tree: &mut Tree) {
        self.content.as_widget_mut().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
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
        self.content
            .as_widget_mut()
            .update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
    }

    /// Its content's accessibility tree, as is: this wrapper adds nothing a
    /// screen reader reads (COSMIC's accesskit, the fork's `a11y`).
    fn a11y_nodes(&self, layout: Layout<'_>, tree: &Tree, cursor: mouse::Cursor) -> iced_accessibility::A11yTree {
        self.content.as_widget().a11y_nodes(layout, tree, cursor)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        self.content.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
        let alpha = strength(viewport.y - layout.bounds().y);
        if alpha <= 0.0 {
            return;
        }
        let ground = (self.ground)(theme).scale_alpha(alpha);
        let tall = self.band + FADE;
        let bounds = Rectangle { height: tall, ..*viewport };
        // Top to bottom: the ground over the band, then nothing at its foot.
        let gradient = Linear::new(Radians(std::f32::consts::PI))
            .add_stop(0.0, ground)
            .add_stop(self.band / tall, ground)
            .add_stop(1.0, Color { a: 0.0, ..ground });
        renderer.with_layer(bounds, |renderer| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    ..renderer::Quad::default()
                },
                Background::Gradient(Gradient::Linear(gradient)),
            );
        });
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &iced::Renderer, operation: &mut dyn widget::Operation) {
        self.content.as_widget_mut().operate(tree, layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        offset: Vector,
    ) -> Option<overlay::Element<'b, M, Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(tree, layout, renderer, viewport, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing at rest, a little a little way down, all of it a fade down
    /// and beyond. Falsified by a ground that is there at rest: a page that
    /// has not moved had a band over its top.
    #[test]
    fn the_edge_comes_in_with_the_scroll() {
        assert_eq!(strength(0.0), 0.0);
        assert_eq!(strength(-3.0), 0.0, "overscrolled up is at rest");
        assert!((strength(FADE / 2.0) - 0.5).abs() < 1e-6);
        assert_eq!(strength(FADE), 1.0);
        assert_eq!(strength(FADE * 40.0), 1.0);
    }
}
