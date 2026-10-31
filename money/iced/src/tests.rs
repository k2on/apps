//! The window, driven the way a person drives it: messages in, state out.
//! Nothing here opens a window — these say that the household the demo
//! seeds is the one described, and that what a button does lands in the
//! domain and comes back through its views.

use std::sync::OnceLock;

use money_domain::budget::ready_to_assign;
use money_domain::dates::{add_months, month_of};
use money_domain::*;

use crate::peer::Ledger;
use crate::{seed, App, Choice, Choosing, Field, Message, Modal, Page, Pane};

/// A day the demo is seeded for: late in a month, so every habit has
/// happened at least once in it.
const TODAY: &str = "2026-10-26";
/// Its moment, in the afternoon.
const NOW: i64 = 1_793_030_400_000;

/// The demo, seeded once per run and handed to each test as a window of its
/// own.
fn app() -> App {
    static TEMPLATE: OnceLock<ark_peer::Memory> = OnceLock::new();
    let disk = TEMPLATE.get_or_init(|| seed::seeded_disk(TODAY, NOW)).clone();
    App::with_peer(seed::open(disk), TODAY)
}

fn account<'a>(app: &'a App, name: &str) -> &'a AccountLine {
    app.peer
        .accounts
        .rows
        .iter()
        .find(|a| a.name == name)
        .unwrap_or_else(|| panic!("no account {name}"))
}

fn category(app: &App, name: &str) -> BudgetLine {
    app.peer
        .lines
        .rows
        .iter()
        .find(|l| l.name == name)
        .cloned()
        .unwrap_or_else(|| panic!("no category {name}"))
}

#[test]
fn the_household_is_seeded() {
    let app = app();
    let names: Vec<&str> = app.peer.accounts.rows.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["Checking", "Savings", "Cash", "Mortgage", "Retirement", "Chase Sapphire"]);
    let card = account(&app, "Chase Sapphire");
    assert!(card.on_budget && card.bank == seed::BANK_NAME && card.mask == "4417");
    assert!(card.balance < 0, "the card owes: {}", card.balance);
    assert_eq!(app.peer.month, "2026-10");
    let s = app.peer.summary();
    assert_eq!(s.ready_to_assign, 128_450, "what is left to assign this month");
    assert!(s.overspent >= 1, "Dining Out was given too little");
    assert!(s.underfunded >= 2, "Vacation and Gifts were given nothing");
    assert!(!app.peer.inbox.rows.is_empty(), "the last few days of the card wait for a look");
    assert!(app.peer.inbox.rows.iter().all(|t| t.date.as_str() >= "2026-10-22"));
    // The budget's figures are the domain's: groups and categories in order.
    let groups: Vec<&str> = app.peer.groups.rows.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(groups, ["Bills", "Everyday", "Quality of Life", "Savings Goals"]);
    let rent = category(&app, "Rent");
    assert_eq!((rent.assigned, rent.activity, rent.available), (185_000, -185_000, 0));
    // Last month's statement was paid, and the bank's payment matched the
    // transfer rather than doubling it.
    let card_id = card.id;
    let mut app = app;
    let _ = app.update(Message::Go(Page::Ledger(Ledger::Account(card_id))));
    let payments: Vec<&TxnLine> = app.peer.register.rows.iter().filter(|t| t.transfer).collect();
    assert_eq!(payments.len(), 2);
    assert!(payments.iter().all(|t| t.imported && t.approved));
    // Some of it Sam entered.
    let _ = app.update(Message::Go(Page::Ledger(Ledger::All)));
    assert!(app.peer.register.rows.iter().any(|t| t.user_id == seed::PARTNER));
    // And checking was reconciled at the end of last month.
    let checking = account(&app, "Checking").id;
    let _ = app.update(Message::Go(Page::Ledger(Ledger::Account(checking))));
    assert!(app.peer.register.rows.iter().any(|t| t.cleared == Cleared::Reconciled));
}

#[test]
fn the_same_household_every_load() {
    let a = seed::seeded_disk(TODAY, NOW);
    let b = seed::seeded_disk(TODAY, NOW);
    assert_eq!(seed::open(a).state(), seed::open(b).state());
}

