//! The tree of categories, split transactions, and receipts: a store's
//! items filed under the tree, and splitting what paid for them.

mod common;

use common::*;
use money_domain::*;

/// Checking, and a budget of two groups: Everyday (Groceries, and Eggs a
/// kind of it) and Home (Home Improvement).
fn household() -> (Book, Id<Account>, Id<Category>, Id<Category>, Id<Category>) {
    let mut b = Book::new();
    let checking = b.account("Checking", "checking", 300_000);
    let everyday = b.group("Everyday");
    let groceries = b.category(everyday, "Groceries");
    let home = b.group("Home");
    let improvement = b.category(home, "Home Improvement");
    let eggs = b.kind(groceries, "Eggs");
    (b, checking, groceries, eggs, improvement)
}

fn txn(b: &Book, payee: &str) -> TxnLine {
    b.register(None).into_iter().find(|l| l.payee == payee).expect("the transaction")
}

fn splits(b: &Book, id: Id<Txn>) -> Vec<(String, i64)> {
    let lines: Vec<SplitLine> = b.list("splits", &OneTxn { id });
    lines.into_iter().map(|l| (l.category, l.amount)).collect()
}

/// A kind's spending is its envelope's; money is given to the envelope,
/// not to a kind and not to a group; the budget draws the envelopes alone,
/// as it did.
#[test]
fn a_kind_spends_from_the_category_it_is_a_kind_of() {
    let (mut b, checking, groceries, eggs, _) = household();
    b.assign("2026-10", groceries, 20_000);
    b.spend(checking, "2026-10-03", "Farm stand", Some(eggs), -600);
    b.spend(checking, "2026-10-04", "Corner shop", Some(groceries), -1_000);
    let l = b.line("2026-10", groceries);
    assert_eq!((l.assigned, l.activity, l.available), (20_000, -1_600, 18_400));
    let refused = b
        .run(
            "assign",
            &Assign {
                month: "2026-10".into(),
                category_id: eggs,
                amount: 1,
            },
        )
        .unwrap_err();
    assert!(refused.contains("assigned to the category it is a kind of"), "{refused}");
    let names: Vec<String> = b
        .list::<BudgetLine>("month", &Month { month: "2026-10".into() })
        .into_iter()
        .map(|l| l.name)
        .collect();
    assert_eq!(names, ["Groceries", "Home Improvement"]);
    let groups: Vec<String> = b.list::<GroupLine>("groups", &()).into_iter().map(|g| g.name).collect();
    assert_eq!(groups, ["Everyday", "Home"]);
    let tree: Vec<(String, i64)> = b
        .list::<CategoryNode>("tree", &())
        .into_iter()
        .filter(|n| !n.income)
        .map(|n| (n.path, n.depth))
        .collect();
    assert_eq!(
        tree,
        [
            ("Everyday".to_string(), 0),
            ("Everyday › Groceries".to_string(), 1),
            ("Everyday › Groceries › Eggs".to_string(), 2),
            ("Home".to_string(), 0),
            ("Home › Home Improvement".to_string(), 1),
        ]
    );
    // Reflect counts a kind's spending in its group.
    let spent: Vec<(String, i64)> = b
        .list::<SpendingLine>("spending", &Month { month: "2026-10".into() })
        .into_iter()
        .map(|s| (s.group, s.spent))
        .collect();
    assert_eq!(spent, [("Everyday".to_string(), 1_600)]);
}

