//! A page's parts (ui/docs/parts.md, "A screen laid out in Rust") in the
//! DOM, as `arkui::view` draws them in iced: the page's figure, its
//! sections of rows in cards, a row's context menu, a form's fields and
//! pickers, and the sheets and questions over the page.

use std::rc::Rc;

use ark_app::view::{Confirm, Input, ListRow, MeterRow, Part, Picture, Section, View};
use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::{amount, glyph, menu, money, row_drawn, send, Row};

/// `view` drawn: what `#[view]`'s `web(&record)` returns.
pub fn render<M: Clone + 'static>(view: View<M>) -> AnyView {
    match view {
        View::Part(Part::Page { top, content, .. }) => page(View::List(vec![*top, *content])),
        v => html::div().class("ark-column").child(nodes(v)).into_any(),
    }
}

/// `v` with its lists opened out: what lies one after another.
pub(crate) fn flatten<M>(v: View<M>) -> Vec<View<M>> {
    match v {
        View::List(vs) => vs.into_iter().flat_map(flatten).collect(),
        View::Empty => vec![],
        v => vec![v],
    }
}

pub(crate) fn nodes<M: Clone + 'static>(v: View<M>) -> Vec<AnyView> {
    flatten(v).into_iter().filter_map(part).collect()
}

/// A tab's page: a scrolling column, its sheets and prompts over it. Its
/// title is the toolbar's.
pub(crate) fn page<M: Clone + 'static>(content: View<M>) -> AnyView {
    let mut body = vec![];
    let mut over = vec![];
    for v in flatten(content) {
        match v {
            View::Part(p @ (Part::Sheet { .. } | Part::Prompt { .. } | Part::Confirm { .. } | Part::Alert { .. })) => over.push(dialog(p)),
            v => body.extend(nodes(v)),
        }
    }
    html::div()
        .class("ark-page")
        .child((html::div().class("ark-page-column").child(body), over))
        .into_any()
}

