use super::*;
use ark_app::Modules;
use ark_app::{Ask, Host, Inbox, Runtime, Ticket};
use ark_peer::store::Store;
use ark_peer::Peer;

/// The demo's household on 2026-10-26, its accounts held, the fakes for modules.
pub(crate) struct Demo {
    pub peer: Peer,
    pub store: Store<MoneyView>,
    pub haptics: ark_haptics::fake::Fake,
    pub clipboard: ark_clipboard::fake::Fake,
    pub modules: Modules,
    pub inbox: Inbox<AccountsMsg>,
    pub opened: Vec<Route>,
    pub requests: Vec<crate::MoneyRequest>,
}

/// 2026-10-26, noon UTC.
const NOW: i64 = 1_792_929_600_000;

impl Demo {
    pub fn new() -> Demo {
        let mut peer = money_core::demo::open(money_core::demo::seeded_disk("2026-10-26", NOW));
        let mut store = Store::new();
        for k in Accounts::holds(&State::default()) {
            store.hold(&mut peer, k);
        }
        store.settle(&mut peer);
        let haptics = ark_haptics::fake::Fake::new();
        let clipboard = ark_clipboard::fake::Fake::new();
        let mut modules = Modules::new();
        haptics.register(&mut modules);
        clipboard.register(&mut modules);
        Demo {
            peer,
            store,
            haptics,
            clipboard,
            modules,
            inbox: Inbox::default(),
            opened: vec![],
            requests: vec![],
        }
    }

    fn accounts(&self) -> &[AccountLine] {
        self.store.rows(&MoneyView::Accounts)
    }
}

impl Host<Accounts> for Demo {
    fn read<R>(&self, f: impl FnOnce(&dyn ark::views::Views<MoneyView>) -> R) -> R {
        f(&self.store)
    }
    fn now_ms(&self) -> i64 {
        NOW
    }
    fn send(&mut self, change: MoneyChange) -> Result<(), String> {
        let r = self.store.sent(self.peer.mutate_bytes(change.name(), change.input()));
        self.store.settle(&mut self.peer);
        r.map(|_| ())
    }
    fn open(&mut self, route: Route) {
        self.opened.push(route)
    }
    fn ask(&mut self, ticket: Ticket, ask: Ask<AccountsMsg>) {
        self.inbox.ask(&self.modules, ticket, ask)
    }
    fn request(&mut self, r: crate::MoneyRequest) {
        self.requests.push(r)
    }
}

fn run(rt: &mut Runtime<Accounts>, d: &mut Demo, msgs: impl IntoIterator<Item = AccountsMsg>) {
    for m in msgs {
        rt.dispatch(m, d);
    }
}

#[test]
fn the_page_adds_up_the_household() {
    let d = Demo::new();
    let rt = Runtime::<Accounts>::new();
    let p = rt.screen(&d);
    assert_eq!(p.net_worth, net_worth(d.accounts()));
    assert!(!p.shelves.is_empty());
    assert_eq!(p.shelves[0].title, "Budget Accounts");
    assert_eq!(p.shelves[0].total, p.all_total);
    let every: usize = p.shelves.iter().map(|s| s.accounts.len()).sum();
    assert_eq!(every, d.accounts().len());
    assert!(p.shelves.iter().all(|s| s.shows_total == (s.title != "Closed Accounts")));
    assert!(p.adding.is_none() && p.renaming.is_none());
}

#[test]
fn a_row_opens_its_register_by_id_and_name() {
    let mut d = Demo::new();
    let mut rt = Runtime::<Accounts>::new();
    let a = d.accounts()[0].clone();
    run(&mut rt, &mut d, [AccountsMsg::OpenAccount { id: a.id.hex() }, AccountsMsg::OpenAll]);
    assert_eq!(d.opened.len(), 2);
    assert!(d.opened[0].names(&a.id.hex(), &a.name));
    assert!(matches!(d.opened[1], Route::Accounts));
    // A row gone since it was drawn lands on every account's.
    run(&mut rt, &mut d, [AccountsMsg::OpenAccount { id: "00".repeat(16) }]);
    assert!(matches!(d.opened[2], Route::Accounts));
}