/// A split is by amounts, receipt or none; its lines add up to the whole;
/// a line for nothing yet leaves it in the inbox; one category is no split.
#[test]
fn a_transaction_splits_by_amounts() {
    let (mut b, checking, groceries, _, improvement) = household();
    b.spend(checking, "2026-10-25", "Target", None, -8_740);
    let id = txn(&b, "Target").id;
    let part = |c, amount| SplitPart {
        category_id: c,
        amount,
        memo: String::new(),
    };
    let short = b
        .run(
            "split",
            &SplitTxn {
                id,
                lines: vec![part(Some(groceries), -4_000), part(Some(improvement), -4_000)],
            },
        )
        .unwrap_err();
    assert!(short.contains("add up to its amount"), "{short}");
    b.run(
        "split",
        &SplitTxn {
            id,
            lines: vec![part(Some(groceries), -4_210), part(Some(improvement), -3_810), part(None, -720)],
        },
    )
    .unwrap();
    assert_eq!(txn(&b, "Target").category, "Needs a category");
    assert!(b.list::<TxnLine>("inbox", &()).iter().any(|l| l.id == id));
    assert_eq!(b.line("2026-10", groceries).activity, -4_210);
    assert_eq!(b.line("2026-10", improvement).activity, -3_810);
    b.run(
        "split",
        &SplitTxn {
            id,
            lines: vec![part(Some(groceries), -4_930), part(Some(improvement), -3_810)],
        },
    )
    .unwrap();
    assert_eq!(txn(&b, "Target").category, "Split (2)");
    // Its amount is its lines'; a category for it is no split.
    let edit = |category_id, amount| EditTransaction {
        transfer_to: None,
        id,
        date: "2026-10-26".into(),
        payee: "Target".into(),
        category_id,
        memo: String::new(),
        amount,
    };
    assert!(b.run("edit_transaction", &edit(None, -9_000)).unwrap_err().contains("split"));
    b.run("edit_transaction", &edit(None, -8_740)).unwrap();
    assert!(b.rows::<Split>("split").iter().all(|s| s.date == "2026-10-26"));
    b.run(
        "categorize",
        &Categorize {
            ids: vec![id],
            category_id: Some(groceries),
        },
    )
    .unwrap();
    assert!(b.rows::<Split>("split").is_empty());
    assert_eq!(
        (b.line("2026-10", groceries).activity, b.line("2026-10", improvement).activity),
        (-8_740, 0)
    );
    b.run(
        "split",
        &SplitTxn {
            id,
            lines: vec![part(Some(groceries), -4_930), part(Some(improvement), -3_810)],
        },
    )
    .unwrap();
    b.run("delete_transactions", &Txns { ids: vec![id] }).unwrap();
    assert!(b.rows::<Split>("split").is_empty());
}

/// A transaction added split, refused whole when its lines do not add up.
#[test]
fn a_transaction_is_split_as_it_is_added() {
    let (mut b, checking, groceries, eggs, improvement) = household();
    let part = |c, amount| SplitPart {
        category_id: c,
        amount,
        memo: String::new(),
    };
    let add = |lines: Vec<SplitPart>| AddSplit {
        account_id: checking,
        date: "2026-10-25".into(),
        payee: "Target".into(),
        memo: String::new(),
        amount: -8_740,
        cleared: false,
        lines,
    };
    let short = b
        .run("add_split", &add(vec![part(Some(groceries), -100), part(Some(improvement), -100)]))
        .unwrap_err();
    assert!(short.contains("add up to its amount"), "{short}");
    assert!(b.register(None).iter().all(|t| t.payee != "Target"), "refused whole");
    b.run("add_split", &add(vec![part(Some(eggs), -4_370), part(Some(improvement), -4_370)]))
        .unwrap();
    let t = txn(&b, "Target");
    assert_eq!((t.category.as_str(), t.approved), ("Split (2)", true));
    assert_eq!(
        (b.line("2026-10", groceries).activity, b.line("2026-10", improvement).activity),
        (-4_370, -4_370)
    );
}

fn target_receipt(items: &[(&str, &str, i64)], total: i64) -> ReceiptImport {
    ReceiptImport {
        store: "target".into(),
        receipts: vec![StoreReceipt {
            order_id: "9120-0045".into(),
            date: "2026-10-25".into(),
            place: "Target Maplewood".into(),
            total,
            items: items
                .iter()
                .map(|(name, sku, amount)| StoreItem {
                    name: name.to_string(),
                    sku: sku.to_string(),
                    quantity: 1000,
                    unit: "ea".into(),
                    amount: *amount,
                })
                .collect(),
        }],
    }
}

