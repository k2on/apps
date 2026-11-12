use super::*;
use crate::shell::view::{money_settings, settings_banks};
use crate::shell::{ShellPage, PANES};
use crate::testing::{Demo, NOW};
use ark_app::view::{Part, View};
use ark_app::Runtime;

fn facts(demo: bool) -> SessionFacts {
    SessionFacts {
        who: "Alex Rivera".into(),
        login: "alex@example.com".into(),
        picture: String::new(),
        demo,
        server: "money.example.com".into(),
        linked: Some(true),
        pending: 2,
        version: "0.1 (7)".into(),
        leaves: true,
        adds_devices: true,
    }
}

fn on(demo: &mut Demo<MoneyShell>, rt: &mut Runtime<MoneyShell>, msg: ShellMsg) -> ShellPage {
    rt.dispatch(msg, demo);
    rt.screen(demo)
}

/// Every title a view's rows and dialogs say, in order.
fn said<M>(v: &View<M>, out: &mut Vec<String>) {
    match v {
        View::List(vs) => vs.iter().for_each(|v| said(v, out)),
        View::Part(p) => {
            match p {
                Part::Row(r) => out.push(r.title.get()),
                Part::Confirm { title, .. } => out.push(title.get()),
                _ => {}
            }
            for (_, slot) in p.call().slots {
                said(slot, out);
            }
        }
        View::Each { .. } | View::When { .. } | View::WhenSome { .. } | View::Empty => {}
    }
}

/// General says who is signed in where and what waits, and Sign Out is
/// asked about before the host is told to; Add a Device is the host's.
/// The demo has no server, so no link and no Add a Device. Falsified by
/// signing out on the first tap, or a demo offering a server's rows.
#[test]
fn general_signs_out_only_once_asked() {
    let mut demo = Demo::<MoneyShell>::new();
    let mut rt = Runtime::<MoneyShell>::new();
    let p = on(&mut demo, &mut rt, ShellMsg::Session { facts: facts(false) }).settings;
    assert_eq!(
        (p.who.as_str(), p.link.as_str(), p.waits, p.pending.as_str()),
        ("Alex Rivera", "Connected", true, "2 changes")
    );
    assert!(p.on_server && !p.leaving);
    let p = on(&mut demo, &mut rt, ShellMsg::SignOut).settings;
    assert!(p.leaving && demo.requests.is_empty(), "asked first");
    let p = on(&mut demo, &mut rt, ShellMsg::ConfirmSignOut).settings;
    assert!(!p.leaving);
    rt.dispatch(ShellMsg::AddDevice, &mut demo);
    assert_eq!(demo.requests, vec![MoneyRequest::SignOut, MoneyRequest::AddDevice]);

    let p = on(&mut demo, &mut rt, ShellMsg::Session { facts: facts(true) }).settings;
    assert_eq!(
        (p.who.as_str(), p.sign_out.as_str(), p.on_server, p.can_connect),
        ("Alex", "Leave the Demo", false, false)
    );
    let mut rows = vec![];
    said(&money_settings::view(&rt.screen(&demo)), &mut rows);
    assert!(!rows.iter().any(|r| r == "Add a Device"), "{rows:?}");
}

/// A launch puts back what the window showed, not who is signed in.
/// Falsified by a launch link that forgets the session's facts.
#[test]
fn a_launch_keeps_who_is_signed_in() {
    let mut demo = Demo::<MoneyShell>::new();
    let mut rt = Runtime::<MoneyShell>::new();
    rt.dispatch(ShellMsg::Session { facts: facts(false) }, &mut demo);
    rt.dispatch(ShellMsg::SignOut, &mut demo);
    let p = on(
        &mut demo,
        &mut rt,
        ShellMsg::Address {
            address: "money:///settings".into(),
        },
    )
    .settings;
    assert_eq!((p.who.as_str(), p.leaving), ("Alex Rivera", false));
}

