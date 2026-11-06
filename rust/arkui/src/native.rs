//! The browser's own scroll driving a page's main list, in a browser, when
//! the address asks for it: `?scroll=native`. Without it (`?scroll=arkui`,
//! or nothing) and on the desktop, nothing here runs and every list
//! scrolls as [`crate::scroll::elastic`] scrolls it.
//!
//! A browser's `wheel` event says nothing of the fingers on a trackpad, so
//! arkui's own scroll has to guess when they lift. The browser knows, and
//! scrolls and coasts a page as its platform does; here it is handed the
//! page's main list. The canvas iced draws into is put inside a real
//! scroller — a `div` the size of the window, `overflow: auto`, its
//! scrollbar hidden — over a spacer as tall as the list can scroll, and the
//! canvas itself is `position: sticky; top: 0`, so it never moves and
//! every pointer event still lands on it (the proxy scroller of Glide Data
//! Grid and Google Sheets). A wheel over the list is let through to the
//! scroller ([`listen`]): winit's own `preventDefault` on it is made to do
//! nothing for that one event, so the browser scrolls, and coasts, the
//! scroller. Each frame the list is put where the scroller is
//! ([`crate::scroll::elastic`]), before anything is drawn, and a list moved
//! by iced (a key, `follow`, its scrollbar) moves the scroller with it.
//!
//! Which list is the page's main one is worked out, not said: the largest
//! [`crate::scroll::elastic`] drawn, as of the frame before ([`Frames`]).
//! Every other one, a scrollable inside it, and the wheel anywhere but
//! over it — the sidebar, a menu, a sheet over the page — stays as it was,
//! arkui's. So does a pull past an end, or a band showing: the wheel is
//! kept from the browser and fed to the band as before.
//!
//! The canvas is taken to be drawn at one CSS pixel to iced's logical one
//! (no `scale_factor` of an app's own), which is how both apps run.
use iced::time::Instant;
use iced::{Rectangle, Subscription};

/// Whether an address's query, `search` (`?a=b&c=d`, or nothing), asks the
/// browser to scroll the main list: `scroll=native`. Anything else, an
/// `arkui` included, is arkui's own scroll.
pub fn asks_native(search: &str) -> bool {
    search.trim_start_matches('?').split('&').any(|pair| pair == "scroll=native")
}

/// Whether the browser scrolls the page's main list: in a browser, and only
/// when the address asks (`?scroll=native`). Read once.
pub fn on() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        use std::sync::OnceLock;
        static ON: OnceLock<bool> = OnceLock::new();
        *ON.get_or_init(|| {
            web_sys::window()
                .and_then(|w| w.location().search().ok())
                .is_some_and(|s| asks_native(&s))
        })
    }
    #[cfg(not(target_arch = "wasm32"))]
    false
}

/// What became of the wheel event now being handled, as the page decided
/// it before winit had it ([`listen`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) enum Wheel {
    /// Not over the main list, or not in this mode: arkui's, as ever.
    #[default]
    Elsewhere,
    /// Over the main list, and the browser scrolls it.
    Browser,
    /// Over the main list, but pulling past an end or with a band showing:
    /// kept from the browser, for the band.
    Owned,
}

/// What a scrollable is, the frame it asks ([`Frames::claim`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Claim {
    /// The page's main list, since the `generation`th change of which one
    /// that is.
    Main { generation: u64 },
    /// Another; `soon` if it is the largest so far this frame, and so the
    /// main list from the next.
    Other { soon: bool },
}

/// Which [`crate::scroll::elastic`] is the page's main list: the largest
/// drawn in the frame before. Pure, so it is tested without a browser.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Frames {
    /// The frame now being drawn.
    frame: Option<Instant>,
    /// The largest asked so far this frame: which, and its area.
    best: Option<(usize, f32)>,
    /// The main list, as of the last frame.
    main: Option<usize>,
    /// How many times the main list has changed.
    generation: u64,
}

impl Frames {
    /// The scrollable `key`, `area` in size, drawn in the frame at `now`:
    /// what it is. The first ask of a frame settles the last.
    pub(crate) fn claim(&mut self, key: usize, area: f32, now: Instant) -> Claim {
        if self.frame != Some(now) {
            let winner = self.best.map(|(k, _)| k);
            if winner != self.main {
                self.main = winner;
                self.generation += 1;
            }
            self.frame = Some(now);
            self.best = None;
        }
        if self.best.is_none_or(|(_, a)| area > a) {
            self.best = Some((key, area));
        }
        match self.main == Some(key) {
            true => Claim::Main { generation: self.generation },
            false => Claim::Other {
                soon: self.best.is_some_and(|(k, _)| k == key),
            },
        }
    }
}

