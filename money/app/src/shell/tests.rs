use super::view::{commands, money_settings, money_shell};
use super::*;
use ark_app::shell::{command_for, MenuPlace};
use ark_app::view::{Part, View};
use ark_app::{Host, ModuleCall, Runtime, Ticket};
use ark_peer::store::Store;
use ark_peer::Peer;

/// 2026-10-26, noon UTC.
const NOW: i64 = 1_792_929_600_000;

/// The demo's household, the shell's views held.
struct Demo {
    _peer: Peer,
    store: Store<MoneyView>,
    native: Vec<String>,
}

impl Demo {
    fn new() -> Demo {
        let mut peer = money_seed::open(money_seed::seeded_disk("2026-10-26", NOW));
        let mut store = Store::new();
        for k in MoneyShell::holds(&ShellState::default()) {
            store.hold(&mut peer, k);
        }
        store.settle(&mut peer);
        Demo {
            _peer: peer,
            store,
            native: vec![],
        }
    }
}

impl Host<MoneyShell> for Demo {
    fn read<R>(&self, f: impl FnOnce(&Store<MoneyView>) -> R) -> R {
        f(&self.store)
    }
    fn now_ms(&self) -> i64 {
        NOW
    }
    fn send(&mut self, _: MoneyChange) -> Result<(), String> {
        Ok(())
    }
    fn open(&mut self, _: Route) {}
    fn ask(&mut self, _: Ticket, _: ModuleCall) {}
    fn native(&mut self, name: &str) {
        self.native.push(name.into());
    }
}

/// Every hosted screen's name, in the order the view says them.
fn hosted<M>(v: &View<M>, out: &mut Vec<&'static str>) {
    match v {
        View::List(vs) => vs.iter().for_each(|v| hosted(v, out)),
        View::Part(p) => {
            if let Part::Hosted { name } = p {
                out.push(name);
            }
            for (_, slot) in p.call().slots {
                hosted(slot, out);
            }
        }
        _ => {}
    }
}

/// The sections are money.md's, in its order, Transactions with what
/// waits for a look; choosing one shows it, and a new place starts with
/// nothing searched. Each open account is a place of its own under its
/// shelf's heading, a child of Accounts (shown in it when compact).
/// Falsified by keeping the search across sections.
#[test]
fn the_sections_then_every_account() {
    let mut demo = Demo::new();
    let mut rt = Runtime::<MoneyShell>::new();
    let page = rt.screen(&demo);
    assert_eq!(page.section, "budget");
    assert!(page.to_review.is_some_and(|n| n > 0), "the demo's Chase card waits for a look: {page:?}");
    let budget: Vec<&str> = page.budget_accounts.iter().map(|a| a.title.as_str()).collect();
    let tracking: Vec<&str> = page.tracking_accounts.iter().map(|a| a.title.as_str()).collect();
    assert_eq!(budget, ["Checking", "Savings", "Cash", "Chase Sapphire"]);
    assert_eq!(tracking, ["Mortgage", "Retirement"]);
    let card = page.budget_accounts[3].clone();
    assert!(card.badge.is_some_and(|n| n > 0), "the card's imports wait: {card:?}");
    rt.dispatch(ShellMsg::Select { tag: "transactions".into() }, &mut demo);
    rt.dispatch(ShellMsg::Search { text: "Costco".into() }, &mut demo);
    let page = rt.screen(&demo);
    assert_eq!(
        (page.section.as_str(), page.search.as_str(), page.showing),
        ("transactions", "Costco", false)
    );
    assert_eq!(demo.native, vec!["search Costco"], "the hosted list is told what to look for");
    rt.dispatch(ShellMsg::Select { tag: card.tag.clone() }, &mut demo);
    let page = rt.screen(&demo);
    assert_eq!((page.section.as_str(), page.search.as_str(), page.showing), (card.tag.as_str(), "", true));
    assert_eq!(page.account_title, "Chase Sapphire");
    assert_eq!(
        ark_app::shell::compact_tag(&page.section),
        "accounts",
        "compact, the account is Accounts' pushed page"
    );
    rt.dispatch(ShellMsg::Back, &mut demo);
    assert_eq!(rt.screen(&demo).section, "accounts");
    rt.dispatch(
        ShellMsg::Run {
            command: "go-insights".into(),
        },
        &mut demo,
    );
    assert_eq!(rt.screen(&demo).section, "insights");
    rt.dispatch(ShellMsg::Select { tag: "nowhere".into() }, &mut demo);
    rt.dispatch(ShellMsg::Select { tag: "accounts/00".into() }, &mut demo);
    assert_eq!(rt.screen(&demo).section, "insights", "an unknown tag shows nothing new");
    rt.dispatch(ShellMsg::Month { month: "2026-10".into() }, &mut demo);
    assert_eq!(rt.screen(&demo).month_title, "October 2026", "the budget's title is its month");
    let mut names = vec![];
    hosted(&money_shell::view(&rt.screen(&demo)), &mut names);
    assert_eq!(
        &names[..9],
        [
            "budget",
            "budget.inspector",
            "accounts",
            "accounts.register",
            "transactions.inspector",
            "transactions",
            "transactions.inspector",
            "insights",
            "accounts.register",
        ]
    );
    assert_eq!(names.last(), Some(&"household"), "who is signed in, at the sidebar's foot");
}

