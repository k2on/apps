//! Where a sound is along its length (`Part::Timeline`), counted from its
//! clock as it is drawn, so it moves with no message: a scrubber (an
//! `<input type="range">` between the time gone and the whole) or a
//! hairline. While one plays, an animation frame moves what is drawn of it
//! — its fill, its handle, its time — straight in the DOM, the way
//! SwiftUI's `TimelineView` redraws a view and nothing else; nothing runs
//! while none plays. Dragged, the handle follows the pointer and the time
//! beside it says where; let go, `seek` is sent once.

use std::cell::Cell;

use ark_app::view::Input;
use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::send;

/// The device's clock, as a timeline's `since_ms` is.
fn now_ms() -> i64 {
    match cfg!(target_arch = "wasm32") {
        true => ark_peer::now_ms(),
        false => 0,
    }
}

fn at(at_ms: i64, since_ms: i64, playing: bool, length: i64, now: i64) -> i64 {
    match playing {
        true => at_ms + (now - since_ms).max(0),
        false => at_ms,
    }
    .clamp(0, length)
}

/// `Part::Timeline` drawn.
pub fn timeline<M: Clone + std::fmt::Debug + 'static>(
    at_ms: i64,
    since_ms: i64,
    playing: bool,
    length_ms: i64,
    seek: Option<Input<i64, M>>,
) -> AnyView {
    let length = length_ms.max(1);
    let now = at(at_ms, since_ms, playing, length, now_ms());
    let clock = |data: html::HtmlElement<html::Div, (), ()>| {
        data.attr("data-at", at_ms.to_string())
            .attr("data-since", since_ms.to_string())
            .attr("data-playing", if playing { "1" } else { "0" })
            .attr("data-length", length.to_string())
    };
    if playing {
        run();
    }
    match seek {
        None => clock(html::div())
            .class("ark-timeline ark-hairline")
            .attr("role", "progressbar")
            .attr("aria-valuemin", "0")
            .attr("aria-valuemax", length.to_string())
            .attr("aria-valuenow", now.to_string())
            .child(
                html::div()
                    .class("ark-hairline-fill")
                    .attr("style", format!("width: {}%", fill(now, length))),
            )
            .into_any(),
        Some(seek) => {
            let on_seek = seek.clone();
            clock(html::div())
                .class("ark-timeline ark-scrubber")
                .child((
                    html::span().class("ark-timeline-gone").child(ark_format::clock(now)),
                    html::input()
                        .attr("type", "range")
                        .attr("min", "0")
                        .attr("max", length.to_string())
                        .attr("step", "1000")
                        .attr("aria-label", "Position")
                        .prop("value", now.to_string())
                        .on(leptos::ev::pointerdown, |e| held(&e, true))
                        .on(leptos::ev::input, move |e| {
                            // The time beside it says where the finger is.
                            if let Some((input, gone)) = parts(&e) {
                                let v = input.value().parse::<i64>().unwrap_or(0);
                                gone.set_text_content(Some(&ark_format::clock(v)));
                            }
                        })
                        .on(leptos::ev::change, move |e| {
                            held(&e, false);
                            if let Some((input, _)) = parts(&e) {
                                send(on_seek.call(input.value().parse::<i64>().unwrap_or(0)));
                            }
                        }),
                    html::span().class("ark-timeline-whole").child(ark_format::clock(length)),
                ))
                .into_any()
        }
    }
}

/// `Part::ClockWords` drawn: words lit in the accent while the clock is
/// between `from_ms` and `until_ms`, moved by the same frame loop as a
/// timeline while it plays.
pub fn words(text: String, at_ms: i64, since_ms: i64, playing: bool, from_ms: i64, until_ms: i64) -> AnyView {
    let lit = |at: i64| at >= from_ms && at < until_ms;
    let now = at_ms_now(at_ms, since_ms, playing, now_ms());
    if playing {
        run();
    }
    html::span()
        .class(words_class(lit(now)))
        .attr("data-at", at_ms.to_string())
        .attr("data-since", since_ms.to_string())
        .attr("data-playing", if playing { "1" } else { "0" })
        .attr("data-from", from_ms.to_string())
        .attr("data-until", until_ms.to_string())
        .child(text)
        .into_any()
}

