//! The steps that read the rows of a schema before this one
//! ([`ark::migrate`]), oldest first. Each pins the schema it reads, as the
//! server last stood on it (`migrations/<name>.json`, a copy of
//! `tests/schema.json` then), and is a function over every row as JSON.

use ark::migrate::{Migration, Tables};
use ark::Refusal;
use serde_json::{json, Value as Json};

/// Every step, oldest first.
pub fn all() -> Vec<Migration> {
    vec![Migration::new(
        "categories as a tree",
        include_str!("../migrations/categories-as-a-tree.json"),
        categories_as_a_tree,
    )]
}

/// Groups were a table of their own, and a category named its group; now
/// there is one tree of categories (`schema.rs`). A group is a category
/// with no parent, its id, name, place, hidden and income as they were and
/// no target; a category's parent is its group. A transaction is for one
/// category, as every one was (`split` false).
fn categories_as_a_tree(t: &mut Tables) -> Result<(), Refusal> {
    let groups = t.remove("group").unwrap_or_default();
    let mut tree = Vec::with_capacity(groups.len());
    for g in groups {
        let mut row = serde_json::Map::new();
        for (k, v) in [
            ("id", g.get("id").cloned().unwrap_or(Json::Null)),
            ("parent_id", Json::Null),
            ("name", g.get("name").cloned().unwrap_or(json!(""))),
            ("pos", g.get("pos").cloned().unwrap_or(json!(0))),
            ("hidden", g.get("hidden").cloned().unwrap_or(json!(false))),
            ("income", g.get("income").cloned().unwrap_or(json!(false))),
            ("target", json!(0)),
            ("note", json!("")),
            ("created_ms", g.get("created_ms").cloned().unwrap_or(json!(0))),
            ("user_id", g.get("user_id").cloned().unwrap_or(json!(""))),
        ] {
            row.insert(k.into(), v);
        }
        tree.push(row);
    }
    for mut c in t.remove("category").unwrap_or_default() {
        let parent = c.remove("group_id").ok_or_else(|| Refusal::new("a category with no group"))?;
        c.insert("parent_id".into(), parent);
        c.insert("income".into(), json!(false));
        tree.push(c);
    }
    t.insert("category".into(), tree);
    for row in t.get_mut("txn").into_iter().flatten() {
        row.insert("split".into(), json!(false));
    }
    Ok(())
}
