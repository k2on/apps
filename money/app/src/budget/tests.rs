use super::*;
use ark_app::{Ask, Host, Inbox, Modules, Runtime, Ticket};
use ark_peer::store::Store;
use ark_peer::Peer;

/// 2026-10-26, noon UTC.
const NOW: i64 = 1_792_929_600_000;
const MONTH: &str = "2026-10";

/// The demo's household on 2026-10-26, the month's views held.
struct Demo {
    peer: Peer,
    store: Store<MoneyView>,
    haptics: ark_haptics::fake::Fake,
    modules: Modules,
    inbox: Inbox<BudgetMsg>,
    opened: Vec<Route>,
    requests: Vec<crate::MoneyRequest>,
}

impl Demo {
    fn new() -> (Demo, Runtime<Budget>) {
        let mut peer = money_core::demo::open(money_core::demo::seeded_disk("2026-10-26", NOW));
        let mut store = Store::new();
        let state = State {
            month: MONTH.into(),
            ..State::default()
        };
        for k in Budget::holds(&state) {
            store.hold(&mut peer, k);
        }
        store.settle(&mut peer);
        let haptics = ark_haptics::fake::Fake::new();
        let mut modules = Modules::new();
        haptics.register(&mut modules);
        let mut d = Demo {
            peer,
            store,
            haptics,
            modules,
            inbox: Inbox::default(),
            opened: vec![],
            requests: vec![],
        };
        let mut rt = Runtime::<Budget>::new();
        rt.dispatch(BudgetMsg::Month { month: MONTH.into() }, &mut d);
        (d, rt)
    }

    fn lines(&self) -> &[BudgetLine] {
        self.store.rows(&MoneyView::Lines { month: MONTH.into() })
    }

    fn line(&self, name: &str) -> &BudgetLine {
        self.lines().iter().find(|l| l.name == name).unwrap_or_else(|| panic!("no {name}"))
    }
}

impl Host<Budget> for Demo {
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
    fn ask(&mut self, ticket: Ticket, ask: Ask<BudgetMsg>) {
        self.inbox.ask(&self.modules, ticket, ask)
    }
    fn request(&mut self, r: crate::MoneyRequest) {
        self.requests.push(r)
    }
}

fn run(rt: &mut Runtime<Budget>, d: &mut Demo, msgs: impl IntoIterator<Item = BudgetMsg>) {
    for m in msgs {
        rt.dispatch(m, d);
        loop {
            let answers = d.inbox.take();
            if answers.is_empty() {
                break;
            }
            for (t, a) in answers {
                rt.answer(t, a, d);
            }
        }
    }
}

fn page(rt: &Runtime<Budget>, d: &Demo) -> BudgetPage {
    rt.screen(d)
}

fn item<'a>(p: &'a BudgetPage, name: &str) -> &'a CategoryItem {
    p.groups
        .iter()
        .flat_map(|g| &g.lines)
        .find(|l| l.title.ends_with(name))
        .unwrap_or_else(|| panic!("no row {name}"))
}

/// The month as the page has it: its title, Ready to Assign worded as the
/// month's summary says, every visible category under its group in order,
/// each group's totals the sum of its rows'. Falsified by a group's total
/// taken from another, a category in the wrong group, or the cards' group
/// anywhere but first.
#[test]
fn the_month_is_laid_out_by_group() {
    let (d, rt) = Demo::new();
    let p = page(&rt, &d);
    assert_eq!(p.title, "October 2026");
    let summary: &[Summary] = d.store.rows(&MoneyView::Summary { month: MONTH.into() });
    assert_eq!(p.ready, ark_format::amount(summary[0].ready_to_assign, &NumberStyle::us()));
    assert!(["Ready to Assign", "Assigned Too Much"].contains(&p.ready_label.as_str()));
    assert!(!p.groups.is_empty());
    let cards = money_core::cards::card_group().hex();
    if let Some(i) = p.groups.iter().position(|g| g.id == cards) {
        assert_eq!(i, 0, "the cards' group comes first");
        assert!(!p.groups[0].own);
    }
    for g in &p.groups {
        let lines: Vec<&BudgetLine> = d.lines().iter().filter(|l| l.group_id.hex() == g.id && !l.hidden).collect();
        assert_eq!(g.lines.len(), lines.len(), "{}", g.title);
        assert_eq!(g.available, lines.iter().map(|l| l.available).sum::<i64>());
        assert_eq!(g.assigned, lines.iter().map(|l| l.assigned).sum::<i64>());
        assert!(g.open);
    }
    let groceries = item(&p, "Groceries");
    assert_eq!(groceries.title, format!("{} Groceries", crate::look::category_emoji("Groceries")));
    assert!(groceries.own && groceries.assignable);
}

