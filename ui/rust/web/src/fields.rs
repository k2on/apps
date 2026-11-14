//! A form's fields in the DOM, and the popovers they open: a search
//! field and its suggestions (as SwiftUI's `.searchSuggestions`, a list
//! under the field), a date field and its calendar (the Mac's graphical
//! `DatePicker` in a popover), a search picker and its searchable list (an
//! ARIA combobox: anchored under its field on a wide window, a sheet from
//! the foot on a compact one), a text field.
//!
//! A popover belongs to its control: it hangs under it, a press outside
//! or `Esc` puts it away, and the keys walk it — `↑`/`↓` a suggestion,
//! `Enter` takes it; the arrows a day (`←`/`→` a day, `↑`/`↓` a week),
//! `PageUp`/`PageDown` a month, `Enter` picks. Opened, it is there at
//! once (motion.md: a control's popover is opened tens of times a day).

use std::rc::Rc;

use leptos::html;
use leptos::prelude::*;

use crate::{glyph, ui, ui_mut};

/// A field's text, sent as it is typed.
pub type Typed = Rc<dyn Fn(String)>;

/// What a field suggests: `(field, typed)` to its suggestions.
type Suggester = Rc<dyn Fn(&str, &str) -> Vec<String>>;

thread_local! {
    static SUGGESTER: std::cell::RefCell<Option<Suggester>> = const { std::cell::RefCell::new(None) };
    static ASKING: std::cell::RefCell<&'static str> = const { std::cell::RefCell::new("") };
}

/// What a field suggests for what is typed: the app's ([`crate::WebApp::suggest`]).
pub(crate) fn suggest_with(f: impl Fn(&str, &str) -> Vec<String> + 'static) {
    SUGGESTER.with(|s| *s.borrow_mut() = Some(Rc::new(f)));
}

fn suggestions_for(id: &str, q: &str) -> Vec<String> {
    match SUGGESTER.with(|s| s.borrow().clone()) {
        Some(f) if !q.trim().is_empty() => f(id, q),
        _ => vec![],
    }
}

/// A search field: its glyph, what is typed, a way to clear it; under it,
/// while it has the keys and something is typed, what the app suggests.
pub fn search_field(prompt: &'static str, value: String, typed: Typed) -> AnyView {
    suggesting("search", prompt, value, typed, true)
}

/// A text field that suggests as it is typed into (a payee): the search
/// field's list, under a form's field.
pub fn suggest_field(id: &'static str, label: &'static str, value: String, typed: Typed) -> AnyView {
    html::label()
        .class("ark-field")
        .child((html::span().class("ark-caption").child(label), suggesting(id, label, value, typed, false)))
        .into_any()
}