#[test]
fn adding_an_account_checks_it_as_the_domain_does() {
    let mut d = Demo::new();
    let mut rt = Runtime::<Accounts>::new();
    let before = d.accounts().len();
    run(&mut rt, &mut d, [AccountsMsg::Add, AccountsMsg::DraftName { name: "   ".into() }]);
    let f = rt.screen(&d).adding.unwrap();
    assert_eq!(f.name_problem.as_deref(), Some("an account needs a name"));
    assert!(!f.can_save);
    // Saving a form with a problem sends nothing.
    run(&mut rt, &mut d, [AccountsMsg::SaveDraft]);
    assert_eq!(d.accounts().len(), before);

    run(
        &mut rt,
        &mut d,
        [
            AccountsMsg::DraftName { name: "Visa".into() },
            AccountsMsg::DraftKind { kind: AccountKind::Credit },
            AccountsMsg::DraftBalance { typed: "12.x".into() },
        ],
    );
    let f = rt.screen(&d).adding.unwrap();
    assert_eq!(f.balance_problem.as_deref(), Some("That isn't an amount"));
    assert_eq!(f.balance_label, "What You Owe on It");
    assert!(f.kinds.iter().any(|k| k.selected && k.kind == AccountKind::Credit));
    assert!(!f.can_save);

    run(
        &mut rt,
        &mut d,
        [AccountsMsg::DraftBalance { typed: "250".into() }, AccountsMsg::SaveDraft],
    );
    for (t, a) in d.inbox.take() {
        rt.answer(t, a, &mut d);
    }
    assert_eq!(d.accounts().len(), before + 1);
    let visa = d.accounts().iter().find(|a| a.name == "Visa").unwrap();
    // What is owed is below nothing however it was typed.
    assert_eq!(visa.balance, -25_000);
    assert_eq!(visa.kind, "credit");
    assert!(rt.state().adding.is_none(), "the sheet goes once it is kept");
    assert_eq!(d.haptics.performed(), vec![Haptic::Selection, Haptic::Success]);
}

#[test]
fn renaming_and_closing_go_through_the_domain() {
    let mut d = Demo::new();
    let mut rt = Runtime::<Accounts>::new();
    let id = d.accounts()[0].id.hex();
    run(
        &mut rt,
        &mut d,
        [
            AccountsMsg::Rename { id: id.clone() },
            AccountsMsg::RenameTyped { name: " Everyday ".into() },
        ],
    );
    assert!(rt.screen(&d).renaming.unwrap().can_save);
    run(&mut rt, &mut d, [AccountsMsg::SaveRename]);
    assert_eq!(find(d.accounts(), &id).unwrap().name, "Everyday");
    assert!(rt.state().renaming.is_none());

    // One with money in it stays open, in the domain's words.
    run(
        &mut rt,
        &mut d,
        [AccountsMsg::SetClosed {
            id: id.clone(),
            closed: true,
        }],
    );
    assert_eq!(d.store.refusal(), Some("an account is closed with nothing in it"));
    assert!(d.haptics.performed().is_empty(), "a refusal is not felt as a success");

    run(
        &mut rt,
        &mut d,
        [AccountsMsg::Add, AccountsMsg::DraftName { name: "Jar".into() }, AccountsMsg::SaveDraft],
    );
    for (t, a) in d.inbox.take() {
        rt.answer(t, a, &mut d);
    }
    let jar = d.accounts().iter().find(|a| a.name == "Jar").unwrap().id.hex();
    run(
        &mut rt,
        &mut d,
        [AccountsMsg::SetClosed {
            id: jar.clone(),
            closed: true,
        }],
    );
    let p = rt.screen(&d);
    let closed = p.shelves.iter().find(|s| s.title == "Closed Accounts").unwrap();
    assert!(closed.accounts.iter().any(|a| a.id == jar && a.close_title == "Reopen"));
    run(
        &mut rt,
        &mut d,
        [AccountsMsg::SetClosed {
            id: jar.clone(),
            closed: false,
        }],
    );
    assert!(!find(d.accounts(), &jar).unwrap().closed);
    for (t, a) in d.inbox.take() {
        rt.answer(t, a, &mut d);
    }
    assert_eq!(d.haptics.performed(), vec![Haptic::Success, Haptic::Warning, Haptic::Success]);
}

