use super::view::{commands, money_settings, money_shell};
use super::*;
use ark_app::shell::{command_for, MenuPlace};
use ark_app::view::{Part, View};
use ark_app::{Ask, Host, Modules, Runtime, Ticket};
use ark_peer::store::Store;
use ark_peer::Peer;

/// 2026-10-26, noon UTC.
const NOW: i64 = 1_792_929_600_000;

/// The demo's household, the shell's views held.
struct Demo {
    _peer: Peer,
    store: Store<MoneyView>,
    requests: Vec<MoneyRequest>,
}

impl Demo {
    fn new() -> Demo {
        let mut peer = money_seed::open(money_seed::seeded_disk("2026-10-26", NOW));
        let mut store = Store::new();
        let october = ShellState {
            month: "2026-10".into(),
            ..ShellState::default()
        };
        for k in MoneyShell::holds(&october) {
            store.hold(&mut peer, k);
        }
        store.settle(&mut peer);
        Demo {
            _peer: peer,
            store,
            requests: vec![],
        }
    }
}

impl Host<MoneyShell> for Demo {
    fn read<R>(&self, f: impl FnOnce(&dyn ark::views::Views<MoneyView>) -> R) -> R {
        f(&self.store)
    }
    fn now_ms(&self) -> i64 {
        NOW
    }
    fn send(&mut self, _: MoneyChange) -> Result<(), String> {
        Ok(())
    }
    fn open(&mut self, _: Route) {}
    fn ask(&mut self, _: Ticket, ask: Ask<ShellMsg>) {
        ask.run(&Modules::new(), |_| {})
    }
    fn request(&mut self, r: MoneyRequest) {
        self.requests.push(r);
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
    assert!(demo.requests.is_empty(), "the hosted list reads what to look for from the page");
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
    rt.dispatch(view::go("insights"), &mut demo);
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
    assert_eq!(
        demo.requests,
        vec![MoneyRequest::Accounts {
            msg: crate::accounts::AccountsMsg::Add
        }]
    );
    let go = command_for(&menus, "3", false, false).expect("⌘3");
    rt.dispatch(go.run.clone(), &mut demo);
    assert_eq!(rt.screen(&demo).section, "transactions");
    let places: Vec<MenuPlace> = menus.iter().map(|m| m.place).collect();
    assert_eq!(places, vec![MenuPlace::File, MenuPlace::View, MenuPlace::Go]);
}

/// Settings has its four panes, one shown at a time, each drawn from
/// Rust: no platform hosts any of them, only what it adds to General. Falsified by leaving one out, a
/// pane still hosted, or showing a pane no title names.
#[test]
fn settings_has_four_panes_none_hosted() {
    let mut demo = Demo::new();
    let mut rt = Runtime::<MoneyShell>::new();
    let mut names = vec![];
    let view = money_settings::view(&rt.screen(&demo));
    hosted(&view, &mut names);
    assert_eq!(names, ["settings.more"], "only what a platform adds of its own");
    let mut panes = vec![];
    fn titles<M>(v: &View<M>, out: &mut Vec<&'static str>) {
        match v {
            View::List(vs) => vs.iter().for_each(|v| titles(v, out)),
            View::Part(p) => {
                if let Part::SettingsPane { title, .. } = p {
                    out.push(*title);
                }
                for (_, slot) in p.call().slots {
                    titles(slot, out);
                }
            }
            _ => {}
        }
    }
    titles(&view, &mut panes);
    assert_eq!(panes, PANES);
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

/// The launch arguments open any place with no steps: an account by a
/// word of its name, the inspector, and what only the platform's screens
/// show yet handed to it by name. An account asked for before the
/// accounts are there is shown once they are, until another is chosen.
/// Falsified by resolving the name only when the arguments arrive.
#[test]
fn launch_arguments_open_a_place() {
    let mut demo = Demo::new();
    let mut rt = Runtime::<MoneyShell>::new();
    let args = |a: &[&str]| ShellMsg::Launch {
        args: a.iter().map(|s| s.to_string()).collect(),
    };
    rt.dispatch(args(&["Money", "-screen", "account:checking", "-inspector", "1"]), &mut demo);
    let page = rt.screen(&demo);
    assert_eq!((page.account_title.as_str(), page.showing, page.inspector), ("Checking", true, true));
    rt.dispatch(
        args(&["-screen", "transaction:receipt", "-inspector", "1", "-appearance", "dark"]),
        &mut demo,
    );
    let page = rt.screen(&demo);
    assert_eq!(page.section, "transactions");
    assert!(
        page.transaction.is_some(),
        "the receipt's transaction in the inspector: {:?}",
        page.transaction
    );
    assert_eq!(page.appearance, Some(Appearance::Dark), "the appearance is the shell's own");
    assert!(demo.requests.is_empty(), "{:?}", demo.requests);
    rt.dispatch(args(&["-screen", "settings", "-settingsTab", "banks"]), &mut demo);
    assert_eq!(rt.screen(&demo).pane, "Connected Banks");
    assert_eq!(
        demo.requests.last(),
        Some(&MoneyRequest::Settings {
            pane: "Connected Banks".into()
        })
    );

    // Another launch (a launch link, as the app runs): what the last one
    // showed put back first, as a fresh launch has it.
    rt.dispatch(args(&["-screen", "insights"]), &mut demo);
    let page = rt.screen(&demo);
    assert_eq!(
        (
            page.section.as_str(),
            page.inspector,
            page.transaction.is_none(),
            page.appearance,
            page.pane.as_str()
        ),
        ("insights", false, true, None, PANES[0])
    );

    // Before the accounts have come: wanted, then shown once they have.
    let mut s = ShellState::default();
    let empty = Store::<MoneyView>::new();
    let data = Data { store: &empty, now_ms: NOW };
    MoneyShell::update(&mut s, args(&["-screen", "account:sapphire"]), &data);
    assert_eq!((s.section.as_str(), s.wanted.as_deref()), ("accounts", Some("sapphire")));
    let page = demo.read(|store| MoneyShell::screen(&s, &Data { store, now_ms: NOW }));
    assert_eq!((page.account_title.as_str(), page.showing), ("Chase Sapphire", true));
    MoneyShell::update(&mut s, ShellMsg::Back, &data);
    assert_eq!(s.wanted, None, "back is back to Accounts");
}

/// Category Details is the reference's, worked out in Rust: the badge in
/// the category's tint, what needs attention, the month's figures, the
/// goal, the month's transactions; Show All goes to Transactions searching
/// for it. Falsified by figures that do not add up, or nothing inspected
/// still drawing an inspector.
#[test]
fn the_category_inspector() {
    let mut demo = Demo::new();
    let mut rt = Runtime::<MoneyShell>::new();
    rt.dispatch(ShellMsg::Month { month: "2026-10".into() }, &mut demo);
    rt.dispatch(
        ShellMsg::Launch {
            args: ["-screen", "category:groceries", "-inspector", "1"].map(String::from).to_vec(),
        },
        &mut demo,
    );
    let page = rt.screen(&demo);
    let c = page.category.expect("Groceries inspected");
    assert_eq!((c.name.as_str(), c.month.as_str()), ("Groceries", "October 2026"));
    assert_eq!(c.available, c.carried + c.assigned + c.activity, "the month's figures add up: {c:?}");
    assert!(["positive", "caution", "negative", "neutral"].contains(&c.tint.as_str()));
    assert_eq!(c.quiet_month, c.activity_lines.is_empty());
    rt.dispatch(ShellMsg::ShowActivity, &mut demo);
    let page = rt.screen(&demo);
    assert_eq!((page.section.as_str(), page.search.as_str()), ("transactions", "Groceries"));
    rt.dispatch(ShellMsg::InspectCategory { id: None }, &mut demo);
    assert!(rt.screen(&demo).category.is_none());
    let mut names = vec![];
    hosted(&money_shell::view(&rt.screen(&demo)), &mut names);
    assert_eq!(names[1], "budget.inspector", "nothing inspected: the platform's own");
}

/// Review is To Review with the newest waiting for a look open: its sheet
/// when the inspector is not shown, Transaction Details when it is.
/// Falsified by Review alone: its shot was Transactions' own.
#[test]
fn review_opens_what_waits_for_a_look() {
    let mut demo = Demo::new();
    let mut rt = Runtime::<MoneyShell>::new();
    let launch = |a: &[&str]| ShellMsg::Launch {
        args: a.iter().map(|s| s.to_string()).collect(),
    };
    rt.dispatch(launch(&["-screen", "review"]), &mut demo);
    assert_eq!(rt.screen(&demo).section, "transactions");
    assert_eq!(
        demo.requests,
        vec![MoneyRequest::Review, MoneyRequest::OpenTransaction { key: "review".into() }]
    );
    demo.requests.clear();
    rt.dispatch(launch(&["-screen", "review", "-inspector", "1"]), &mut demo);
    assert_eq!(demo.requests, vec![MoneyRequest::Review]);
    assert!(rt.screen(&demo).transaction.is_some(), "Transaction Details shows it");
}

/// A phone: the screen drawn between messages.
struct Phone(Demo);

impl Host<MoneyShell> for Phone {
    fn read<R>(&self, f: impl FnOnce(&dyn ark::views::Views<MoneyView>) -> R) -> R {
        self.0.read(f)
    }
    fn now_ms(&self) -> i64 {
        NOW
    }
    fn send(&mut self, c: MoneyChange) -> Result<(), String> {
        self.0.send(c)
    }
    fn open(&mut self, _: Route) {}
    fn ask(&mut self, t: Ticket, ask: Ask<ShellMsg>) {
        self.0.ask(t, ask)
    }
    fn request(&mut self, r: MoneyRequest) {
        self.0.request(r)
    }
    fn draws_between(&self) -> bool {
        true
    }
}

/// A launch link while the inspector is up puts it away where it is, and
/// opens its place only once the screen without it has been drawn; idle
/// only then. Falsified by a launch that switched the section in the same
/// update that put the inspector away (the iPhone's sheet left over
/// another tab), or by one that waited where nothing was up.
#[test]
fn a_launch_puts_the_inspector_away_before_it_moves() {
    let args = |a: &[&str]| ShellMsg::Launch {
        args: a.iter().map(|s| s.to_string()).collect(),
    };
    let mut phone = Phone(Demo::new());
    let mut rt = Runtime::<MoneyShell>::new();
    rt.dispatch(args(&["-screen", "category:Dining Out", "-inspector", "1"]), &mut phone);
    assert_eq!((rt.screen(&phone).section.as_str(), rt.screen(&phone).inspector), ("budget", true));
    assert!(!rt.awaits_draw(), "nothing was up: it opened at once");

    rt.dispatch(args(&["-screen", "accounts"]), &mut phone);
    let page = rt.screen(&phone);
    assert_eq!((page.section.as_str(), page.inspector), ("budget", false), "put away where it is");
    assert!(rt.awaits_draw());
    assert!(!phone.read(|st| rt.is_idle(st)));
    assert!(rt.drawn(&mut phone));
    let page = rt.screen(&phone);
    assert_eq!((page.section.as_str(), page.inspector), ("accounts", false));
    assert!(phone.read(|st| rt.is_idle(st)));

    // An iced window or the browser draws what the last message left: at once.
    let mut demo = Demo::new();
    let mut rt = Runtime::<MoneyShell>::new();
    rt.dispatch(args(&["-screen", "category:Dining Out", "-inspector", "1"]), &mut demo);
    rt.dispatch(args(&["-screen", "insights"]), &mut demo);
    let page = rt.screen(&demo);
    assert_eq!((page.section.as_str(), page.inspector), ("insights", false));
}

/// An address is a launch: `money:///budget` selects the Budget wherever
/// the shell was, and pushes nothing; a row's route opens the row; a bank
/// is Settings' Banks; a sign-in link is not the shell's. Falsified by
/// taking an address as a route pushed on the section shown (dry run
/// 43113069044: the iPad's Budget pushed over itself, an empty page).
#[test]
fn an_address_selects_what_it_names() {
    let mut demo = Phone(Demo::new());
    let mut rt = Runtime::<MoneyShell>::new();
    let to = |a: &str| ShellMsg::Address { address: a.into() };
    rt.dispatch(ShellMsg::Select { tag: "insights".into() }, &mut demo);
    rt.dispatch(to("money:///budget"), &mut demo);
    let page = rt.screen(&demo);
    assert_eq!((page.section.as_str(), page.showing), ("budget", false));
    assert!(demo.0.requests.is_empty(), "nothing pushed: {:?}", demo.0.requests);

    rt.dispatch(to("money:///account/sapphire?inspector=1"), &mut demo);
    let page = rt.screen(&demo);
    assert_eq!(
        (page.account_title.as_str(), page.showing, page.inspector),
        ("Chase Sapphire", true, true)
    );
    let id = page.section.trim_start_matches("accounts/").to_string();
    // The route's own spelling (`Route::Account`'s url), by its id; the
    // inspector shown, so put away first.
    rt.dispatch(to(&format!("money:///account/{id}/chase-sapphire/")), &mut demo);
    assert!(rt.awaits_draw());
    assert!(rt.drawn(&mut demo));
    let page = rt.screen(&demo);
    assert_eq!((page.section, page.inspector), (format!("accounts/{id}"), false));

    rt.dispatch(to("money:///banks"), &mut demo);
    assert_eq!(
        demo.0.requests.last(),
        Some(&MoneyRequest::Settings {
            pane: "Connected Banks".into()
        })
    );
    rt.dispatch(to("money:///inbox?appearance=dark"), &mut demo);
    let page = rt.screen(&demo);
    assert_eq!((page.section.as_str(), page.appearance), ("transactions", Some(Appearance::Dark)));
    rt.dispatch(to("money:///filtered"), &mut demo);
    assert_eq!(rt.screen(&demo).section, "transactions");

    let before = rt.screen(&demo);
    demo.0.requests.clear();
    for not_ours in ["money://join?server=https%3A%2F%2Fh&code=1", "money:///welcome", "money:///"] {
        rt.dispatch(to(not_ours), &mut demo);
    }
    assert_eq!(rt.screen(&demo), before, "the onboarding's links leave the shell as it was");
    assert!(demo.0.requests.is_empty());

    // The browser's own spelling of an account's place: its tag.
    rt.dispatch(to(&format!("/accounts/{id}/")), &mut demo);
    assert_eq!(rt.screen(&demo).section, format!("accounts/{id}"));
}

/// Every screenshot's `route` (money/demo/shots.json) opens what its
/// launch arguments open: the same page, the same requests of the
/// platform. Falsified by an address the shell reads otherwise than its
/// `-screen` (a bank's, a sheet's, To Review's).
#[test]
fn every_shot_route_is_its_launch() {
    // Read where the run finds it (money/demo beside this crate), not
    // compiled in: the crate's build holds only its own directory.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../demo/shots.json");
    let shots: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).expect(path)).unwrap();
    let mut n = 0;
    for screen in shots["screens"].as_array().expect("screens") {
        let (Some(route), Some(args)) = (screen["route"].as_str(), screen["args"].as_array()) else {
            continue;
        };
        let args: Vec<String> = args.iter().filter_map(|a| a.as_str().map(String::from)).collect();
        let open = |msg: ShellMsg| {
            let mut demo = Demo::new();
            let mut rt = Runtime::<MoneyShell>::new();
            rt.dispatch(msg, &mut demo);
            (rt.screen(&demo), demo.requests)
        };
        assert_eq!(
            open(ShellMsg::Address {
                address: format!("money://{route}")
            }),
            open(ShellMsg::Launch { args: args.clone() }),
            "{route} against {args:?}"
        );
        n += 1;
    }
    assert!(n >= 12, "the shots' routes: {n}");
}