/// Clock words' classes, lit or not: body text in the accent or quiet.
fn words_class(lit: bool) -> &'static str {
    match lit {
        true => "ark-clock-words ark-text ark-type-body ark-ink-accent",
        false => "ark-clock-words ark-text ark-type-body ark-ink-quiet",
    }
}

/// A clock's place at `now`, unbounded (`ark_app::view::clock_at`).
fn at_ms_now(at_ms: i64, since_ms: i64, playing: bool, now: i64) -> i64 {
    ark_app::view::clock_at(at_ms, since_ms, playing, now)
}

fn fill(at: i64, length: i64) -> String {
    format!("{:.3}", at as f64 * 100.0 / length as f64)
}

/// The range and the time gone beside it, of an event on the range.
fn parts(e: &web_sys::Event) -> Option<(web_sys::HtmlInputElement, web_sys::Element)> {
    let input: web_sys::HtmlInputElement = e.target()?.dyn_into().ok()?;
    let gone = input.parent_element()?.query_selector(".ark-timeline-gone").ok()??;
    Some((input, gone))
}

/// A scrubber held by a finger is not moved by its clock.
fn held(e: &web_sys::Event, on: bool) {
    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
        let _ = match on {
            true => el.set_attribute("data-held", "1"),
            false => el.remove_attribute("data-held"),
        };
    }
}

thread_local! {
    /// Whether the frame loop runs.
    static RUNNING: Cell<bool> = const { Cell::new(false) };
}

/// The frame loop, started once for as long as a timeline plays.
fn run() {
    if !cfg!(target_arch = "wasm32") || RUNNING.with(|r| r.replace(true)) {
        return;
    }
    crate::request_frame(tick);
}

/// Every playing timeline moved to where its clock says; the loop goes on
/// while one does.
fn tick() {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        RUNNING.with(|r| r.set(false));
        return;
    };
    let Ok(found) = doc.query_selector_all(".ark-timeline[data-playing=\"1\"]") else {
        RUNNING.with(|r| r.set(false));
        return;
    };
    let now = now_ms();
    let words = doc.query_selector_all(".ark-clock-words[data-playing=\"1\"]").ok();
    for i in 0..words.as_ref().map_or(0, |w| w.length()) {
        let Some(el) = words.as_ref().and_then(|w| w.item(i)).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) else {
            continue;
        };
        let read = |k: &str| el.get_attribute(k).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
        let here = at_ms_now(read("data-at"), read("data-since"), true, now);
        let lit = here >= read("data-from") && here < read("data-until");
        let _ = el.set_attribute("class", words_class(lit));
    }
    let playing_words = words.as_ref().map_or(0, |w| w.length());
    for i in 0..found.length() {
        let Some(el) = found.item(i).and_then(|n| n.dyn_into::<web_sys::Element>().ok()) else {
            continue;
        };
        let read = |k: &str| el.get_attribute(k).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
        let length = read("data-length").max(1);
        let here = at(read("data-at"), read("data-since"), true, length, now);
        if let Ok(Some(f)) = el.query_selector(".ark-hairline-fill") {
            let _ = f.set_attribute("style", &format!("width: {}%", fill(here, length)));
        }
        if let Ok(Some(input)) = el.query_selector("input[type=range]:not([data-held])") {
            if let Ok(input) = input.dyn_into::<web_sys::HtmlInputElement>() {
                input.set_value(&here.to_string());
                if let Ok(Some(gone)) = el.query_selector(".ark-timeline-gone") {
                    gone.set_text_content(Some(&ark_format::clock(here)));
                }
            }
        }
    }
    match found.length() + playing_words {
        0 => RUNNING.with(|r| r.set(false)),
        _ => crate::request_frame(tick),
    }
}