fn part<M: Clone + 'static>(v: View<M>) -> Option<AnyView> {
    let View::Part(p) = v else {
        // A recording's `for` and `if` are the emitters'; a real view has none.
        return None;
    };
    Some(match p {
        Part::Hero { label, cents, note } => html::div()
            .class("ark-hero")
            .child((
                html::div().class("ark-hero-label").child(label),
                html::div().class("ark-hero-figure").child(amount(cents.get(), false)),
                html::div().class("ark-hero-note").child(note),
            ))
            .into_any(),
        Part::Summary {
            label,
            cents,
            first,
            first_cents,
            second,
            second_cents,
            trend,
        } => html::div()
            .class("ark-card ark-summary")
            .child((
                html::div().class("ark-caption").child(label),
                html::div().class("ark-headline").child(amount(cents.get(), false)),
                html::div().class("ark-summary-foot").child((
                    html::div()
                        .class("ark-facts")
                        .child((fact(first, first_cents.get()), fact(second, second_cents.get()))),
                    html::div()
                        .class("ark-summary-trend")
                        .child(crate::figures::sparkline(&trend.get(), 160.0, 34.0)),
                )),
            ))
            .into_any(),
        // An inspector's page of cards, as iced's `Part::Detail` is: its
        // cards one under another in the pane's own scroll.
        Part::Detail { content } => html::div().class("ark-inspector-body ark-detail").child(nodes(*content)).into_any(),
        Part::HeaderCard(h) => header_card(*h),
        Part::GoalProgress { fraction, tint, note } => html::div()
            .class("ark-goal-progress")
            .child((
                crate::figures::goal_bar(fraction.get() as f32, crate::figures::Tint::named(&tint.get())),
                html::div().class("ark-caption").child(note.get()),
            ))
            .into_any(),
        Part::Section(s) => section(s),
        Part::Row(r) => list_row(*r),
        Part::MeterRow(r) => meter_row(*r),
        Part::Headline(h) => {
            let ark_app::view::Headline {
                label,
                figure,
                tint,
                caption,
                action,
            } = *h;
            let act = action.map(|(title, glyph, tap)| crate::figures::capsule_button(Some(glyph), title, true, tap));
            crate::figures::headline(&label.get(), figure.get(), crate::figures::Tint::named(&tint.get()), &caption.get(), act)
        }
        Part::NameField {
            label,
            value,
            problem,
            typed,
        } => field(label, value.get(), problem.get(), typed),
        Part::AmountField { label, typed, problem, on } => field(&label.get(), typed.get(), problem.get(), on),
        Part::AmountEntry {
            label,
            typed,
            problem,
            on_typed,
            ..
        } => field(&label.get(), typed.get(), problem.get(), on_typed),
        Part::Chips(c) => html::div().class("ark-chips").child(nodes(*c)).into_any(),
        Part::Chip { label, selected, tap, .. } => chip(label.get(), selected.get(), tap),
        Part::FormSection { header, footer, content } => html::div()
            .class("ark-form-section")
            .child((
                header.map(|h| html::div().class("ark-caption").child(h.get())),
                nodes(*content),
                footer.map(|f| html::div().class("ark-caption").child(f.get())),
            ))
            .into_any(),
        Part::Picker {
            selected,
            pick,
            choices,
            label,
        } => {
            let chosen = selected.get();
            let chips: Vec<AnyView> = flatten(*choices)
                .into_iter()
                .filter_map(|v| match v {
                    View::Part(Part::Choice { title, tag, .. }) => {
                        let tag = tag.get();
                        Some(chip(title.get(), tag == chosen, pick.call(tag)))
                    }
                    _ => None,
                })
                .collect();
            html::div()
                .class("ark-chips")
                .role("radiogroup")
                .attr("aria-label", label)
                .child(chips)
                .into_any()
        }
        Part::Choice { .. } => return None,
        Part::Toggle { title, note, on, change } => {
            let on = on.get();
            html::label()
                .class("ark-toggle")
                .child((
                    crate::wired!(
                        html::input()
                            .attr("type", "checkbox")
                            .prop("checked", on)
                            .attr("checked", (crate::page::writing() && on).then_some("")),
                        crate::page::Wired {
                            disabled: crate::page::writing().then_some(""),
                            off: crate::page::writing().then_some(""),
                            ..Default::default()
                        }
                    )
                    .on(leptos::ev::change, {
                        let change = change.clone();
                        move |_| send(change.call(!on))
                    }),
                    crate::hit(title, Rc::new(move || send(change.call(!on)))),
                    html::span().child((
                        html::span().class("ark-toggle-title").child(title),
                        note.map(|n| html::span().class("ark-caption").child(n.get())),
                    )),
                ))
                .into_any()
        }
        Part::Button { title, enabled, tap } => {
            let enabled = enabled.get();
            crate::wired!(html::button(), crate::tapped(title, &tap))
                .class("ark-button")
                .prop("disabled", !enabled)
                .on(leptos::ev::click, move |_| send(tap.clone()))
                .child(title)
                .into_any()
        }
        // A first run's page (welcome.rs's look): centred, its mark on a
        // tile in the accent, the title large, the sentence quiet.
        Part::FirstRunPage {
            glyph: g,
            title,
            sentence,
            content,
        } => html::div()
            .class("ark-welcome")
            .child(html::div().class("ark-welcome-card").child((
                html::span().class("ark-welcome-mark").child(glyph(&g.get(), 36)),
                html::h1().class("ark-welcome-title").child(title.get()),
                html::p().class("ark-caption ark-welcome-sentence").child(sentence.get()),
                html::div().class("ark-welcome-actions").child(nodes(*content)),
            )))
            .into_any(),
        Part::AddressField {
            label,
            prompt,
            text,
            note,
            problem,
            enabled,
            typed,
            submit,
        } => {
            let value = text.get();
            let problem = problem.get();
            html::label()
                .class("ark-field")
                .child((
                    html::span().class("ark-caption").child(label),
                    crate::wired!(html::input(), {
                        let typed = typed.clone();
                        crate::page::text(label, &value, Rc::new(move |s| send(typed.call(s))))
                    })
                    .class("ark-input")
                    .attr("type", "url")
                    .attr("inputmode", "url")
                    .attr("autocapitalize", "none")
                    .attr("autocorrect", "off")
                    .attr("spellcheck", "false")
                    .attr("placeholder", prompt.get())
                    .prop("disabled", !enabled.get())
                    .prop("value", value)
                    .on(leptos::ev::input, move |e| send(typed.call(event_target_value(&e))))
                    .on(leptos::ev::keydown, move |e: web_sys::KeyboardEvent| {
                        if e.key() == "Enter" {
                            send(submit.clone())
                        }
                    }),
                    match problem {
                        Some(p) => html::span().class("ark-problem").attr("role", "alert").child(p).into_any(),
                        None => html::span().class("ark-caption").child(note.get()).into_any(),
                    },
                ))
                .into_any()
        }
        Part::WideButton {
            title,
            prominent,
            enabled,
            busy,
            tap,
        } => {
            let title = title.get();
            let busy = busy.get();
            crate::wired!(html::button(), crate::tapped(&title, &tap))
                .class(if prominent {
                    "ark-push ark-default ark-wide"
                } else {
                    "ark-push ark-wide"
                })
                .prop("disabled", !enabled.get() || busy)
                .attr("aria-busy", busy.then_some("true"))
                .on(leptos::ev::click, move |_| send(tap.clone()))
                .child(title)
                .into_any()
        }
        Part::StatusLine { text, busy, problem } => html::div()
            .class("ark-status")
            .child((
                text.get().map(|t| {
                    html::p()
                        .class("ark-caption ark-welcome-status")
                        .attr("role", "status")
                        .attr("aria-busy", busy.get().then_some("true"))
                        .child(t)
                }),
                problem
                    .get()
                    .map(|p| html::p().class("ark-problem ark-welcome-error").attr("role", "alert").child(p)),
            ))
            .into_any(),
        Part::CodeImage { size, dark } => crate::figures::qr_svg(size.get().max(0) as usize, &dark.get()),
        Part::BarItem {
            title,
            glyph: g,
            enabled,
            tap,
            ..
        } => crate::shell::bar_button(title, g, enabled.get(), tap),
        Part::Writing { markdown } => crate::writing::markdown(&markdown.get()),
        p @ (Part::Sheet { .. } | Part::Prompt { .. } | Part::Confirm { .. } | Part::Alert { .. }) => dialog(p),
        // The shell's are `shell`'s.
        _ => return None,
    })
}