/// The appearance follows the device's by default, everywhere the shell
/// starts: its page before a session runs it (`shellBlank` on the phones),
/// a launch that names none (the screenshots' dark pass sets the device's
/// appearance and names none), and Settings' System after Dark. Settings
/// says System then. Falsified by a default of Light: app-screens
/// 43131582873's dark iPhone and iPad shots were their light ones, byte for
/// byte.
#[test]
fn the_appearance_follows_the_system_by_default() {
    let mut demo = Demo::new();
    let mut rt = Runtime::<MoneyShell>::new();
    let page = rt.screen(&demo);
    assert_eq!(page.appearance, None, "no appearance over the device's");
    assert_eq!(page.settings.appearance, "system");
    let launch = |a: &[&str]| ShellMsg::Launch {
        args: a.iter().map(|s| s.to_string()).collect(),
    };
    rt.dispatch(launch(&["-shots", "YES", "-demo", "2026-11-18", "-screen", "budget"]), &mut demo);
    assert_eq!(rt.screen(&demo).appearance, None, "a launch that names none keeps the device's");
    rt.dispatch(ShellMsg::AppearanceKey { key: "dark".into() }, &mut demo);
    assert_eq!(rt.screen(&demo).appearance, Some(Appearance::Dark));
    rt.dispatch(ShellMsg::AppearanceKey { key: "system".into() }, &mut demo);
    let page = rt.screen(&demo);
    assert_eq!((page.appearance, page.settings.appearance.as_str()), (None, "system"));
    rt.dispatch(launch(&["-appearance", "dark"]), &mut demo);
    rt.dispatch(launch(&["-screen", "insights"]), &mut demo);
    assert_eq!(rt.screen(&demo).appearance, None, "another launch puts the last one's appearance back");
}