#[test]
fn assigning_in_a_cell_moves_ready_to_assign() {
    let mut app = app();
    let groceries = category(&app, "Groceries");
    let before = app.peer.summary().ready_to_assign;
    let _ = app.update(Message::EditAssigned(groceries.category_id));
    assert_eq!(app.assigning.as_ref().map(|(_, t)| t.as_str()), Some("650.00"));
    assert_eq!(
        (app.selected, app.target.as_str()),
        (Some(groceries.category_id), "650.00"),
        "the inspector follows"
    );
    let _ = app.update(Message::AssignedTyped("700".into()));
    let _ = app.update(Message::AssignedDone);
    assert_eq!(category(&app, "Groceries").assigned, 70_000);
    assert_eq!(app.peer.summary().ready_to_assign, before - 5_000);
    // Not an amount: said, and nothing moves.
    let _ = app.update(Message::EditAssigned(groceries.category_id));
    let _ = app.update(Message::AssignedTyped("lots".into()));
    let _ = app.update(Message::AssignedDone);
    assert!(app.note.contains("not an amount"));
    assert_eq!(category(&app, "Groceries").assigned, 70_000);
}

#[test]
fn a_target_is_met_and_overspending_covered() {
    let mut app = app();
    let vacation = category(&app, "Vacation");
    let _ = app.update(Message::Select(Some(vacation.category_id)));
    assert_eq!(app.target, "250.00");
    let _ = app.update(Message::AssignTo(vacation.category_id, 25_000));
    assert_eq!(category(&app, "Vacation").available, vacation.available + 25_000);

    let dining = category(&app, "Dining Out");
    assert!(dining.available < 0, "the seed overspends it: {}", dining.available);
    let _ = app.update(Message::Select(Some(dining.category_id)));
    let _ = app.update(Message::MoveMoney);
    let short = -dining.available;
    assert!(matches!(&app.modal, Some(Modal::Choose(crate::Choosing::Move { amount, .. })) if crate::amount::parse(amount) == Some(short)));
    let ef = category(&app, "Emergency Fund");
    let _ = app.update(Message::Chose(Choice::Category(Some(ef.category_id))));
    assert_eq!(category(&app, "Dining Out").available, 0);
    assert_eq!(category(&app, "Emergency Fund").assigned, ef.assigned - short);
}

#[test]
fn a_transaction_is_entered_and_corrected() {
    let mut app = app();
    let checking = account(&app, "Checking").id;
    let _ = app.update(Message::Go(Page::Ledger(Ledger::Account(checking))));
    let before = account(&app, "Checking").balance;
    let _ = app.update(Message::AddTxn);
    // A payee paid before brings its category.
    let _ = app.update(Message::Form(Field::Payee, "Mint Mobile".into()));
    let phone = category(&app, "Phone").category_id;
    assert_eq!(app.form.as_ref().unwrap().category, Some(phone));
    let _ = app.update(Message::Form(Field::Outflow, "12.34".into()));
    let _ = app.update(Message::Form(Field::Memo, "a top-up".into()));
    let _ = app.update(Message::SaveTxn);
    assert!(app.form.is_none(), "saved: {}", app.note);
    assert_eq!(account(&app, "Checking").balance, before - 1_234);
    let t = app.peer.register.rows.iter().find(|t| t.memo == "a top-up").unwrap().clone();
    assert_eq!(
        (t.payee.as_str(), t.category.as_str(), t.user_id.as_str()),
        ("Mint Mobile", "Phone", seed::ME)
    );

    let _ = app.update(Message::EditTxn(t.id));
    let _ = app.update(Message::Form(Field::Outflow, "20".into()));
    let _ = app.update(Message::SaveTxn);
    assert_eq!(account(&app, "Checking").balance, before - 2_000);
    let _ = app.update(Message::EditTxn(t.id));
    let _ = app.update(Message::DeleteTxn(t.id));
    assert_eq!(account(&app, "Checking").balance, before);
}

#[test]
fn a_transfer_is_chosen_in_the_category_column() {
    let mut app = app();
    let (checking, savings) = (account(&app, "Checking").id, account(&app, "Savings").clone());
    let _ = app.update(Message::Go(Page::Ledger(Ledger::Account(checking))));
    let _ = app.update(Message::AddTxn);
    let _ = app.update(Message::ChooseCategory);
    let _ = app.update(Message::Chose(Choice::Transfer(savings.id)));
    let _ = app.update(Message::Form(Field::Outflow, "100".into()));
    let _ = app.update(Message::SaveTxn);
    assert_eq!(account(&app, "Savings").balance, savings.balance + 10_000);
}