/// What an inspector is about, in a card (iced's `header_card`): its
/// glyph, its title, its figure in a status badge or large in its tint,
/// the line in its tint, the caption quiet, centred; its actions under a
/// rule.
fn header_card<M: Clone + 'static>(h: ark_app::view::HeaderCard<M>) -> AnyView {
    use crate::figures::{status_badge, Tint};
    let tint = Tint::named(&h.tint.get());
    let title = h.title.get();
    let figure = match h.badge {
        true => status_badge(h.figure.get(), tint),
        false => html::div()
            .class(format!("ark-headline {}", tint.class()))
            .child(h.figure.get())
            .into_any(),
    };
    let said = |v: Option<ark_app::view::Val<String>>| v.map(|v| v.get()).filter(|v| !v.is_empty());
    let line = said(h.line).map(|l| html::div().class(format!("ark-header-line {}", tint.class())).child(l));
    let caption = said(h.caption).map(|c| html::div().class("ark-caption").child(c));
    let actions = nodes(*h.actions);
    let actions = (!actions.is_empty()).then(|| {
        (
            html::div().class("ark-row-divider"),
            html::div().class("ark-header-actions").child(actions),
        )
    });
    html::div()
        .class("ark-card ark-header-card")
        .child((
            html::div().class("ark-header-face").child((
                h.glyph.map(|g| tile(&g.get(), "ark-tile ark-tile-large")),
                (!title.is_empty()).then(|| html::div().class("ark-header-title").child(title)),
                figure,
                line,
                caption,
            )),
            actions,
        ))
        .into_any()
}

fn fact(label: &'static str, cents: i64) -> AnyView {
    html::div()
        .class("ark-fact")
        .child((html::div().class("ark-caption").child(label), amount(cents, false)))
        .into_any()
}