/// A Target receipt splits the Target charge it matches: eggs filed by
/// their name, paint unfiled — and so where Target's charges go, Groceries
/// — tax spread in proportion. Filing the paint splits it again, and files
/// the same item on every other receipt.
#[test]
fn a_receipt_splits_what_paid_for_it() {
    let (mut b, checking, groceries, eggs, improvement) = household();
    b.spend(checking, "2026-10-01", "Target", Some(groceries), -2_000);
    b.from_bank(checking, "2026-10-25", "Target", Some(groceries), -8_740);
    let id = b.register(None).into_iter().find(|l| l.amount == -8_740).unwrap().id;
    let r = target_receipt(
        &[
            ("GG LARGE EGGS 12CT", "14713534", -429),
            ("DUTCH BOY INT PAINT", "53212", -3_599),
            ("MYSTERY THING", "", -4_181),
        ],
        -8_740,
    );
    b.run("add_receipts", &r).unwrap();
    let receipt = &b.list::<ReceiptLine>("receipts", &())[0];
    assert_eq!((receipt.txn_id, receipt.items, receipt.filed), (Some(id), 3, 1));
    assert_eq!(
        splits(&b, id),
        [
            ("Everyday › Groceries › Eggs".to_string(), -457),
            ("Everyday › Groceries".to_string(), -8_283)
        ]
    );
    assert_eq!(b.line("2026-10", groceries).activity, -10_740);
    // Said again, the same receipt changes nothing.
    b.run("add_receipts", &r).unwrap();
    assert_eq!(b.last.len(), 0);
    b.run(
        "file_items",
        &ClassifyItems {
            items: vec![ItemRef {
                receipt_id: "target:9120-0045".into(),
                line: 2,
            }],
            category_id: Some(improvement),
        },
    )
    .unwrap();
    assert_eq!(
        splits(&b, id),
        [
            ("Everyday › Groceries › Eggs".to_string(), -457),
            ("Home › Home Improvement".to_string(), -3_832),
            ("Everyday › Groceries".to_string(), -4_451),
        ]
    );
    assert_eq!(b.line("2026-10", improvement).activity, -3_832);
    // The next receipt with that paint is filed by what was said of it.
    let mut again = target_receipt(&[("DUTCH BOY INT PAINT", "53212", -3_599)], -3_900);
    again.receipts[0].order_id = "9120-0046".into();
    b.run("add_receipts", &again).unwrap();
    let paint: Vec<ItemLine> = b.list(
        "items",
        &ItemsIn {
            category_id: Some(improvement),
        },
    );
    assert_eq!(paint.len(), 2);
    // Waiting for its charge: typed in, it is paid, and all one category
    // is that category, not a split.
    assert_eq!(
        b.list::<ReceiptLine>("receipts", &())
            .iter()
            .find(|r| r.id == "target:9120-0046")
            .unwrap()
            .txn_id,
        None
    );
    b.spend(checking, "2026-10-27", "Target", None, -3_900);
    let t = b.register(None).into_iter().find(|l| l.amount == -3_900).unwrap();
    assert_eq!(t.category, "Home Improvement");
    let _ = eggs;
}

/// Eggs are Eggs from every store: Costco's filed by the name, Amazon's by
/// somebody, and each store's next eggs by what was said before.
#[test]
fn eggs_from_every_store_are_one_category() {
    let (mut b, _, groceries, eggs, _) = household();
    let one = |store: &str, id: &str, date: &str, name: &str, sku: &str, quantity: i64, amount: i64| ReceiptImport {
        store: store.into(),
        receipts: vec![StoreReceipt {
            order_id: id.into(),
            date: date.into(),
            place: String::new(),
            total: amount,
            items: vec![StoreItem {
                name: name.into(),
                sku: sku.into(),
                quantity,
                unit: "ea".into(),
                amount,
            }],
        }],
    };
    b.run(
        "add_receipts",
        &one("costco", "211343", "2026-08-03", "KS ORGANIC EGGS 24CT", "1234", 2000, -1_398),
    )
    .unwrap();
    b.run(
        "add_receipts",
        &one("amazon", "112-1", "2026-10-02", "Vital Farms Pasture-Raised, 12ct", "B07X", 1000, -649),
    )
    .unwrap();
    let unfiled: Vec<ItemLine> = b.list("items", &ItemsIn { category_id: None });
    assert_eq!(unfiled.len(), 1);
    b.run(
        "file_items",
        &ClassifyItems {
            items: vec![ItemRef {
                receipt_id: "amazon:112-1".into(),
                line: 1,
            }],
            category_id: Some(eggs),
        },
    )
    .unwrap();
    b.run(
        "add_receipts",
        &one("amazon", "112-2", "2026-10-20", "Vital Farms Pasture-Raised, 12ct", "B07X", 1000, -699),
    )
    .unwrap();
    let all: Vec<(String, String, i64)> = b
        .list::<ItemLine>(
            "items",
            &ItemsIn {
                category_id: Some(groceries),
            },
        )
        .into_iter()
        .map(|i| (i.store, i.category, i.unit_price))
        .collect();
    assert_eq!(
        all,
        [
            ("amazon".to_string(), "Everyday › Groceries › Eggs".to_string(), -699),
            ("amazon".to_string(), "Everyday › Groceries › Eggs".to_string(), -649),
            ("costco".to_string(), "Everyday › Groceries › Eggs".to_string(), -699),
        ]
    );
}