#[test]
fn sync_brings_the_bank_and_posts_what_was_pending() {
    let mut app = app();
    let card = account(&app, "Chase Sapphire").id;
    let _ = app.update(Message::Go(Page::Ledger(Ledger::Account(card))));
    let before = app.peer.register.rows.len();
    let _ = app.update(Message::Sync);
    let coffee: Vec<TxnLine> = app
        .peer
        .register
        .rows
        .iter()
        .filter(|t| t.payee == "Blue Bottle Coffee" && t.date == TODAY)
        .cloned()
        .collect();
    assert_eq!(coffee.len(), 1);
    assert_eq!(
        (coffee[0].amount, coffee[0].cleared, coffee[0].approved),
        (-650, Cleared::Uncleared, false)
    );
    assert_eq!(app.peer.register.rows.len(), before + 2);
    // A category given while it was pending survives its posting.
    let dining = category(&app, "Dining Out").category_id;
    let _ = app.update(Message::Categorize(vec![coffee[0].id]));
    let _ = app.update(Message::Chose(Choice::Category(Some(dining))));
    let _ = app.update(Message::Sync);
    let posted: Vec<&TxnLine> = app
        .peer
        .register
        .rows
        .iter()
        .filter(|t| t.payee == "Blue Bottle Coffee" && t.date == TODAY)
        .collect();
    assert_eq!(posted.len(), 1, "posted in place, not doubled");
    assert_eq!(
        (posted[0].id, posted[0].amount, posted[0].cleared),
        (coffee[0].id, -775, Cleared::Cleared)
    );
    assert_eq!(posted[0].category, "Dining Out");
    assert_eq!(app.peer.register.rows.len(), before + 3);
    let _ = app.update(Message::Sync);
    assert_eq!(app.note, "Chase: nothing new at the bank");
}

#[test]
fn what_needs_a_look_is_approved() {
    let mut app = app();
    let _ = app.update(Message::Go(Page::Ledger(Ledger::Inbox)));
    let waiting: Vec<Id<Txn>> = app.peer.register.rows.iter().filter(|t| !t.approved).map(|t| t.id).collect();
    assert!(!waiting.is_empty());
    let _ = app.update(Message::Approve(waiting));
    assert!(app.peer.register.rows.iter().all(|t| t.approved && t.category != "Needs a category"));
}

#[test]
fn months_move_and_the_budget_follows() {
    let mut app = app();
    let _ = app.update(Message::Month(-1));
    assert_eq!(app.peer.month, add_months(&month_of(TODAY), -1));
    let s = app.peer.summary();
    assert_eq!(s.ready_to_assign, 0, "last month everything was assigned");
    assert!(app
        .peer
        .lines
        .rows
        .iter()
        .all(|l| l.available >= 0 || l.name == "Dining Out" || l.activity < 0));
    let _ = app.update(Message::ThisMonth);
    assert_eq!(app.peer.month, "2026-10");
}

#[test]
fn an_account_is_opened_and_a_refusal_is_said() {
    let mut app = app();
    let _ = app.update(Message::AddAccount);
    let _ = app.update(Message::AccountName("Rainy Day Jar".into()));
    let _ = app.update(Message::AccountKind("cash"));
    let _ = app.update(Message::AccountBalance("40".into()));
    let _ = app.update(Message::CreateAccount);
    assert!(app.modal.is_none());
    let before = app.peer.summary().ready_to_assign;
    assert_eq!(account(&app, "Rainy Day Jar").balance, 4_000);
    assert_eq!(before, 128_450 + 4_000);
    // Ready to Assign is not a category to assign to: the domain says so.
    let _ = app.update(Message::AssignTo(ready_to_assign(), 100));
    assert!(app.note.contains("not to it"), "{}", app.note);
}