/// The page as the main list last left it: where it is, what inside it
/// scrolls itself, whether a band shows, whether the pointer reaches it.
#[derive(Default)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
struct Page {
    frames: Frames,
    /// The main list's bounds, in the window.
    list: Option<Rectangle>,
    /// Scrollables inside it, in the window.
    inner: Vec<Rectangle>,
    /// A band is drawn, or springing back.
    banding: bool,
    /// The pointer is kept from the list: a menu, a sheet or a dialog is
    /// over it.
    covered: bool,
    /// What the page decided of the wheel event now being handled.
    decided: Wheel,
    /// The main list asked for the next frame itself: the scroller moved.
    watching: bool,
    /// Whether motion is reduced: nothing bands.
    reduced: bool,
    #[cfg(target_arch = "wasm32")]
    dom: Option<Dom>,
}

/// The scroller, its spacer, and the canvas inside it.
#[cfg(target_arch = "wasm32")]
struct Dom {
    scroller: web_sys::HtmlElement,
    spacer: web_sys::HtmlElement,
    canvas: web_sys::Element,
}

thread_local! {
    static PAGE: std::cell::RefCell<Page> = std::cell::RefCell::new(Page::default());
}

/// What rings when the browser scrolls the list with nothing else moving
/// (its own coast, a smooth scroll), so the window draws it.
fn bell() -> &'static crate::wake::Bell {
    static BELL: std::sync::OnceLock<crate::wake::Bell> = std::sync::OnceLock::new();
    BELL.get_or_init(crate::wake::Bell::new)
}

/// A message each time the browser scrolls the main list while the window
/// is at rest: for an app to answer with nothing, so it draws again
/// ([`crate::window::wakes`]).
pub(crate) fn wakes() -> Subscription<()> {
    match on() {
        true => bell().subscription(),
        false => Subscription::none(),
    }
}

/// The scrollable `key`, `area` in size, asks at the frame at `now` what it
/// is ([`Frames::claim`]). The page is built the first time ([`listen`]);
/// until it is, every list is arkui's.
pub(crate) fn claim(key: usize, area: f32, now: Instant) -> Option<Claim> {
    if !on() || !listen() {
        return None;
    }
    Some(PAGE.with(|p| p.borrow_mut().frames.claim(key, area, now)))
}

/// What the main list says of itself this frame, for the wheel to come:
/// its bounds, what inside it scrolls itself, whether a band shows.
pub(crate) fn publish(list: Rectangle, inner: Vec<Rectangle>, banding: bool, reduced: bool) {
    PAGE.with(|p| {
        let mut p = p.borrow_mut();
        p.list = Some(list);
        p.inner = inner;
        p.banding = banding;
        p.reduced = reduced;
    });
}

/// Whether the pointer reaches the main list, as its last event said: a
/// menu, a sheet or a dialog over it keeps the wheel arkui's.
pub(crate) fn pointer(free: bool) {
    PAGE.with(|p| p.borrow_mut().covered = !free);
}

/// The main list asked for the next frame itself, because the scroller
/// moved: until it stops, the scroller's own events need not wake it.
pub(crate) fn watching(on: bool) {
    PAGE.with(|p| p.borrow_mut().watching = on);
}

/// What the page decided of the wheel event now being handled.
pub(crate) fn wheel() -> Wheel {
    PAGE.with(|p| p.borrow().decided)
}

/// How far the scroller is scrolled, in CSS pixels.
pub(crate) fn top() -> f32 {
    #[cfg(target_arch = "wasm32")]
    {
        PAGE.with(|p| p.borrow().dom.as_ref().map_or(0.0, |d| d.scroller.scroll_top() as f32))
    }
    #[cfg(not(target_arch = "wasm32"))]
    0.0
}

/// Scroll the scroller to `y`, as far as it goes: where iced moved the
/// list. Where it went.
pub(crate) fn set_top(y: f32) -> f32 {
    #[cfg(target_arch = "wasm32")]
    {
        PAGE.with(|p| {
            p.borrow().dom.as_ref().map_or(0.0, |d| {
                d.scroller.set_scroll_top(y.round() as i32);
                d.scroller.scroll_top() as f32
            })
        })
    }
    #[cfg(not(target_arch = "wasm32"))]
    y
}