fn suggesting(id: &'static str, prompt: &'static str, value: String, typed: Typed, is_search: bool) -> AnyView {
    #[allow(non_snake_case)]
    let ID: &'static str = id;
    let open = ui(|u| u.popover.as_ref().filter(|(id, _)| id == ID).map(|(_, lit)| *lit));
    let found = if open.is_some() { suggestions_for(ID, &value) } else { vec![] };
    let lit = open.unwrap_or(-1);
    let keys = {
        let typed = typed.clone();
        let value = value.clone();
        move |e: web_sys::KeyboardEvent| {
            let found = suggestions_for(ID, &value);
            let n = found.len() as i64;
            match e.key().as_str() {
                "ArrowDown" | "ArrowUp" if n > 0 => {
                    e.prevent_default();
                    let d = if e.key() == "ArrowDown" { 1 } else { -1 };
                    ui_mut(|u| {
                        let at = u.popover.as_ref().filter(|(id, _)| id == ID).map_or(-1, |p| p.1);
                        u.popover = Some((ID.into(), (at + d).clamp(0, n - 1)));
                    });
                }
                "Enter" => {
                    let at = ui(|u| u.popover.as_ref().filter(|(id, _)| id == ID).map_or(-1, |p| p.1));
                    if let Some(s) = (at >= 0).then(|| found.get(at as usize)).flatten() {
                        typed(s.clone());
                    }
                    ui_mut(|u| u.popover = None);
                    blur(&e);
                }
                "Escape" => {
                    e.stop_propagation();
                    if ui(|u| u.popover.is_some()) {
                        ui_mut(|u| u.popover = None);
                    } else {
                        blur(&e);
                    }
                }
                _ => {}
            }
        }
    };
    let input = {
        let typed = typed.clone();
        crate::wired!(html::input(), crate::page::text(prompt, &value, typed.clone()))
            .class(if is_search { "ark-search-input" } else { "ark-input" })
            .attr("type", if is_search { "search" } else { "text" })
            .attr("placeholder", prompt)
            .attr("aria-label", prompt)
            .attr("data-search", is_search.then_some("true"))
            .attr("autocomplete", "off")
            .prop("value", value.clone())
            .on(leptos::ev::input, move |e| {
                typed(event_target_value(&e));
                ui_mut(|u| u.popover = Some((ID.into(), -1)));
            })
            .on(leptos::ev::focus, |_| ui_mut(|u| u.popover = Some((ID.into(), -1))))
            .on(leptos::ev::blur, move |_| {
                // After a suggestion's own press, which comes first.
                let cb = wasm_bindgen::closure::Closure::once_into_js(move || {
                    if ui(|u| u.popover.as_ref().is_some_and(|p| p.0 == ID)) {
                        ui_mut(|u| u.popover = None)
                    }
                });
                if let Some(w) = web_sys::window() {
                    use wasm_bindgen::JsCast;
                    let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), 120);
                }
            })
            .on(leptos::ev::keydown, keys)
    };
    let clear = (is_search && !value.is_empty()).then(|| {
        let typed = typed.clone();
        html::button()
            .class("ark-search-clear")
            .attr("aria-label", "Clear")
            .on(leptos::ev::click, move |_| typed(String::new()))
            .child(glyph("x", 12))
    });
    let list = (!found.is_empty()).then(|| {
        let rows: Vec<AnyView> = found
            .into_iter()
            .enumerate()
            .map(|(i, s)| {
                let typed = typed.clone();
                let pick = s.clone();
                html::div()
                    .class(if i as i64 == lit { "ark-suggestion ark-lit" } else { "ark-suggestion" })
                    .role("option")
                    .on(leptos::ev::mousedown, move |e: web_sys::MouseEvent| {
                        e.prevent_default();
                        typed(pick.clone());
                        ui_mut(|u| u.popover = None);
                    })
                    .child((is_search.then(|| glyph("search", 13)), html::span().child(s)))
                    .into_any()
            })
            .collect();
        html::div().class("ark-suggestions").role("listbox").child(rows)
    });
    html::div()
        .class(if is_search { "ark-search" } else { "ark-search ark-plain" })
        .child((is_search.then(|| glyph("search", 14)), input, clear, list))
        .into_any()
}

fn blur(e: &web_sys::KeyboardEvent) {
    use wasm_bindgen::JsCast;
    if let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
        let _ = el.blur();
    }
}

/// A plain text field.
pub fn text_field(label: &str, value: String, typed: Typed) -> AnyView {
    html::label()
        .class("ark-field")
        .child((
            html::span().class("ark-caption").child(label.to_string()),
            crate::wired!(html::input(), crate::page::text(label, &value, typed.clone()))
                .class("ark-input")
                .attr("placeholder", label.to_string())
                .prop("value", value)
                .on(leptos::ev::input, move |e| typed(event_target_value(&e))),
        ))
        .into_any()
}

// * Days, as a calendar counts them

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { yoe + era * 400 + 1 } else { yoe + era * 400 }, m, d)
}

fn parse(day: &str) -> Option<i64> {
    let mut p = day.split('-').map(|s| s.parse::<i64>().ok());
    let (y, m, d) = (p.next()??, p.next()??, p.next()??);
    Some(days_from_civil(y, m, d))
}