/// The window on a server, as the build without `demo` opens it: a
/// replica that dials a money server in this process, signed in. What a
/// button does goes to the server and comes back; "Sync" is an ask the
/// server holds (`banks.sync`), not an import authored here, which nobody
/// signed in may author. Falsified by `sync` authoring the demo's import on
/// a server: the bank's role was refused and nothing was asked.
#[test]
fn on_a_server_sync_is_an_ask() {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let data = tempfile::tempdir().unwrap();
    let server = rt
        .block_on(money_server::start(money_server::Config::dev("127.0.0.1:0", data.path())))
        .unwrap();
    let url = format!("http://{}", server.running.addr);
    let login = server
        .auth
        .issue(
            &ark_auth::Account {
                id: "alice".into(),
                ..Default::default()
            },
            "a test",
        )
        .unwrap();
    let mut client = crate::client::open(Box::new(ark_peer::Memory::default()));
    client.connect(&ark_auth::socket_url(&url));
    let mut app = App::on_server(client, TODAY, url, None);
    app.signed_in(login);
    let settle = |app: &mut App, done: &dyn Fn(&App) -> bool| {
        let held = ark_peer::link::until(std::time::Duration::from_secs(10), std::time::Duration::from_millis(10), || {
            let _ = app.update(Message::Wake);
            done(app)
        });
        assert!(held, "{}", app.note);
    };
    settle(&mut app, &|a| a.peer.client.status().link == "online");

    let _ = app.update(Message::AddAccount);
    let _ = app.update(Message::AccountName("Checking".into()));
    let _ = app.update(Message::AccountBalance("1200".into()));
    let _ = app.update(Message::CreateAccount);
    settle(&mut app, &|a| a.peer.client.pending_len() == 0);
    let accounts = server.running.hub.with(|h| h.log.store.rows("account").count()).unwrap();
    assert_eq!(accounts, 1);

    let _ = app.update(Message::Sync);
    assert!(app.note.starts_with("asked the server"), "{}", app.note);
    settle(&mut app, &|a| a.peer.client.pending_len() == 0);
    let asked: Vec<Asked> = server
        .running
        .hub
        .with(|h| h.log.store.rows("asked").map(|r| ark::format::Encode::from_bytes(r).unwrap()).collect())
        .unwrap();
    assert_eq!(asked.iter().map(|a| a.user_id.as_str()).collect::<Vec<_>>(), ["alice"]);
    rt.block_on(server.stop());
}

// * The budget's keys

use arkui::vim::{Action, Motion};

/// The budget as drawn: each group's name, then its categories'.
fn drawn(app: &App) -> Vec<String> {
    app.shown()
        .iter()
        .flat_map(|s| std::iter::once(s.group.name.to_uppercase()).chain(s.rows.iter().map(|l| l.name.clone())))
        .collect()
}

/// The category under the cursor.
fn cursor(app: &App) -> String {
    let c = app.selected.expect("a cursor");
    app.peer.line(c).expect("its line").name.clone()
}

/// The rows' names, one group's.
fn group_rows(app: &App, group: usize) -> Vec<String> {
    app.shown()[group].rows.iter().map(|l| l.name.clone()).collect()
}

/// `j` and `k` walk the categories alone: off a group's last, onto the
/// next group's first, never its heading.
#[test]
fn the_cursor_walks_categories_not_groups() {
    let mut app = app();
    let first = group_rows(&app, 0);
    let second = group_rows(&app, 1);
    let _ = app.act(Action::Move(Motion::Down(1)));
    assert_eq!(cursor(&app), first[0], "the first `j` lands on the first category");
    let _ = app.act(Action::Move(Motion::Down(first.len())));
    assert_eq!(cursor(&app), second[0], "past the heading, not onto it");
    let _ = app.act(Action::Move(Motion::Up(1)));
    assert_eq!(cursor(&app), *first.last().unwrap());
}

/// `J` carries the cursor's category down past the next, then off its
/// group's end into the next group's top; `K` brings it back.
#[test]
fn shift_j_and_k_move_a_category() {
    let mut app = app();
    let first = group_rows(&app, 0);
    let _ = app.act(Action::Move(Motion::First));
    let moved = cursor(&app);
    let _ = app.act(Action::Key('J'));
    assert_eq!(group_rows(&app, 0)[1], moved, "past the next one");
    assert_eq!(cursor(&app), moved, "the cursor goes with it");
    for _ in 1..first.len() {
        let _ = app.act(Action::Key('J'));
    }
    assert_eq!(group_rows(&app, 1)[0], moved, "into the next group's top");
    let _ = app.act(Action::Key('K'));
    assert_eq!(group_rows(&app, 0).last(), Some(&moved), "back to the end of its own");
}

