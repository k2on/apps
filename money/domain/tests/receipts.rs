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
    b.spend(checking, "2026-10-25", "Target", Some(groceries), -8_740);
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