/// An Amazon order of eggs and a sponge, shipped in two and charged as
/// each shipped, then the sponge sent back: no charge is the order's whole,
/// so nothing pays it whole; each charge finds its own transaction, the one
/// that arrives after its charge too, and each is split by the order's
/// items, its own amount shared. Said again, nothing changes.
#[test]
fn an_order_charged_in_pieces_is_paid_by_each() {
    let (mut b, checking, groceries, eggs, _) = household();
    let household_cat = b.category(b.line("2026-10", groceries).group_id, "Household");
    b.spend(checking, "2026-10-04", "AMZN Mktp US", None, -1_003);
    let order = ReceiptImport {
        store: "amazon".into(),
        receipts: vec![StoreReceipt {
            order_id: "112-1".into(),
            date: "2026-10-02".into(),
            place: "Amazon.com".into(),
            total: -1_653,
            items: vec![
                StoreItem {
                    name: "Vital Farms Eggs, 12ct".into(),
                    sku: "B07X".into(),
                    quantity: 1000,
                    unit: "ea".into(),
                    amount: -649,
                },
                StoreItem {
                    name: "Scotch-Brite Sponges".into(),
                    sku: "B000".into(),
                    quantity: 2000,
                    unit: "ea".into(),
                    amount: -900,
                },
            ],
        }],
    };
    b.run("add_receipts", &order).unwrap();
    b.run(
        "file_items",
        &ClassifyItems {
            items: vec![ItemRef {
                receipt_id: "amazon:112-1".into(),
                line: 2,
            }],
            category_id: Some(household_cat),
        },
    )
    .unwrap();
    let charge = |date: &str, amount| StoreCharge {
        order_id: "112-1".into(),
        date: date.into(),
        amount,
        card: "Visa ending in 4417".into(),
    };
    let history = ChargeImport {
        store: "amazon".into(),
        charges: vec![charge("2026-10-03", -1_003), charge("2026-10-05", -650), charge("2026-10-20", 450)],
    };
    b.run("add_charges", &history).unwrap();
    let r = &b.list::<ReceiptLine>("receipts", &())[0];
    assert_eq!(
        (r.txn_id, r.charges, r.charged),
        (None, 3, 1),
        "only the first charge's transaction is in yet"
    );
    // The second shipment's charge, and the refund, reach the bank.
    b.spend(checking, "2026-10-06", "Amazon.com", None, -650);
    let t = |b: &Book, amount| b.register(None).into_iter().find(|t| t.amount == amount).unwrap();
    let refund = AddTransaction {
        account_id: checking,
        date: "2026-10-21".into(),
        payee: "Amazon".into(),
        amount: 450,
        ..AddTransaction::default()
    };
    b.run("add_transaction", &refund).unwrap();
    let r = &b.list::<ReceiptLine>("receipts", &())[0];
    assert_eq!((r.charges, r.charged), (3, 3));
    for (amount, lines) in [(-1_003, [-420, -583]), (-650, [-272, -378]), (450, [189, 261])] {
        let id = t(&b, amount).id;
        let got: Vec<(String, i64)> = splits(&b, id);
        assert_eq!(
            got,
            [
                ("Everyday \u{203a} Groceries \u{203a} Eggs".to_string(), lines[0]),
                ("Everyday \u{203a} Household".to_string(), lines[1])
            ],
            "{amount}"
        );
    }
    let _ = eggs;
    // The same history again changes nothing.
    b.run("add_charges", &history).unwrap();
    assert!(b.last.is_empty(), "{:?}", b.last.len());
    // Somebody says the first was paid by nothing found: its split goes
    // when they give it a category, and it can be paid again by hand.
    let first = b.list::<ChargeLine>(
        "charges",
        &OneReceipt {
            receipt_id: "amazon:112-1".into(),
        },
    )[0]
    .clone();
    assert_eq!((first.payee.as_str(), first.date.as_str()), ("AMZN Mktp US", "2026-10-03"));
    let cands: Vec<CandidateLine> = b.list(
        "candidates",
        &PaidFor {
            receipt_id: "amazon:112-1".into(),
            charge_id: Some(first.id.clone()),
        },
    );
    assert_eq!(cands[0].txn_id, first.txn_id.unwrap(), "what paid it is the nearest");
    assert_eq!(cands[0].off_by, 0);
    b.run(
        "pay_charge",
        &PayCharge {
            charge_id: first.id.clone(),
            txn_id: None,
        },
    )
    .unwrap();
    assert_eq!(b.list::<ReceiptLine>("receipts", &())[0].charged, 2);
    let other = t(&b, -650).id;
    let taken = b
        .run(
            "pay_charge",
            &PayCharge {
                charge_id: first.id,
                txn_id: Some(other),
            },
        )
        .unwrap_err();
    assert!(taken.contains("another receipt"), "{taken}");
}