/// Every section's toolbar ends with who is signed in: the shell's tabs
/// carry an account item with their name and picture, and tapping it opens
/// Settings on General — on every platform, the same item (the phones'
/// generated `ToolbarAccount`). Falsified by leaving the item out, by the
/// demo's household going nameless, or by the tap opening another pane.
#[test]
fn the_toolbars_end_with_the_account() {
    let mut demo = Demo::new();
    let mut rt = Runtime::<MoneyShell>::new();
    fn account<M: Clone>(v: &View<M>) -> Option<(String, String, M)> {
        match v {
            View::List(vs) => vs.iter().find_map(account),
            View::Part(Part::Account { name, picture, tap }) => Some((name.get(), picture.get(), tap.clone())),
            View::Part(p) => p.call().slots.into_iter().find_map(|(_, s)| account(s)),
            _ => None,
        }
    }
    let facts = |demo: bool, picture: &str| ShellMsg::Session {
        facts: SessionFacts {
            who: "Sam Lee".into(),
            picture: picture.into(),
            demo,
            ..SessionFacts::default()
        },
    };
    rt.dispatch(facts(true, ""), &mut demo);
    let (name, picture, tap) = account(&money_shell::view(&rt.screen(&demo))).expect("the shell's account item");
    assert_eq!(
        (name.as_str(), picture.as_str()),
        ("Alex", ""),
        "the demo's household is Alex's, with no picture"
    );
    assert_eq!(tap, ShellMsg::Account);
    rt.dispatch(facts(false, "https://money.example.com/auth/picture/sam"), &mut demo);
    let (name, picture, _) = account(&money_shell::view(&rt.screen(&demo))).unwrap();
    assert_eq!(
        (name.as_str(), picture.as_str()),
        ("Sam Lee", "https://money.example.com/auth/picture/sam")
    );
    rt.dispatch(ShellMsg::Pane { title: "Appearance".into() }, &mut demo);
    rt.dispatch(ShellMsg::Account, &mut demo);
    assert_eq!(rt.screen(&demo).pane, "General");
    assert_eq!(demo.requests.last(), Some(&MoneyRequest::Settings { pane: "General".into() }));
    assert_eq!(ark_app::shell::account_label("Sam Lee"), "Account, Sam Lee");
    assert_eq!(ark_app::shell::account_initial("sam"), "S");
    assert_eq!(ark_app::shell::account_initial(""), "?");
}

