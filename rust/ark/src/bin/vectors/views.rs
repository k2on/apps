//! `views/` (§13): a maintained plan through a straight run of an
//! authority's facts — the playlist's items ordered by position and
//! limited to two, and the same with the items hanging beneath the
//! playlist row. The patches are what a screen splices; the contract is
//! that the rows equal a fresh hydrate after every step.

use ark::eval::Args;
use ark::hash::closures;
use ark::ir::{module_value, CmpOp};
use ark::log::Entry;
use ark::peer::{Authority, Sequenced};
use ark::protocol::change_value;
use ark::schema::{Dir, Relation};
use ark::store::{Change, MemoryStore, Store};
use ark::value::Value;
use ark::view::{contract, hydrate, push, Filter, Patch, ViewPlan};

use super::demo::{self, hash_of, id_n};
use super::json::{array, json, obj, quoted};
use super::show;
use super::Out;

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Args {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn patch_value(p: &Patch) -> Value {
    let t = |s: &str| ("t", Value::text(s));
    match p {
        Patch::Insert { at, node } => Value::record(vec![t("insert"), ("at", Value::Int(*at as i64)), ("node", node.clone())]),
        Patch::Remove { at } => Value::record(vec![t("remove"), ("at", Value::Int(*at as i64))]),
        Patch::Update { at, node } => Value::record(vec![t("update"), ("at", Value::Int(*at as i64)), ("node", node.clone())]),
    }
}

pub fn views(out: &Out) {
    out.dir("views/");
    let m = demo::module();
    let sch = &m.schema;
    let pid = id_n(1);
    let h_add = hash_of(&m, "add_to_playlist");
    // A straight sequence of entries on one authority: create, add 1..4.
    let mut a = Authority::new(sch.clone(), closures(&m));
    let add = |k: u8, eid: u8| Entry {
        id: id_n(eid),
        actor: "alice".into(),
        session: "dev".into(),
        fn_hash: h_add.clone(),
        args: args([("playlist_id", Value::Id(pid)), ("track_id", Value::text(format!("t{k}")))]),
        autos: Args::new(),
    };
    let entries = [
        Entry {
            id: id_n(100),
            actor: "alice".into(),
            session: "dev".into(),
            fn_hash: hash_of(&m, "create_playlist"),
            args: args([("name", Value::text("Viewed"))]),
            autos: args([("id", Value::Id(pid))]),
        },
        add(1, 101),
        add(2, 102),
        add(3, 103),
        add(4, 104),
    ];
    let sequence = |a: &mut Authority, e: &Entry| match a.sequence_entry(e) {
        Sequenced::Appended(_, f) => f,
        other => panic!("view seq: {other:?}"),
    };
    let mut all_facts: Vec<Vec<Change>> = entries.iter().map(|e| sequence(&mut a, e)).collect();
    // A delete written by hand as a fact, then another add, to exercise a
    // refill.
    let remove_first = vec![Change::Remove("item".into(), a.store.scan("item").remove(0))];
    a.store.apply_changes(&remove_first);
    let added = sequence(&mut a, &add(5, 105));
    all_facts.push(remove_first);
    all_facts.push(added);
    let plans: Vec<(&str, ViewPlan)> = vec![
        (
            "top-two-by-pos",
            ViewPlan {
                table: "item".into(),
                filter: Some(Filter::Cmp("playlist_id".into(), CmpOp::Eq, Value::Id(pid))),
                order: vec![("pos".into(), Dir::Asc)],
                limit: Some(2),
                related: vec![],
            },
        ),
        (
            "playlist-with-items",
            ViewPlan {
                table: "playlist".into(),
                filter: None,
                order: vec![("name".into(), Dir::Asc)],
                limit: None,
                related: vec![(
                    "item".into(),
                    Relation {
                        parent: "playlist".into(),
                        child: "item".into(),
                        column: "playlist_id".into(),
                    },
                    ViewPlan {
                        table: "item".into(),
                        filter: None,
                        order: vec![("pos".into(), Dir::Desc)],
                        limit: Some(3),
                        related: vec![],
                    },
                )],
            },
        ),
    ];
    for (name, vp) in &plans {
        let mut st = MemoryStore::empty(sch.clone());
        let mut view = hydrate(sch, vp, &st);
        let mut steps: Vec<(Vec<Patch>, Vec<Value>)> = Vec::new();
        for facts in &all_facts {
            let mut patches = Vec::new();
            for ch in facts {
                st.apply_change(ch);
                let (v2, ps) = push(sch, &st, ch, &view);
                view = v2;
                patches.extend(ps);
            }
            assert!(contract(sch, vp, &st, &view), "view contract broken");
            steps.push((patches, view.rows()));
        }
        // The top-level plan must show a removal and the refill after it;
        // the nested one must show a child change surfacing as an update of
        // its parent.
        let ok = match *name {
            "top-two-by-pos" => {
                steps.iter().any(|(ps, _)| ps.iter().any(|p| matches!(p, Patch::Remove { .. }))) && steps.iter().any(|(ps, _)| ps.len() >= 2)
            }
            _ => steps.iter().any(|(ps, _)| ps.iter().any(|p| matches!(p, Patch::Update { .. }))),
        };
        assert!(ok, "view {name}: the patches do not show what the plan is for");
        out.write(
            &format!("views/{name}.json"),
            &obj(&[
                ("module", json(&module_value(&m))),
                ("plan", quoted(&show::view_plan(vp))),
                (
                    "changes",
                    json(&Value::List(
                        all_facts.iter().map(|f| Value::List(f.iter().map(change_value).collect())).collect(),
                    )),
                ),
                (
                    "steps",
                    array(steps.iter().map(|(ps, rows)| {
                        obj(&[
                            ("patches", json(&Value::List(ps.iter().map(patch_value).collect()))),
                            ("rows", json(&Value::List(rows.clone()))),
                        ])
                    })),
                ),
            ]),
        );
    }
    println!("  {} changes through {} plans", all_facts.len(), plans.len());
}