/// A receipt paid whole before its charges were known gives that payment
/// to the charge it is.
#[test]
fn a_whole_payment_becomes_its_charge() {
    let (mut b, checking, ..) = household();
    b.spend(checking, "2026-10-03", "Amazon", None, -1_653);
    let order = ReceiptImport {
        store: "amazon".into(),
        receipts: vec![StoreReceipt {
            order_id: "112-9".into(),
            date: "2026-10-02".into(),
            place: "Amazon.com".into(),
            total: -1_653,
            items: vec![StoreItem {
                name: "A thing".into(),
                sku: "B1".into(),
                quantity: 1000,
                unit: "ea".into(),
                amount: -1_549,
            }],
        }],
    };
    b.run("add_receipts", &order).unwrap();
    let paid = b.list::<ReceiptLine>("receipts", &())[0].txn_id;
    assert!(paid.is_some());
    let one = ChargeImport {
        store: "amazon".into(),
        charges: vec![StoreCharge {
            order_id: "112-9".into(),
            date: "2026-10-03".into(),
            amount: -1_653,
            card: String::new(),
        }],
    };
    b.run("add_charges", &one).unwrap();
    let r = &b.list::<ReceiptLine>("receipts", &())[0];
    assert_eq!((r.txn_id, r.charged), (None, 1));
    assert_eq!(
        b.list::<ChargeLine>(
            "charges",
            &OneReceipt {
                receipt_id: "amazon:112-9".into()
            }
        )[0]
        .txn_id,
        paid
    );
}

/// A category gone takes its kinds with it, and hands their spending,
/// split lines and items to where its money goes.
#[test]
fn a_deleted_category_takes_its_kinds() {
    let (mut b, checking, groceries, eggs, improvement) = household();
    b.spend(checking, "2026-10-03", "Farm stand", Some(eggs), -600);
    b.run("add_receipts", &target_receipt(&[("EGGS", "1", -500)], -500)).unwrap();
    b.run(
        "delete_category",
        &DeleteCategory {
            id: groceries,
            move_to: Some(improvement),
        },
    )
    .unwrap();
    assert!(b.rows::<Category>("category").iter().all(|c| c.id != eggs && c.id != groceries));
    assert_eq!(b.line("2026-10", improvement).activity, -600);
    assert!(b.rows::<Item>("item").iter().all(|i| i.category_id == Some(improvement)));
}