/// A choice in a wrapping row, lit when chosen.
pub fn chip<M: Clone + 'static>(label: String, selected: bool, tap: M) -> AnyView {
    crate::wired!(html::button(), crate::tapped(&label, &tap))
        .class(if selected { "ark-chip ark-on" } else { "ark-chip" })
        .attr("aria-pressed", if selected { "true" } else { "false" })
        .on(leptos::ev::click, move |_| send(tap.clone()))
        .child(label)
        .into_any()
}

fn section<M: Clone + 'static>(s: Section<M>) -> AnyView {
    let shows = s.shows.as_ref().is_none_or(|b| b.get());
    let total = match s.note.as_ref().and_then(|n| n.get()) {
        Some(words) => Some(html::span().class("ark-caution-words").child(words).into_any()),
        None => s.amount.filter(|_| shows).map(|a| {
            let c = a.get();
            amount(c, c >= 0)
        }),
    };
    // The figures in their columns, each under its title (hidden when compact).
    let trailing = match &s.columns {
        Some(k) => {
            let cell = |title: &'static str, figure: Option<AnyView>, class: &'static str| {
                html::span()
                    .class(class)
                    .child((html::span().class("ark-figure-col-title").child(title), figure))
                    .into_any()
            };
            Some(
                html::span()
                    .class("ark-figure-cols")
                    .child((
                        cell(k.titles[0], Some(amount(k.first.get(), true)), "ark-figure-col ark-unless-compact"),
                        cell(k.titles[1], Some(amount(k.second.get(), true)), "ark-figure-col ark-unless-compact"),
                        cell(k.titles[2], total, "ark-figure-col"),
                    ))
                    .into_any(),
            )
        }
        None => total,
    };
    let open = s.open.as_ref().is_none_or(|(o, _)| o.get());
    let toggle = s.open.as_ref().map(|(_, t)| t.clone());
    let entries = menu::of_row(&s.menu);
    let head = s.title.map(|t| {
        let title = t.get();
        let chevron = toggle.is_some().then(|| {
            html::span()
                .class("ark-chevron")
                .child(glyph(if open { "chevron-down" } else { "chevron-right" }, 12))
        });
        let (t2, e2) = (title.clone(), entries.clone());
        html::div()
            .class(if toggle.is_some() {
                "ark-section-header ark-toggles"
            } else {
                "ark-section-header"
            })
            .attr("aria-expanded", toggle.is_some().then(|| open.to_string()))
            .on(leptos::ev::click, move |_| {
                if let Some(t) = &toggle {
                    send(t.clone())
                }
            })
            .on(leptos::ev::contextmenu, move |e: web_sys::MouseEvent| {
                if !e2.is_empty() {
                    e.prevent_default();
                    menu::open(
                        t2.clone(),
                        e2.clone(),
                        (e.client_x() as f64, e.client_y() as f64),
                        menu::Anchor::Pointer,
                        false,
                    );
                }
            })
            .child((html::span().child((chevron, title)), trailing))
    });
    let rows = if open { nodes(*s.rows) } else { vec![] };
    let n = rows.len();
    let rows: Vec<AnyView> = rows
        .into_iter()
        .enumerate()
        .flat_map(|(i, r)| {
            let mut v = vec![r];
            if i + 1 < n {
                v.push(html::div().class("ark-row-divider").into_any());
            }
            v
        })
        .collect();
    let foot = s.footer.and_then(|f| f.get()).map(|n| html::div().class("ark-section-footer").child(n));
    html::section()
        .class("ark-section")
        .child((head, html::div().class("ark-card ark-group").child(rows), foot))
        .into_any()
}