/// `vig` selects the cursor's group's categories; `J` then moves the
/// group, as `}` does without a selection.
#[test]
fn a_whole_group_selected_moves_the_group() {
    let mut app = app();
    let before = drawn(&app);
    let name = app.shown()[0].group.name.clone();
    let _ = app.act(Action::Move(Motion::First));
    let _ = app.act(Action::Visual(true));
    let _ = app.act(Action::Inner('g'));
    assert_eq!(app.lit(), Some((0, group_rows(&app, 0).len() - 1)), "every row of the group");
    let _ = app.act(Action::Key('J'));
    assert_eq!(app.shown()[1].group.name, name, "the group went down past the next");
    let _ = app.act(Action::Visual(false));
    let _ = app.act(Action::Key('{'));
    assert_eq!(drawn(&app), before, "and `{{` brings it back");
}

/// `cc` and `cg` open a new category and a new group; `rc` and `rg`
/// rename the cursor's.
#[test]
fn bound_sequences_make_and_rename() {
    let mut app = app();
    let _ = app.act(Action::Move(Motion::First));
    let group = app.shown()[0].group.id;
    let _ = app.act(Action::Bound("cc"));
    assert_eq!(app.new_category.as_ref().map(|(g, _)| *g), Some(group));
    let _ = app.act(Action::Bound("cg"));
    assert!(app.new_group.is_some());
    let _ = app.update(Message::Escape);
    let _ = app.act(Action::Bound("rc"));
    let _ = app.update(Message::RenameTyped("Groceries & Household".into()));
    let _ = app.update(Message::RenameDone);
    assert_eq!(cursor(&app), "Groceries & Household");
    let _ = app.act(Action::Bound("rg"));
    let _ = app.update(Message::RenameTyped("Every Month".into()));
    let _ = app.update(Message::RenameDone);
    assert_eq!(app.shown()[0].group.name, "Every Month");
}

/// The budget drawn to `$MONEY_SNAPSHOT.png`, light and dark, to be looked
/// at: `MONEY_SNAPSHOT=/tmp/budget cargo test -p money-iced the_budget_drawn
/// -- --ignored`.
#[test]
#[ignore]
fn the_budget_drawn() {
    let Ok(path) = std::env::var("MONEY_SNAPSHOT") else {
        return;
    };
    arkui::theme::install(crate::palette::MONEY);
    let mut budget = app();
    let _ = budget.update(Message::Resized(iced::Size::new(1400.0, 820.0)));
    let _ = budget.act(Action::Move(Motion::Down(3)));
    let mut register = app();
    let _ = register.update(Message::Resized(iced::Size::new(1400.0, 820.0)));
    let _ = register.update(Message::Go(Page::Ledger(Ledger::All)));
    let _ = register.register_act(Action::Move(Motion::Down(3)));
    let _ = register.register_act(Action::Visual(true));
    let _ = register.register_act(Action::Move(Motion::Down(1)));
    let _ = register.register_act(Action::Key('c'));
    let _ = register.update(Message::DropTyped("di".into()));
    let mut form = app();
    let _ = form.update(Message::Resized(iced::Size::new(1400.0, 820.0)));
    let _ = form.update(Message::Go(Page::Ledger(Ledger::All)));
    let _ = form.register_act(Action::Move(Motion::First));
    let _ = form.register_act(Action::Activate);
    let mut banks = app();
    let _ = banks.update(Message::Resized(iced::Size::new(1400.0, 820.0)));
    let _ = banks.update(Message::Go(Page::Settings));
    let mut above = app();
    let _ = above.update(Message::Resized(iced::Size::new(1400.0, 820.0)));
    let _ = above.update(Message::Go(Page::Ledger(Ledger::All)));
    let _ = above.register_act(Action::Move(Motion::To(14)));
    let _ = above.register_act(Action::Key('c'));
    let _ = above.update(Message::DropTyped("din".into()));
    for (app, scene, theme, side) in [
        (&above, "above", iced::Theme::Dark, "dark"),
        (&form, "form", iced::Theme::Dark, "dark"),
        (&budget, "budget", iced::Theme::Dark, "dark"),
        (&budget, "budget", iced::Theme::Light, "light"),
        (&register, "register", iced::Theme::Dark, "dark"),
        (&banks, "banks", iced::Theme::Dark, "dark"),
        (&banks, "banks", iced::Theme::Light, "light"),
    ] {
        let settings = iced::Settings {
            fonts: vec![arkui::hero::FIRA_SANS_BOLD.into()],
            ..iced::Settings::default()
        };
        let mut sim = iced_test::Simulator::with_size(settings, (1400.0, 820.0), app.view());
        let shot = sim.snapshot(&theme).expect("a snapshot");
        let file = format!("{path}-{scene}-{side}");
        let _ = std::fs::remove_file(format!("{file}.png"));
        shot.matches_image(&file).expect("written");
    }
}

