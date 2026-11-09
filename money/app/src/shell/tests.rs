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

/// The sections are money.md's, in its order, Accounts with what waits
/// for a look; choosing one shows it, and a new section starts at its list
/// with nothing searched. Falsified by keeping the search across sections.
#[test]
fn the_sections_are_budget_accounts_transactions_insights() {
    let mut demo = Demo::new();
    let mut rt = Runtime::<MoneyShell>::new();
    let page = rt.screen(&demo);
    assert_eq!(page.section, "budget");
    assert!(page.to_review.is_some_and(|n| n > 0), "the demo's Chase card waits for a look: {page:?}");
    rt.dispatch(ShellMsg::Select { tag: "accounts".into() }, &mut demo);
    rt.dispatch(ShellMsg::Search { text: "Costco".into() }, &mut demo);
    rt.dispatch(ShellMsg::Opened, &mut demo);
    let page = rt.screen(&demo);
    assert_eq!((page.section.as_str(), page.search.as_str(), page.showing), ("accounts", "Costco", true));
    assert_eq!(demo.native, vec!["search Costco"], "the hosted list is told what to look for");
    rt.dispatch(
        ShellMsg::Run {
            command: "go-insights".into(),
        },
        &mut demo,
    );
    let page = rt.screen(&demo);
    assert_eq!((page.section.as_str(), page.search.as_str(), page.showing), ("insights", "", false));
    rt.dispatch(ShellMsg::Select { tag: "nowhere".into() }, &mut demo);
    assert_eq!(rt.screen(&demo).section, "insights", "an unknown tag shows nothing new");
    let mut names = vec![];
    hosted(&money_shell::view(&rt.screen(&demo)), &mut names);
    assert_eq!(
        names,
        vec![
            "budget",
            "budget.inspector",
            "accounts",
            "accounts.register",
            "accounts.inspector",
            "transactions",
            "transactions.detail",
            "transactions.inspector",
            "insights",
            "insights.inspector"
        ]
    );
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

/// Settings has money.md's four panes. Falsified by leaving one out.
#[test]
fn settings_has_four_panes() {
    let demo = Demo::new();
    let rt = Runtime::<MoneyShell>::new();
    let mut names = vec![];
    hosted(&money_settings::view(&rt.screen(&demo)), &mut names);
    assert_eq!(
        names,
        vec!["settings.server", "settings.banks", "settings.household", "settings.appearance"]
    );
}