/// The phones' walk (money/demo/shots.json, taken in the app: ui/docs/
/// shots.md) reaches what it taps in the views every phone draws: the
/// sections' tabs, the account's button, a new transaction, Auto-Assign and
/// a category of the budget the Budget tab hosts. Falsified by a label the
/// walk names that no view says (a tab renamed, the account's button gone).
#[test]
fn the_walk_reaches_what_it_taps() {
    use ark_app::shots::{compose, nodes, take, tree, Role, Screen, Step, Took};
    use std::cell::RefCell;
    use std::rc::Rc;
    let mut demo = Demo::new();
    let mut rt = Runtime::<MoneyShell>::new();
    rt.dispatch(ShellMsg::Month { month: "2026-10".into() }, &mut demo);
    rt.dispatch(ShellMsg::Select { tag: "budget".into() }, &mut demo);
    let mut bd = crate::testing::Demo::<crate::budget::Budget>::new();
    let mut brt = Runtime::<crate::budget::Budget>::new();
    bd.run(&mut brt, [crate::budget::BudgetMsg::Month { month: "2026-10".into() }]);
    let sent: Rc<RefCell<Vec<String>>> = Rc::default();
    let (s1, s2) = (sent.clone(), sent.clone());
    let targets = compose(vec![
        Screen {
            name: "shell".into(),
            nodes: nodes(
                &money_shell::view(&rt.screen(&demo)),
                Rc::new(move |m: ShellMsg| s1.borrow_mut().push(format!("{m:?}"))),
            ),
        },
        Screen {
            name: "budget".into(),
            nodes: nodes(
                &crate::budget::view::budget::view(&brt.screen(&bd)),
                Rc::new(move |m: crate::budget::BudgetMsg| s2.borrow_mut().push(format!("{m:?}"))),
            ),
        },
    ]);
    let said = tree(&targets);
    for want in [
        "[tab] Budget (taps)",
        "[tab] Accounts (taps)",
        "[tab] Transactions (taps)",
        "[tab] Insights (taps)",
        "[button] Account (taps)",
        "[button] New Transaction (taps)",
        "[button] Auto-Assign (taps)",
        "[row] 🍜 Dining Out (taps",
    ] {
        assert!(said.contains(want), "{want}, in:\n{said}");
    }
    let mut focus = None;
    let tap = |label: &str, role: Option<Role>| Step::Tap {
        label: label.into(),
        role,
        bottom: true,
    };
    assert_eq!(take(&tap("Auto-Assign", Some(Role::Button)), &targets, &mut focus), Took::Done);
    assert_eq!(take(&tap("Accounts", Some(Role::Tab)), &targets, &mut focus), Took::Done);
    assert_eq!(sent.borrow().clone(), ["AutoAssign", "Select { tag: \"accounts\" }"]);
}