/// ArkUI's `ListRow`: its picture, its words, its count, its amount, a
/// chevron when it pushes; pressed, its message; right-clicked (or the
/// menu key, Shift-F10 on its cursor), its menu at the pointer.
pub fn list_row<M: Clone + 'static>(r: ListRow<M>) -> AnyView {
    let title = r.title.get();
    let entries = menu::of_row(&r.menu);
    let open: Option<crate::Action> = r.tap.clone().map(|t| Rc::new(move || send(t.clone())) as crate::Action);
    let opener: Option<crate::Opener> = (!entries.is_empty()).then(|| {
        let title = title.clone();
        let entries = entries.clone();
        Rc::new(move |x: f64, y: f64| menu::open(title.clone(), entries.clone(), (x, y), menu::Anchor::Pointer, false)) as crate::Opener
    });
    if let Some(o) = &opener {
        let (t, o) = (title.clone(), o.clone());
        crate::drawn(|d| d.contexts.push((t, o)));
    }
    let (_, lit) = row_drawn(Row {
        title: title.clone(),
        open: open.clone(),
        menu: opener.clone(),
    });
    let picture = match &r.picture {
        Picture::None => None,
        Picture::Tile(g) => Some(tile(g, "ark-tile")),
        Picture::Mark { glyph: g, .. } => Some(tile(&g.get(), "ark-tile ark-tile-accent")),
        Picture::Glyph(g) => Some(html::span().class("ark-row-glyph").child(glyph(g, 16)).into_any()),
    };
    let words = html::div().class("ark-row-words").child((
        html::div().class("ark-row-title").child(title.clone()),
        r.subtitle.map(|s| html::div().class("ark-row-subtitle").child(s.get())),
    ));
    let badge = r
        .badge
        .and_then(|b| b.get())
        .map(|n| html::span().class("ark-badge").child(n.to_string()));
    let figure = r.amount.map(|a| amount(a.get(), r.quiet));
    // Words where an amount would be, quiet: a goal, a date, a status.
    let value = r.value.map(|v| html::span().class("ark-row-value").child(v.get()));
    let chevron = r.pushes.then(|| html::span().class("ark-chevron").child(glyph("chevron-right", 14)));
    let mut class = String::from("ark-row");
    if r.accent {
        class.push_str(" ark-accent-row");
    }
    if lit {
        class.push_str(" ark-cursor");
    }
    let menu_at = opener.clone();
    let wired = match &open {
        Some(o) => crate::page::tap(&title, o.clone()),
        None => crate::page::Wired::default(),
    };
    crate::wired!(html::button(), wired)
        .class(class)
        .attr("data-cursor", lit.then_some("true"))
        .attr("data-title", title.clone())
        .attr("aria-label", r.said.map(|s| s.get()).unwrap_or(title))
        .on(leptos::ev::click, move |_| {
            if let Some(o) = &open {
                o()
            }
        })
        .on(leptos::ev::contextmenu, move |e: web_sys::MouseEvent| {
            if let Some(o) = &menu_at {
                e.prevent_default();
                o(e.client_x() as f64, e.client_y() as f64);
            }
        })
        .child((picture, words, badge, figure, value, chevron))
        .into_any()
}