/// A server's groups and categories, as the schema before the tree kept
/// them, are one tree: a group a heading, a category under its group.
#[test]
fn groups_and_categories_become_one_tree() {
    let d = money_domain::domain();
    let was: ark::schema::Schema = serde_json::from_str(include_str!("../migrations/categories-as-a-tree.json")).unwrap();
    let id = |n: u8| format!("{n:032x}");
    let rows = |t: &str, objects: Vec<serde_json::Value>| {
        let (_, table) = was.table(t).unwrap();
        (
            t.to_string(),
            objects.iter().map(|o| ark::dynamic::row_bytes(table, o).unwrap()).collect::<Vec<_>>(),
        )
    };
    let old: ark::log::Rows = [
        rows("group", vec![serde_json::json!({"id": id(1), "name": "Bills", "pos": 1, "hidden": false, "income": false, "created_ms": 1, "user_id": "sam"})]),
        rows("category", vec![serde_json::json!({"id": id(2), "group_id": id(1), "name": "Rent", "pos": 1, "hidden": false, "target": 150000, "note": "", "created_ms": 1, "user_id": "sam"})]),
    ]
    .into_iter()
    .collect();
    let now = d.schema.clone();
    let rows = ark::migrate::migrate(&was, &old, &now, &d.migrations).unwrap().expect("migrated");
    let (store, _) = ark::store::Store::from_rows(now, ark::schema::Space::Durable, &rows).unwrap();
    let cats: Vec<Category> = store
        .rows("category")
        .map(|r| <Category as ark::format::Encode>::from_bytes(r).unwrap())
        .collect();
    let bills = cats.iter().find(|c| c.name == "Bills").unwrap();
    let rent = cats.iter().find(|c| c.name == "Rent").unwrap();
    assert_eq!((bills.parent_id, rent.parent_id, rent.target), (None, Some(bills.id), 150_000));
}

/// A receipt typed by hand for a transaction pays for it, marks it, and
/// splits it; written again (a reading put right) its items are replaced
/// and the transaction split again. Falsified by a hand receipt keyed by
/// what it says, which a second one alike would overwrite.
#[test]
fn a_receipt_written_by_hand_pays_for_its_transaction() {
    let (mut b, checking, groceries, eggs, improvement) = household();
    b.spend(checking, "2026-10-25", "ALDI 72031", Some(groceries), -1_000);
    let t = txn(&b, "ALDI 72031");
    assert_eq!((t.receipt.as_str(), t.photos), ("", 0));
    let item = |name: &str, amount| StoreItem {
        name: name.into(),
        sku: String::new(),
        quantity: 1000,
        unit: "ea".into(),
        amount,
    };
    let write = |id: &str, items| WriteReceipt {
        receipt_id: id.into(),
        txn_id: Some(t.id),
        place: "ALDI".into(),
        date: "2026-10-25".into(),
        total: None,
        items,
    };
    b.run("write_receipt", &write("", vec![item("EGGS", -400), item("PAINT", -600)])).unwrap();
    let r = b.list::<ReceiptLine>("for_txn", &OneTxn { id: t.id });
    assert_eq!(r.len(), 1);
    assert!(r[0].id.starts_with("paper:"), "{}", r[0].id);
    assert_eq!((r[0].total, r[0].items, r[0].txn_id), (-1_000, 2, Some(t.id)));
    assert_eq!(txn(&b, "ALDI 72031").receipt, r[0].id, "the register marks it");

    let refused = b.run("write_receipt", &write(&r[0].id, vec![item("", -1_000)])).unwrap_err();
    assert!(refused.contains("needs a name"), "{refused}");
    b.run("write_receipt", &write(&r[0].id, vec![item("EGGS", -1_000)])).unwrap();
    let again = b.list::<ReceiptLine>("for_txn", &OneTxn { id: t.id });
    assert_eq!((again.len(), again[0].items), (1, 1), "put right, not another");
    let _ = (eggs, improvement);
}