#[test]
fn copy_balance_and_refresh_reach_the_device_and_the_shell() {
    let mut d = Demo::new();
    let mut rt = Runtime::<Accounts>::new();
    let a = d.accounts()[0].clone();
    run(&mut rt, &mut d, [AccountsMsg::CopyBalance { id: a.id.hex() }, AccountsMsg::Refresh]);
    assert_eq!(d.clipboard.copied(), vec![ark_format::amount(a.balance, &NumberStyle::us())]);
    assert_eq!(d.requests, vec![crate::MoneyRequest::Refresh]);
}

#[test]
fn ids_read_back_from_hex() {
    let id: Id<Account> = Id::new([7; 16]);
    assert_eq!(id_of(&id.hex()), Some(id));
    assert_eq!(id_of("zz"), None);
}

/// Reconcile is offered for an open account and asks the shell for its
/// sheet; a closed account's is not. Falsified by dropping the menu's
/// Reconcile (the prototype's regression) or offering it for a closed one.
#[test]
fn reconcile_asks_the_shell_for_an_open_account() {
    let mut d = Demo::new();
    let mut rt = Runtime::<Accounts>::new();
    let a = d.accounts().iter().find(|a| !a.closed).unwrap().id.hex();
    run(&mut rt, &mut d, [AccountsMsg::Reconcile { id: a.clone() }]);
    assert_eq!(d.requests, vec![crate::MoneyRequest::Reconcile { account: a.clone() }]);
    let p = rt.screen(&d);
    let row = p.shelves.iter().flat_map(|s| &s.accounts).find(|r| r.id == a).unwrap();
    assert!(row.is_open);
    let v = view::account_row::view(row);
    let ark_app::view::View::Part(ark_app::view::Part::Row(r)) = v else {
        panic!("a row")
    };
    let offered: Vec<String> = r.menu.iter().filter(|m| m.offered()).map(|m| m.title.get()).collect();
    assert_eq!(offered, ["Rename", "Reconcile…", "Copy Balance", "Close Account"]);
    assert!(r.pushes, "an account's row opens its register as a pushed page");
}

/// The iPhone's amount entry says cents; the form keeps what was entered
/// last, typed or entered, and says it back as cents.
#[test]
fn an_amount_entered_in_cents_is_the_balance() {
    let mut d = Demo::new();
    let mut rt = Runtime::<Accounts>::new();
    run(
        &mut rt,
        &mut d,
        [
            AccountsMsg::Add,
            AccountsMsg::DraftKindKey { key: "credit".into() },
            AccountsMsg::DraftCents { cents: 1_250 },
        ],
    );
    let f = rt.screen(&d).adding.unwrap();
    assert_eq!(
        (f.kind_key.as_str(), f.cents, f.balance_header.as_str()),
        ("credit", 1_250, "Balance Owed")
    );
    assert_eq!(rt.state().adding.as_ref().unwrap().balance(&NumberStyle::us()), Some(-1_250));
    run(&mut rt, &mut d, [AccountsMsg::DraftBalance { typed: "3".into() }]);
    assert_eq!(rt.screen(&d).adding.unwrap().cents, 300);
}

/// Net worth's card: what is owned and owed add up to it, and its trend
/// ends at today's figure and does not take back a starting balance (the
/// mortgage was always owed). Falsified by counting the mortgage's opening
/// as a change in what the household is worth.
#[test]
fn net_worth_is_owned_less_owed_and_its_trend_ends_today() {
    let d = Demo::new();
    let rt = Runtime::<Accounts>::new();
    let p = rt.screen(&d);
    assert_eq!(p.assets + p.liabilities, p.net_worth);
    assert_eq!(p.trend.last(), Some(&p.net_worth));
    assert_eq!(p.trend.len(), 13);
    let swing = p.trend.iter().max().unwrap() - p.trend.iter().min().unwrap();
    assert!(swing < 10_000_000, "a household's twelve weeks, not its mortgage: {:?}", p.trend);
    assert_eq!(p.shelves[0].title, "Budget Accounts");
}