/// The inspector toggles from the toolbar and ⌥⌘I, and is put away by its
/// own close; a command the shell cannot carry out goes to the platform by
/// name. Falsified by `ToggleInspector` setting it rather than flipping it.
#[test]
fn the_inspector_and_the_commands() {
    let mut demo = Demo::new();
    let mut rt = Runtime::<MoneyShell>::new();
    let menus = commands();
    let toggle = command_for(&menus, "i", false, true).expect("⌥⌘I");
    rt.dispatch(toggle.run.clone(), &mut demo);
    assert!(rt.screen(&demo).inspector);
    rt.dispatch(ShellMsg::ToggleInspector, &mut demo);
    assert!(!rt.screen(&demo).inspector);
    rt.dispatch(ShellMsg::Inspector { shown: true }, &mut demo);
    assert!(rt.screen(&demo).inspector);
    let new_account = command_for(&menus, "N", true, false).expect("⇧⌘N");
    assert_eq!(new_account.title, "New Account");
    assert_eq!(command_for(&menus, "n", false, false).map(|c| c.title), Some("New Transaction"));
    rt.dispatch(new_account.run.clone(), &mut demo);
    assert_eq!(demo.native, vec!["new-account"]);
    let go = command_for(&menus, "3", false, false).expect("⌘3");
    rt.dispatch(go.run.clone(), &mut demo);
    assert_eq!(rt.screen(&demo).section, "transactions");
    let places: Vec<MenuPlace> = menus.iter().map(|m| m.place).collect();
    assert_eq!(places, vec![MenuPlace::File, MenuPlace::View, MenuPlace::Go]);
}

/// Settings has the reference's three panes, one shown at a time.
/// Falsified by leaving one out, or showing a pane no title names.
#[test]
fn settings_has_three_panes() {
    let mut demo = Demo::new();
    let mut rt = Runtime::<MoneyShell>::new();
    let mut names = vec![];
    hosted(&money_settings::view(&rt.screen(&demo)), &mut names);
    assert_eq!(names, vec!["settings.general", "settings.household", "settings.banks"]);
    assert_eq!(rt.screen(&demo).pane, "General");
    rt.dispatch(
        ShellMsg::Pane {
            title: "Connected Banks".into(),
        },
        &mut demo,
    );
    rt.dispatch(ShellMsg::Pane { title: "Nowhere".into() }, &mut demo);
    assert_eq!(rt.screen(&demo).pane, "Connected Banks");
}
