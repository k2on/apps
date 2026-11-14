use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::shell::*;
use crate::view::*;

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Select(String),
    Open(String),
    New,
    Typed(String),
    Check,
    Cancel,
    Underfunded,
}

/// What was sent, by which screen.
type Sent = Rc<RefCell<Vec<(&'static str, Msg)>>>;

fn screen(name: &'static str, v: View<Msg>, sent: &Sent) -> Screen {
    let s = sent.clone();
    Screen {
        name: name.into(),
        nodes: nodes(&v, Rc::new(move |m| s.borrow_mut().push((name, m)))),
    }
}

/// A shell of two tabs, Budget's hosting the budget screen and Accounts'
/// the accounts', Accounts selected.
fn shell(selected: &str) -> View<Msg> {
    adaptive_tabs(
        selected.to_string(),
        Msg::Select,
        (
            section_tab(
                "Budget",
                "mail",
                "budget",
                toolbar(
                    "Budget",
                    hosted("budget"),
                    bar_item("New Transaction", "plus", Placement::Primary, Msg::New),
                ),
            ),
            section_tab("Accounts", "landmark", "accounts", toolbar("Accounts", hosted("accounts"), ())),
        ),
    )
    .into()
}

fn budget(asking: bool) -> View<Msg> {
    (
        list_row("Groceries").on_tap(Msg::Open("groceries".into())),
        if asking {
            confirmation_dialog("Auto-Assign", "How?", Msg::Cancel)
                .action("Underfunded", false, Msg::Underfunded)
                .into()
        } else {
            View::Empty
        },
    )
        .into()
}

fn accounts() -> View<Msg> {
    list_row("Checking").on_tap(Msg::Open("checking".into())).into()
}

/// Each element by its id: a tab by its section's tag, a row by its title,
/// a button by its message; a row of the screen a tab hosts only while that
/// tab shows. Falsified by every tab's content reached (Groceries tapped
/// from Accounts), or an id that moves with what the element says.
#[test]
fn a_tab_and_what_it_shows() {
    let sent: Sent = Rc::default();
    let all = |sel: &str| {
        compose(vec![
            screen("shell", shell(sel), &sent),
            screen("budget", budget(false), &sent),
            screen("accounts", accounts(), &sent),
        ])
    };
    let mut focus = None;
    let tap = |id: &str| Step::Tap { id: id.into(), text: false };
    let t = all("accounts");
    assert_eq!(take(&tap("budget.row:groceries"), &t, &mut focus), Took::NotYet, "{}", tree(&t));
    assert_eq!(take(&tap("tab:budget"), &t, &mut focus), Took::Done);
    assert_eq!(take(&tap("accounts.row:checking"), &t, &mut focus), Took::Done);
    let t = all("budget");
    assert_eq!(take(&tap("budget.row:groceries"), &t, &mut focus), Took::Done);
    assert_eq!(take(&tap("shell.new"), &t, &mut focus), Took::Done);
    assert_eq!(
        sent.borrow().clone(),
        vec![
            ("shell", Msg::Select("budget".into())),
            ("accounts", Msg::Open("checking".into())),
            ("budget", Msg::Open("groceries".into())),
            ("shell", Msg::New),
        ]
    );
    assert!(tree(&t).contains("[tab] tab:budget \u{2014} Budget (taps)"), "{}", tree(&t));
}

/// While a dialog is up only what is in it is reached, whichever screen
/// raised it. Falsified by the row under the dialog tapped.
#[test]
fn a_dialog_over_everything() {
    let sent: Sent = Rc::default();
    let t = compose(vec![
        screen("shell", shell("budget"), &sent),
        screen("budget", budget(true), &sent),
        screen("accounts", accounts(), &sent),
    ]);
    let mut focus = None;
    let tap = |id: &str| Step::Tap { id: id.into(), text: false };
    assert_eq!(take(&tap("budget.row:groceries"), &t, &mut focus), Took::NotYet, "{}", tree(&t));
    assert_eq!(take(&tap("budget.cancel"), &t, &mut focus), Took::Done);
    assert_eq!(sent.borrow().clone(), vec![("budget", Msg::Cancel)]);
    assert!(
        tree(&t).contains("[button] budget.underfunded \u{2014} Underfunded (taps) (modal)"),
        "{}",
        tree(&t)
    );
}

/// A field tapped takes what is typed, through its input; a wide button
/// sends its message; a label is reached only as `text`. Falsified by
/// typing with nothing focused when there are two fields.
#[test]
fn a_field_is_typed_into() {
    let sent: Sent = Rc::default();
    let page: View<Msg> = first_run_page(
        "app",
        "Connect to Your Server",
        "Where it is kept.",
        (
            address_field(
                "Server Address",
                "money.example.com",
                "",
                "",
                None::<String>,
                true,
                Msg::Typed,
                Msg::Check,
            ),
            wide_button("Continue", true, true, false, Msg::Check),
        ),
    );
    let t = compose(vec![screen("welcome", page, &sent)]);
    let mut focus = None;
    let tap = |id: &str| Step::Tap { id: id.into(), text: false };
    assert_eq!(take(&tap("welcome.typed"), &t, &mut focus), Took::Done, "{}", tree(&t));
    assert_eq!(focus.as_deref(), Some("welcome.typed"));
    assert_eq!(
        take(
            &Step::Type {
                text: "http://10.0.2.2:8788".into()
            },
            &t,
            &mut focus
        ),
        Took::Done
    );
    assert_eq!(take(&tap("welcome.check"), &t, &mut focus), Took::Done);
    assert_eq!(
        take(
            &Step::Tap {
                id: "Continue".into(),
                text: true
            },
            &t,
            &mut focus
        ),
        Took::Done
    );
    assert_eq!(
        take(
            &Step::Await {
                id: "welcome.text:connect-to-your-server".into()
            },
            &t,
            &mut focus
        ),
        Took::Done
    );
    assert_eq!(
        sent.borrow().clone(),
        vec![
            ("welcome", Msg::Typed("http://10.0.2.2:8788".into())),
            ("welcome", Msg::Check),
            ("welcome", Msg::Check)
        ]
    );
}

/// Ids are words a step can say: an emoji, punctuation and case gone; rows
/// of one title are each their own by their place (`~2`).
#[test]
fn ids_are_slugs() {
    let sent: Sent = Rc::default();
    let twice: View<Msg> = (
        list_row("Target").on_tap(Msg::Open("a".into())),
        list_row("Target").on_tap(Msg::Open("b".into())),
    )
        .into();
    let t = compose(vec![screen("transactions", twice, &sent)]);
    let took = take(
        &Step::Tap {
            id: "transactions.row:target~2".into(),
            text: false,
        },
        &t,
        &mut None,
    );
    assert_eq!(took, Took::Done);
    assert_eq!(sent.borrow().clone(), vec![("transactions", Msg::Open("b".into()))]);
    assert_eq!(slug("🍜 Dining Out"), "dining-out");
    assert_eq!(slug("accounts/0a1B"), "accounts/0a1b");
    assert_eq!(action("AutoAssign"), "auto-assign");
    assert_eq!(action("Select { tag: \"x\" }"), "select");
    assert_eq!(id_of(Role::Tab, "Budget", Some("Select { tag: \"budget\" }".into())), "tab:budget");
}

/// An element's id as the toolkits write it from a recording is the id a
/// walk's step names it by on a drawn screen: a row's by its title (a path's
/// words made a slug when drawn), a chip whose message carries a value by
/// its words, a field by what its input sends. Falsified by an id worked
/// out apart from `id_of`: the drawn screen's ids and the recorded ones
/// differed.
#[test]
fn recorded_ids_are_the_walks() {
    let sent: Sent = Default::default();
    let drawn: View<Msg> = (
        list_row("Dining Out").on_tap(Msg::Open("d".into())),
        chip("To Review (5)", "inbox", true, Msg::Select("review".into())),
        name_field("Payee", "", None, Msg::Typed),
    )
        .into();
    let targets = compose(vec![screen("budget", drawn.clone(), &sent)]);
    let ids: Vec<&str> = targets.iter().filter(|t| t.role != Role::Text).map(|t| t.id.as_str()).collect();
    let View::List(parts) = &drawn else { panic!("a list") };
    let written: Vec<String> = parts
        .iter()
        .filter_map(|v| match v {
            View::Part(p) => recorded("budget", p),
            _ => None,
        })
        .map(|ps| {
            ps.into_iter()
                .map(|p| match p {
                    Piece::Text(t) => t,
                    Piece::Slug(p) => panic!("no path in a drawn view: {p}"),
                })
                .collect::<String>()
        })
        .collect();
    assert_eq!(ids, written);
    // A title at a path: its words made a slug where the id says so.
    let row: View<Msg> = list_row(Val::At("t.payee".into())).on_tap(Msg::New).into();
    let View::Part(p) = &row else { panic!("a part") };
    assert_eq!(
        recorded("transactions", p),
        Some(vec![Piece::Text("transactions.row:".into()), Piece::Slug("t.payee".into())])
    );
}