/// `h` from the budget gives the sidebar the keys, `j` there walks the
/// places with the page following, and `l` hands the keys back.
#[test]
fn the_keys_cross_into_the_sidebar_and_back() {
    let mut app = app();
    let _ = app.act(Action::Move(Motion::Left(1)));
    assert_eq!(app.pane, Pane::Sidebar);
    let _ = app.update(Message::Key(
        iced::keyboard::Key::Character("j".into()),
        iced::keyboard::Modifiers::default(),
    ));
    assert_eq!(app.page, Page::Reflect, "the page follows the sidebar's cursor");
    let _ = app.update(Message::Key(
        iced::keyboard::Key::Character("l".into()),
        iced::keyboard::Modifiers::default(),
    ));
    assert_eq!(app.pane, Pane::Page);
}

/// `^O` goes back where the window was, and `^I` forward again.
#[test]
fn back_and_forward() {
    let mut app = app();
    let ctrl = iced::keyboard::Modifiers::CTRL;
    let _ = app.update(Message::Go(Page::Reflect));
    let _ = app.update(Message::Go(Page::Ledger(Ledger::All)));
    let _ = app.update(Message::Key(iced::keyboard::Key::Character("o".into()), ctrl));
    assert_eq!(app.page, Page::Reflect, "back");
    let _ = app.update(Message::Key(iced::keyboard::Key::Character("o".into()), ctrl));
    assert_eq!(app.page, Page::Budget, "and back again");
    let _ = app.update(Message::Key(iced::keyboard::Key::Character("i".into()), ctrl));
    assert_eq!(app.page, Page::Reflect, "forward");
}

/// In the register `v` and `j` select two transactions, `c` drops the
/// categories under the cursor's, typing narrows them, and the one chosen
/// is every selected transaction's.
#[test]
fn the_register_categorizes_a_selection_from_the_keys() {
    let mut app = app();
    let _ = app.update(Message::Go(Page::Ledger(Ledger::All)));
    let _ = app.register_act(Action::Move(Motion::First));
    let _ = app.register_act(Action::Visual(true));
    let _ = app.register_act(Action::Move(Motion::Down(1)));
    let ids = app.txn_selected();
    assert_eq!(ids.len(), 2);
    // Pressed as a key: nothing bound starts with `c` here, so it is at once.
    let _ = app.update(Message::Key(
        iced::keyboard::Key::Character("c".into()),
        iced::keyboard::Modifiers::default(),
    ));
    assert!(matches!(app.dropdown.as_ref().map(|d| &d.choosing), Some(Choosing::Txns(t)) if *t == ids));
    let _ = app.update(Message::DropTyped("groc".into()));
    let groups = app.drop_groups();
    let found: Vec<&str> = groups.iter().flat_map(|g| g.items.iter().map(|i| i.label.as_str())).collect();
    assert_eq!(found, ["Groceries"]);
    let choose = groups[0].items[0].choose.clone();
    let _ = app.update(choose);
    assert!(app.dropdown.is_none());
    let groceries = category(&app, "Groceries").category_id;
    for id in ids {
        let t = app.peer.register.rows.iter().find(|t| t.id == id).expect("still there");
        assert_eq!(t.category_id, Some(groceries), "{}", t.payee);
    }
}

/// What the bank called an imported transaction is said where it says
/// more than its payee; not for one typed in, nor one named alike.
#[test]
fn the_bank_says_what_the_payee_does_not() {
    let mut t = TxnLine {
        payee: "Chipotle".into(),
        imported: true,
        imported_payee: "CHIPOTLE 1234 SAN FRANCISCO CA".into(),
        ..TxnLine::default()
    };
    assert_eq!(crate::bank_said(&t), "CHIPOTLE 1234 SAN FRANCISCO CA");
    t.imported_payee = "CHIPOTLE".into();
    assert_eq!(crate::bank_said(&t), "", "named alike");
    t.imported_payee = "CHIPOTLE 1234".into();
    t.imported = false;
    assert_eq!(crate::bank_said(&t), "", "typed in");
}