/// ArkUI's `MeterRow`: its mark and title over the slim bar and its words,
/// two figures in columns (when not compact), the status badge at its end.
pub fn meter_row<M: Clone + 'static>(r: MeterRow<M>) -> AnyView {
    let title = r.title.get();
    let entries = menu::of_row(&r.menu);
    let open: Option<crate::Action> = r.tap.clone().map(|t| Rc::new(move || send(t.clone())) as crate::Action);
    let opener: Option<crate::Opener> = (!entries.is_empty()).then(|| {
        let title = title.clone();
        let entries = entries.clone();
        Rc::new(move |x: f64, y: f64| menu::open(title.clone(), entries.clone(), (x, y), menu::Anchor::Pointer, false)) as crate::Opener
    });
    if let Some(o) = &opener {
        let (t, o) = (title.clone(), o.clone());
        crate::drawn(|d| d.contexts.push((t, o)));
    }
    let (_, lit) = row_drawn(Row {
        title: title.clone(),
        open: open.clone(),
        menu: opener.clone(),
    });
    let faded = r.faded.as_ref().is_some_and(|f| f.get());
    let mark = r
        .mark
        .as_ref()
        .and_then(|m| m.get())
        .map(|k| tile(if k == "credit" { "credit-card" } else { &k }, "ark-tile ark-tile-small ark-tile-accent"));
    let bar = match (r.fraction.as_ref().and_then(|f| f.get()), &r.meter_tint) {
        (Some(f), Some(t)) => Some(crate::figures::goal_bar(f as f32, crate::figures::Tint::named(&t.get()))),
        _ => None,
    };
    let note = r.note.as_ref().and_then(|n| n.get());
    let end = r.note_end.as_ref().and_then(|n| n.get());
    let tint = crate::figures::Tint::named(&r.note_tint.as_ref().map(|t| t.get()).unwrap_or_default());
    let notes = (note.is_some() || end.is_some()).then(|| {
        html::div().class("ark-meter-notes").child((
            note.map(|n| html::span().class(format!("ark-meter-note {}", tint.class())).child(n)),
            end.map(|e| html::span().class("ark-quiet").child(e)),
        ))
    });
    let words = html::div().class("ark-row-words").child((
        html::div()
            .class(if faded { "ark-row-title ark-quiet" } else { "ark-row-title" })
            .child((mark, title.clone())),
        bar,
        notes,
    ));
    let columns = r.columns.map(|(a, b)| {
        (
            html::span().class("ark-figure-col ark-unless-compact").child(amount(a.get(), false)),
            html::span().class("ark-figure-col ark-unless-compact").child(amount(b.get(), true)),
        )
    });
    let badge = html::span().class("ark-figure-col").child(crate::figures::status_badge(
        r.status.get(),
        crate::figures::Tint::named(&r.status_tint.get()),
    ));
    let menu_at = opener.clone();
    let wired = match &open {
        Some(o) => crate::page::tap(&title, o.clone()),
        None => crate::page::Wired::default(),
    };
    crate::wired!(html::button(), wired)
        .class(if lit {
            "ark-row ark-meter-row ark-cursor"
        } else {
            "ark-row ark-meter-row"
        })
        .attr("data-cursor", lit.then_some("true"))
        .attr("data-title", title.clone())
        .attr("aria-label", r.said.map(|s| s.get()).unwrap_or(title))
        .on(leptos::ev::click, move |_| {
            if let Some(o) = &open {
                o()
            }
        })
        .on(leptos::ev::contextmenu, move |e: web_sys::MouseEvent| {
            if let Some(o) = &menu_at {
                e.prevent_default();
                o(e.client_x() as f64, e.client_y() as f64);
            }
        })
        .child((words, columns, badge))
        .into_any()
}

/// A glyph on a rounded square.
pub fn tile(g: &str, class: &'static str) -> AnyView {
    html::span().class(class).child(glyph(g, 16)).into_any()
}

/// A labelled text field and the problem with what is in it.
pub fn field<M: Clone + 'static>(label: &str, value: String, problem: Option<String>, typed: Input<String, M>) -> AnyView {
    html::label()
        .class("ark-field")
        .child((
            html::span().class("ark-caption").child(label.to_string()),
            crate::wired!(html::input(), {
                let typed = typed.clone();
                crate::page::text(label, &value, Rc::new(move |s| send(typed.call(s))))
            })
            .class("ark-input")
            .attr("placeholder", label.to_string())
            .prop("value", value)
            .on(leptos::ev::input, move |e| send(typed.call(event_target_value(&e)))),
            problem.map(|p| html::span().class("ark-problem").child(p)),
        ))
        .into_any()
}