/// A split transaction needs a look while a line of it has no category, and
/// not once every line has one. Falsified by asking a split transaction for
/// its own category, which it never has: it stayed in the inbox for good.
#[test]
fn a_split_transaction_needs_a_look_only_while_a_line_has_no_category() {
    let (mut b, checking, groceries, _, improvement) = household();
    b.spend(checking, "2026-10-25", "Hardware Store", Some(groceries), -5_000);
    let id = txn(&b, "Hardware Store").id;
    let inbox = |b: &Book| -> Vec<Id<Txn>> {
        let f = Filtered {
            saved: None,
            filter: Filter::needs_a_look(),
            month: None,
        };
        b.list::<TxnLine>("filtered", &f).into_iter().map(|t| t.id).collect()
    };
    let part = |c, amount| SplitPart {
        category_id: c,
        amount,
        memo: String::new(),
    };
    b.run("approve", &Txns { ids: vec![id] }).unwrap();
    b.run(
        "split",
        &SplitTxn {
            id,
            lines: vec![part(Some(groceries), -2_000), part(None, -3_000)],
        },
    )
    .unwrap();
    assert!(inbox(&b).contains(&id), "a line for no category");
    b.run(
        "split",
        &SplitTxn {
            id,
            lines: vec![part(Some(groceries), -2_000), part(Some(improvement), -3_000)],
        },
    )
    .unwrap();
    assert!(!inbox(&b).contains(&id), "every line has one");
}

/// A Costco fill-up somebody approved under Groceries, and then its
/// receipt, whose one line ("REGULAR GAS") is filed under Gas by its name:
/// the receipt is paid by it, and its category stays Groceries — through
/// the import, the import again, and the line filed by hand. Only saying
/// the receipt paid it makes it the receipt's. One the bank brought in and
/// nobody approved yet takes the receipt's. Falsified by a receipt
/// recategorizing whatever paid it: the fill-up became Gas, and a month
/// already budgeted went overspent (as money's did on 2026-11-03).
#[test]
fn a_receipt_never_overrides_a_category_somebody_confirmed() {
    let (mut b, checking, groceries, _, _) = household();
    let fuel = b.group("Car");
    let gas = b.category(fuel, "Gas");
    b.spend(checking, "2026-10-16", "Costco", Some(groceries), -4_590);
    let fillup = |id: &str, date: &str, total| ReceiptImport {
        store: "costco".into(),
        receipts: vec![StoreReceipt {
            order_id: id.into(),
            date: date.into(),
            place: "Costco Gas".into(),
            total,
            items: vec![StoreItem {
                name: "REGULAR GAS".into(),
                sku: String::new(),
                quantity: 1000,
                unit: "ea".into(),
                amount: total,
            }],
        }],
    };
    let line = |b: &Book, amount| b.register(None).into_iter().find(|t| t.amount == amount).unwrap();
    b.run("add_receipts", &fillup("2103771660808261016", "2026-10-16", -4_590)).unwrap();
    let t = line(&b, -4_590);
    assert_eq!((t.category.as_str(), t.receipt.as_str()), ("Groceries", "costco:2103771660808261016"), "paid by it, its category kept");
    assert_eq!(b.line("2026-10", gas).activity, 0, "nothing spent from Gas");
    b.run("add_receipts", &fillup("2103771660808261016", "2026-10-16", -4_590)).unwrap();
    let item = vec![ItemRef {
        receipt_id: "costco:2103771660808261016".into(),
        line: 1,
    }];
    b.run("file_items", &ClassifyItems { items: item, category_id: Some(gas) }).unwrap();
    assert_eq!(line(&b, -4_590).category, "Groceries", "an item filed leaves it");

    // Said to be the receipt's: it is.
    let pay = MatchReceipt {
        receipt_id: "costco:2103771660808261016".into(),
        txn_id: Some(t.id),
    };
    b.run("pay", &pay).unwrap();
    assert_eq!(line(&b, -4_590).category, "Gas");

    // Not approved yet: the receipt's category.
    b.from_bank(checking, "2026-10-21", "Costco", Some(groceries), -8_578);
    b.run("add_receipts", &fillup("2103771734395261021", "2026-10-21", -8_578)).unwrap();
    assert_eq!(line(&b, -8_578).category, "Gas");
}