/// A page come back to by `^O` is where it was left; a new visit is at the
/// top.
#[test]
fn back_lands_where_the_register_was_scrolled() {
    let mut app = app();
    let ctrl = iced::keyboard::Modifiers::CTRL;
    let _ = app.update(Message::Go(Page::Ledger(Ledger::All)));
    let _ = app.update(Message::RegisterScrolled(300.0));
    let _ = app.update(Message::Go(Page::Budget));
    let _ = app.update(Message::Key(iced::keyboard::Key::Character("o".into()), ctrl));
    assert_eq!(app.page, Page::Ledger(Ledger::All));
    assert_eq!(app.register_scroll, 300.0, "where it was left");
    let _ = app.update(Message::Go(Page::Reflect));
    let _ = app.update(Message::Go(Page::Ledger(Ledger::All)));
    assert_eq!(app.register_scroll, 0.0, "a visit is the top");
}

// * Settings' Banks section

use crate::banks::Msg;

/// Settings, from the account menu as harken has it, at `#settings`: the
/// demo's card's bank and the account it feeds, drawn without a panic;
/// and connecting, which needs a server, says so. `h` gives the sidebar
/// the keys, as on any page.
#[test]
fn the_demo_shows_its_bank_and_cannot_connect_one() {
    use arkui::route::Route;
    let mut app = app();
    let _ = app.update(Message::AccountMenu);
    let menu = app.ctx.menu.as_ref().expect("the account's menu");
    assert_eq!(menu.entries[0].label, "Settings");
    let _ = app.update(Message::MenuActivate);
    assert_eq!(app.page, Page::Settings);
    assert_eq!(app.place(), crate::Place::Settings);
    assert_eq!(app.place().fragment(), crate::Place::parse("settings").fragment());
    assert_eq!(crate::Place::parse("settings"), crate::Place::Settings);
    let _ = app.act(Action::Move(Motion::Left(1)));
    let _ = app.update(Message::Key(
        iced::keyboard::Key::Character("h".into()),
        iced::keyboard::Modifiers::default(),
    ));
    assert_eq!(app.pane, Pane::Sidebar);
    assert_eq!(app.peer.banks.rows.len(), 1);
    let card = app.peer.linked.rows.first().expect("the card's link");
    assert!(!card.waiting && !card.account.is_empty());
    drop(app.view());
    let _ = app.update(Message::Banks(Msg::Connect));
    assert!(app.note.contains("no server"), "{}", app.note);
    assert!(app.peer.asks.rows.is_empty() && app.opened.is_empty());
}

