//! The sidebar: a column of places down the left of a window, grouped under
//! headings, one of them the page beside it.
//!
//! Lifted from harken's, where the rules were paid for. **One highlight, not
//! two**: the sidebar's cursor *is* what the page is showing, so the row
//! under it is marked whether or not the sidebar has the keyboard, and lit
//! (the accent, [`style::entry`]) only while it has. A heading is drawn when
//! the group changes rather than stored as a row, so the cursor's row `n`
//! and the reader's are the same and the keyboard walks a plain column
//! ([`crate::vim::Grid::column`]).
use iced::widget::{column, container, mouse_area, row, scrollable, text};
use iced::{Alignment, Element, Length, Padding};

use crate::{icon, style, theme};

/// What a window that has a sidebar usually makes it, edge included.
pub const WIDTH: f32 = 200.0;

/// One place in the sidebar.
#[derive(Debug, Clone)]
pub struct Item<M> {
    pub glyph: &'static [u8],
    pub label: String,
    /// The group it is under. A heading is drawn above the first item of a
    /// run with the same one; `None` draws none.
    pub heading: Option<&'static str>,
    /// The tally drawn on the right, where there is one to draw.
    pub count: Option<i64>,
    /// What choosing it does.
    pub press: M,
}

/// The sidebar `width` wide, edge included, with the cursor on `cursor`.
///
/// `focused` is whether it has the keyboard. `padding` is above the first
/// item and below the last — a window's chrome padding, so the first place
/// lines up with whatever starts the page. `id` is its scrollable's, for
/// [`crate::scroll::reveal`].
pub fn view<'a, M: Clone + 'a>(items: &[Item<M>], cursor: usize, focused: bool, width: f32, padding: f32, id: &'static str) -> Element<'a, M> {
    let mut side = column![].spacing(1).padding(Padding {
        top: padding,
        right: 10.0,
        bottom: padding,
        left: 10.0,
    });
    let mut under: Option<&str> = None;
    for (i, item) in items.iter().enumerate() {
        if item.heading.is_some() && item.heading != under {
            side = side.push(container(text(item.heading.unwrap_or_default()).size(10).style(style::faint(0.5))).padding([8, 10]));
        }
        under = item.heading;
        let on_cursor = i == cursor;
        let lit = on_cursor && focused;
        side = side.push(
            mouse_area(
                container(
                    row![
                        icon::line(item.glyph, lit),
                        text(item.label.clone())
                            .size(13)
                            .width(Length::Fill)
                            .wrapping(text::Wrapping::None)
                            .style(move |theme| text::Style {
                                color: Some(style::entry_text(theme, lit)),
                            }),
                        text(item.count.map(|n| n.to_string()).unwrap_or_default()).size(10).style(move |t| {
                            let p = theme::of(t);
                            text::Style {
                                color: Some(match lit {
                                    true => p.primary.base.text.scale_alpha(0.7),
                                    false => p.background.base.text.scale_alpha(0.5),
                                }),
                            }
                        }),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([5, 10])
                .style(move |theme| style::entry(theme, on_cursor, focused)),
            )
            .on_press(item.press.clone()),
        );
    }
    container(scrollable(side).id(id).style(style::bars))
        // Its edge, a pixel, is part of its width (drawn by the window beside
        // it, `style::edge`), so the page is as wide as the window less this.
        .width(Length::Fixed(width - 1.0))
        .height(Length::Fill)
        .style(style::surface)
        .into()
}
