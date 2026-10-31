//! Copy, cut and paste in a text box — in a browser too.
//!
//! On the desktop iced's text box has the system's clipboard and needs
//! nothing. In a browser iced's clipboard is a stand-in that holds nothing
//! and keeps nothing, so ⌘C/Ctrl+C, X and V did nothing at all. [`view`]
//! wraps a window's view and hands every text box in it the browser's
//! clipboard instead:
//!
//! - **Copy and cut** write what the box selected with
//!   `navigator.clipboard.writeText`, which a key press is allowed to do.
//! - **Paste** cannot be read on the spot: the browser hands the clipboard
//!   over only when asked, asynchronously (`readText`, the first time
//!   behind the browser's own permission). So V asks, and the text, when it
//!   arrives, goes into whichever box has the keys, at its cursor, the way
//!   iced puts in what an input method typed (`input_method::Event::Commit`):
//!   the selection replaced, the box's own message sent. [`pastes`] wakes
//!   the window when it arrives, so it goes in at once rather than at the
//!   next event.
//!
//! An app wraps its window's view in [`view`], once, and subscribes to
//! [`pastes`]; on the desktop both cost nothing. Every text box under it is
//! covered, arkui's own (the command box, a picker's name) included.
use std::cell::RefCell;

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{mouse, overlay, renderer, Clipboard, Shell};
use iced::{Element, Event, Length, Rectangle, Size, Subscription, Vector};

/// How long a paste waits for a box with the keys to put it in, in
/// milliseconds: one that arrives with none (the box was left) is dropped
/// rather than put into whichever box has the keys next.
const FRESH_MS: f64 = 2000.0;

thread_local! {
    /// A paste the browser has handed over, and when.
    static PENDING: RefCell<Option<(String, f64)>> = const { RefCell::new(None) };
    /// The running [`pastes`] subscription's end of its channel.
    static BELL: RefCell<Option<iced::futures::channel::mpsc::UnboundedSender<()>>> = const { RefCell::new(None) };
}

/// `view` with every text box in it copying, cutting and pasting in a
/// browser as it does on the desktop: the whole window's, once.
pub fn view<'a, M: 'a>(view: impl Into<Element<'a, M>>) -> Element<'a, M> {
    Element::new(Field { content: view.into() })
}

/// Rings when a paste has arrived, for the app to answer with any message:
/// the update draws a frame, and the box with the keys takes it then.
pub fn pastes() -> Subscription<()> {
    #[cfg(target_arch = "wasm32")]
    {
        fn stream() -> iced::futures::channel::mpsc::UnboundedReceiver<()> {
            let (tx, rx) = iced::futures::channel::mpsc::unbounded();
            BELL.with(|b| *b.borrow_mut() = Some(tx));
            rx
        }
        Subscription::run(stream)
    }
    #[cfg(not(target_arch = "wasm32"))]
    Subscription::none()
}

/// What a paste brought, if it is fresh: taken, so one box takes it.
fn take_pending() -> Option<String> {
    PENDING
        .with(|p| p.borrow_mut().take())
        .filter(|(_, at)| now_ms() - at < FRESH_MS)
        .map(|(text, _)| text)
}

fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    return js_sys::Date::now();
    #[cfg(not(target_arch = "wasm32"))]
    0.0
}

struct Field<'a, M> {
    content: Element<'a, M>,
}

/// The browser's clipboard, for the standard one; whatever iced has for
/// the rest.
#[cfg(target_arch = "wasm32")]
struct Browser<'c> {
    inner: &'c mut dyn Clipboard,
}

#[cfg(target_arch = "wasm32")]
impl Clipboard for Browser<'_> {
    fn read(&self, kind: iced::advanced::clipboard::Kind) -> Option<String> {
        if kind != iced::advanced::clipboard::Kind::Standard {
            return self.inner.read(kind);
        }
        // Nothing now; the text when the browser hands it over.
        let Some(clipboard) = web_sys::window().map(|w| w.navigator().clipboard()) else {
            return None;
        };
        let promise = clipboard.read_text();
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(text) = wasm_bindgen_futures::JsFuture::from(promise).await {
                if let Some(text) = text.as_string() {
                    let text: String = text.chars().filter(|c| !c.is_control()).collect();
                    PENDING.with(|p| *p.borrow_mut() = Some((text, now_ms())));
                    BELL.with(|b| {
                        if let Some(tx) = b.borrow().as_ref() {
                            let _ = tx.unbounded_send(());
                        }
                    });
                }
            }
        });
        None
    }

    fn write(&mut self, kind: iced::advanced::clipboard::Kind, contents: String) {
        if kind != iced::advanced::clipboard::Kind::Standard {
            return self.inner.write(kind, contents);
        }
        if let Some(window) = web_sys::window() {
            let _ = window.navigator().clipboard().write_text(&contents);
        }
    }
}

impl<M> Widget<M, iced::Theme, iced::Renderer> for Field<'_, M> {
    fn tag(&self) -> widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
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
        // A paste that has arrived, before whatever this event is: every
        // widget hears it as typed, and only the box with the keys takes it.
        if let Some(text) = take_pending() {
            let commit = Event::InputMethod(iced::advanced::input_method::Event::Commit(text));
            self.content
                .as_widget_mut()
                .update(tree, &commit, layout, cursor, renderer, clipboard, shell, viewport);
        }
        #[cfg(target_arch = "wasm32")]
        let clipboard = &mut Browser { inner: clipboard };
        self.content
            .as_widget_mut()
            .update(tree, event, layout, cursor, renderer, clipboard, shell, viewport);
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
        self.content.as_widget().draw(tree, renderer, theme, style, layout, cursor, viewport);
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
    ) -> Option<overlay::Element<'b, M, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(tree, layout, renderer, viewport, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A paste that has arrived goes into the box with the keys, at its
    /// cursor, and that box sends its message; the other is left alone.
    /// Falsified by delivering it nowhere: the text came back unchanged.
    #[test]
    fn a_paste_goes_into_the_box_with_the_keys() {
        #[derive(Debug, Clone)]
        enum Typed {
            Text(String),
        }
        let other = iced::widget::text_input("", "Rent").id("other").on_input(Typed::Text);
        let input = iced::widget::text_input("", "Groceries").id("box").on_input(Typed::Text);
        let mut ui = iced_test::simulator(view(iced::widget::column![other, input]));
        // Focused, with the cursor at its end.
        let _ = ui.click(iced::widget::Id::new("box"));
        PENDING.with(|p| *p.borrow_mut() = Some((" & Household".into(), now_ms())));
        let _ = ui.tap_key(iced::keyboard::key::Named::End);
        let typed: Vec<String> = ui.into_messages().map(|Typed::Text(t)| t).collect();
        assert!(typed.contains(&"Groceries & Household".to_string()), "{typed:?}");
        assert!(!typed.iter().any(|t| t.starts_with("Rent")), "the other box is left alone: {typed:?}");
        assert!(take_pending().is_none(), "taken once");
    }
}