/// The live window on a server in this process, with a peer standing in
/// for its importer (the `bank` role): "Connect a bank" is an ask; the
/// importer's answer with Plaid's page is opened as soon as it arrives;
/// the bank connected, its accounts wait, and the card is given a credit
/// account of its own and checking linked to the account kept by hand;
/// Reconnect is an ask for the same bank, and Disconnect one, asked twice.
/// Falsified by opening every waiting ask's page: the stale page opened
/// again at the second wake.
#[test]
fn on_a_server_a_bank_is_connected_from_the_banks_page() {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let data = tempfile::tempdir().unwrap();
    let server = rt
        .block_on(money_server::start(money_server::Config::dev("127.0.0.1:0", data.path())))
        .unwrap();
    let url = format!("http://{}", server.running.addr);
    let issue = |id: &str| {
        let account = ark_auth::Account {
            id: id.into(),
            ..Default::default()
        };
        server.auth.issue(&account, "a test").unwrap()
    };
    let mut client = crate::client::open(Box::new(ark_peer::Memory::default()));
    client.connect(&ark_auth::socket_url(&url));
    let mut app = App::on_server(client, TODAY, url.clone(), None);
    app.signed_in(issue("alice"));
    // The importer, by its account: the server grants it the bank's role.
    let bank_login = issue(money_server::importer::ACCOUNT);
    let mut importer = crate::client::open(Box::new(ark_peer::Memory::default()));
    importer.connect(&ark_auth::socket_url(&url));
    importer.login(ark_peer::who(&bank_login), bank_login.token.clone());
    let settle = |app: &mut App, importer: &mut crate::client::Replica, done: &dyn Fn(&App) -> bool| {
        let held = ark_peer::link::until(std::time::Duration::from_secs(10), std::time::Duration::from_millis(10), || {
            let _ = app.update(Message::Wake);
            importer.pump();
            done(app)
        });
        assert!(held, "{}", app.note);
    };
    settle(&mut app, &mut importer, &|a| a.peer.client.status().link == "online");
    let open = OpenAccount {
        name: "Checking".into(),
        kind: "checking".into(),
        on_budget: true,
        balance: 50_000,
        date: TODAY.into(),
    };
    app.author("open", "open_account", &open);

    let _ = app.update(Message::Banks(Msg::Connect));
    assert_eq!(app.page, Page::Settings);
    assert!(app.awaiting.is_some());
    settle(&mut app, &mut importer, &|a| {
        a.peer.asks.rows.first().is_some_and(|x| x.what == "connect")
    });
    assert!(app.opened.is_empty(), "nothing to open before the server answers");
    let ask = app.peer.asks.rows[0].id;
    let answer = |importer: &mut crate::client::Replica, a: Answer| {
        importer.mutate("answer", &a).expect("the importer answers");
    };
    let held = ark_peer::link::until(std::time::Duration::from_secs(10), std::time::Duration::from_millis(10), || {
        importer.pump();
        importer.log.store.rows("bank_ask").count() == 1
    });
    assert!(held, "the importer hears the ask");
    answer(
        &mut importer,
        Answer {
            id: ask,
            state: AskState::Waiting,
            url: "https://hosted.plaid.test/link-1".into(),
            expires_ms: ark_peer::now_ms() + 600_000,
            ..Answer::default()
        },
    );
    settle(&mut app, &mut importer, &|a| !a.opened.is_empty());
    assert_eq!(app.opened, ["https://hosted.plaid.test/link-1"]);
    drop(app.view());
    settle(&mut app, &mut importer, &|_| true);
    assert_eq!(app.opened.len(), 1, "opened once");

    answer(
        &mut importer,
        Answer {
            id: ask,
            state: AskState::Done,
            bank: "wells-fargo".into(),
            institution: "Wells Fargo".into(),
            ..Answer::default()
        },
    );
    let plaid = |id: &str, name: &str, kind: &str, subtype: &str| money_domain::plaid::PlaidAccount {
        account_id: id.into(),
        name: name.into(),
        mask: Some("4321".into()),
        kind: kind.into(),
        subtype: Some(subtype.into()),
        balances: money_domain::plaid::Balances { current: Some(300.0) },
    };
    let accounts = [
        plaid("wf-chk", "Everyday Checking", "depository", "checking"),
        plaid("wf-card", "Active Cash Card", "credit", "credit card"),
    ];
    let import = Import {
        choose: true,
        from_start: true,
        ..money_domain::plaid::import_of("wells-fargo", "Wells Fargo", &accounts, &Default::default())
    };
    importer.mutate("import", &import).expect("the importer imports");
    settle(&mut app, &mut importer, &|a| a.peer.linked.rows.len() == 2);
    assert_eq!(app.banks_wanting(), 2, "two accounts wait for a choice");
    drop(app.view());

    let _ = app.update(Message::Banks(Msg::Open("wf-card".into())));
    let _ = app.update(Message::Banks(Msg::LinkTo("wf-chk".into())));
    assert_eq!(app.modal, Some(Modal::LinkTo("wf-chk".into())));
    drop(app.view());
    let checking = account(&app, "Checking").id;
    let _ = app.update(Message::Banks(Msg::Link("wf-chk".into(), checking)));
    settle(&mut app, &mut importer, &|a| a.peer.client.pending_len() == 0 && a.banks_wanting() == 0);
    let card = account(&app, "Active Cash Card");
    assert_eq!(card.kind, "credit");
    assert_eq!(account(&app, "Checking").bank, "wells-fargo");

    let _ = app.update(Message::Banks(Msg::Reconnect("wells-fargo".into())));
    settle(&mut app, &mut importer, &|a| {
        a.peer.asks.rows.first().is_some_and(|x| x.what == "reconnect")
    });
    assert_eq!(app.peer.asks.rows[0].bank, "wells-fargo");
    let _ = app.update(Message::Banks(Msg::Disconnect("wells-fargo".into())));
    assert_eq!(app.modal, Some(Modal::Disconnect("wells-fargo".into())));
    drop(app.view());
    let _ = app.update(Message::Banks(Msg::Disconnected("wells-fargo".into())));
    settle(&mut app, &mut importer, &|a| a.peer.asks.rows.iter().any(|x| x.what == "disconnect"));
    let order: Vec<(&str, i64)> = app.peer.asks.rows.iter().map(|x| (x.what.as_str(), x.asked_ms)).collect();
    assert_eq!(order[0].0, "disconnect", "newest first: {order:?}");
    assert_eq!(app.opened.len(), 1, "a disconnection opens no page");
    rt.block_on(server.stop());
}
