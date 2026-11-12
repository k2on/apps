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

/// A tab is tapped by its label and role; a row of the screen a tab hosts
/// only while that tab shows. Falsified by every tab's content reached
/// (Groceries tapped from Accounts), or a tab not sent its select.
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
    let tap = |label: &str, role| Step::Tap {
        label: label.into(),
        role,
        bottom: false,
    };
    let t = all("accounts");
    assert_eq!(take(&tap("Groceries", None), &t, &mut focus), Took::NotYet, "{}", tree(&t));
    assert_eq!(take(&tap("Budget", Some(Role::Tab)), &t, &mut focus), Took::Done);
    assert_eq!(take(&tap("Checking", Some(Role::Row)), &t, &mut focus), Took::Done);
    let t = all("budget");
    assert_eq!(take(&tap("Groceries", None), &t, &mut focus), Took::Done);
    assert_eq!(take(&tap("New Transaction", Some(Role::Button)), &t, &mut focus), Took::Done);
    assert_eq!(
        sent.borrow().clone(),
        vec![
            ("shell", Msg::Select("budget".into())),
            ("accounts", Msg::Open("checking".into())),
            ("budget", Msg::Open("groceries".into())),
            ("shell", Msg::New),
        ]
    );
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
    let tap = |label: &str| Step::Tap {
        label: label.into(),
        role: None,
        bottom: false,
    };
    assert_eq!(take(&tap("Groceries"), &t, &mut focus), Took::NotYet, "{}", tree(&t));
    assert_eq!(take(&tap("Cancel"), &t, &mut focus), Took::Done);
    assert_eq!(sent.borrow().clone(), vec![("budget", Msg::Cancel)]);
    assert!(tree(&t).contains("[button] Underfunded (taps) (modal)"), "{}", tree(&t));
}

/// A field tapped takes what is typed, through its input; a wide button
/// sends its message; `a|b` is either. Falsified by typing with nothing
/// focused when there are two fields.
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
    let tapped = take(
        &Step::Tap {
            label: "Server Address".into(),
            role: Some(Role::Field),
            bottom: false,
        },
        &t,
        &mut focus,
    );
    assert_eq!(tapped, Took::Done);
    assert_eq!(focus.as_deref(), Some("Server Address"));
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
    assert_eq!(
        take(
            &Step::Tap {
                label: "Go|Continue".into(),
                role: None,
                bottom: false
            },
            &t,
            &mut focus
        ),
        Took::Done
    );
    assert_eq!(
        take(
            &Step::Await {
                label: "Connect to Your Server".into()
            },
            &t,
            &mut focus
        ),
        Took::Done
    );
    assert_eq!(
        sent.borrow().clone(),
        vec![("welcome", Msg::Typed("http://10.0.2.2:8788".into())), ("welcome", Msg::Check)]
    );
}