/// Make the scroller scroll `room` pixels: the spacer under the canvas is
/// that tall, so the page is the window plus it.
pub(crate) fn fit(room: f32) {
    #[cfg(target_arch = "wasm32")]
    PAGE.with(|p| {
        if let Some(d) = p.borrow().dom.as_ref() {
            let height = format!("{}px", room.max(0.0).round());
            if d.spacer.style().get_property_value("height").ok().as_deref() != Some(height.as_str()) {
                let _ = d.spacer.style().set_property("height", &height);
            }
        }
    });
    #[cfg(not(target_arch = "wasm32"))]
    let _ = room;
}

/// Build the page once, if it can be yet: the scroller around the canvas,
/// the spacer, and the listeners. Whether it is built.
///
/// The wheel is decided as it lands on the scroller, in the capture phase
/// and so before winit's listener on the canvas: over the main list, with
/// no band showing and not pulling past an end, winit's `preventDefault`
/// is made a no-op for that event, and the browser scrolls the scroller.
/// Everything else — elsewhere on the page, a pinch (`ctrlKey`), a band —
/// winit prevents as ever, and arkui has it. The keyboard, the pointer and
/// every other event are winit's, untouched. The scroller's own `scroll`
/// rings [`wakes`] when nothing else is drawing.
fn listen() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        if PAGE.with(|p| p.borrow().dom.is_some()) {
            return true;
        }
        let Some(dom) = build() else {
            return false;
        };
        PAGE.with(|p| p.borrow_mut().dom = Some(dom));
        true
    }
    #[cfg(not(target_arch = "wasm32"))]
    false
}

#[cfg(target_arch = "wasm32")]
fn build() -> Option<Dom> {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    let document = web_sys::window()?.document()?;
    let canvas = document.query_selector("canvas").ok()??;
    let parent = canvas.parent_node()?;
    let make = |tag: &str| document.create_element(tag).ok()?.dyn_into::<web_sys::HtmlElement>().ok();
    let scroller = make("div")?;
    let spacer = make("div")?;
    let sheet = make("style")?;
    scroller.set_class_name("arkui-scroller");
    // Only `-webkit-scrollbar` needs a sheet: the rest is the element's own.
    sheet.set_text_content(Some(".arkui-scroller::-webkit-scrollbar { display: none; }"));
    parent.append_child(&sheet).ok()?;
    let _ = scroller.set_attribute(
        "style",
        "position: fixed; inset: 0; overflow-x: hidden; overflow-y: auto; overscroll-behavior: none; \
         overflow-anchor: none; scrollbar-width: none;",
    );
    let _ = spacer.set_attribute("style", "height: 0px; width: 1px; pointer-events: none;");
    parent.insert_before(&scroller, Some(&canvas)).ok()?;
    scroller.append_child(&canvas).ok()?;
    scroller.append_child(&spacer).ok()?;
    if let Some(c) = canvas.dyn_ref::<web_sys::HtmlElement>() {
        let _ = c.style().set_property("position", "sticky");
        let _ = c.style().set_property("top", "0");
        // Moved, it lost the keys.
        let _ = c.focus();
    }

    let on_wheel = Closure::<dyn FnMut(web_sys::WheelEvent)>::new(|event: web_sys::WheelEvent| {
        let decided = PAGE.with(|p| decide(&p.borrow(), &event));
        if decided == Wheel::Browser {
            // winit calls `preventDefault` on every wheel; for this one it
            // does nothing, and the browser scrolls the scroller.
            let nothing = js_sys::Function::new_no_args("");
            let _ = js_sys::Reflect::set(&event, &"preventDefault".into(), &nothing);
        }
        PAGE.with(|p| p.borrow_mut().decided = decided);
    });
    scroller
        .add_event_listener_with_callback_and_bool("wheel", on_wheel.as_ref().unchecked_ref(), true)
        .ok()?;
    on_wheel.forget();
    let on_scroll = Closure::<dyn FnMut()>::new(|| {
        if !PAGE.with(|p| p.borrow().watching) {
            bell().ring();
        }
    });
    scroller
        .add_event_listener_with_callback("scroll", on_scroll.as_ref().unchecked_ref())
        .ok()?;
    on_scroll.forget();
    Some(Dom { scroller, spacer, canvas })
}