/// A group folded and unfolded by its heading. Falsified by Toggle
/// folding every group, or not unfolding again.
#[test]
fn a_group_folds() {
    let (mut d, mut rt) = Demo::new();
    let id = page(&rt, &d).groups.last().map(|g| g.id.clone()).unwrap_or_default();
    run(&mut rt, &mut d, [BudgetMsg::Toggle { id: id.clone() }]);
    let p = page(&rt, &d);
    assert!(p.groups.iter().all(|g| g.open == (g.id != id)));
    run(&mut rt, &mut d, [BudgetMsg::Toggle { id }]);
    assert!(page(&rt, &d).groups.iter().all(|g| g.open));
}

/// Assign's sheet: raised with what the category has been given, a new
/// amount typed and kept assigns it, the sheet goes and the success is
/// felt; what is not an amount cannot be kept. Falsified by the sheet
/// staying up, the amount not assigned, or a haptic missed.
#[test]
fn assigning_from_the_sheet() {
    let (mut d, mut rt) = Demo::new();
    let id = d.line("Groceries").category_id.hex();
    run(&mut rt, &mut d, [BudgetMsg::Assign { id: id.clone() }]);
    let f = page(&rt, &d).assigning.expect("the sheet is up");
    assert_eq!(f.subject, "Groceries");
    run(&mut rt, &mut d, [BudgetMsg::Typed { typed: "twelve".into() }]);
    assert!(!page(&rt, &d).assigning.unwrap().can_save);
    run(&mut rt, &mut d, [BudgetMsg::Typed { typed: "712.34".into() }, BudgetMsg::Save]);
    assert_eq!(d.line("Groceries").assigned, 71_234);
    assert!(page(&rt, &d).assigning.is_none());
    assert_eq!(d.haptics.performed(), vec![Haptic::Success]);
}

/// Move Money from a category to Ready to Assign, and Cover Overspending
/// raised into an overspent one from Ready to Assign. Falsified by the
/// ends the wrong way round, or money moved to where it came from.
#[test]
fn moving_money() {
    let (mut d, mut rt) = Demo::new();
    let g = d
        .lines()
        .iter()
        .find(|l| l.available > 1_000 && l.assignable())
        .expect("one with money")
        .clone();
    let over = d.lines().iter().find(|l| l.available < 0 && l.assignable()).cloned();
    run(&mut rt, &mut d, [BudgetMsg::Move { id: g.category_id.hex() }]);
    let m = page(&rt, &d).moving.expect("the sheet is up");
    assert_eq!((m.from.as_str(), m.to.as_str()), (g.category_id.hex().as_str(), ""));
    run(&mut rt, &mut d, [BudgetMsg::MoveTo { key: g.category_id.hex() }]);
    assert!(!page(&rt, &d).moving.unwrap().amount.can_save, "nothing moves to where it is");
    run(
        &mut rt,
        &mut d,
        [
            BudgetMsg::MoveTo { key: String::new() },
            BudgetMsg::Typed { typed: "10".into() },
            BudgetMsg::Save,
        ],
    );
    assert_eq!(d.line(&g.name).available, g.available - 1_000);
    assert!(page(&rt, &d).moving.is_none());
    if let Some(o) = over {
        run(&mut rt, &mut d, [BudgetMsg::Move { id: o.category_id.hex() }]);
        let m = page(&rt, &d).moving.expect("cover is up");
        assert_eq!((m.from.as_str(), m.to.as_str()), ("", o.category_id.hex().as_str()));
        assert_eq!(m.amount.cents, -o.available);
    }
}

/// Auto-Assign asks first, then Reset takes back every assignment the
/// month made by hand. Falsified by assigning before the question is
/// answered, or the question staying up.
#[test]
fn auto_assign_asks_then_does() {
    let (mut d, mut rt) = Demo::new();
    run(&mut rt, &mut d, [BudgetMsg::AutoAssign]);
    assert!(page(&rt, &d).auto);
    assert_ne!(d.line("Groceries").assigned, 0);
    run(&mut rt, &mut d, [BudgetMsg::AutoReset]);
    assert!(!page(&rt, &d).auto);
    assert_eq!(d.line("Groceries").assigned, 0);
    run(&mut rt, &mut d, [BudgetMsg::AutoAssign, BudgetMsg::CancelAuto]);
    assert!(!page(&rt, &d).auto);
}

/// A target set from its sheet, Meet Target assigning what it lacks.
/// Falsified by the target not kept, or Meet Target giving the whole
/// target on top of what was assigned.
#[test]
fn a_target_set_and_met() {
    let (mut d, mut rt) = Demo::new();
    let id = d.line("Groceries").category_id.hex();
    run(
        &mut rt,
        &mut d,
        [
            BudgetMsg::Target { id: id.clone() },
            BudgetMsg::Typed { typed: "900".into() },
            BudgetMsg::Save,
        ],
    );
    assert_eq!(d.line("Groceries").target, 90_000);
    assert!(item(&page(&rt, &d), "Groceries").can_meet);
    run(&mut rt, &mut d, [BudgetMsg::MeetTarget { id }]);
    assert_eq!(d.line("Groceries").assigned, 90_000);
    assert!(!item(&page(&rt, &d), "Groceries").can_meet);
}

