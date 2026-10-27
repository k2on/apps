//! A component drawn off screen: an iced element laid out at a width, drawn
//! by tiny-skia on the CPU at twice its size, cut to its own height and
//! written as lossless WebP. The same layout, the same widgets and the same
//! renderer an app's window uses, with no window and no GPU, so it runs in
//! a nix build.
use iced::advanced::renderer::{self, Headless};
use iced::{mouse, Color, Element, Font, Pixels, Size, Theme};
use iced_runtime::user_interface::{self, UserInterface};

/// Pixels per logical pixel: a picture stays sharp on a phone's screen.
pub const SCALE: f32 = 2.0;
/// The tallest a picture is laid out to.
const TALLEST: f32 = 2400.0;
/// Room left under a picture's lowest pixel, in logical pixels.
const FOOT: u32 = 2;

pub struct Painter {
    renderer: iced::Renderer,
}

impl Painter {
    pub fn new() -> Painter {
        let fonts = iced::advanced::graphics::text::font_system();
        fonts.write().expect("the font system").load_font(arkui::hero::FIRA_SANS_BOLD.into());
        let renderer = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
            Font::with_name("Fira Sans"),
            Pixels(16.0),
            Some("tiny-skia"),
        ))
        .expect("tiny-skia draws without a GPU");
        Painter { renderer }
    }

    /// `element` at `width`, on `theme`'s ground. `height` fixes how tall it
    /// is; `None` is as tall as it draws.
    pub fn draw<M>(&mut self, element: Element<'_, M>, theme: &Theme, width: f32, height: Option<f32>) -> Picture {
        let size = Size::new(width, height.unwrap_or(TALLEST));
        let mut ui = UserInterface::build(element, size, user_interface::Cache::default(), &mut self.renderer);
        let mut messages = Vec::new();
        let _ = ui.update(
            &[iced::Event::Window(iced::window::Event::RedrawRequested(iced::time::Instant::now()))],
            mouse::Cursor::Unavailable,
            &mut self.renderer,
            &mut iced::advanced::clipboard::Null,
            &mut messages,
        );
        // arkui's ground, which the page's stylesheet is too (site::colours).
        let ground = arkui::theme::of(theme).background.base.color;
        ui.draw(
            &mut self.renderer,
            theme,
            &renderer::Style {
                text_color: arkui::theme::of(theme).background.base.text,
            },
            mouse::Cursor::Unavailable,
        );
        let physical = Size::new((size.width * SCALE).round() as u32, (size.height * SCALE).round() as u32);
        let rgba = self.renderer.screenshot(physical, SCALE, ground);
        let mut picture = Picture {
            width: physical.width,
            height: physical.height,
            rgba,
        };
        if height.is_none() {
            picture.cut(ground);
        }
        picture
    }
}

pub struct Picture {
    pub width: u32,
    pub height: u32,
    rgba: Vec<u8>,
}

impl Picture {
    /// Cut off the rows of bare ground under the lowest drawn pixel.
    fn cut(&mut self, ground: Color) {
        let ground = ground.into_rgba8();
        let row = self.width as usize * 4;
        let drawn = self
            .rgba
            .chunks_exact(row)
            .rposition(|line| line.as_chunks::<4>().0.iter().any(|p| *p != ground))
            .map(|last| last as u32 + 1)
            .unwrap_or(0);
        self.height = (drawn + FOOT * SCALE as u32).min(self.height).max(1);
        self.rgba.truncate(self.height as usize * row);
    }

    pub fn webp(&self) -> Vec<u8> {
        let mut out = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut out)
            .encode(&self.rgba, self.width, self.height, image::ExtendedColorType::Rgba8)
            .expect("an RGBA picture encodes");
        out
    }

    /// Its logical size, for the page's `width` and `height`.
    pub fn logical(&self) -> (u32, u32) {
        ((self.width as f32 / SCALE) as u32, (self.height as f32 / SCALE) as u32)
    }
}