/// What the page makes of a wheel event as it lands ([`listen`]).
#[cfg(target_arch = "wasm32")]
fn decide(page: &Page, event: &web_sys::WheelEvent) -> Wheel {
    let (Some(list), Some(dom)) = (page.list, page.dom.as_ref()) else {
        return Wheel::Elsewhere;
    };
    if page.covered || event.ctrl_key() {
        return Wheel::Elsewhere;
    }
    let at = dom.canvas.get_bounding_client_rect();
    let point = iced::Point::new((event.client_x() as f64 - at.left()) as f32, (event.client_y() as f64 - at.top()) as f32);
    if !list.contains(point) || page.inner.iter().any(|r| r.contains(point)) {
        return Wheel::Elsewhere;
    }
    let scroller = &dom.scroller;
    let top = scroller.scroll_top() as f32;
    let room = (scroller.scroll_height() - scroller.client_height()) as f32;
    let lines = event.delta_mode() != web_sys::WheelEvent::DOM_DELTA_PIXEL;
    let owned = event.cancelable() && !lines && !page.reduced && (page.banding || pulls_past(-event.delta_y() as f32, top, room));
    match owned {
        true => Wheel::Owned,
        false => Wheel::Browser,
    }
}

/// Whether the wheel's `y` (iced's sense: positive toward the top) pulls a
/// scroller at `top` of `room` past an end it is at.
pub(crate) fn pulls_past(y: f32, top: f32, room: f32) -> bool {
    let at = crate::motion_tokens::REST_PX;
    (y > 0.0 && top <= at) || (y < 0.0 && top >= room - at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::time::Duration;

    /// Only `scroll=native` asks; `arkui`, nothing, or another word is the
    /// scroll as it was. Falsified by matching a prefix: `scroll=nativeX`
    /// asked.
    #[test]
    fn only_scroll_native_asks() {
        assert!(asks_native("?scroll=native"));
        assert!(asks_native("?a=1&scroll=native"));
        assert!(!asks_native("?scroll=arkui"));
        assert!(!asks_native(""));
        assert!(!asks_native("?scroll=nativeX"));
    }

    /// The largest list drawn is the main one from the frame after, and a
    /// change of which is a new generation; the one that will be says so,
    /// to ask for that frame. Falsified by keeping the first asked: the
    /// small sidebar, asked first, stayed main.
    #[test]
    fn the_largest_list_is_the_main_one_from_the_next_frame() {
        let mut frames = Frames::default();
        let t = Instant::now();
        assert_eq!(frames.claim(1, 100.0, t), Claim::Other { soon: true });
        assert_eq!(frames.claim(2, 900.0, t), Claim::Other { soon: true });
        let t = t + Duration::from_millis(16);
        assert_eq!(frames.claim(1, 100.0, t), Claim::Other { soon: true });
        assert_eq!(frames.claim(2, 900.0, t), Claim::Main { generation: 1 });
        let t = t + Duration::from_millis(16);
        assert_eq!(frames.claim(1, 100.0, t), Claim::Other { soon: true });
        assert_eq!(
            frames.claim(2, 900.0, t),
            Claim::Main { generation: 1 },
            "the same main, the same generation"
        );
        // The page changed: list 3 takes over.
        let t = t + Duration::from_millis(16);
        assert_eq!(frames.claim(3, 800.0, t), Claim::Other { soon: true });
        let t = t + Duration::from_millis(16);
        assert_eq!(frames.claim(3, 800.0, t), Claim::Main { generation: 2 });
    }

    /// Pulling past an end it is at, either way; not away from it, and not
    /// in the middle.
    #[test]
    fn a_pull_past_an_end_is_seen() {
        assert!(pulls_past(10.0, 0.0, 500.0), "at the top, pulled down");
        assert!(!pulls_past(-10.0, 0.0, 500.0), "at the top, scrolled on");
        assert!(pulls_past(-10.0, 500.0, 500.0), "at the foot, pulled up");
        assert!(!pulls_past(10.0, 250.0, 500.0), "in the middle");
        assert!(pulls_past(10.0, 0.0, 0.0) && pulls_past(-10.0, 0.0, 0.0), "a list that fits");
    }
}