/// The demo's bank is a row with how it is doing; its page says when it
/// last synced and what each of its accounts feeds, and a choice for one
/// of them is sent as the domain's change. Falsified by a bank page that
/// shows no accounts, or a choice that sends nothing.
#[test]
fn a_bank_and_what_its_accounts_feed() {
    let mut demo = Demo::<MoneyShell>::new();
    let mut rt = Runtime::<MoneyShell>::new();
    let p = on(&mut demo, &mut rt, ShellMsg::Session { facts: facts(false) }).settings;
    let bank = p.banks.first().expect("the demo's bank").clone();
    assert!(!bank.status.is_empty());
    let p = on(&mut demo, &mut rt, ShellMsg::OpenBank { name: bank.name.clone() }).settings;
    let page = p.bank.expect("its page");
    assert!(p.showing_bank && page.has_feeds, "{page:?}");
    let mut rows = vec![];
    said(&settings_banks::view(&rt.screen(&demo)), &mut rows);
    assert!(rows.contains(&"Disconnect".to_string()), "{rows:?}");

    let feed = page.feeds[0].clone();
    let p = on(&mut demo, &mut rt, ShellMsg::ChooseFeed { plaid: feed.plaid.clone() }).settings;
    let choice = p.bank.unwrap().choosing.expect("the choice");
    assert_eq!(choice.title, feed.title);
    rt.dispatch(ShellMsg::FeedIgnored { ignored: !choice.ignored }, &mut demo);
    let p = rt.screen(&demo).settings;
    let now = p.bank.unwrap();
    assert!(now.choosing.is_none());
    let after = now.feeds.iter().find(|f| f.plaid == feed.plaid).unwrap();
    assert_ne!(after.feeds, feed.feeds, "the change was sent and the store moved");

    // Disconnect is asked about; the demo's is never sent.
    let p = on(&mut demo, &mut rt, ShellMsg::Disconnect).settings;
    assert!(p.bank.unwrap().disconnecting);
    let p = on(&mut demo, &mut rt, ShellMsg::CloseBank).settings;
    assert!(p.bank.is_none() && !p.showing_bank);
}

fn ask(id: u8, state: AskState, url: &str) -> AskLine {
    let mut b = [0u8; 16];
    b[15] = id;
    AskLine {
        id: Id::new(b),
        what: "connect".into(),
        state,
        url: url.into(),
        asked_ms: NOW,
        expires_ms: NOW + 10 * 60_000,
        ..AskLine::default()
    }
}

/// Plaid's page is shown once the server answers this device's ask with
/// it, not before and not for an older ask; until the ask is over or the
/// person closes it; and again from the ask's row. Falsified by following
/// by the clock, or keeping the page up after the ask is done.
#[test]
fn plaids_page_follows_the_ask() {
    let old = ask(1, AskState::Done, "");
    let mut st = SettingsState {
        awaiting: Some(Some(old.id)),
        ..SettingsState::default()
    };
    assert_eq!(plaid(&st, std::slice::from_ref(&old)), None, "the server has not answered");
    let asked = ask(2, AskState::Asked, "");
    assert_eq!(plaid(&st, &[asked, old.clone()]), None);
    let waiting = ask(2, AskState::Waiting, "https://plaid/x");
    assert_eq!(plaid(&st, &[waiting.clone(), old.clone()]).as_deref(), Some("https://plaid/x"));
    let done = ask(2, AskState::Done, "https://plaid/x");
    assert_eq!(plaid(&st, &[done, old.clone()]), None, "over: put away");
    st.closed = Some("https://plaid/x".into());
    assert_eq!(plaid(&st, &[waiting.clone(), old.clone()]), None, "closed by the person");
    st = SettingsState {
        reopened: Some("https://plaid/x".into()),
        ..SettingsState::default()
    };
    assert_eq!(plaid(&st, &[waiting, old]).as_deref(), Some("https://plaid/x"), "opened again");
}

/// Connect a Bank on a server sends the domain's ask and follows from the
/// newest before it; in the demo it sends nothing. The Appearance pane is
/// the shell's appearance. Falsified by the demo asking a server it does
/// not have, or a choice of appearance the shell does not wear.
#[test]
fn connect_and_appearance() {
    let mut demo = Demo::<MoneyShell>::new();
    let mut rt = Runtime::<MoneyShell>::new();
    rt.dispatch(ShellMsg::Session { facts: facts(true) }, &mut demo);
    rt.dispatch(ShellMsg::ConnectBank, &mut demo);
    assert_eq!(rt.state().settings.awaiting, None);
    rt.dispatch(ShellMsg::Session { facts: facts(false) }, &mut demo);
    rt.dispatch(ShellMsg::ConnectBank, &mut demo);
    assert!(rt.state().settings.awaiting.is_some());

    let p = on(&mut demo, &mut rt, ShellMsg::AppearanceKey { key: "dark".into() });
    assert_eq!((p.appearance, p.settings.appearance.as_str()), (Some(Appearance::Dark), "dark"));
    let p = on(&mut demo, &mut rt, ShellMsg::AppearanceKey { key: "system".into() });
    assert_eq!((p.appearance, p.settings.appearance.as_str()), (None, "system"));
    assert_eq!(PANES[3], "Appearance");
}