/// A group made from New Group's alert, a category added to it from New
/// Category's sheet (the domain's sentence while the name is wrong),
/// renamed, hidden (and shown with hidden ones), deleted. Falsified by
/// any of them not reaching the budget.
#[test]
fn groups_and_categories_made_and_put_away() {
    let (mut d, mut rt) = Demo::new();
    run(
        &mut rt,
        &mut d,
        [BudgetMsg::NewGroup, BudgetMsg::Named { name: "Pets".into() }, BudgetMsg::SaveName],
    );
    let groups: &[GroupLine] = d.store.rows(&MoneyView::Groups);
    let pets = groups.iter().find(|g| g.name == "Pets").expect("Pets made").id.hex();
    let p = page(&rt, &d);
    assert!(
        p.groups.iter().any(|g| g.id == pets && g.empty),
        "an empty group is offered a first category"
    );
    run(&mut rt, &mut d, [BudgetMsg::NewCategory { group: pets.clone() }]);
    assert_eq!(page(&rt, &d).adding.unwrap().group, pets);
    run(&mut rt, &mut d, [BudgetMsg::CategoryName { name: "Vet".into() }, BudgetMsg::Save]);
    assert!(d.lines().iter().any(|l| l.name == "Vet" && l.group_id.hex() == pets));
    run(
        &mut rt,
        &mut d,
        [
            BudgetMsg::RenameGroup { id: pets.clone() },
            BudgetMsg::Named { name: "Animals".into() },
            BudgetMsg::SaveName,
        ],
    );
    assert!(page(&rt, &d).groups.iter().any(|g| g.title == "Animals"));
    let vet = d.line("Vet").category_id.hex();
    run(&mut rt, &mut d, [BudgetMsg::HideCategory { id: vet.clone() }]);
    assert!(page(&rt, &d).groups.iter().flat_map(|g| &g.lines).all(|l| l.id != vet));
    run(&mut rt, &mut d, [BudgetMsg::ToggleHidden]);
    assert!(item(&page(&rt, &d), "Vet").faded);
    run(
        &mut rt,
        &mut d,
        [BudgetMsg::DeleteCategory { id: vet }, BudgetMsg::DeleteGroup { id: pets.clone() }],
    );
    assert!(page(&rt, &d).groups.iter().all(|g| g.id != pets));
}

/// A category stepped down and back up its group; the first offers no Up,
/// the last no Down. Falsified by a step landing two places away.
#[test]
fn a_category_moves_a_place() {
    let (mut d, mut rt) = Demo::new();
    let p = page(&rt, &d);
    let g = p.groups.iter().find(|g| g.own && g.lines.len() >= 3).expect("a group of three");
    let order = |p: &BudgetPage| {
        p.groups
            .iter()
            .find(|x| x.id == g.id)
            .unwrap()
            .lines
            .iter()
            .map(|l| l.id.clone())
            .collect::<Vec<_>>()
    };
    let before = order(&p);
    assert!(!g.lines[0].can_up && g.lines[0].can_down);
    assert!(!g.lines.last().unwrap().can_down);
    run(&mut rt, &mut d, [BudgetMsg::CategoryDown { id: before[0].clone() }]);
    let after = order(&page(&rt, &d));
    assert_eq!((&after[0], &after[1]), (&before[1], &before[0]));
    run(&mut rt, &mut d, [BudgetMsg::CategoryUp { id: before[0].clone() }]);
    assert_eq!(order(&page(&rt, &d)), before);
}

/// A row tapped opens its category's month; stepping the month is felt
/// as a selection, the first month given is not. Falsified by a route of
/// another category, or a haptic on the first month.
#[test]
fn a_row_opens_and_a_month_steps() {
    let (mut d, mut rt) = Demo::new();
    assert!(d.haptics.performed().is_empty());
    let id = d.line("Groceries").category_id.hex();
    run(&mut rt, &mut d, [BudgetMsg::Open { id: id.clone() }]);
    assert_eq!(
        d.opened,
        vec![Route::Category {
            id,
            name: "Groceries".into()
        }]
    );
    run(&mut rt, &mut d, [BudgetMsg::Month { month: "2026-09".into() }]);
    assert_eq!(d.haptics.performed(), vec![Haptic::Selection]);
    assert_eq!(page(&rt, &d).title, "September 2026");
    run(&mut rt, &mut d, [BudgetMsg::Refresh]);
    assert_eq!(d.requests, vec![crate::MoneyRequest::Refresh]);
}