/// A sheet, a prompt, a question or a notice over the page, centred (a
/// modal belongs to the window, motion.md), the page dimmed under it;
/// pressing outside or Esc is its dismissal.
pub(crate) fn dialog<M: Clone + 'static>(p: Part<M>) -> AnyView {
    let (dismiss, title, body, buttons): (M, String, Vec<AnyView>, Vec<AnyView>) = match p {
        Part::Sheet {
            title,
            dismiss,
            confirm,
            content,
        } => {
            let mut b = vec![push_button("Cancel", true, false, dismiss.clone())];
            if let Some(Confirm { title, enabled, tap }) = confirm {
                b.push(push_button(title, enabled.get(), true, tap));
            }
            (dismiss, title.to_string(), nodes(*content), b)
        }
        Part::Prompt {
            title,
            text,
            typed,
            confirm,
            ok,
            cancel,
        } => {
            let submit = ok.clone();
            let now = text.get();
            let wired = {
                let typed = typed.clone();
                crate::page::text(title, &now, Rc::new(move |s| send(typed.call(s))))
            };
            let input = crate::wired!(html::input(), wired)
                .class("ark-input")
                .attr("autofocus", "")
                .prop("value", text.get())
                .on(leptos::ev::input, move |e| send(typed.call(event_target_value(&e))))
                .on(leptos::ev::keydown, move |e: web_sys::KeyboardEvent| {
                    if e.key() == "Enter" {
                        send(submit.clone())
                    }
                })
                .into_any();
            (
                cancel.clone(),
                title.to_string(),
                vec![input],
                vec![push_button("Cancel", true, false, cancel), push_button(confirm, true, true, ok)],
            )
        }
        Part::Confirm {
            title,
            message,
            actions,
            cancel,
        } => {
            let mut b = vec![push_button("Cancel", true, false, cancel.clone())];
            for a in actions {
                let class = if a.destructive {
                    "ark-push ark-destructive"
                } else {
                    "ark-push ark-default"
                };
                let tap = a.tap;
                let words = a.title.get();
                b.push(
                    crate::wired!(html::button(), crate::tapped(&words, &tap))
                        .class(class)
                        .on(leptos::ev::click, move |_| send(tap.clone()))
                        .child(a.title.get())
                        .into_any(),
                );
            }
            (
                cancel,
                title.get(),
                vec![html::p().class("ark-dialog-message").child(message.get()).into_any()],
                b,
            )
        }
        Part::Alert { title, message, ok } => (
            ok.clone(),
            title.get(),
            vec![html::p().class("ark-dialog-message").child(message.get()).into_any()],
            vec![push_button("OK", true, true, ok)],
        ),
        _ => return ().into_any(),
    };
    modal(title, body, buttons, Rc::new(move || send(dismiss.clone())))
}

/// A modal panel: its title, its body, its buttons at its foot.
pub fn modal(title: String, body: Vec<AnyView>, buttons: Vec<AnyView>, dismiss: crate::Action) -> AnyView {
    let away = dismiss.clone();
    let esc = dismiss.clone();
    // In a specimen it is drawn in its frame: the window's keys stay the
    // window's, and it is there as written, so nothing moves in.
    let here = crate::in_specimen();
    if !here {
        crate::drawn(|d| d.modal = Some(dismiss.clone()));
    }
    // Compact, a sheet from the foot (the iPhone's); else centred, as a
    // modal belongs to the window (motion.md).
    let compact = crate::shell::size_class() == crate::shell::SizeClass::Compact;
    let motion = match (here, compact) {
        (true, _) => "none",
        (false, true) => "rise",
        (false, false) => "grow",
    };
    html::div()
        .class(if here { "ark-modal-layer ark-in-place" } else { "ark-modal-layer" })
        .child((
            html::div().class("ark-scrim").on(leptos::ev::mousedown, move |_| away()),
            html::div()
                .class(if compact { "ark-dialog ark-dialog-sheet" } else { "ark-dialog" })
                .role("dialog")
                .attr("aria-modal", (!here).then_some("true"))
                .attr("aria-label", title.clone())
                .attr("data-motion", motion)
                .on(leptos::ev::keydown, move |e: web_sys::KeyboardEvent| {
                    if e.key() == "Escape" {
                        e.stop_propagation();
                        esc();
                    }
                })
                .child((
                    html::h2().class("ark-dialog-title").child(title),
                    html::div().class("ark-dialog-body").child(body),
                    html::div().class("ark-dialog-buttons").child(buttons),
                )),
        ))
        .into_any()
}

/// A dialog's button: the default one filled in the accent.
pub fn push_button<M: Clone + 'static>(title: &'static str, enabled: bool, default: bool, tap: M) -> AnyView {
    crate::wired!(html::button(), crate::tapped(title, &tap))
        .class(if default { "ark-push ark-default" } else { "ark-push" })
        .prop("disabled", !enabled)
        .on(leptos::ev::click, move |_| send(tap.clone()))
        .child(title)
        .into_any()
}

/// The element an event was on.
pub fn target_element(e: &web_sys::Event) -> Option<web_sys::Element> {
    e.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok())
}

/// An amount's words, for a part that writes its own.
pub fn words(cents: i64) -> String {
    money(cents)
}