fn iso(days: i64) -> String {
    let (y, m, d) = civil(days);
    format!("{y:04}-{m:02}-{d:02}")
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// A day in a form: the day written (`Nov 10, 2026`), and pressed, a
/// month's calendar under it to pick another. `id` names its popover
/// (`?popover=` opens it).
pub fn date_field(id: &'static str, label: &str, day: String, pick: Typed) -> AnyView {
    let today = parse(&day).unwrap_or(0);
    let open = ui(|u| u.popover.as_ref().filter(|(p, _)| p == id).map(|(_, d)| *d));
    // Without script, the browser's own day field in its place (`.ark-nojs`).
    let nojs = (crate::page::mode() != crate::page::Mode::Spa).then(|| {
        crate::wired!(html::input(), crate::page::text(label, &day, pick.clone()))
            .class("ark-input ark-nojs")
            .attr("type", "date")
            .attr("aria-label", label.to_string())
    });
    let face = html::button()
        .class(match crate::page::mode() {
            crate::page::Mode::Spa => "ark-date-button",
            _ => "ark-date-button ark-js",
        })
        .attr("data-popover", id)
        .attr("aria-haspopup", "dialog")
        .on(leptos::ev::click, move |_| {
            ui_mut(|u| {
                u.popover = match &u.popover {
                    Some((p, _)) if p == id => None,
                    _ => Some((id.into(), today)),
                }
            })
        })
        .child((ark_format::day_long(&day), glyph("chevron-down", 12)));
    let calendar = open.map(|at| calendar(id, today, if at == 0 { today } else { at }, pick.clone()));
    html::div()
        .class("ark-field")
        .child((
            html::span().class("ark-caption").child(label.to_string()),
            html::div().class("ark-popover-anchor").child((face, calendar, nojs)),
        ))
        .into_any()
}

/// Opens the popover `id` on the day it shows, for `?popover=`.
pub fn open_popover(id: &str, at: i64) {
    ui_mut(|u| u.popover = Some((id.into(), at)));
}

fn calendar(id: &'static str, chosen: i64, at: i64, pick: Typed) -> AnyView {
    let (y, m, _) = civil(at);
    let first = days_from_civil(y, m, 1);
    let next = if m == 12 {
        days_from_civil(y + 1, 1, 1)
    } else {
        days_from_civil(y, m + 1, 1)
    };
    // Sunday first, as the US calendar is.
    let lead = (first + 4).rem_euclid(7);
    let step = move |d: i64| ui_mut(|u| u.popover = Some((id.into(), d)));
    let month_by = move |n: i64| {
        let (yy, mm, dd) = civil(at);
        let (yy, mm) = {
            let k = yy * 12 + (mm - 1) + n;
            (k.div_euclid(12), k.rem_euclid(12) + 1)
        };
        let last = {
            let nx = if mm == 12 {
                days_from_civil(yy + 1, 1, 1)
            } else {
                days_from_civil(yy, mm + 1, 1)
            };
            nx - days_from_civil(yy, mm, 1)
        };
        days_from_civil(yy, mm, dd.min(last))
    };
    let mut cells: Vec<AnyView> = ["S", "M", "T", "W", "T", "F", "S"]
        .iter()
        .map(|d| html::span().class("ark-cal-weekday").child(*d).into_any())
        .collect();
    for _ in 0..lead {
        cells.push(html::span().class("ark-cal-blank").into_any());
    }
    for d in first..next {
        let mut class = String::from("ark-cal-day");
        if d == chosen {
            class.push_str(" ark-on");
        }
        if d == at {
            class.push_str(" ark-lit");
        }
        let pick = pick.clone();
        cells.push(
            html::button()
                .class(class)
                .attr("tabindex", "-1")
                .on(leptos::ev::click, move |_| {
                    pick(iso(d));
                    ui_mut(|u| u.popover = None);
                })
                .child(civil(d).2.to_string())
                .into_any(),
        );
    }
    let keys = {
        let pick = pick.clone();
        move |e: web_sys::KeyboardEvent| {
            let to = match e.key().as_str() {
                "ArrowLeft" | "h" => Some(at - 1),
                "ArrowRight" | "l" => Some(at + 1),
                "ArrowUp" | "k" => Some(at - 7),
                "ArrowDown" | "j" => Some(at + 7),
                "PageUp" => Some(month_by(-1)),
                "PageDown" => Some(month_by(1)),
                "Enter" | " " => {
                    pick(iso(at));
                    ui_mut(|u| u.popover = None);
                    None
                }
                "Escape" => {
                    ui_mut(|u| u.popover = None);
                    None
                }
                _ => return,
            };
            e.prevent_default();
            e.stop_propagation();
            if let Some(d) = to {
                step(d);
            }
        }
    };
    let prev = month_by(-1);
    let after = month_by(1);
    html::div()
        .class("ark-popover-layer")
        .child((
            html::div()
                .class("ark-popover-catch")
                .on(leptos::ev::mousedown, |_| ui_mut(|u| u.popover = None)),
            html::div()
                .class("ark-popover ark-calendar")
                .role("dialog")
                .attr("aria-label", "Choose a Day")
                .attr("tabindex", "0")
                .attr("data-autofocus", "true")
                .on(leptos::ev::keydown, keys)
                .child((
                    html::div().class("ark-cal-head").child((
                        html::span().class("ark-cal-month").child(format!("{} {y}", MONTHS[(m - 1) as usize])),
                        html::button()
                            .class("ark-bar-button")
                            .attr("aria-label", "Previous Month")
                            .on(leptos::ev::click, move |_| step(prev))
                            .child(glyph("chevron-left", 14)),
                        html::button()
                            .class("ark-bar-button")
                            .attr("aria-label", "Next Month")
                            .on(leptos::ev::click, move |_| step(after))
                            .child(glyph("chevron-right", 14)),
                    )),
                    html::div().class("ark-cal-grid").child(cells),
                )),
        ))
        .into_any()
}

/// A picker in a form's row (SwiftUI's `Picker` in its menu style): the
/// chosen one's title and a chevron; pressed, a menu of the choices with
/// the chosen one checked.
pub fn picker(id: &'static str, label: &'static str, chosen: String, choices: Vec<(String, crate::Action)>) -> AnyView {
    let face = html::span()
        .class("ark-picker-face")
        .child((html::span().child(chosen.clone()), glyph("chevron-down", 12)))
        .into_any();
    let entries = Rc::new(move || {
        choices
            .iter()
            .map(|(t, a)| crate::menu::Entry::Run {
                glyph: None,
                label: t.clone(),
                destructive: false,
                checked: *t == chosen,
                shortcut: None,
                action: a.clone(),
            })
            .collect()
    });
    html::div()
        .class("ark-field")
        .child((
            html::span().class("ark-caption").child(label),
            crate::menu::button(id, label, face, "ark-picker", entries),
        ))
        .into_any()
}

// * A search picker (`ark_app::view::search_picker`, ui/docs/parts.md)

/// One of many, found by typing: the field says `label` and the chosen
/// one (or `unset` after a question mark in the caution colour); pressed,
/// the list — anchored under the field on a wide window, a sheet from the
/// foot under its title and Close when compact —
/// its search field (`role=combobox`) with the keys, the choices found by
/// their groups (`role=listbox`, `role=group`, `role=option`), the chosen
/// one checked. What is found and the keys are `ark_app::view::search`'s.
/// Opened, it is there at once (a control's popover, tens of times a day).
/// Compact is the shell's size class (`compact-width`), as the phones'.
pub fn search_picker(
    id: &'static str,
    selected: String,
    unset_words: String,
    prompt: &'static str,
    choices: Vec<ark_app::view::search::Choice>,
    pick: Typed,
) -> AnyView {
    use ark_app::view::search::Searching;
    let open = ui(|u| u.combo.as_ref().filter(|(i, _)| i == id).map(|(_, s)| s.clone()));
    let chosen = choices.iter().find(|c| c.tag == selected).cloned();
    let choices = Rc::new(choices);
    let value = chosen.as_ref().map_or(unset_words.clone(), |c| c.title.clone());
    let face_glyph = chosen.as_ref().map_or("circle-help".to_string(), |c| c.glyph.clone());
    let face = {
        let choices = choices.clone();
        let selected = selected.clone();
        html::button()
            .class(if chosen.is_some() {
                "ark-combo-face"
            } else {
                "ark-combo-face ark-unset"
            })
            .attr("type", "button")
            .attr("aria-haspopup", "listbox")
            .attr("aria-expanded", if open.is_some() { "true" } else { "false" })
            .attr("aria-label", format!("{id}, {value}"))
            .on(leptos::ev::click, move |_| {
                ui_mut(|u| {
                    u.combo = match &u.combo {
                        Some((i, _)) if i == id => None,
                        _ => Some((id.into(), Searching::open(&choices, &selected))),
                    }
                })
            })
            .child((
                glyph(&face_glyph, 14),
                html::span().class("ark-combo-value").child(value),
                glyph("chevrons-up-down", 12),
            ))
    };
    let list = open.map(|s| combo_list(id, prompt, choices.clone(), &selected, s, pick));
    html::div()
        .class("ark-field")
        .child((
            html::span().class("ark-caption").child(id),
            html::div().class("ark-popover-anchor ark-combo-anchor").child((face, list)),
        ))
        .into_any()
}

/// The open list: what is typed, and what it finds.
fn combo_list(
    id: &'static str,
    prompt: &'static str,
    choices: Rc<Vec<ark_app::view::search::Choice>>,
    selected: &str,
    s: ark_app::view::search::Searching,
    pick: Typed,
) -> AnyView {
    use ark_app::view::search::{Done, Key};
    let close = || ui_mut(|u| u.combo = None);
    let slug: String = id.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
    let list_id = format!("ark-combo-{slug}");
    let option_id = move |i: usize| format!("ark-combo-{slug}-{i}");
    let choose = {
        let pick = pick.clone();
        Rc::new(move |tag: String| {
            ui_mut(|u| u.combo = None);
            pick(tag);
        })
    };
    let keys = {
        let choices = choices.clone();
        let choose = choose.clone();
        move |e: web_sys::KeyboardEvent| {
            let key = match e.key().as_str() {
                "ArrowDown" => Key::Down,
                "ArrowUp" => Key::Up,
                "Enter" => Key::Choose,
                "Escape" => Key::Close,
                _ => return,
            };
            e.prevent_default();
            e.stop_propagation();
            let done = ui_mut(|u| u.combo.as_mut().and_then(|(_, s)| s.key(&choices, key)));
            match done {
                Some(Done::Chose(tag)) => choose(tag),
                Some(Done::Closed) => ui_mut(|u| u.combo = None),
                None => {}
            }
        }
    };
    let typed = {
        let choices = choices.clone();
        move |e: leptos::ev::Event| {
            let q = event_target_value(&e);
            ui_mut(|u| {
                if let Some((_, s)) = &mut u.combo {
                    s.typed(&choices, q)
                }
            })
        }
    };
    let input = html::input()
        .class("ark-search-input")
        .attr("type", "search")
        .role("combobox")
        .attr("aria-expanded", "true")
        .attr("aria-controls", list_id.clone())
        .attr("aria-autocomplete", "list")
        .attr("aria-activedescendant", s.at.map(option_id.clone()))
        .attr("aria-label", prompt)
        .attr("placeholder", prompt)
        .attr("autocomplete", "off")
        .attr("data-autofocus", "true")
        .prop("value", s.query.clone())
        .on(leptos::ev::input, typed)
        .on(leptos::ev::keydown, keys);
    let groups = s.found(&choices);
    let body: AnyView = match groups.is_empty() {
        true => html::div().class("ark-combo-empty").role("status").child("No Matches").into_any(),
        false => {
            let gs: Vec<AnyView> = groups
                .into_iter()
                .enumerate()
                .map(|(k, g)| {
                    let heading_id = format!("{list_id}-group-{k}");
                    let rows: Vec<AnyView> = g
                        .choices
                        .iter()
                        .map(|&i| {
                            let c = &choices[i];
                            let on = c.tag == selected;
                            let mut class = String::from("ark-combo-option");
                            if s.at == Some(i) {
                                class.push_str(" ark-lit");
                            }
                            let choose = choose.clone();
                            let tag = c.tag.clone();
                            html::div()
                                .class(class)
                                .id(option_id(i))
                                .role("option")
                                .attr("aria-selected", if on { "true" } else { "false" })
                                .attr("data-cursor", (s.at == Some(i)).then_some("true"))
                                .on(leptos::ev::mousedown, move |e: web_sys::MouseEvent| {
                                    e.prevent_default();
                                    choose(tag.clone());
                                })
                                .on(leptos::ev::mouseenter, move |_| {
                                    ui_mut(|u| {
                                        if let Some((_, s)) = &mut u.combo {
                                            s.land(i)
                                        }
                                    })
                                })
                                .child((
                                    glyph(&c.glyph, 14),
                                    html::span().class("ark-combo-title").child(c.title.clone()),
                                    (!c.detail.is_empty()).then(|| {
                                        html::span()
                                            .class(format!(
                                                "ark-amount ark-ink-tint ark-tint-{}",
                                                if c.tint.is_empty() { "neutral" } else { &c.tint }
                                            ))
                                            .child(c.detail.clone())
                                    }),
                                    html::span()
                                        .class(if on { "ark-combo-check ark-on" } else { "ark-combo-check" })
                                        .child(glyph("check", 12)),
                                ))
                                .into_any()
                        })
                        .collect();
                    let named = !g.name.is_empty();
                    html::div()
                        .role("group")
                        .attr("aria-labelledby", named.then(|| heading_id.clone()))
                        .child((
                            named.then(|| {
                                html::div()
                                    .class("ark-combo-heading")
                                    .id(heading_id)
                                    .attr("aria-hidden", "true")
                                    .child(g.name.clone())
                            }),
                            rows,
                        ))
                        .into_any()
                })
                .collect();
            html::div()
                .class("ark-combo-list")
                .id(list_id.clone())
                .role("listbox")
                .attr("aria-label", id)
                .child(gs)
                .into_any()
        }
    };
    let search = html::div().class("ark-search").child((glyph("search", 14), input));
    // Compact, a sheet from the foot under its title and Close, as the
    // phones' is; else anchored under its field. Either is there at once,
    // with nothing moving in: the page is drawn again on every key typed
    // into it, and an entrance would play again with each.
    if crate::shell::size_class() == crate::shell::SizeClass::Compact {
        return html::div()
            .class("ark-modal-layer ark-combo-layer")
            .child((
                html::div().class("ark-scrim").on(leptos::ev::mousedown, move |_| close()),
                html::div()
                    .class("ark-combo ark-combo-sheet")
                    .role("dialog")
                    .attr("aria-label", id)
                    .child((
                        html::div().class("ark-combo-head").child((
                            html::span().class("ark-combo-name").child(id),
                            html::button()
                                .class("ark-bar-button")
                                .attr("type", "button")
                                .attr("aria-label", "Close")
                                .on(leptos::ev::click, move |_| close())
                                .child(glyph("x", 14)),
                        )),
                        search,
                        body,
                    )),
            ))
            .into_any();
    }
    html::div()
        .class("ark-popover-layer ark-combo-layer")
        .child((
            html::div().class("ark-popover-catch").on(leptos::ev::mousedown, move |_| close()),
            html::div()
                .class("ark-popover ark-combo")
                .role("dialog")
                .attr("aria-label", id)
                .child((search, body)),
        ))
        .into_any()
}
