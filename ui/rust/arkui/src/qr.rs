//! A QR code, drawn: what "Add a Device" shows a phone's camera. The code
//! itself — which modules are dark — is the sign-in's, made once in Rust
//! (`ark_auth::onboarding::qr_code`, which every platform's ArkUI draws);
//! here it is squares on a light ground, with the quiet zone of four
//! modules a scanner needs. Always dark on light, whatever the theme: a
//! scanner reads that way round.
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{mouse, renderer};
use iced::{Color, Element, Length, Rectangle, Size, Theme};

/// The light modules around the code, in modules.
pub const QUIET: usize = 4;

/// The code `size` modules a side, `dark` row by row from the top left,
/// drawn `side` points a side, quiet zone and all.
pub fn qr_code<'a, M: 'a>(size: usize, dark: Vec<bool>, side: f32) -> Element<'a, M> {
    Element::new(QrCode { size, dark, side })
}

struct QrCode {
    size: usize,
    dark: Vec<bool>,
    side: f32,
}

impl<M> Widget<M, Theme, iced::Renderer> for QrCode {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.side), Length::Fixed(self.side))
    }

    fn layout(&mut self, _: &mut Tree, _: &iced::Renderer, limits: &layout::Limits) -> layout::Node {
        layout::Node::new(limits.resolve(self.side, self.side, Size::new(self.side, self.side)))
    }

    fn draw(&self, _: &Tree, renderer: &mut iced::Renderer, _: &Theme, _: &renderer::Style, layout: Layout<'_>, _: mouse::Cursor, _: &Rectangle) {
        use iced::advanced::Renderer as _;
        let b = layout.bounds();
        renderer.fill_quad(
            renderer::Quad {
                bounds: b,
                ..renderer::Quad::default()
            },
            Color::WHITE,
        );
        let across = (self.size + 2 * QUIET) as f32;
        let m = b.width.min(b.height) / across;
        for (i, _) in self.dark.iter().enumerate().filter(|(_, d)| **d) {
            let (x, y) = ((i % self.size + QUIET) as f32, (i / self.size + QUIET) as f32);
            let bounds = Rectangle {
                x: b.x + x * m,
                y: b.y + y * m,
                width: m,
                height: m,
            };
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    ..renderer::Quad::default()
                },
                Color::BLACK,
            );
        }
    }
}
